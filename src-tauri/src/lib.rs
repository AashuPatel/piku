mod commands;
mod database;
mod icons;
mod startup;
mod state;
mod tracking;
mod tray;
mod windowing;

use state::AppState;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Native Windows named mutex: exit cleanly before initializing Tauri/tray/tracker if already running
    let Some(_single_instance_guard) = startup::SingleInstanceGuard::acquire() else {
        return;
    };

    let app = tauri::Builder::default()
        .manage(AppState::new_default())
        .setup(|app| {
            let handle = app.handle();
            app.state::<AppState>().init_services(handle)?;
            windowing::init_pet_window(handle);
            tray::setup_tray(handle)?;
            let (paused, visible) = {
                let s = app.state::<AppState>();
                let g = s.0.lock().expect("AppState mutex poisoned");
                (g.tracking_paused, g.pet_visible)
            };
            let _ = handle.emit(
                "pet:state-hint",
                serde_json::json!({ "kind": if paused { "paused" } else { "active" } }),
            );
            let _ = handle.emit(
                "pet:visibility-changed",
                serde_json::json!({ "visible": visible }),
            );
            Ok(())
        })
        .on_window_event(|window, event| match (window.label(), event) {
            // Destroy floating greeting panel on focus loss (guarded briefly during initial open)
            ("panel", WindowEvent::Focused(false)) => {
                let app = window.app_handle();
                if !app.state::<AppState>().is_panel_blur_guarded() {
                    windowing::close_panel(app);
                }
            }
            // Closing pet cleanly unhooks Win32 events, flushes tracking usage, persists settings, and exits
            ("pet", WindowEvent::CloseRequested { .. }) => {
                let app = window.app_handle();
                app.state::<AppState>().shutdown_cleanly(app);
                windowing::close_bubble(app);
                windowing::close_panel(app);
                let _ = app.remove_tray_by_id("piku-tray");
                app.exit(0);
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_pet_runtime,
            commands::begin_pet_drag,
            commands::move_pet_drag,
            commands::end_pet_drag,
            commands::sync_pet_position,
            commands::open_greeting_panel,
            commands::open_preferences_panel,
            commands::close_panel,
            commands::resize_panel,
            commands::show_bubble,
            commands::get_today_summary,
            commands::get_day_detail,
            commands::get_week_summary,
            commands::get_month_summary,
            commands::get_settings,
            commands::update_settings,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Piku desktop application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            app_handle.state::<AppState>().shutdown_cleanly(app_handle);
        }
    });
}
