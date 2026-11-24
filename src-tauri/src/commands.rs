use crate::database::{AppSettings, AppSettingsPatch, DailyTotal, DaySummary};
use crate::state::AppState;
use crate::windowing;
use chrono::Local;
use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize)]
pub struct PetRuntimeSnapshot {
    pub tracking_paused: bool,
    pub pet_visible: bool,
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Serialize)]
struct BubblePayload {
    text: String,
    visible: bool,
}

#[tauri::command]
pub fn get_pet_runtime(app: AppHandle) -> PetRuntimeSnapshot {
    let state = app.state::<AppState>();
    let g = state.0.lock().expect("AppState mutex poisoned");
    PetRuntimeSnapshot {
        tracking_paused: g.tracking_paused,
        pet_visible: g.pet_visible,
        x: g.pet_x,
        y: g.pet_y,
    }
}

#[tauri::command]
pub fn begin_pet_drag(app: AppHandle) {
    windowing::begin_pet_drag(&app);
}

#[tauri::command]
pub fn move_pet_drag(app: AppHandle) {
    windowing::move_pet_drag(&app);
}

#[tauri::command]
pub fn end_pet_drag(app: AppHandle) -> (i32, i32) {
    windowing::end_pet_drag(&app)
}

#[tauri::command]
pub fn sync_pet_position(app: AppHandle) -> (i32, i32) {
    windowing::clamp_and_persist_pet(&app)
}

#[tauri::command]
pub async fn open_greeting_panel(app: AppHandle) {
    windowing::open_or_focus_panel(&app);
}

#[tauri::command]
pub async fn open_preferences_panel(app: AppHandle) {
    windowing::open_panel_view(&app, "preferences");
}

#[tauri::command]
pub fn close_panel(app: AppHandle) {
    windowing::close_panel(&app);
}

#[tauri::command]
pub fn resize_panel(app: AppHandle, width: f64, height: f64) {
    windowing::resize_panel(&app, width, height);
}

#[tauri::command]
pub async fn show_bubble(app: AppHandle, text: String, duration_ms: Option<u64>) {
    let (pet_visible, interactions_enabled) = {
        let state = app.state::<AppState>();
        let g = state.0.lock().expect("AppState mutex poisoned");
        (g.pet_visible, g.cute_moments_enabled)
    };
    if !pet_visible || !interactions_enabled {
        return;
    }

    let seq = {
        let state = app.state::<AppState>();
        let mut g = state.0.lock().expect("AppState mutex poisoned");
        g.bubble_seq = g.bubble_seq.wrapping_add(1);
        g.bubble_seq
    };

    windowing::open_bubble(&app, &text);

    let handle = app.clone();
    let wait_ms = duration_ms.unwrap_or(4500).clamp(1200, 10000);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(wait_ms));
        let is_latest = {
            let state = handle.state::<AppState>();
            let g = state.0.lock().expect("AppState mutex poisoned");
            g.bubble_seq == seq
        };
        if is_latest {
            let _ = handle.emit(
                "bubble:update",
                BubblePayload {
                    text: String::new(),
                    visible: false,
                },
            );
            std::thread::sleep(Duration::from_millis(240));
            let still_latest = {
                let state = handle.state::<AppState>();
                let g = state.0.lock().expect("AppState mutex poisoned");
                g.bubble_seq == seq
            };
            if still_latest {
                let destroy_handle = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    windowing::close_bubble(&destroy_handle);
                });
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Phase 3 Screen-Time & Settings IPC Commands (SQLite source of truth)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_today_summary(app: AppHandle) -> Result<DaySummary, String> {
    let state = app.state::<AppState>();
    state.flush_tracker();
    let db = state.db()?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    db.get_day_summary(&today)
}

#[tauri::command]
pub fn get_day_detail(app: AppHandle, date: String) -> Result<DaySummary, String> {
    let state = app.state::<AppState>();
    state.flush_tracker();
    let db = state.db()?;
    db.get_day_summary(&date)
}

#[tauri::command]
pub fn get_week_summary(
    app: AppHandle,
    end_date: Option<String>,
) -> Result<Vec<DailyTotal>, String> {
    let state = app.state::<AppState>();
    state.flush_tracker();
    let db = state.db()?;
    db.get_week_summary(end_date.as_deref())
}

#[tauri::command]
pub fn get_month_summary(
    app: AppHandle,
    year: i32,
    month: u32,
) -> Result<Vec<DailyTotal>, String> {
    let state = app.state::<AppState>();
    state.flush_tracker();
    let db = state.db()?;
    db.get_month_summary(year, month)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<AppSettings, String> {
    let state = app.state::<AppState>();
    let db = state.db()?;
    Ok(db.load_settings())
}

#[tauri::command]
pub fn update_settings(app: AppHandle, patch: AppSettingsPatch) -> Result<AppSettings, String> {
    let state = app.state::<AppState>();
    state.apply_settings_patch(&app, patch)
}
