use crate::state::AppState;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

#[cfg(windows)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(windows)]
use std::sync::{Mutex, OnceLock};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
#[cfg(windows)]
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetClassNameW, GetCursorPos, GetWindow, GetWindowLongPtrW,
    IsIconic, IsWindowVisible, KillTimer, SetTimer, SetWindowLongPtrW, SetWindowPos,
    EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GWL_EXSTYLE,
    GWL_STYLE, GW_HWNDNEXT, GW_HWNDPREV, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    MA_NOACTIVATE, SPI_SETWORKAREA, SWP_FRAMECHANGED, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, WINDOWPOS, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENDSESSION, WM_EXITSIZEMOVE,
    WM_MOUSEACTIVATE, WM_QUERYENDSESSION, WM_SETTINGCHANGE, WM_TIMER, WM_WINDOWPOSCHANGING,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_MAXIMIZEBOX,
    WS_MINIMIZEBOX,
};

#[cfg(windows)]
const PET_SUBCLASS_ID: usize = 0x5049_4B55; // "PIKU"
#[cfg(windows)]
const PET_WIND_RECONCILE_TIMER_ID: usize = 0x5049_5744; // "PIWD"
#[cfg(windows)]
const PET_WIND_RECONCILE_DELAY_MS: u32 = 200;
#[cfg(windows)]
const PET_DISPLAY_RECONCILE_TIMER_ID: usize = 0x5049_4450; // "PIDP"
#[cfg(windows)]
const PET_DISPLAY_RECONCILE_DELAY_MS: u32 = 180;
#[cfg(windows)]
static APP_HANDLE_GLOBAL: OnceLock<AppHandle> = OnceLock::new();
#[cfg(windows)]
static HOOK_HANDLES: OnceLock<Mutex<Vec<isize>>> = OnceLock::new();
#[cfg(windows)]
static HOOK_REENTRANT: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static RAISED_FOR_SHOW_DESKTOP: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static INTERNAL_ZORDER: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static INTERNAL_DISPLAY_RECONCILE: AtomicBool = AtomicBool::new(false);

const BUBBLE_LOGICAL_WIDTH: f64 = 236.0;
const BUBBLE_LOGICAL_HEIGHT: f64 = 68.0;
const PANEL_GAP_LOGICAL: f64 = 12.0;

#[cfg(windows)]
#[derive(Clone, Copy)]
struct MonitorInfo {
    bounds: RECT,
    work: RECT,
    scale_factor: f64,
    is_primary: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ReconcilePlan {
    target_x: i32,
    target_y: i32,
    target_w: i32,
    target_h: i32,
    needs_window_update: bool,
    needs_persist: bool,
}

#[cfg(windows)]
fn hwnd_of(window: &WebviewWindow) -> Option<HWND> {
    window.hwnd().ok().map(|h| h.0 as HWND)
}

#[cfg(windows)]
fn work_area_for_window(hwnd: HWND) -> RECT {
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            return info.rcWork;
        }
    }
    primary_work_area()
}

#[cfg(windows)]
fn work_area_for_point(x: i32, y: i32) -> RECT {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            return info.rcWork;
        }
    }
    primary_work_area()
}

#[cfg(windows)]
pub fn primary_work_area() -> RECT {
    unsafe {
        let hmon = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(hmon, &mut info) != 0 {
            return info.rcWork;
        }
    }
    RECT {
        left: 0,
        top: 0,
        right: 1280,
        bottom: 720,
    }
}

#[cfg(windows)]
fn clamp_rect_pos(x: i32, y: i32, width: i32, height: i32, work: RECT) -> (i32, i32) {
    let max_x = (work.right - width).max(work.left);
    let max_y = (work.bottom - height).max(work.top);
    (x.clamp(work.left, max_x), y.clamp(work.top, max_y))
}

#[cfg(windows)]
fn collect_monitors(window: &WebviewWindow) -> Vec<MonitorInfo> {
    let primary_pos = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| (m.position().x, m.position().y));

    if let Ok(list) = window.available_monitors() {
        let mut out = Vec::with_capacity(list.len());
        for m in list {
            let pos = m.position();
            let sz = m.size();
            let wa = m.work_area();
            let scale = m.scale_factor();
            let safe_scale = if scale.is_finite() && scale > 0.25 {
                scale
            } else {
                1.0
            };
            let is_primary = primary_pos
                .map(|(px, py)| pos.x == px && pos.y == py)
                .unwrap_or(pos.x == 0 && pos.y == 0);
            out.push(MonitorInfo {
                bounds: RECT {
                    left: pos.x,
                    top: pos.y,
                    right: pos.x + sz.width as i32,
                    bottom: pos.y + sz.height as i32,
                },
                work: RECT {
                    left: wa.position.x,
                    top: wa.position.y,
                    right: wa.position.x + wa.size.width as i32,
                    bottom: wa.position.y + wa.size.height as i32,
                },
                scale_factor: safe_scale,
                is_primary,
            });
        }
        if !out.is_empty() {
            return out;
        }
    }

    let work = primary_work_area();
    let scale = window.scale_factor().unwrap_or(1.0);
    vec![MonitorInfo {
        bounds: work,
        work,
        scale_factor: if scale.is_finite() && scale > 0.25 {
            scale
        } else {
            1.0
        },
        is_primary: true,
    }]
}

#[cfg(windows)]
fn rect_contains_point(r: RECT, x: i32, y: i32) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

#[cfg(windows)]
fn dist_sq_to_rect(r: RECT, x: i32, y: i32) -> i64 {
    let cx = x.clamp(r.left, r.right.max(r.left)) as i64;
    let cy = y.clamp(r.top, r.bottom.max(r.top)) as i64;
    let dx = cx - (x as i64);
    let dy = cy - (y as i64);
    dx * dx + dy * dy
}

#[cfg(windows)]
fn nearest_monitor_index(monitors: &[MonitorInfo], x: i32, y: i32) -> Option<usize> {
    if monitors.is_empty() {
        return None;
    }
    if let Some(idx) = monitors
        .iter()
        .position(|m| rect_contains_point(m.bounds, x, y))
    {
        return Some(idx);
    }
    monitors
        .iter()
        .enumerate()
        .min_by_key(|(_, m)| dist_sq_to_rect(m.work, x, y))
        .map(|(i, _)| i)
}

#[cfg(windows)]
fn physical_pet_dim(logical_size: u32, scale_factor: f64) -> i32 {
    let clamped = logical_size.clamp(100, 160) as f64;
    let safe_scale = if scale_factor.is_finite() && scale_factor > 0.25 {
        scale_factor
    } else {
        1.0
    };
    (clamped * safe_scale).round().max(1.0) as i32
}

/// Rescale pointer-to-pet drag offset proportionally if Windows changes window physical size
/// mid-drag upon crossing onto a monitor with a different DPI scaling factor.
#[cfg(windows)]
fn rescale_drag_offset(
    offset: (i32, i32),
    old_size: (i32, i32),
    new_size: (i32, i32),
) -> (i32, i32) {
    if old_size == new_size || old_size.0 <= 0 || old_size.1 <= 0 || new_size.0 <= 0 || new_size.1 <= 0
    {
        return offset;
    }
    let rx = (offset.0 as f64) * (new_size.0 as f64) / (old_size.0 as f64);
    let ry = (offset.1 as f64) * (new_size.1 as f64) / (old_size.1 as f64);
    (
        (rx.round() as i32).clamp(0, new_size.0),
        (ry.round() as i32).clamp(0, new_size.1),
    )
}

/// Clamp active drag position smoothly across adjacent monitors without seam dead-zones
/// or teleportation, while keeping outer edges and dead-space areas strictly bounded.
#[cfg(windows)]
fn clamp_drag_position(
    raw_x: i32,
    raw_y: i32,
    w: i32,
    h: i32,
    cursor_pt: (i32, i32),
    monitors: &[MonitorInfo],
) -> (i32, i32) {
    let Some(cur_idx) = nearest_monitor_index(monitors, cursor_pt.0, cursor_pt.1) else {
        return (raw_x, raw_y);
    };
    let cur = monitors[cur_idx];

    if monitors.len() == 1 {
        return clamp_rect_pos(raw_x, raw_y, w, h, cur.work);
    }

    const SEAM_TOLERANCE: i32 = 4;
    let (cx_pt, cy_pt) = cursor_pt;

    let has_left = monitors.iter().enumerate().any(|(i, m)| {
        i != cur_idx
            && (m.bounds.right - cur.bounds.left).abs() <= SEAM_TOLERANCE
            && cy_pt >= m.bounds.top
            && cy_pt < m.bounds.bottom
    });
    let has_right = monitors.iter().enumerate().any(|(i, m)| {
        i != cur_idx
            && (m.bounds.left - cur.bounds.right).abs() <= SEAM_TOLERANCE
            && cy_pt >= m.bounds.top
            && cy_pt < m.bounds.bottom
    });
    let has_top = monitors.iter().enumerate().any(|(i, m)| {
        i != cur_idx
            && (m.bounds.bottom - cur.bounds.top).abs() <= SEAM_TOLERANCE
            && cx_pt >= m.bounds.left
            && cx_pt < m.bounds.right
    });
    let has_bottom = monitors.iter().enumerate().any(|(i, m)| {
        i != cur_idx
            && (m.bounds.top - cur.bounds.bottom).abs() <= SEAM_TOLERANCE
            && cx_pt >= m.bounds.left
            && cx_pt < m.bounds.right
    });

    let union_left = monitors.iter().map(|m| m.work.left).min().unwrap_or(cur.work.left);
    let union_top = monitors.iter().map(|m| m.work.top).min().unwrap_or(cur.work.top);
    let union_right = monitors
        .iter()
        .map(|m| m.work.right)
        .max()
        .unwrap_or(cur.work.right);
    let union_bottom = monitors
        .iter()
        .map(|m| m.work.bottom)
        .max()
        .unwrap_or(cur.work.bottom);

    let min_x = if has_left { union_left } else { cur.work.left };
    let max_x = if has_right {
        (union_right - w).max(union_left)
    } else {
        (cur.work.right - w).max(cur.work.left)
    };
    let min_y = if has_top { union_top } else { cur.work.top };
    let max_y = if has_bottom {
        (union_bottom - h).max(union_top)
    } else {
        (cur.work.bottom - h).max(cur.work.top)
    };

    (
        raw_x.clamp(min_x, max_x.max(min_x)),
        raw_y.clamp(min_y, max_y.max(min_y)),
    )
}

/// Compute DPI-accurate initial pet placement `(x, y, phys_w, phys_h)` using the target
/// monitor's own DPI and work area (avoiding primary-monitor DPI mismatch on startup).
#[cfg(windows)]
fn compute_startup_placement(
    logical_size: u32,
    saved_pos: Option<(i32, i32)>,
    monitors: &[MonitorInfo],
) -> (i32, i32, i32, i32) {
    if monitors.is_empty() {
        let work = primary_work_area();
        let dim = physical_pet_dim(logical_size, 1.0);
        let (cx, cy) = match saved_pos {
            Some((sx, sy)) => clamp_rect_pos(sx, sy, dim, dim, work),
            None => clamp_rect_pos(work.right - dim - 32, work.bottom - dim - 20, dim, dim, work),
        };
        return (cx, cy, dim, dim);
    }

    if let Some((saved_x, saved_y)) = saved_pos {
        let initial_idx = nearest_monitor_index(monitors, saved_x, saved_y).unwrap_or(0);
        let mut mon = monitors[initial_idx];
        let mut dim = physical_pet_dim(logical_size, mon.scale_factor);

        // Refine using the pet center in case saved top-left sat near a monitor edge
        if let Some(center_idx) =
            nearest_monitor_index(monitors, saved_x + dim / 2, saved_y + dim / 2)
        {
            if center_idx != initial_idx {
                mon = monitors[center_idx];
                dim = physical_pet_dim(logical_size, mon.scale_factor);
            }
        }

        let (cx, cy) = clamp_rect_pos(saved_x, saved_y, dim, dim, mon.work);
        (cx, cy, dim, dim)
    } else {
        let primary = monitors
            .iter()
            .copied()
            .find(|m| m.is_primary)
            .unwrap_or(monitors[0]);
        let dim = physical_pet_dim(logical_size, primary.scale_factor);
        let margin_x = (32.0 * primary.scale_factor).round() as i32;
        let margin_y = (20.0 * primary.scale_factor).round() as i32;
        let (cx, cy) = clamp_rect_pos(
            primary.work.right - dim - margin_x,
            primary.work.bottom - dim - margin_y,
            dim,
            dim,
            primary.work,
        );
        (cx, cy, dim, dim)
    }
}

/// Compute idempotent display/DPI/work-area reconciliation plan for the pet window.
/// Returns `needs_window_update = false` and `needs_persist = false` when already valid.
#[cfg(windows)]
fn plan_display_reconcile(
    logical_size: u32,
    current_pos: (i32, i32),
    current_size: (i32, i32),
    saved_pos: (i32, i32),
    target_work: RECT,
    target_scale: f64,
) -> ReconcilePlan {
    let target_dim = physical_pet_dim(logical_size, target_scale);
    let (target_x, target_y) = clamp_rect_pos(
        current_pos.0,
        current_pos.1,
        target_dim,
        target_dim,
        target_work,
    );
    let needs_window_update = (target_x, target_y) != current_pos
        || target_dim != current_size.0
        || target_dim != current_size.1;
    let needs_persist = (target_x, target_y) != saved_pos;
    ReconcilePlan {
        target_x,
        target_y,
        target_w: target_dim,
        target_h: target_dim,
        needs_window_update,
        needs_persist,
    }
}

/// Compute target-monitor DPI-scaled physical `(x, y, w, h)` for a popup (panel or bubble)
/// anchored cleanly above or below the pet without escaping `target_work`.
#[cfg(windows)]
fn compute_popup_placement(
    pet_rect: (i32, i32, i32, i32),
    popup_logical: (f64, f64),
    gap_logical: f64,
    target_work: RECT,
    target_scale: f64,
) -> (i32, i32, i32, i32) {
    let safe_scale = if target_scale.is_finite() && target_scale > 0.25 {
        target_scale
    } else {
        1.0
    };
    let pw = (popup_logical.0 * safe_scale).round().max(1.0) as i32;
    let ph = (popup_logical.1 * safe_scale).round().max(1.0) as i32;
    let gap = (gap_logical * safe_scale).round() as i32;

    let (pet_x, pet_y, pet_w, pet_h) = pet_rect;
    let raw_x = pet_x + (pet_w - pw) / 2;
    let above_y = pet_y - ph - gap;
    let below_y = pet_y + pet_h + gap;

    let candidate_y = if above_y >= target_work.top {
        above_y
    } else if below_y + ph <= target_work.bottom {
        below_y
    } else {
        // Choose whichever side offers more vertical clearance before clamping into work area
        let space_above = pet_y - target_work.top;
        let space_below = target_work.bottom - (pet_y + pet_h);
        if space_above >= space_below {
            above_y
        } else {
            below_y
        }
    };

    let (px, py) = clamp_rect_pos(raw_x, candidate_y, pw, ph, target_work);
    (px, py, pw, ph)
}

#[cfg(windows)]
fn find_desktop_shell_hwnd() -> Option<HWND> {
    const PROGMAN: &[u16] = &[
        b'P' as u16, b'r' as u16, b'o' as u16, b'g' as u16, b'm' as u16, b'a' as u16, b'n' as u16,
        0,
    ];
    const WORKERW: &[u16] = &[
        b'W' as u16, b'o' as u16, b'r' as u16, b'k' as u16, b'e' as u16, b'r' as u16, b'W' as u16,
        0,
    ];
    const DEFVIEW: &[u16] = &[
        b'S' as u16, b'H' as u16, b'E' as u16, b'L' as u16, b'L' as u16, b'D' as u16, b'L' as u16,
        b'L' as u16, b'_' as u16, b'D' as u16, b'e' as u16, b'f' as u16, b'V' as u16, b'i' as u16,
        b'e' as u16, b'w' as u16, 0,
    ];

    unsafe {
        let progman = FindWindowW(PROGMAN.as_ptr(), std::ptr::null());
        if !progman.is_null() {
            let def = FindWindowExW(
                progman,
                std::ptr::null_mut(),
                DEFVIEW.as_ptr(),
                std::ptr::null(),
            );
            if !def.is_null() {
                return Some(progman);
            }
        }

        // Read-only fallback lookup if Explorer hosts SHELLDLL_DefView on a top-level WorkerW
        let mut worker = FindWindowExW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            WORKERW.as_ptr(),
            std::ptr::null(),
        );
        while !worker.is_null() {
            let def = FindWindowExW(
                worker,
                std::ptr::null_mut(),
                DEFVIEW.as_ptr(),
                std::ptr::null(),
            );
            if !def.is_null() {
                return Some(worker);
            }
            worker = FindWindowExW(std::ptr::null_mut(), worker, WORKERW.as_ptr(), std::ptr::null());
        }

        if !progman.is_null() {
            Some(progman)
        } else {
            None
        }
    }
}

/// Returns true ONLY if `hwnd` is genuinely positioned BEHIND the desktop shell (`Progman`).
#[cfg(windows)]
fn is_behind_desktop_shell(hwnd: HWND) -> bool {
    let Some(desktop) = find_desktop_shell_hwnd() else {
        return false;
    };
    unsafe {
        let mut cur = GetWindow(desktop, GW_HWNDNEXT);
        let mut steps = 0;
        while !cur.is_null() && steps < 512 {
            if cur == hwnd {
                return true;
            }
            cur = GetWindow(cur, GW_HWNDNEXT);
            steps += 1;
        }
    }
    false
}

/// Returns true if `hwnd` is either behind `Progman` (occluded by wallpaper) OR accidentally
/// floating above a visible non-tool normal application window (e.g., after Win+D toggle-back).
#[cfg(windows)]
fn needs_desktop_layer_reconcile(hwnd: HWND) -> bool {
    let Some(desktop) = find_desktop_shell_hwnd() else {
        return false;
    };
    if is_behind_desktop_shell(hwnd) {
        return true;
    }
    unsafe {
        let mut cur = GetWindow(hwnd, GW_HWNDNEXT);
        let mut steps = 0;
        while !cur.is_null() && cur != desktop && steps < 256 {
            if IsWindowVisible(cur) != 0 && IsIconic(cur) == 0 {
                let ex = GetWindowLongPtrW(cur, GWL_EXSTYLE);
                if (ex & (WS_EX_TOOLWINDOW as isize)) == 0 {
                    return true;
                }
            }
            cur = GetWindow(cur, GW_HWNDNEXT);
            steps += 1;
        }
    }
    false
}

/// Returns `Some(insert_after)` placing `self_hwnd` strictly BELOW the lowest visible
/// non-topmost normal application window and strictly ABOVE the desktop shell (`Progman`),
/// or `None` if already in that exact slot.
#[cfg(windows)]
fn desktop_layer_insert_after(self_hwnd: HWND) -> Option<HWND> {
    let Some(desktop) = find_desktop_shell_hwnd() else {
        return Some(HWND_BOTTOM);
    };
    unsafe {
        let mut cursor = GetWindow(desktop, GW_HWNDPREV);
        let mut seen_self_directly_above_desktop = cursor == self_hwnd;
        let mut steps = 0;
        while !cursor.is_null() && steps < 512 {
            if cursor != self_hwnd {
                let ex = GetWindowLongPtrW(cursor, GWL_EXSTYLE);
                let is_topmost = (ex & (WS_EX_TOPMOST as isize)) != 0;
                if is_topmost {
                    // Reached the topmost band with no visible normal app between topmost & Progman
                    // (e.g., right after Win+D): place right at top of non-topmost band, above Progman.
                    return if seen_self_directly_above_desktop {
                        None
                    } else {
                        Some(HWND_TOP)
                    };
                }
                if IsWindowVisible(cursor) != 0 && IsIconic(cursor) == 0 {
                    // Found lowest visible non-topmost window above Progman.
                    // If self_hwnd sits immediately below it (and above Progman), no change needed.
                    if GetWindow(cursor, GW_HWNDNEXT) == self_hwnd && !is_behind_desktop_shell(self_hwnd) {
                        return None;
                    }
                    return Some(cursor);
                }
            } else {
                seen_self_directly_above_desktop = true;
            }
            cursor = GetWindow(cursor, GW_HWNDPREV);
            steps += 1;
        }
        if seen_self_directly_above_desktop {
            None
        } else {
            Some(HWND_TOP)
        }
    }
}

#[cfg(windows)]
fn place_on_desktop_layer(hwnd: HWND, extra_flags: u32) {
    unsafe {
        match desktop_layer_insert_after(hwnd) {
            Some(insert_after) => {
                INTERNAL_ZORDER.store(true, Ordering::SeqCst);
                if insert_after == HWND_TOP {
                    // During Windows 11 Show Desktop (Win+D), Progman is raised above the
                    // normal HWND_TOP band. Promoting briefly to HWND_TOPMOST then immediately
                    // demoting to HWND_NOTOPMOST places the window strictly ABOVE raised Progman,
                    // strictly BELOW the taskbar, and strictly NON-TOPMOST (WS_EX_TOPMOST = 0).
                    RAISED_FOR_SHOW_DESKTOP.store(true, Ordering::SeqCst);
                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    SetWindowPos(
                        hwnd,
                        HWND_NOTOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | extra_flags,
                    );
                } else {
                    RAISED_FOR_SHOW_DESKTOP.store(false, Ordering::SeqCst);
                    SetWindowPos(
                        hwnd,
                        insert_after,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | extra_flags,
                    );
                }
                INTERNAL_ZORDER.store(false, Ordering::SeqCst);
            }
            None => {
                if extra_flags != 0 {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | extra_flags,
                    );
                }
            }
        }
    }
}

#[cfg(windows)]
fn is_desktop_shell_class_name(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "SHELLDLL_DefView"
            | "SysListView32"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
    )
}

#[cfg(windows)]
fn is_desktop_shell_class(hwnd: HWND) -> bool {
    if hwnd.is_null() {
        return false;
    }
    if find_desktop_shell_hwnd() == Some(hwnd) {
        return true;
    }
    let mut buf = [0u16; 48];
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len <= 0 {
        return false;
    }
    let class_name = String::from_utf16_lossy(&buf[..len as usize]);
    is_desktop_shell_class_name(&class_name)
}

#[cfg(windows)]
fn reconcile_desktop_windows(app: &AppHandle) {
    let pet_visible = {
        let s = app.state::<AppState>();
        let g = s.0.lock().expect("AppState mutex poisoned");
        g.pet_visible
    };

    if !pet_visible {
        return;
    }

    if let Some(pet) = app.get_webview_window("pet") {
        if let Some(pet_hwnd) = hwnd_of(&pet) {
            if needs_desktop_layer_reconcile(pet_hwnd) {
                place_on_desktop_layer(pet_hwnd, 0);
            }
        }
    }
    if let Some(bubble) = app.get_webview_window("bubble") {
        if let Some(bub_hwnd) = hwnd_of(&bubble) {
            if needs_desktop_layer_reconcile(bub_hwnd) {
                place_on_desktop_layer(bub_hwnd, 0);
            }
        }
    }
}

#[cfg(windows)]
fn schedule_one_shot_reconcile(app: &AppHandle) {
    if let Some(pet) = app.get_webview_window("pet") {
        if let Some(pet_hwnd) = hwnd_of(&pet) {
            unsafe {
                // Single coalesced one-shot timer on the pet HWND; killed immediately in WM_TIMER.
                SetTimer(
                    pet_hwnd,
                    PET_WIND_RECONCILE_TIMER_ID,
                    PET_WIND_RECONCILE_DELAY_MS,
                    None,
                );
            }
        }
    }
}

/// Event-driven Win32 callback (zero polling, 0% idle CPU).
/// Fires only on foreground change or window restore/minimize completion; restores Piku ONLY
/// if Explorer/Windows actually pushed `Progman` in front of `pet` (e.g., desktop click or Win+D).
#[cfg(windows)]
unsafe extern "system" fn desktop_win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if HOOK_REENTRANT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    if let Some(app) = APP_HANDLE_GLOBAL.get() {
        reconcile_desktop_windows(app);

        // On Windows 11, Win+D emits EVENT_SYSTEM_FOREGROUND for Progman/WorkerW ~100-200ms
        // before DWM finishes raising Progman. Schedule a single coalesced one-shot check
        // (no polling) on desktop-shell foreground events, and once on the next foreground
        // switch after a Show Desktop raise so restored apps always cover Piku.
        if event == EVENT_SYSTEM_FOREGROUND {
            if is_desktop_shell_class(hwnd) {
                schedule_one_shot_reconcile(app);
            } else if RAISED_FOR_SHOW_DESKTOP.swap(false, Ordering::SeqCst) {
                schedule_one_shot_reconcile(app);
            }
        }
    }

    HOOK_REENTRANT.store(false, Ordering::SeqCst);
}

#[cfg(windows)]
fn install_desktop_event_hooks() {
    let store = HOOK_HANDLES.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = store.lock().expect("HOOK_HANDLES mutex poisoned");
    if !guard.is_empty() {
        return; // Register once only
    }

    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    for ev in [
        EVENT_SYSTEM_FOREGROUND,
        EVENT_SYSTEM_MINIMIZESTART,
        EVENT_SYSTEM_MINIMIZEEND,
    ] {
        let h = unsafe {
            SetWinEventHook(
                ev,
                ev,
                std::ptr::null_mut(),
                Some(desktop_win_event_proc),
                0,
                0,
                flags,
            )
        };
        if !h.is_null() {
            guard.push(h as isize);
        }
    }
}

#[cfg(windows)]
fn schedule_display_reconcile(hwnd: HWND) {
    if INTERNAL_DISPLAY_RECONCILE.load(Ordering::SeqCst) {
        return;
    }
    unsafe {
        // Coalesced one-shot timer on the pet HWND; killed immediately when WM_TIMER fires.
        SetTimer(
            hwnd,
            PET_DISPLAY_RECONCILE_TIMER_ID,
            PET_DISPLAY_RECONCILE_DELAY_MS,
            None,
        );
    }
}

/// Safely reconciles Piku's physical size, work-area bounds, popups, and desktop Z-order
/// after WM_DISPLAYCHANGE, WM_DPICHANGED, or SPI_SETWORKAREA.
/// Idempotent: performs no SetWindowPos or SQLite write when already valid.
#[cfg(windows)]
fn reconcile_pet_after_display_change(app: &AppHandle) {
    if INTERNAL_DISPLAY_RECONCILE
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Some(pet) = app.get_webview_window("pet") else {
            return;
        };
        let (is_dragging, logical_size, saved_pos, pet_visible) = {
            let s = app.state::<AppState>();
            let g = s.0.lock().expect("AppState mutex poisoned");
            (
                g.drag_offset.is_some(),
                g.pet_size.clamp(100, 160),
                (g.pet_x, g.pet_y),
                g.pet_visible,
            )
        };
        if is_dragging {
            return;
        }

        let pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
        let size = pet
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(logical_size, logical_size));
        let monitors = collect_monitors(&pet);
        let center_x = pos.x + (size.width as i32) / 2;
        let center_y = pos.y + (size.height as i32) / 2;
        let target_mon = nearest_monitor_index(&monitors, center_x, center_y)
            .map(|i| monitors[i])
            .unwrap_or(MonitorInfo {
                bounds: primary_work_area(),
                work: primary_work_area(),
                scale_factor: pet.scale_factor().unwrap_or(1.0),
                is_primary: true,
            });

        let plan = plan_display_reconcile(
            logical_size,
            (pos.x, pos.y),
            (size.width as i32, size.height as i32),
            saved_pos,
            target_mon.work,
            target_mon.scale_factor,
        );

        if plan.needs_window_update {
            if let Some(hwnd) = hwnd_of(&pet) {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        plan.target_x,
                        plan.target_y,
                        plan.target_w,
                        plan.target_h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }

        if plan.needs_persist {
            let state = app.state::<AppState>();
            {
                let mut g = state.0.lock().expect("AppState mutex poisoned");
                g.pet_x = plan.target_x;
                g.pet_y = plan.target_y;
                g.has_saved_position = true;
            }
            state.persist(app);
        }

        if let Some(bubble) = app.get_webview_window("bubble") {
            position_bubble_above_pet(app, &bubble);
        }
        if let Some(panel) = app.get_webview_window("panel") {
            position_panel_near_pet(app, &panel);
        }
        if pet_visible {
            reconcile_desktop_windows(app);
        }
    }));

    INTERNAL_DISPLAY_RECONCILE.store(false, Ordering::SeqCst);
    if let Err(err) = result {
        std::panic::resume_unwind(err);
    }
}

pub fn cleanup_hooks() {
    #[cfg(windows)]
    {
        if let Some(app) = APP_HANDLE_GLOBAL.get() {
            if let Some(pet) = app.get_webview_window("pet") {
                if let Some(pet_hwnd) = hwnd_of(&pet) {
                    unsafe {
                        KillTimer(pet_hwnd, PET_WIND_RECONCILE_TIMER_ID);
                        KillTimer(pet_hwnd, PET_DISPLAY_RECONCILE_TIMER_ID);
                    }
                }
            }
        }
        if let Some(store) = HOOK_HANDLES.get() {
            let mut guard = store.lock().expect("HOOK_HANDLES mutex poisoned");
            for raw in guard.drain(..) {
                unsafe {
                    UnhookWinEvent(raw as HWINEVENTHOOK);
                }
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn pet_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid: usize,
    _ref_data: usize,
) -> LRESULT {
    match msg {
        // Never steal focus when Piku is clicked, hovered, dragged, or shown
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        // Coalesced deferred recovery when displays, resolution, DPI, or taskbar work area change
        WM_DISPLAYCHANGE => {
            schedule_display_reconcile(hwnd);
        }
        WM_DPICHANGED => {
            schedule_display_reconcile(hwnd);
        }
        WM_SETTINGCHANGE => {
            if wparam == SPI_SETWORKAREA as usize {
                schedule_display_reconcile(hwnd);
            }
        }
        // One-shot deferred Z-order reconcile or display-change reconcile
        WM_TIMER => {
            if wparam == PET_WIND_RECONCILE_TIMER_ID {
                KillTimer(hwnd, PET_WIND_RECONCILE_TIMER_ID);
                if HOOK_REENTRANT
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    if let Some(app) = APP_HANDLE_GLOBAL.get() {
                        reconcile_desktop_windows(app);
                    }
                    HOOK_REENTRANT.store(false, Ordering::SeqCst);
                }
                return 0;
            }
            if wparam == PET_DISPLAY_RECONCILE_TIMER_ID {
                KillTimer(hwnd, PET_DISPLAY_RECONCILE_TIMER_ID);
                if let Some(app) = APP_HANDLE_GLOBAL.get() {
                    let handle = app.clone();
                    let _ = app.run_on_main_thread(move || {
                        reconcile_pet_after_display_change(&handle);
                    });
                }
                return 0;
            }
        }
        // Avoid Z-order manipulation during pure position changes/drags (when SWP_NOZORDER is set)
        // or during our own controlled place_on_desktop_layer() transitions.
        WM_WINDOWPOSCHANGING => {
            if lparam != 0 {
                let wp = &mut *(lparam as *mut WINDOWPOS);
                wp.flags |= SWP_NOACTIVATE;
                if (wp.flags & SWP_NOZORDER) == 0 && !INTERNAL_ZORDER.load(Ordering::SeqCst) {
                    match desktop_layer_insert_after(hwnd) {
                        Some(insert_after) => {
                            wp.hwndInsertAfter = insert_after;
                        }
                        None => {
                            wp.flags |= SWP_NOZORDER;
                        }
                    }
                }
            }
        }
        // Approve Windows logoff/shutdown immediately without blocking
        WM_QUERYENDSESSION => return 1,
        // Flush pending SQLite usage and release hooks when Windows ends the user session
        WM_ENDSESSION => {
            if wparam != 0 {
                if let Some(app) = APP_HANDLE_GLOBAL.get() {
                    app.state::<AppState>().shutdown_cleanly(app);
                }
            }
            return 0;
        }
        WM_EXITSIZEMOVE => {
            if let Some(app) = APP_HANDLE_GLOBAL.get() {
                let handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    clamp_and_persist_pet(&handle);
                });
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(windows)]
unsafe extern "system" fn bubble_subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uid: usize,
    _ref_data: usize,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return MA_NOACTIVATE as LRESULT,
        WM_WINDOWPOSCHANGING => {
            if lparam != 0 {
                let wp = &mut *(lparam as *mut WINDOWPOS);
                wp.flags |= SWP_NOACTIVATE;
                if (wp.flags & SWP_NOZORDER) == 0 && !INTERNAL_ZORDER.load(Ordering::SeqCst) {
                    match desktop_layer_insert_after(hwnd) {
                        Some(insert_after) => {
                            wp.hwndInsertAfter = insert_after;
                        }
                        None => {
                            wp.flags |= SWP_NOZORDER;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[cfg(windows)]
fn apply_tool_noactivate_styles(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        let cleaned_style = style & !((WS_MINIMIZEBOX | WS_MAXIMIZEBOX) as isize);
        SetWindowLongPtrW(hwnd, GWL_STYLE, cleaned_style);

        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let updated = (ex_style | (WS_EX_TOOLWINDOW as isize) | (WS_EX_NOACTIVATE as isize))
            & !(WS_EX_APPWINDOW as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated);
    }
    place_on_desktop_layer(hwnd, SWP_FRAMECHANGED);
}

#[cfg(windows)]
fn apply_tool_style_only(hwnd: HWND) {
    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let updated = (ex_style | (WS_EX_TOOLWINDOW as isize)) & !(WS_EX_APPWINDOW as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, updated);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn init_pet_window(app: &AppHandle) {
    #[cfg(windows)]
    let _ = APP_HANDLE_GLOBAL.set(app.clone());

    if let Some(pet) = app.get_webview_window("pet") {
        let saved_size = {
            let s = app.state::<AppState>();
            let g = s.0.lock().expect("AppState mutex poisoned");
            g.pet_size.clamp(100, 160)
        };

        #[cfg(windows)]
        {
            INTERNAL_DISPLAY_RECONCILE.store(true, Ordering::SeqCst);
            let (x, y, phys_w, phys_h, should_show) =
                compute_initial_pet_placement(app, &pet, saved_size);

            if let Some(hwnd) = hwnd_of(&pet) {
                unsafe {
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        phys_w,
                        phys_h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                apply_tool_noactivate_styles(hwnd);
                unsafe {
                    SetWindowSubclass(hwnd, Some(pet_subclass_proc), PET_SUBCLASS_ID, 0);
                }
            } else {
                let _ = pet.set_size(LogicalSize::new(saved_size as f64, saved_size as f64));
                let _ = pet.set_position(PhysicalPosition::new(x, y));
            }

            if should_show {
                show_window_noactivate(&pet);
            }
            INTERNAL_DISPLAY_RECONCILE.store(false, Ordering::SeqCst);
        }

        #[cfg(not(windows))]
        {
            let _ = pet.set_size(LogicalSize::new(saved_size as f64, saved_size as f64));
            let (x, y, _w, _h, should_show) =
                compute_initial_pet_placement(app, &pet, saved_size);
            let _ = pet.set_position(PhysicalPosition::new(x, y));
            if should_show {
                show_window_noactivate(&pet);
            }
        }
    }

    #[cfg(windows)]
    install_desktop_event_hooks();
}

/// Resizes the desktop pet window cleanly without stealing focus or mutating Z-order.
/// Keeps Piku anchored around his horizontal center and bottom baseline within monitor work area.
pub fn apply_pet_size(app: &AppHandle, logical_size: u32) {
    let clamped = logical_size.clamp(100, 160);
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    let old_pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
    let old_size = pet
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(clamped, clamped));
    let scale = pet.scale_factor().unwrap_or(1.0);
    let new_w = ((clamped as f64) * scale).round() as i32;
    let new_h = new_w;

    // Anchor horizontally at center and vertically at feet so Piku stays grounded in place
    let raw_x = old_pos.x + (old_size.width as i32 - new_w) / 2;
    let raw_y = old_pos.y + (old_size.height as i32 - new_h);

    #[cfg(windows)]
    let (cx, cy) = {
        let work = if let Some(hwnd) = hwnd_of(&pet) {
            work_area_for_window(hwnd)
        } else {
            work_area_for_point(
                old_pos.x + old_size.width as i32 / 2,
                old_pos.y + old_size.height as i32 / 2,
            )
        };
        clamp_rect_pos(raw_x, raw_y, new_w, new_h, work)
    };
    #[cfg(not(windows))]
    let (cx, cy) = (raw_x, raw_y);

    #[cfg(windows)]
    if let Some(hwnd) = hwnd_of(&pet) {
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                cx,
                cy,
                new_w,
                new_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    #[cfg(not(windows))]
    {
        let _ = pet.set_size(LogicalSize::new(clamped as f64, clamped as f64));
        let _ = pet.set_position(PhysicalPosition::new(cx, cy));
    }

    let state = app.state::<AppState>();
    {
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.pet_x = cx;
        g.pet_y = cy;
        g.pet_size = clamped;
        g.has_saved_position = true;
    }
    state.persist(app);

    if let Some(bubble) = app.get_webview_window("bubble") {
        position_bubble_above_pet(app, &bubble);
    }
    if let Some(panel) = app.get_webview_window("panel") {
        position_panel_near_pet(app, &panel);
    }
}

pub fn show_window_noactivate(window: &WebviewWindow) {
    #[cfg(windows)]
    if let Some(hwnd) = hwnd_of(window) {
        place_on_desktop_layer(hwnd, SWP_SHOWWINDOW);
        return;
    }
    let _ = window.show();
}

pub fn hide_window_noactivate(window: &WebviewWindow) {
    #[cfg(windows)]
    if let Some(hwnd) = hwnd_of(window) {
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_HIDEWINDOW,
            );
        }
        return;
    }
    let _ = window.hide();
}

// ---------------------------------------------------------------------------
// Explicit Cursor-Based Non-Activating Drag (Bug 1 fix)
// ---------------------------------------------------------------------------

pub fn begin_pet_drag(app: &AppHandle) {
    close_bubble(app);
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    #[cfg(windows)]
    {
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } != 0 {
            let pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
            let size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));
            let state = app.state::<AppState>();
            let mut g = state.0.lock().expect("AppState mutex poisoned");
            g.drag_offset = Some((pt.x - pos.x, pt.y - pos.y));
            g.drag_window_size = Some((size.width as i32, size.height as i32));
        }
    }
}

pub fn move_pet_drag(app: &AppHandle) {
    let (offset, prev_drag_size) = {
        let state = app.state::<AppState>();
        let g = state.0.lock().expect("AppState mutex poisoned");
        (g.drag_offset, g.drag_window_size)
    };
    let Some(mut cur_offset) = offset else {
        return;
    };
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    #[cfg(windows)]
    {
        let mut pt = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut pt) } == 0 {
            return;
        }
        let size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));
        let w = size.width as i32;
        let h = size.height as i32;

        // If Windows resized the window mid-drag due to crossing a DPI boundary,
        // rescale drag_offset proportionally so the cursor stays anchored to Piku.
        if let Some(old_sz) = prev_drag_size {
            if old_sz != (w, h) {
                cur_offset = rescale_drag_offset(cur_offset, old_sz, (w, h));
                let state = app.state::<AppState>();
                let mut g = state.0.lock().expect("AppState mutex poisoned");
                g.drag_offset = Some(cur_offset);
                g.drag_window_size = Some((w, h));
            }
        }

        let monitors = collect_monitors(&pet);
        let (cx, cy) = clamp_drag_position(
            pt.x - cur_offset.0,
            pt.y - cur_offset.1,
            w,
            h,
            (pt.x, pt.y),
            &monitors,
        );

        if let Some(hwnd) = hwnd_of(&pet) {
            unsafe {
                // Pure position update: SWP_NOZORDER + SWP_NOACTIVATE avoids Z-order churn & focus steal
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    cx,
                    cy,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            let state = app.state::<AppState>();
            let mut g = state.0.lock().expect("AppState mutex poisoned");
            g.pet_x = cx;
            g.pet_y = cy;
            g.has_saved_position = true;
        }
    }
}

pub fn end_pet_drag(app: &AppHandle) -> (i32, i32) {
    {
        let state = app.state::<AppState>();
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.drag_offset = None;
        g.drag_window_size = None;
    }
    clamp_and_persist_pet(app)
}

// ---------------------------------------------------------------------------
// Lazy Greeting Panel & Ambient Speech Bubble (Bug 2 fix: called from async commands)
// ---------------------------------------------------------------------------

pub fn open_or_focus_panel(app: &AppHandle) {
    open_panel_view(app, "greeting");
}

pub fn open_panel_view(app: &AppHandle, view: &str) {
    close_bubble(app);
    app.state::<AppState>().mark_panel_opened();

    let safe_view = match view {
        "today" | "history" | "preferences" => view,
        _ => "greeting",
    };

    let (init_w, init_h) = match safe_view {
        "preferences" => (368.0, 428.0),
        "today" => (380.0, 472.0),
        "history" => (412.0, 576.0),
        _ => (348.0, 268.0),
    };

    if let Some(panel) = app.get_webview_window("panel") {
        {
            let state = app.state::<AppState>();
            let mut g = state.0.lock().expect("AppState mutex poisoned");
            if g.panel_logical_size.is_none() {
                g.panel_logical_size = Some((init_w, init_h));
            }
        }
        position_panel_near_pet(app, &panel);
        let _ = panel.show();
        let _ = panel.set_focus();
        let _ = app.emit("panel:navigate", serde_json::json!({ "view": safe_view }));
        let _ = app.emit("panel:opened", ());
        return;
    }

    {
        let state = app.state::<AppState>();
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.panel_logical_size = Some((init_w, init_h));
    }

    let url = if safe_view == "greeting" {
        "index.html?window=panel".to_string()
    } else {
        format!("index.html?window=panel&view={safe_view}")
    };

    let builder = WebviewWindowBuilder::new(app, "panel", WebviewUrl::App(url.into()))
        .title("Piku Greeting")
        .inner_size(init_w, init_h)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .visible(false);

    if let Ok(panel) = builder.build() {
        #[cfg(windows)]
        if let Some(hwnd) = hwnd_of(&panel) {
            apply_tool_style_only(hwnd);
        }
        position_panel_near_pet(app, &panel);
        app.state::<AppState>().mark_panel_opened();
        let _ = panel.show();
        let _ = panel.set_focus();
    }
}

pub fn resize_panel(app: &AppHandle, width: f64, height: f64) {
    if let Some(panel) = app.get_webview_window("panel") {
        let w = width.clamp(300.0, 440.0);
        let h = height.clamp(220.0, 620.0);
        {
            let state = app.state::<AppState>();
            let mut g = state.0.lock().expect("AppState mutex poisoned");
            g.panel_logical_size = Some((w, h));
        }
        app.state::<AppState>().mark_panel_opened();
        #[cfg(not(windows))]
        let _ = panel.set_size(LogicalSize::new(w, h));
        position_panel_near_pet(app, &panel);
    }
}

pub fn close_panel(app: &AppHandle) {
    {
        let state = app.state::<AppState>();
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.panel_logical_size = None;
    }
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.destroy();
    }
}

pub fn open_bubble(app: &AppHandle, text: &str) {
    close_bubble(app);

    let encoded = percent_encode_query(text);
    let url = format!("index.html?window=bubble&text={encoded}");
    let builder = WebviewWindowBuilder::new(app, "bubble", WebviewUrl::App(url.into()))
        .title("Piku Bubble")
        .inner_size(BUBBLE_LOGICAL_WIDTH, BUBBLE_LOGICAL_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .skip_taskbar(true)
        .focused(false)
        .visible(false);

    if let Ok(bubble) = builder.build() {
        let _ = bubble.set_ignore_cursor_events(true);
        #[cfg(windows)]
        if let Some(hwnd) = hwnd_of(&bubble) {
            apply_tool_noactivate_styles(hwnd);
            unsafe {
                SetWindowSubclass(hwnd, Some(bubble_subclass_proc), PET_SUBCLASS_ID + 1, 0);
            }
        }
        position_bubble_above_pet(app, &bubble);
        show_window_noactivate(&bubble);
    }
}

pub fn close_bubble(app: &AppHandle) {
    if let Some(bubble) = app.get_webview_window("bubble") {
        let _ = bubble.destroy();
    }
}

fn percent_encode_query(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 2);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn compute_initial_pet_placement(
    app: &AppHandle,
    pet: &WebviewWindow,
    logical_size: u32,
) -> (i32, i32, i32, i32, bool) {
    let state = app.state::<AppState>();
    let (has_saved, saved_x, saved_y, visible) = {
        let g = state.0.lock().expect("AppState mutex poisoned");
        (g.has_saved_position, g.pet_x, g.pet_y, g.pet_visible)
    };

    #[cfg(windows)]
    let (cx, cy, phys_w, phys_h) = {
        let monitors = collect_monitors(pet);
        let saved = if has_saved {
            Some((saved_x, saved_y))
        } else {
            None
        };
        compute_startup_placement(logical_size, saved, &monitors)
    };

    #[cfg(not(windows))]
    let (cx, cy, phys_w, phys_h) = {
        let _ = pet;
        (saved_x, saved_y, logical_size as i32, logical_size as i32)
    };

    let should_persist = !has_saved || cx != saved_x || cy != saved_y;
    {
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.pet_x = cx;
        g.pet_y = cy;
        g.has_saved_position = true;
    }
    if should_persist {
        state.persist(app);
    }
    (cx, cy, phys_w, phys_h, visible)
}

pub fn clamp_and_persist_pet(app: &AppHandle) -> (i32, i32) {
    let Some(pet) = app.get_webview_window("pet") else {
        return (0, 0);
    };
    let pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
    let size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));
    let logical_size = {
        let state = app.state::<AppState>();
        let g = state.0.lock().expect("AppState mutex poisoned");
        g.pet_size.clamp(100, 160)
    };

    #[cfg(windows)]
    let (cx, cy, target_w, target_h) = {
        let monitors = collect_monitors(&pet);
        let center_x = pos.x + (size.width as i32) / 2;
        let center_y = pos.y + (size.height as i32) / 2;
        let mon = nearest_monitor_index(&monitors, center_x, center_y)
            .map(|i| monitors[i])
            .unwrap_or(MonitorInfo {
                bounds: primary_work_area(),
                work: primary_work_area(),
                scale_factor: pet.scale_factor().unwrap_or(1.0),
                is_primary: true,
            });
        let dim = physical_pet_dim(logical_size, mon.scale_factor);
        let (x, y) = clamp_rect_pos(pos.x, pos.y, dim, dim, mon.work);
        (x, y, dim, dim)
    };
    #[cfg(not(windows))]
    let (cx, cy, target_w, target_h) = (pos.x, pos.y, size.width as i32, size.height as i32);

    let needs_update =
        cx != pos.x || cy != pos.y || target_w != size.width as i32 || target_h != size.height as i32;
    if needs_update {
        #[cfg(windows)]
        if let Some(hwnd) = hwnd_of(&pet) {
            INTERNAL_DISPLAY_RECONCILE.store(true, Ordering::SeqCst);
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    cx,
                    cy,
                    target_w,
                    target_h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            INTERNAL_DISPLAY_RECONCILE.store(false, Ordering::SeqCst);
        }
        #[cfg(not(windows))]
        let _ = pet.set_position(PhysicalPosition::new(cx, cy));
    }

    #[cfg(windows)]
    if let Some(hwnd) = hwnd_of(&pet) {
        if is_behind_desktop_shell(hwnd) {
            place_on_desktop_layer(hwnd, 0);
        }
    }

    let state = app.state::<AppState>();
    let should_persist = {
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        let changed = !g.has_saved_position || g.pet_x != cx || g.pet_y != cy;
        g.pet_x = cx;
        g.pet_y = cy;
        g.has_saved_position = true;
        changed
    };
    if should_persist {
        state.persist(app);
    }

    if let Some(bubble) = app.get_webview_window("bubble") {
        position_bubble_above_pet(app, &bubble);
    }
    (cx, cy)
}

#[cfg(windows)]
fn target_monitor_for_pet(pet: &WebviewWindow) -> MonitorInfo {
    let pet_pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
    let pet_size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));
    let monitors = collect_monitors(pet);
    let center_x = pet_pos.x + (pet_size.width as i32) / 2;
    let center_y = pet_pos.y + (pet_size.height as i32) / 2;
    nearest_monitor_index(&monitors, center_x, center_y)
        .map(|i| monitors[i])
        .unwrap_or(MonitorInfo {
            bounds: primary_work_area(),
            work: primary_work_area(),
            scale_factor: pet.scale_factor().unwrap_or(1.0),
            is_primary: true,
        })
}

fn position_bubble_above_pet(app: &AppHandle, bubble: &WebviewWindow) {
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    let pet_pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
    let pet_size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));

    #[cfg(windows)]
    {
        let target_mon = target_monitor_for_pet(&pet);
        let (bx, by, bw, bh) = compute_popup_placement(
            (
                pet_pos.x,
                pet_pos.y,
                pet_size.width as i32,
                pet_size.height as i32,
            ),
            (BUBBLE_LOGICAL_WIDTH, BUBBLE_LOGICAL_HEIGHT),
            0.0,
            target_mon.work,
            target_mon.scale_factor,
        );
        if let Some(hwnd) = hwnd_of(bubble) {
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    bx,
                    by,
                    bw,
                    bh,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            return;
        }
        let _ = bubble.set_position(PhysicalPosition::new(bx, by));
    }

    #[cfg(not(windows))]
    {
        let bub_size = bubble
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(236, 68));
        let raw_x = pet_pos.x + (pet_size.width as i32 - bub_size.width as i32) / 2;
        let raw_y = pet_pos.y - bub_size.height as i32;
        let _ = bubble.set_position(PhysicalPosition::new(raw_x, raw_y));
    }
}

fn position_panel_near_pet(app: &AppHandle, panel: &WebviewWindow) {
    let Some(pet) = app.get_webview_window("pet") else {
        return;
    };

    let pet_pos = pet.outer_position().unwrap_or(PhysicalPosition::new(0, 0));
    let pet_size = pet.outer_size().unwrap_or(tauri::PhysicalSize::new(148, 148));
    let logical_size = {
        let state = app.state::<AppState>();
        let g = state.0.lock().expect("AppState mutex poisoned");
        g.panel_logical_size
    }
    .unwrap_or_else(|| {
        let cur_scale = panel.scale_factor().unwrap_or(1.0).max(0.5);
        let cur_size = panel
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(348, 268));
        (
            (cur_size.width as f64) / cur_scale,
            (cur_size.height as f64) / cur_scale,
        )
    });

    #[cfg(windows)]
    {
        let target_mon = target_monitor_for_pet(&pet);
        let (px, py, pw, ph) = compute_popup_placement(
            (
                pet_pos.x,
                pet_pos.y,
                pet_size.width as i32,
                pet_size.height as i32,
            ),
            logical_size,
            PANEL_GAP_LOGICAL,
            target_mon.work,
            target_mon.scale_factor,
        );
        if let Some(hwnd) = hwnd_of(panel) {
            unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    px,
                    py,
                    pw,
                    ph,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            return;
        }
        let _ = panel.set_position(PhysicalPosition::new(px, py));
    }

    #[cfg(not(windows))]
    {
        let pan_size = panel
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(348, 268));
        let raw_x = pet_pos.x + (pet_size.width as i32 - pan_size.width as i32) / 2;
        let above_y = pet_pos.y - pan_size.height as i32 - (PANEL_GAP_LOGICAL as i32);
        let _ = panel.set_position(PhysicalPosition::new(raw_x, above_y));
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    fn mon(bounds: RECT, work: RECT, scale_factor: f64, is_primary: bool) -> MonitorInfo {
        MonitorInfo {
            bounds,
            work,
            scale_factor,
            is_primary,
        }
    }

    #[test]
    fn clamps_within_single_and_negative_coordinate_monitors() {
        // Left secondary monitor with negative X coordinates
        let left_work = rect(-1920, 0, 0, 1040);
        assert_eq!(clamp_rect_pos(-1980, -40, 200, 200, left_work), (-1920, 0));
        assert_eq!(clamp_rect_pos(-100, 950, 200, 200, left_work), (-200, 840));

        // Top secondary monitor with negative Y coordinates
        let top_work = rect(0, -1080, 1920, -60);
        assert_eq!(clamp_rect_pos(500, -120, 200, 200, top_work), (500, -260));
        assert_eq!(clamp_rect_pos(-50, -1150, 200, 200, top_work), (0, -1080));
    }

    #[test]
    fn smooth_cross_monitor_drag_left_right_and_above_below_without_seam_teleport() {
        // Left/Right arrangement (including negative left monitor)
        let left_mon = mon(rect(-1920, 0, 0, 1080), rect(-1920, 0, 0, 1040), 1.0, false);
        let right_mon = mon(rect(0, 0, 1920, 1080), rect(0, 0, 1920, 1020), 1.25, true);
        let lr = [left_mon, right_mon];

        // Pet width 200, grabbed at center (off_x = 100), moving 1px across x = 0 seam
        let p_before = clamp_drag_position(-101, 400, 200, 200, (-1, 500), &lr);
        let p_after = clamp_drag_position(-100, 400, 200, 200, (0, 500), &lr);
        assert_eq!(p_before, (-101, 400), "must straddle left->right seam cleanly");
        assert_eq!(p_after, (-100, 400), "1px cursor move across seam must move pet by exactly 1px");

        // Outer far-left edge still clamped cleanly
        let far_left = clamp_drag_position(-2100, 400, 200, 200, (-1910, 500), &lr);
        assert_eq!(far_left.0, -1920);

        // Above/Below arrangement where top monitor has a 60px bottom taskbar gap in rcWork
        let top_mon = mon(rect(0, -1080, 1920, 0), rect(0, -1080, 1920, -60), 1.0, false);
        let bot_mon = mon(rect(0, 0, 1920, 1080), rect(0, 0, 1920, 1020), 1.0, true);
        let ab = [top_mon, bot_mon];

        let v_before = clamp_drag_position(500, -101, 200, 200, (600, -1), &ab);
        let v_after = clamp_drag_position(500, -100, 200, 200, (600, 0), &ab);
        assert_eq!(v_before, (500, -101), "must straddle top->bottom seam even with taskbar gap");
        assert_eq!(v_after, (500, -100), "1px vertical seam cross must move pet by exactly 1px");

        // Single monitor keeps tight rcWork clamping
        let single = [right_mon];
        assert_eq!(
            clamp_drag_position(1850, 950, 200, 200, (1900, 1000), &single),
            (1720, 820)
        );
    }

    #[test]
    fn rescales_drag_offset_proportionally_on_mid_drag_dpi_change() {
        // Crossing from 125% (200x200) to 100% (160x160) while holding exact center (100, 100)
        assert_eq!(rescale_drag_offset((100, 100), (200, 200), (160, 160)), (80, 80));
        // Crossing from 100% (160x160) to 150% (240x240)
        assert_eq!(rescale_drag_offset((40, 120), (160, 160), (240, 240)), (60, 180));
        // Unchanged size is a no-op
        assert_eq!(rescale_drag_offset((73, 91), (200, 200), (200, 200)), (73, 91));
    }

    #[test]
    fn startup_positioning_uses_target_monitor_dpi_and_recovers_disconnected_monitor() {
        let primary_150 = mon(rect(0, 0, 1920, 1080), rect(0, 0, 1920, 1020), 1.5, true);
        let secondary_100 = mon(
            rect(1920, 0, 3840, 1080),
            rect(1920, 0, 3840, 1020),
            1.0,
            false,
        );
        let dual = [primary_150, secondary_100];

        // Saved at bottom-right of secondary 100% monitor (Large 160px -> 160x160 physical)
        let (x, y, w, h) = compute_startup_placement(160, Some((3680, 860)), &dual);
        assert_eq!((w, h), (160, 160), "must use secondary monitor 100% DPI, not primary 150%");
        assert_eq!((x, y), (3680, 860), "must preserve exact saved corner with 0px drift");

        // Reverse: Primary 100%, Secondary 150% (Large 160px -> 240x240 physical)
        let primary_100 = mon(rect(0, 0, 1920, 1080), rect(0, 0, 1920, 1020), 1.0, true);
        let secondary_150 = mon(
            rect(1920, 0, 3840, 1080),
            rect(1920, 0, 3840, 1020),
            1.5,
            false,
        );
        let (x2, y2, w2, h2) =
            compute_startup_placement(160, Some((3680, 860)), &[primary_100, secondary_150]);
        assert_eq!((w2, h2), (240, 240));
        assert_eq!((x2, y2), (3600, 780), "must clamp inside 150% monitor rcWork without clipping");

        // Disconnected secondary monitor -> recovers smoothly to primary monitor rcWork
        let (rx, ry, rw, rh) = compute_startup_placement(160, Some((3680, 860)), &[primary_100]);
        assert_eq!((rw, rh), (160, 160));
        assert_eq!((rx, ry), (1760, 860));
    }

    #[test]
    fn display_reconcile_recovers_offscreen_pet_and_is_strictly_idempotent() {
        let new_work = rect(0, 0, 1280, 680);

        // Pet stranded at old 1920x1080 corner (1720, 820) after resolution drop & DPI 100% -> 125%
        let first = plan_display_reconcile(128, (1720, 820), (128, 128), (1720, 820), new_work, 1.25);
        assert_eq!((first.target_w, first.target_h), (160, 160));
        assert_eq!((first.target_x, first.target_y), (1120, 520));
        assert!(first.needs_window_update);
        assert!(first.needs_persist);

        // Second pass after applying plan must be a strict no-op (prevents recursive resize/DB loops)
        let second = plan_display_reconcile(
            128,
            (first.target_x, first.target_y),
            (first.target_w, first.target_h),
            (first.target_x, first.target_y),
            new_work,
            1.25,
        );
        assert!(!second.needs_window_update);
        assert!(!second.needs_persist);
    }

    #[test]
    fn popup_placement_scales_to_target_monitor_dpi_and_stays_in_work_area() {
        // Secondary 150% monitor [1920, 0, 3840, 1020]
        let work = rect(1920, 0, 3840, 1020);
        // Pet near bottom-right corner on 150% monitor (Large 160 -> 240x240)
        let (px, py, pw, ph) =
            compute_popup_placement((3600, 780, 240, 240), (348.0, 268.0), 12.0, work, 1.5);
        assert_eq!((pw, ph), (522, 402), "panel must scale to target 150% DPI");
        assert!(px >= work.left && px + pw <= work.right);
        assert!(py >= work.top && py + ph <= work.bottom);
        assert_eq!(py + ph, 780 - 18, "panel sits cleanly above pet with 12*1.5=18px gap");

        // Pet near top edge -> popup flips below pet cleanly
        let (bx, by, bw, bh) =
            compute_popup_placement((2000, 10, 240, 240), (236.0, 68.0), 0.0, work, 1.5);
        assert_eq!((bw, bh), (354, 102));
        assert_eq!(by, 250, "bubble flips below pet when top clearance is insufficient");
        assert!(bx >= work.left && bx + bw <= work.right);
    }

    #[test]
    fn recognizes_primary_and_secondary_taskbar_as_desktop_shell_classes() {
        assert!(is_desktop_shell_class_name("Progman"));
        assert!(is_desktop_shell_class_name("WorkerW"));
        assert!(is_desktop_shell_class_name("Shell_TrayWnd"));
        assert!(is_desktop_shell_class_name("Shell_SecondaryTrayWnd"));
        assert!(!is_desktop_shell_class_name("CabinetWClass"));
        assert!(!is_desktop_shell_class_name("Chrome_WidgetWin_1"));
    }
}
