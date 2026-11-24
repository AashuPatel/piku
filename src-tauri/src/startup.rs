use std::path::Path;

#[cfg(windows)]
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, HANDLE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn CreateMutexW(
        lp_mutex_attributes: *const std::ffi::c_void,
        b_initial_owner: i32,
        lp_name: *const u16,
    ) -> HANDLE;
}

#[cfg(windows)]
const SINGLE_INSTANCE_MUTEX: &str = "Local\\com.piku.desktop.single_instance";
#[cfg(windows)]
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
#[cfg(windows)]
const RUN_VALUE_NAME: &str = "Piku";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileOutcome {
    Unchanged,
    UpdatedStale,
    RestoredMissing,
    RemovedStale,
}

/// Native per-session Windows named mutex guard preventing duplicate Piku instances.
/// Uses `bInitialOwner = FALSE` so ownership/abandonment deadlocks cannot occur;
/// the kernel releases the handle automatically on process exit or Drop.
pub struct SingleInstanceGuard {
    #[cfg(windows)]
    handle: HANDLE,
}

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        #[cfg(windows)]
        {
            Self::acquire_named(SINGLE_INSTANCE_MUTEX)
        }
        #[cfg(not(windows))]
        {
            Some(Self {})
        }
    }

    #[cfg(windows)]
    fn acquire_named(name: &str) -> Option<Self> {
        let name_w = to_wide_null(name);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name_w.as_ptr()) };
        if handle.is_null() {
            // Unexpected OS error: fail-open rather than preventing the user from launching Piku
            return Some(Self {
                handle: std::ptr::null_mut(),
            });
        }
        let last_err = unsafe { GetLastError() };
        if last_err == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }
        Some(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                CloseHandle(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}

pub fn format_startup_command(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.display())
}

#[cfg(windows)]
fn commands_match(existing: &str, expected: &str) -> bool {
    existing.trim().eq_ignore_ascii_case(expected.trim())
}

#[cfg(windows)]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn write_run_value(value_name: &str, command: &str) -> Result<(), String> {
    let subkey_w = to_wide_null(RUN_SUBKEY);
    let name_w = to_wide_null(value_name);

    let mut hkey: HKEY = std::ptr::null_mut();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if open_status != 0 {
        return Err(format!(
            "Failed to open per-user Windows Run key (error {open_status})"
        ));
    }

    let data_w = to_wide_null(command);
    let byte_len = (data_w.len() * std::mem::size_of::<u16>()) as u32;
    let status = unsafe {
        RegSetValueExW(
            hkey,
            name_w.as_ptr(),
            0,
            REG_SZ,
            data_w.as_ptr() as *const u8,
            byte_len,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }
    if status == 0 {
        Ok(())
    } else {
        Err(format!("Failed to write launch on startup value (error {status})"))
    }
}

#[cfg(windows)]
fn delete_run_value(value_name: &str) -> Result<(), String> {
    let subkey_w = to_wide_null(RUN_SUBKEY);
    let name_w = to_wide_null(value_name);

    let mut hkey: HKEY = std::ptr::null_mut();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if open_status != 0 {
        return Err(format!(
            "Failed to open per-user Windows Run key (error {open_status})"
        ));
    }

    let status = unsafe { RegDeleteValueW(hkey, name_w.as_ptr()) };
    unsafe {
        RegCloseKey(hkey);
    }
    if status == 0 || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        Err(format!("Failed to remove launch on startup value (error {status})"))
    }
}

#[cfg(windows)]
fn read_run_value(value_name: &str) -> Option<String> {
    let subkey_w = to_wide_null(RUN_SUBKEY);
    let name_w = to_wide_null(value_name);

    let mut hkey: HKEY = std::ptr::null_mut();
    let open_status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey_w.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        )
    };
    if open_status != 0 {
        return None;
    }

    let mut value_type: u32 = 0;
    let mut byte_len: u32 = 0;
    let size_status = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            std::ptr::null_mut(),
            &mut byte_len,
        )
    };
    if size_status != 0 || value_type != REG_SZ || byte_len < 2 {
        unsafe {
            RegCloseKey(hkey);
        }
        return None;
    }

    let word_len = (byte_len as usize).div_ceil(2);
    let mut buf = vec![0u16; word_len];
    let mut out_len = byte_len;
    let read_status = unsafe {
        RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null_mut(),
            &mut value_type,
            buf.as_mut_ptr() as *mut u8,
            &mut out_len,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }
    if read_status != 0 {
        return None;
    }

    let used_words = (out_len as usize / 2).min(buf.len());
    let trimmed_end = buf[..used_words]
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(used_words);
    Some(String::from_utf16_lossy(&buf[..trimmed_end]))
}

#[cfg(windows)]
fn reconcile_run_value(
    value_name: &str,
    desired_enabled: bool,
    exe_path: &Path,
) -> Result<ReconcileOutcome, String> {
    let existing = read_run_value(value_name);
    if desired_enabled {
        let expected = format_startup_command(exe_path);
        match existing {
            Some(ref cur) if commands_match(cur, &expected) => Ok(ReconcileOutcome::Unchanged),
            Some(_) => {
                write_run_value(value_name, &expected)?;
                Ok(ReconcileOutcome::UpdatedStale)
            }
            None => {
                write_run_value(value_name, &expected)?;
                Ok(ReconcileOutcome::RestoredMissing)
            }
        }
    } else {
        match existing {
            None => Ok(ReconcileOutcome::Unchanged),
            Some(_) => {
                delete_run_value(value_name)?;
                Ok(ReconcileOutcome::RemovedStale)
            }
        }
    }
}

pub fn set_launch_on_startup(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        if enabled {
            let exe = std::env::current_exe()
                .map_err(|e| format!("Failed to resolve Piku executable path: {e}"))?;
            let command = format_startup_command(&exe);
            write_run_value(RUN_VALUE_NAME, &command)
        } else {
            delete_run_value(RUN_VALUE_NAME)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

pub fn reconcile_launch_on_startup(desired_enabled: bool) -> Result<ReconcileOutcome, String> {
    #[cfg(windows)]
    {
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to resolve Piku executable path: {e}"))?;
        reconcile_run_value(RUN_VALUE_NAME, desired_enabled, &exe)
    }
    #[cfg(not(windows))]
    {
        let _ = desired_enabled;
        Ok(ReconcileOutcome::Unchanged)
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn single_instance_mutex_blocks_second_acquire_until_released() {
        let name = format!("Local\\com.piku.desktop.test_mutex_{}", std::process::id());
        let first = SingleInstanceGuard::acquire_named(&name);
        assert!(first.is_some(), "first mutex acquisition should succeed");

        let second = SingleInstanceGuard::acquire_named(&name);
        assert!(
            second.is_none(),
            "second mutex acquisition must fail while first guard is held"
        );

        drop(first);

        let third = SingleInstanceGuard::acquire_named(&name);
        assert!(
            third.is_some(),
            "acquisition should succeed again after first guard drops"
        );
    }

    #[test]
    fn startup_reconciliation_handles_all_path_and_quoting_cases() {
        const TEST_VALUE: &str = "PikuStartupUnitTestTemp";
        let current_exe = PathBuf::from(r"C:\Program Files\Piku App\piku.exe");
        let stale_exe = PathBuf::from(r"C:\Old Folder\piku.exe");

        // Ensure clean initial state
        delete_run_value(TEST_VALUE).expect("initial cleanup");
        assert!(read_run_value(TEST_VALUE).is_none());

        // Case 3: Startup enabled + missing entry -> restored with quotes for paths with spaces
        let r_missing =
            reconcile_run_value(TEST_VALUE, true, &current_exe).expect("restore missing");
        assert_eq!(r_missing, ReconcileOutcome::RestoredMissing);
        assert_eq!(
            read_run_value(TEST_VALUE).as_deref(),
            Some("\"C:\\Program Files\\Piku App\\piku.exe\"")
        );

        // Case 1: Startup enabled + correct path (case-insensitive) -> unchanged (no rewrite)
        let r_same =
            reconcile_run_value(TEST_VALUE, true, &PathBuf::from(r"c:\program files\piku app\piku.exe"))
                .expect("unchanged on match");
        assert_eq!(r_same, ReconcileOutcome::Unchanged);

        // Case 2: Startup enabled + stale path -> updated automatically
        write_run_value(TEST_VALUE, &format_startup_command(&stale_exe)).expect("seed stale");
        let r_stale = reconcile_run_value(TEST_VALUE, true, &current_exe).expect("update stale");
        assert_eq!(r_stale, ReconcileOutcome::UpdatedStale);
        assert_eq!(
            read_run_value(TEST_VALUE).as_deref(),
            Some("\"C:\\Program Files\\Piku App\\piku.exe\"")
        );

        // Case 4: Startup disabled + stale entry -> removed
        let r_removed =
            reconcile_run_value(TEST_VALUE, false, &current_exe).expect("remove stale when off");
        assert_eq!(r_removed, ReconcileOutcome::RemovedStale);
        assert!(read_run_value(TEST_VALUE).is_none());

        // Case 5: Startup disabled + already absent -> unchanged
        let r_off_clean =
            reconcile_run_value(TEST_VALUE, false, &current_exe).expect("unchanged when off");
        assert_eq!(r_off_clean, ReconcileOutcome::Unchanged);
    }
}
