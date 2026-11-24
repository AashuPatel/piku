use crate::state::AppState;
use crate::windowing;
use serde::Serialize;
use std::sync::OnceLock;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const MENU_TOGGLE_VISIBILITY: &str = "toggle_visibility";
const MENU_TOGGLE_PAUSE: &str = "toggle_pause";
const MENU_PREFERENCES: &str = "preferences";
const MENU_QUIT: &str = "quit";

struct TrayMenuHandles {
    vis_item: MenuItem<Wry>,
    pause_item: MenuItem<Wry>,
}

static TRAY_HANDLES: OnceLock<TrayMenuHandles> = OnceLock::new();

#[derive(Clone, Serialize)]
struct VisibilityPayload {
    visible: bool,
}

pub fn sync_pause_label(paused: bool) {
    if let Some(h) = TRAY_HANDLES.get() {
        let _ = h.pause_item.set_text(if paused {
            "Resume Tracking"
        } else {
            "Pause Tracking"
        });
    }
}

pub fn sync_visibility_label(visible: bool) {
    if let Some(h) = TRAY_HANDLES.get() {
        let _ = h
            .vis_item
            .set_text(if visible { "Hide Piku" } else { "Show Piku" });
    }
}

pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let (initial_visible, initial_paused) = {
        let state = app.state::<AppState>();
        let g = state.0.lock().expect("AppState mutex poisoned");
        (g.pet_visible, g.tracking_paused)
    };

    let vis_item = MenuItem::with_id(
        app,
        MENU_TOGGLE_VISIBILITY,
        if initial_visible { "Hide Piku" } else { "Show Piku" },
        true,
        None::<&str>,
    )?;
    let pause_item = MenuItem::with_id(
        app,
        MENU_TOGGLE_PAUSE,
        if initial_paused {
            "Resume Tracking"
        } else {
            "Pause Tracking"
        },
        true,
        None::<&str>,
    )?;
    let sep_top = PredefinedMenuItem::separator(app)?;
    let prefs_item = MenuItem::with_id(app, MENU_PREFERENCES, "Preferences", true, None::<&str>)?;
    let sep_bottom = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "Quit Piku", true, None::<&str>)?;

    let _ = TRAY_HANDLES.set(TrayMenuHandles {
        vis_item: vis_item.clone(),
        pause_item: pause_item.clone(),
    });

    let menu = Menu::with_items(
        app,
        &[
            &vis_item,
            &pause_item,
            &sep_top,
            &prefs_item,
            &sep_bottom,
            &quit_item,
        ],
    )?;
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;

    TrayIconBuilder::with_id("piku-tray")
        .icon(icon)
        .tooltip("Piku Desktop Pet")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            MENU_TOGGLE_VISIBILITY => {
                let now_visible = {
                    let state = app.state::<AppState>();
                    let mut g = state.0.lock().expect("AppState mutex poisoned");
                    g.pet_visible = !g.pet_visible;
                    g.pet_visible
                };
                app.state::<AppState>().persist(app);
                sync_visibility_label(now_visible);

                if let Some(pet) = app.get_webview_window("pet") {
                    if now_visible {
                        windowing::show_window_noactivate(&pet);
                    } else {
                        windowing::hide_window_noactivate(&pet);
                        windowing::close_bubble(app);
                    }
                }
                let _ = app.emit(
                    "pet:visibility-changed",
                    VisibilityPayload {
                        visible: now_visible,
                    },
                );
            }
            MENU_TOGGLE_PAUSE => {
                let state = app.state::<AppState>();
                let now_paused = {
                    let g = state.0.lock().expect("AppState mutex poisoned");
                    !g.tracking_paused
                };
                state.set_tracking_paused(app, now_paused);
            }
            MENU_PREFERENCES => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    windowing::open_panel_view(&handle, "preferences");
                });
            }
            MENU_QUIT => {
                app.state::<AppState>().shutdown_cleanly(app);
                let _ = app.remove_tray_by_id("piku-tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click tray icon toggles greeting panel without moving or focusing Piku
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle().clone();
                if app.get_webview_window("panel").is_some() {
                    windowing::close_panel(&app);
                } else {
                    tauri::async_runtime::spawn(async move {
                        windowing::open_or_focus_panel(&app);
                    });
                }
            }
        })
        .build(app)?;

    Ok(())
}
