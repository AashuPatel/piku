use crate::tracking::tracker_core::{
    is_excluded_explorer_window_class, is_excluded_process, AppIdentity, ForegroundSample,
};
use std::collections::HashMap;

#[cfg(windows)]
use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use windows_sys::core::BOOL;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
#[cfg(windows)]
use windows_sys::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop};
#[cfg(windows)]
use windows_sys::Win32::System::SystemInformation::GetTickCount;
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(windows)]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
};

#[cfg(windows)]
const DESKTOP_READOBJECTS: u32 = 0x0001;

/// Lightweight Windows foreground & idle/lock prober with in-memory PE display-name cache.
pub struct WindowsProbe {
    self_pid: u32,
    display_name_cache: HashMap<String, Option<String>>,
}

impl WindowsProbe {
    pub fn new() -> Self {
        Self {
            self_pid: std::process::id(),
            display_name_cache: HashMap::new(),
        }
    }

    pub fn sample(&mut self, idle_threshold_secs: u64) -> ForegroundSample {
        #[cfg(not(windows))]
        {
            let _ = idle_threshold_secs;
            return ForegroundSample::None;
        }

        #[cfg(windows)]
        {
            if is_desktop_locked() {
                return ForegroundSample::Locked;
            }

            let idle_secs = idle_duration_secs();
            if idle_secs >= idle_threshold_secs {
                return ForegroundSample::Idle { idle_secs };
            }

            let hwnd = unsafe { GetForegroundWindow() };
            if hwnd.is_null() {
                return ForegroundSample::None;
            }

            let mut pid: u32 = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
            if pid == 0 {
                return ForegroundSample::None;
            }
            if pid == self.self_pid {
                return ForegroundSample::SelfOrExcluded;
            }

            let exe_path = query_process_image_path(pid);
            let window_class = if exe_path
                .as_deref()
                .and_then(extract_exe_name)
                .as_deref()
                == Some("explorer.exe")
            {
                read_window_class_name(hwnd)
            } else {
                None
            };

            let uwp_child_path = if exe_path
                .as_deref()
                .and_then(extract_exe_name)
                .as_deref()
                == Some("applicationframehost.exe")
            {
                match find_uwp_child_pid(hwnd, pid) {
                    Some(child_pid) if child_pid == self.self_pid => {
                        return ForegroundSample::SelfOrExcluded;
                    }
                    Some(child_pid) => query_process_image_path(child_pid),
                    None => None,
                }
            } else {
                None
            };

            classify_foreground_process(
                exe_path,
                window_class.as_deref(),
                uwp_child_path,
                |path| self.cached_pe_display_name(path),
            )
        }
    }

    #[cfg(windows)]
    fn cached_pe_display_name(&mut self, exe_path: &str) -> Option<String> {
        if let Some(cached) = self.display_name_cache.get(exe_path) {
            return cached.clone();
        }
        let fetched = read_pe_friendly_name(exe_path);
        // Keep cache bounded
        if self.display_name_cache.len() > 256 {
            self.display_name_cache.clear();
        }
        self.display_name_cache
            .insert(exe_path.to_string(), fetched.clone());
        fetched
    }
}

#[cfg(windows)]
fn is_desktop_locked() -> bool {
    unsafe {
        let hdesk = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if hdesk.is_null() {
            true
        } else {
            CloseDesktop(hdesk);
            false
        }
    }
}

#[cfg(windows)]
fn idle_duration_secs() -> u64 {
    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii) == 0 {
            return 0;
        }
        let now_tick = GetTickCount();
        let elapsed_ms = now_tick.wrapping_sub(lii.dwTime);
        (elapsed_ms / 1000) as u64
    }
}

#[cfg(windows)]
fn query_process_image_path(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        buf.truncate(len as usize);
        Some(String::from_utf16_lossy(&buf))
    }
}

#[cfg(windows)]
fn extract_exe_name(full_path: &str) -> Option<String> {
    full_path
        .rsplit(['\\', '/'])
        .next()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

#[cfg(windows)]
struct UwpSearchCtx {
    host_pid: u32,
    found_pid: u32,
}

#[cfg(windows)]
unsafe extern "system" fn enum_uwp_child_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam as *mut UwpSearchCtx);
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != 0 && pid != ctx.host_pid {
        ctx.found_pid = pid;
        return 0; // stop enumeration
    }
    1
}

#[cfg(windows)]
fn find_uwp_child_pid(hwnd: HWND, host_pid: u32) -> Option<u32> {
    let mut ctx = UwpSearchCtx {
        host_pid,
        found_pid: 0,
    };
    unsafe {
        EnumChildWindows(hwnd, Some(enum_uwp_child_proc), &mut ctx as *mut _ as LPARAM);
    }
    if ctx.found_pid != 0 {
        Some(ctx.found_pid)
    } else {
        None
    }
}

#[cfg(windows)]
fn read_window_class_name(hwnd: HWND) -> Option<String> {
    let mut buf = vec![0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return None;
    }
    buf.truncate(len as usize);
    let s = String::from_utf16_lossy(&buf).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(windows)]
fn read_pe_friendly_name(exe_path: &str) -> Option<String> {
    let wide_path: Vec<u16> = OsStr::new(exe_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut handle: u32 = 0;
        let size = GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut handle);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(wide_path.as_ptr(), 0, size, data.as_mut_ptr() as *mut _) == 0 {
            return None;
        }

        // Discover translation codepage table first; fallback to US English (040904B0)
        let trans_query: Vec<u16> = OsStr::new("\\VarFileInfo\\Translation")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len: u32 = 0;
        let lang_code = if VerQueryValueW(
            data.as_ptr() as *const _,
            trans_query.as_ptr(),
            &mut ptr,
            &mut len,
        ) != 0
            && len >= 4
            && !ptr.is_null()
        {
            let pair = std::slice::from_raw_parts(ptr as *const u16, 2);
            format!("{:04X}{:04X}", pair[0], pair[1])
        } else {
            "040904B0".to_string()
        };

        for field in ["FileDescription", "ProductName"] {
            let subblock = format!("\\StringFileInfo\\{lang_code}\\{field}");
            let wide_sub: Vec<u16> = OsStr::new(&subblock)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut val_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut val_len: u32 = 0;
            if VerQueryValueW(
                data.as_ptr() as *const _,
                wide_sub.as_ptr(),
                &mut val_ptr,
                &mut val_len,
            ) != 0
                && val_len > 1
                && !val_ptr.is_null()
            {
                let slice = std::slice::from_raw_parts(val_ptr as *const u16, val_len as usize);
                let text = String::from_utf16_lossy(slice)
                    .trim_matches('\0')
                    .trim()
                    .to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }
}

/// Pure classification step for resolved foreground process metadata.
/// Keeps Explorer shell-class filtering, UWP child unwrapping, shell exclusions,
/// and unresolvable-process filtering deterministic and unit-testable.
#[cfg(windows)]
fn classify_foreground_process<F>(
    mut exe_path: Option<String>,
    window_class: Option<&str>,
    uwp_child_path: Option<String>,
    mut resolve_friendly_name: F,
) -> ForegroundSample
where
    F: FnMut(&str) -> Option<String>,
{
    let mut exe_name = exe_path.as_deref().and_then(extract_exe_name);

    // If process path cannot be resolved (e.g. protected/system or dying PID),
    // skip rather than fabricating a persistent "Unknown App" record.
    let Some(initial_name) = exe_name.as_deref() else {
        return ForegroundSample::None;
    };

    // Exclude Windows Desktop & Taskbar shell chrome hosted by explorer.exe
    // while keeping real File Explorer windows (e.g. CabinetWClass) tracked.
    if initial_name == "explorer.exe" {
        if let Some(cls) = window_class {
            if is_excluded_explorer_window_class(cls) {
                return ForegroundSample::SelfOrExcluded;
            }
        }
    }

    // Unwrap UWP host windows (ApplicationFrameHost.exe). Skip unresolved transitional
    // frames until the real child process attaches so no temporary `uwp_*` rows are created.
    if initial_name == "applicationframehost.exe" {
        let Some(child_path) = uwp_child_path else {
            return ForegroundSample::None;
        };
        let Some(child_exe) = extract_exe_name(&child_path) else {
            return ForegroundSample::None;
        };
        if child_exe == "applicationframehost.exe" {
            return ForegroundSample::None;
        }
        exe_name = Some(child_exe);
        exe_path = Some(child_path);
    }

    let Some(ref resolved_name) = exe_name else {
        return ForegroundSample::None;
    };
    if is_excluded_process(resolved_name) {
        return ForegroundSample::SelfOrExcluded;
    }

    let friendly = exe_path.as_deref().and_then(&mut resolve_friendly_name);
    ForegroundSample::Active(AppIdentity::normalized(
        Some(resolved_name.as_str()),
        exe_path.as_deref(),
        friendly.as_deref(),
    ))
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn probes_real_windows_process_path_and_pe_friendly_name() {
        let self_path = query_process_image_path(std::process::id());
        assert!(self_path.is_some(), "should resolve current test process path via PROCESS_QUERY_LIMITED_INFORMATION");

        let notepad = r"C:\Windows\System32\notepad.exe";
        if std::path::Path::new(notepad).exists() {
            let friendly = read_pe_friendly_name(notepad);
            assert!(
                friendly.as_deref().map(|s| s.to_lowercase().contains("notepad")).unwrap_or(false),
                "expected Notepad PE FileDescription, got {:?}",
                friendly
            );
        }
    }

    #[test]
    fn excludes_desktop_and_taskbar_but_tracks_file_explorer() {
        let explorer = Some(r"C:\Windows\explorer.exe".to_string());

        for shell_class in [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
            "NotifyIconOverflowWindow",
            "TaskListThumbnailWnd",
            "TopLevelWindowForOverflowXamlIsland",
            "XamlExplorerHostIslandWindow",
        ] {
            let sample = classify_foreground_process(
                explorer.clone(),
                Some(shell_class),
                None,
                |_| Some("Windows Explorer".to_string()),
            );
            assert_eq!(
                sample,
                ForegroundSample::SelfOrExcluded,
                "explorer shell class {shell_class} must be excluded"
            );
        }

        // Real File Explorer window (CabinetWClass) and unknown classes stay tracked
        for tracked_class in ["CabinetWClass", "ExploreWClass", "#32770"] {
            let sample = classify_foreground_process(
                explorer.clone(),
                Some(tracked_class),
                None,
                |_| Some("Windows Explorer".to_string()),
            );
            match sample {
                ForegroundSample::Active(app) => {
                    assert_eq!(app.exe_name, "explorer.exe");
                    assert_eq!(app.display_name, "Windows Explorer");
                }
                other => panic!("expected explorer class {tracked_class} to be tracked, got {other:?}"),
            }
        }
    }

    #[test]
    fn skips_unresolved_uwp_transition_and_stabilizes_resolved_uwp_apps() {
        let host = Some(r"C:\Windows\System32\ApplicationFrameHost.exe".to_string());

        // Unresolved launch/transition frame must NOT mint a temporary `uwp_*` app
        let transitional = classify_foreground_process(host.clone(), None, None, |_| None);
        assert_eq!(transitional, ForegroundSample::None);

        // Resolved UWP Calculator
        let calc = classify_foreground_process(
            host.clone(),
            None,
            Some(r"C:\Program Files\WindowsApps\Microsoft.WindowsCalculator\CalculatorApp.exe".to_string()),
            |_| Some("Calculator".to_string()),
        );
        match calc {
            ForegroundSample::Active(app) => {
                assert_eq!(app.exe_name, "calculatorapp.exe");
                assert_eq!(app.display_name, "Calculator");
                assert!(app.exe_path.is_some());
            }
            other => panic!("expected Active Calculator, got {other:?}"),
        }

        // Resolved UWP Settings
        let settings = classify_foreground_process(
            host,
            None,
            Some(r"C:\Windows\ImmersiveControlPanel\SystemSettings.exe".to_string()),
            |_| Some("Settings".to_string()),
        );
        match settings {
            ForegroundSample::Active(app) => {
                assert_eq!(app.exe_name, "systemsettings.exe");
                assert_eq!(app.display_name, "Settings");
                assert!(app.exe_path.is_some());
            }
            other => panic!("expected Active Settings, got {other:?}"),
        }
    }

    #[test]
    fn excludes_win11_shell_and_skips_unresolvable_processes_while_keeping_user_apps() {
        for shell_path in [
            r"C:\Windows\System32\ShellHost.exe",
            r"C:\Windows\System32\PickerHost.exe",
        ] {
            let sample = classify_foreground_process(
                Some(shell_path.to_string()),
                None,
                None,
                |_| Some("Shell".to_string()),
            );
            assert_eq!(
                sample,
                ForegroundSample::SelfOrExcluded,
                "{shell_path} must be excluded"
            );
        }

        // Unresolvable protected/system process must NOT create "Unknown App"
        let unresolvable = classify_foreground_process(None, None, None, |_| None);
        assert_eq!(unresolvable, ForegroundSample::None);

        // Normal user app stays tracked normally
        let chrome = classify_foreground_process(
            Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".to_string()),
            None,
            None,
            |_| Some("Google Chrome".to_string()),
        );
        match chrome {
            ForegroundSample::Active(app) => {
                assert_eq!(app.exe_name, "chrome.exe");
                assert_eq!(app.display_name, "Google Chrome");
            }
            other => panic!("expected Active Chrome, got {other:?}"),
        }
    }
}
