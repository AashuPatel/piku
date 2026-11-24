use crate::database::{AppSettings, AppSettingsPatch, Database, RuntimePetSettings};
use crate::tracking::TrackingService;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const PANEL_BLUR_GRACE: Duration = Duration::from_millis(450);

pub struct RuntimeState {
    pub tracking_paused: bool,
    pub pet_visible: bool,
    pub has_saved_position: bool,
    pub pet_x: i32,
    pub pet_y: i32,
    pub pet_size: u32,
    pub cute_moments_enabled: bool,
    pub bubble_seq: u64,
    pub drag_offset: Option<(i32, i32)>,
    pub drag_window_size: Option<(i32, i32)>,
    pub panel_logical_size: Option<(f64, f64)>,
    pub panel_opened_at: Option<Instant>,
}

/// Tuple struct preserves `state.0` compatibility with `windowing.rs` untouched.
/// Field 3 is an idempotent shutdown guard preventing double cleanup across Quit / WM_ENDSESSION / Exit.
pub struct AppState(
    pub Mutex<RuntimeState>,
    OnceLock<Arc<Database>>,
    OnceLock<Arc<TrackingService>>,
    AtomicBool,
);

impl AppState {
    pub fn new_default() -> Self {
        Self(
            Mutex::new(RuntimeState {
                tracking_paused: false,
                pet_visible: true,
                has_saved_position: false,
                pet_x: 0,
                pet_y: 0,
                pet_size: 128,
                cute_moments_enabled: true,
                bubble_seq: 0,
                drag_offset: None,
                drag_window_size: None,
                panel_logical_size: None,
                panel_opened_at: None,
            }),
            OnceLock::new(),
            OnceLock::new(),
            AtomicBool::new(false),
        )
    }

    pub fn init_services(&self, app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
        let data_dir = app.path().app_local_data_dir()?;
        let db = Arc::new(Database::open(&data_dir).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?);
        let settings = db.load_settings();

        // Reconcile per-user HKCU\...\Run\Piku entry against persisted preference and current exe path
        if let Err(err) = crate::startup::reconcile_launch_on_startup(settings.launch_on_startup) {
            eprintln!("[Piku startup] reconcile launch_on_startup failed: {err}");
        }

        {
            let mut g = self.0.lock().expect("AppState mutex poisoned");
            g.tracking_paused = settings.tracking_paused;
            g.pet_visible = settings.pet_visible;
            g.has_saved_position = settings.has_saved_position;
            g.pet_x = settings.pet_x;
            g.pet_y = settings.pet_y;
            g.pet_size = settings.pet_size.clamp(100, 160);
            g.cute_moments_enabled = settings.cute_moments_enabled;
        }

        let _ = self.1.set(Arc::clone(&db));

        let tracker = TrackingService::start(
            app.clone(),
            db,
            settings.tracking_paused,
            settings.idle_threshold_secs,
        );
        let _ = self.2.set(tracker);
        Ok(())
    }

    pub fn db(&self) -> Result<Arc<Database>, String> {
        self.1
            .get()
            .cloned()
            .ok_or_else(|| "database not initialized".to_string())
    }

    pub fn flush_tracker(&self) {
        if let Some(tracker) = self.2.get() {
            tracker.flush_now();
        }
    }

    pub fn set_tracking_paused(&self, app: &AppHandle, paused: bool) {
        {
            let mut g = self.0.lock().expect("AppState mutex poisoned");
            g.tracking_paused = paused;
        }
        self.persist(app);
        crate::tray::sync_pause_label(paused);
        if let Some(tracker) = self.2.get() {
            tracker.set_paused(app, paused);
        }
        if let Ok(db) = self.db() {
            let _ = app.emit("settings:changed", db.load_settings());
        }
    }

    pub fn apply_settings_patch(
        &self,
        app: &AppHandle,
        patch: AppSettingsPatch,
    ) -> Result<AppSettings, String> {
        if let Some(enabled) = patch.launch_on_startup {
            crate::startup::set_launch_on_startup(enabled)?;
        }

        let db = self.db()?;
        let prev_size = {
            let g = self.0.lock().expect("AppState mutex poisoned");
            g.pet_size
        };
        let updated = db.update_settings(patch)?;

        {
            let mut g = self.0.lock().expect("AppState mutex poisoned");
            g.tracking_paused = updated.tracking_paused;
            g.pet_visible = updated.pet_visible;
            g.has_saved_position = updated.has_saved_position;
            g.pet_x = updated.pet_x;
            g.pet_y = updated.pet_y;
            g.pet_size = updated.pet_size;
            g.cute_moments_enabled = updated.cute_moments_enabled;
        }

        if let Some(tracker) = self.2.get() {
            tracker.set_idle_threshold(updated.idle_threshold_secs);
            tracker.set_paused(app, updated.tracking_paused);
        }

        crate::tray::sync_pause_label(updated.tracking_paused);
        crate::tray::sync_visibility_label(updated.pet_visible);

        if updated.pet_size != prev_size {
            crate::windowing::apply_pet_size(app, updated.pet_size);
        }

        if !updated.cute_moments_enabled {
            crate::windowing::close_bubble(app);
        }

        let final_settings = db.load_settings();
        let _ = app.emit(
            "pet:settings-changed",
            serde_json::json!({
                "petSize": final_settings.pet_size,
                "cuteMomentsEnabled": final_settings.cute_moments_enabled,
            }),
        );
        let _ = app.emit("settings:changed", &final_settings);

        Ok(final_settings)
    }

    /// Persist only the runtime-owned pet fields into SQLite `settings` via a targeted
    /// transactional upsert—never reading/rewriting unrelated preferences or substituting defaults.
    pub fn persist(&self, _app: &AppHandle) {
        let Some(db) = self.1.get() else {
            return;
        };
        let runtime = {
            let g = self.0.lock().expect("AppState mutex poisoned");
            RuntimePetSettings {
                tracking_paused: g.tracking_paused,
                pet_visible: g.pet_visible,
                has_saved_position: g.has_saved_position,
                pet_x: g.pet_x,
                pet_y: g.pet_y,
            }
        };

        if let Err(err) = db.save_runtime_settings(&runtime) {
            eprintln!("[Piku settings] failed to persist runtime settings: {err}");
        }
    }

    pub fn mark_panel_opened(&self) {
        let mut g = self.0.lock().expect("AppState mutex poisoned");
        g.panel_opened_at = Some(Instant::now());
    }

    pub fn is_panel_blur_guarded(&self) -> bool {
        let g = self.0.lock().expect("AppState mutex poisoned");
        g.panel_opened_at
            .map(|t| t.elapsed() < PANEL_BLUR_GRACE)
            .unwrap_or(false)
    }

    /// Idempotently unhook native Win32 listeners, stop the tracker with final SQLite flush,
    /// persist runtime settings, checkpoint WAL, and refresh rolling local backup.
    /// Safe to invoke from Tray Quit, CloseRequested, WM_ENDSESSION, or RunEvent::Exit;
    /// checkpoint/backup failures are logged and never block application termination.
    pub fn shutdown_cleanly(&self, app: &AppHandle) {
        if self
            .3
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        crate::windowing::cleanup_hooks();
        if let Some(tracker) = self.2.get() {
            tracker.shutdown();
        }
        self.persist(app);
        if let Some(db) = self.1.get() {
            finalize_database_on_shutdown(db);
        }
    }
}

/// Runs post-flush WAL truncation checkpoint and rolling local backup during clean shutdown.
/// Never panics or blocks shutdown if checkpointing or backup fails (e.g., busy external reader).
fn finalize_database_on_shutdown(db: &Database) {
    if let Err(err) = db.checkpoint_wal() {
        eprintln!("[Piku shutdown] WAL checkpoint incomplete (continuing shutdown): {err}");
    }
    if let Err(err) = db.create_rolling_backup() {
        eprintln!("[Piku shutdown] rolling backup failed (continuing shutdown): {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutdown_guard_is_strictly_idempotent() {
        let state = AppState::new_default();
        assert_eq!(
            state
                .3
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst),
            Ok(false)
        );
        assert!(
            state
                .3
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err(),
            "second shutdown signal must be ignored"
        );
    }

    #[test]
    fn shutdown_finalization_completes_cleanly_even_when_wal_checkpoint_fails() {
        let dir = std::env::temp_dir().join(format!(
            "piku_shutdown_chkpt_fail_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Database::open(&dir).unwrap();

        // Seed data into WAL and hold an active read transaction on an external connection
        // so PRAGMA wal_checkpoint(TRUNCATE) is guaranteed to report busy/incomplete.
        db.save_runtime_settings(&RuntimePetSettings {
            tracking_paused: false,
            pet_visible: true,
            has_saved_position: true,
            pet_x: 300,
            pet_y: 200,
        })
        .unwrap();

        let reader = rusqlite::Connection::open(dir.join("piku.db")).unwrap();
        reader.execute_batch("BEGIN; SELECT COUNT(*) FROM settings;").unwrap();

        // Direct checkpoint reports Err (busy), yet shutdown finalizer must not panic or block.
        assert!(db.checkpoint_wal().is_err());
        finalize_database_on_shutdown(&db);

        reader.execute_batch("COMMIT;").unwrap();
        drop(reader);
        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
