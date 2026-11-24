pub mod tracker_core;
pub mod win_probe;

use crate::database::Database;
use crate::icons;
use chrono::Local;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tracker_core::{PetStateHint, TrackerCore};
use win_probe::WindowsProbe;

#[derive(Clone, Serialize)]
struct StateHintPayload {
    kind: &'static str,
}

pub struct TrackingService {
    db: Arc<Database>,
    core: Arc<Mutex<TrackerCore>>,
    /// Serializes take_flush_batch -> SQLite commit -> mark/restore so concurrent callers
    /// (background worker, UI queries, pause, shutdown) can never invert addition/deduction order.
    flush_lock: Mutex<()>,
    idle_threshold_secs: Arc<AtomicU64>,
    stop_flag: Arc<AtomicBool>,
    attempted_icons: Arc<Mutex<HashSet<String>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TrackingService {
    pub fn start(
        app: AppHandle,
        db: Arc<Database>,
        initial_paused: bool,
        initial_idle_threshold: u64,
    ) -> Arc<Self> {
        let service = Arc::new(Self {
            db,
            core: Arc::new(Mutex::new(TrackerCore::new(initial_paused))),
            flush_lock: Mutex::new(()),
            idle_threshold_secs: Arc::new(AtomicU64::new(initial_idle_threshold.clamp(30, 3600))),
            stop_flag: Arc::new(AtomicBool::new(false)),
            attempted_icons: Arc::new(Mutex::new(HashSet::new())),
            worker: Mutex::new(None),
        });

        let svc = Arc::clone(&service);
        let handle = thread::Builder::new()
            .name("piku-tracker".to_string())
            .spawn(move || {
                let mut probe = WindowsProbe::new();
                while !svc.stop_flag.load(Ordering::Relaxed) {
                    let threshold = svc.idle_threshold_secs.load(Ordering::Relaxed);
                    let sample = probe.sample(threshold);
                    let now = Local::now();

                    let outcome = {
                        let mut guard = svc.core.lock().expect("tracker core mutex poisoned");
                        guard.record_sample(now, sample)
                    };

                    if let Some(hint) = outcome.state_hint_changed {
                        emit_hint(&app, hint);
                    }

                    if outcome.should_flush {
                        svc.flush_now();
                    }

                    // Sleep in short slices so shutdown stops cleanly within ~100ms
                    for _ in 0..10 {
                        if svc.stop_flag.load(Ordering::Relaxed) {
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                }
                // Guaranteed final flush when worker exits
                svc.flush_now();
            })
            .expect("failed to spawn piku-tracker thread");

        *service.worker.lock().expect("worker mutex poisoned") = Some(handle);
        service
    }

    pub fn set_paused(&self, app: &AppHandle, paused: bool) {
        let outcome = {
            let mut guard = self.core.lock().expect("tracker core mutex poisoned");
            guard.set_paused(paused)
        };
        if outcome.should_flush {
            self.flush_now();
        }
        if let Some(hint) = outcome.state_hint_changed {
            emit_hint(app, hint);
        }
    }

    pub fn set_idle_threshold(&self, secs: u64) {
        self.idle_threshold_secs
            .store(secs.clamp(30, 3600), Ordering::Relaxed);
    }

    /// Flush all buffered usage additions and idle-tail deductions into SQLite transactionally.
    /// Serialized end-to-end by `flush_lock` (lock order: `flush_lock` -> `core` | `db.conn`).
    pub fn flush_now(&self) {
        self.flush_serialized_with_hook(|| {});
    }

    fn flush_serialized_with_hook<F: FnOnce()>(&self, before_db_commit: F) {
        let needs_icon = {
            let _flush_guard = self.flush_lock.lock().expect("flush_lock mutex poisoned");
            flush_core_into_db_serialized(&self.core, &self.db, before_db_commit)
        };
        if !needs_icon.is_empty() {
            self.spawn_icon_extraction(needs_icon);
        }
    }

    /// Stop background polling thread cleanly and guarantee final SQLite flush.
    pub fn shutdown(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(handle) = self.worker.lock().expect("worker mutex poisoned").take() {
            let _ = handle.join();
        }
        self.flush_now();
    }

    /// Extract missing app icons asynchronously so tracking ticks never stall on disk/shell I/O.
    fn spawn_icon_extraction(&self, candidates: Vec<(String, String)>) {
        let mut to_extract = Vec::new();
        {
            let mut attempted = self
                .attempted_icons
                .lock()
                .expect("attempted_icons mutex poisoned");
            for (exe_name, exe_path) in candidates {
                if attempted.insert(exe_name.clone()) {
                    to_extract.push((exe_name, exe_path));
                }
            }
        }
        if to_extract.is_empty() {
            return;
        }

        let db = Arc::clone(&self.db);
        let _ = thread::Builder::new()
            .name("piku-icon-cache".to_string())
            .spawn(move || {
                let cache_dir = db.icon_cache_dir().to_path_buf();
                for (exe_name, exe_path) in to_extract {
                    if let Some(hash) = icons::ensure_cached_icon(&exe_path, &cache_dir) {
                        let _ = db.set_app_icon_hash(&exe_name, &hash);
                    }
                }
            });
    }
}

/// Shared serialized flush kernel invoked under `flush_lock`.
/// Lock ordering: caller holds `flush_lock` -> briefly acquires `core` -> releases `core` ->
/// acquires `db.conn` -> releases `db.conn` -> briefly acquires `core` to mark/restore.
fn flush_core_into_db_serialized<F: FnOnce()>(
    core: &Mutex<TrackerCore>,
    db: &Database,
    before_db_commit: F,
) -> Vec<(String, String)> {
    let batch = {
        let mut guard = core.lock().expect("tracker core mutex poisoned");
        guard.take_flush_batch()
    };
    if batch.is_empty() {
        return Vec::new();
    }

    before_db_commit();

    let now_unix = Local::now().timestamp();
    match db.flush_usage_delta(&batch.additions, &batch.deductions, now_unix) {
        Ok(needs_icon) => {
            let mut guard = core.lock().expect("tracker core mutex poisoned");
            guard.mark_flush_succeeded();
            needs_icon
        }
        Err(err) => {
            eprintln!("[Piku tracker] flush failed (restoring buffer for prompt retry): {err}");
            let mut guard = core.lock().expect("tracker core mutex poisoned");
            guard.restore_failed_flush(batch);
            Vec::new()
        }
    }
}

fn emit_hint(app: &AppHandle, hint: PetStateHint) {
    let kind = match hint {
        PetStateHint::Active => "active",
        PetStateHint::IdleSleep => "idle_sleep",
        PetStateHint::Paused => "paused",
    };
    let _ = app.emit("pet:state-hint", StateHintPayload { kind });
}

#[cfg(test)]
mod integration_tests {
    use super::tracker_core::{AppIdentity, ForegroundSample, TrackerCore};
    use crate::database::Database;
    use chrono::{Duration, Local, TimeZone};
    use std::path::PathBuf;

    #[test]
    fn tracker_flushes_app_switch_midnight_and_idle_into_sqlite() {
        let db = Database::open_in_memory(PathBuf::from("unused_cache")).unwrap();
        let mut core = TrackerCore::new(false);
        let t0 = Local
            .with_ymd_and_hms(2026, 11, 23, 23, 59, 57)
            .earliest()
            .unwrap();

        let vs = ForegroundSample::Active(AppIdentity::normalized(
            Some("Code.exe"),
            Some("C:\\VSCode\\Code.exe"),
            Some("Visual Studio Code"),
        ));
        let malformed = ForegroundSample::Active(AppIdentity::normalized(
            Some("  \u{0000} "),
            None,
            Some("\u{0007}"),
        ));

        core.record_sample(t0, vs.clone());
        // 5s crossing midnight: 3s on 2026-11-23 h23, 2s on 2026-11-24 h00
        core.record_sample(t0 + Duration::seconds(5), vs.clone());
        // Switch to malformed/unknown app -> triggers flush & credits [t0+5, t0+6) to VS Code
        let out = core.record_sample(t0 + Duration::seconds(6), malformed.clone());
        assert!(out.should_flush);
        db.flush_usage_buckets(&core.take_pending(), t0.timestamp())
            .unwrap();

        // 3s on unknown app, then enter Idle (must not count idle time)
        core.record_sample(t0 + Duration::seconds(9), malformed);
        let idle = core.record_sample(
            t0 + Duration::seconds(11),
            ForegroundSample::Idle { idle_secs: 0 },
        );
        assert!(idle.should_flush);
        let batch = core.take_flush_batch();
        db.flush_usage_delta(&batch.additions, &batch.deductions, t0.timestamp() + 11)
            .unwrap();

        let d1 = db.get_day_summary("2026-11-23").unwrap();
        assert_eq!(d1.total_secs, 3);
        assert_eq!(d1.hourly_secs[23], 3);
        assert_eq!(d1.apps[0].display_name, "Visual Studio Code");

        let d2 = db.get_day_summary("2026-11-24").unwrap();
        assert_eq!(d2.total_secs, 6); // 3s VS Code + 3s Unknown App
        assert_eq!(d2.hourly_secs[0], 6);
        assert_eq!(d2.apps.len(), 2);
        assert!(d2.apps.iter().any(|a| a.exe_name == "unknown_app" && a.display_name == "Unknown App" && a.active_secs == 3));
        assert!(d2.apps.iter().any(|a| a.exe_name == "code.exe" && a.active_secs == 3));
    }

    #[test]
    fn idle_tail_rollback_across_multiple_flushes_hour_and_app_switch() {
        let db = Database::open_in_memory(PathBuf::from("unused_cache")).unwrap();
        let mut core = TrackerCore::new(false);

        // Start at 10:55:00:
        // - 10:55:00..10:56:50 (110s) active in Chrome
        // - 10:56:50..10:57:00 (10s) active in VS Code (app switch shortly before idle)
        // - 10:57:00..11:02:00 (300s) inactive in VS Code crossing 11:00 hour boundary & multiple 30s flushes
        let t0 = Local
            .with_ymd_and_hms(2026, 11, 23, 10, 55, 0)
            .earliest()
            .unwrap();

        let chrome = ForegroundSample::Active(AppIdentity::normalized(
            Some("chrome.exe"),
            Some("C:\\Chrome\\chrome.exe"),
            Some("Google Chrome"),
        ));
        let vscode = ForegroundSample::Active(AppIdentity::normalized(
            Some("code.exe"),
            Some("C:\\VSCode\\Code.exe"),
            Some("Visual Studio Code"),
        ));

        let mut flush_count = 0usize;
        // Total elapsed: 110s Chrome + 10s active VS Code + 299s idle VS Code ticks = 419s -> Idle at +420s
        for sec in 0..420i64 {
            let sample = if sec < 110 {
                chrome.clone()
            } else {
                vscode.clone()
            };
            let now = t0 + Duration::seconds(sec);
            let out = core.record_sample(now, sample);
            if out.should_flush {
                let batch = core.take_flush_batch();
                db.flush_usage_delta(&batch.additions, &batch.deductions, now.timestamp())
                    .unwrap();
                core.mark_flush_succeeded();
                flush_count += 1;
            }
        }
        assert!(
            flush_count >= 10,
            "expected multiple 30s periodic flushes during 300s idle tail, got {flush_count}"
        );

        // At 11:02:00 (t0 + 420s), idle_duration_secs reaches 300s
        let idle_now = t0 + Duration::seconds(420);
        let idle_out = core.record_sample(idle_now, ForegroundSample::Idle { idle_secs: 300 });
        assert!(idle_out.should_flush);
        let batch = core.take_flush_batch();
        assert!(
            !batch.deductions.is_empty(),
            "persisted SQLite idle tail must produce deductions"
        );
        db.flush_usage_delta(&batch.additions, &batch.deductions, idle_now.timestamp())
            .unwrap();
        core.mark_flush_succeeded();

        let day = db.get_day_summary("2026-11-23").unwrap();
        // Legitimate active usage ONLY: 110s Chrome + 10s VS Code = 120s (all in hour 10; hour 11 pruned to 0!)
        assert_eq!(day.total_secs, 120);
        assert_eq!(day.hourly_secs[10], 120);
        assert_eq!(day.hourly_secs[11], 0);
        assert_eq!(
            day.apps.iter().find(|a| a.exe_name == "chrome.exe").unwrap().active_secs,
            110
        );
        assert_eq!(
            day.apps.iter().find(|a| a.exe_name == "code.exe").unwrap().active_secs,
            10
        );
    }

    #[test]
    fn idle_tail_rollback_across_midnight_boundary_and_never_negative() {
        let db = Database::open_in_memory(PathBuf::from("unused_cache")).unwrap();
        let mut core = TrackerCore::new(false);

        // Start at 23:56:00: 60s active Firefox [23:56:00..23:57:00), then 300s idle crossing midnight to 00:02:00
        let t0 = Local
            .with_ymd_and_hms(2026, 11, 23, 23, 56, 0)
            .earliest()
            .unwrap();
        let firefox = ForegroundSample::Active(AppIdentity::normalized(
            Some("firefox.exe"),
            Some("C:\\Firefox\\firefox.exe"),
            Some("Firefox"),
        ));

        for sec in 0..360i64 {
            let now = t0 + Duration::seconds(sec);
            let out = core.record_sample(now, firefox.clone());
            if out.should_flush {
                let batch = core.take_flush_batch();
                db.flush_usage_delta(&batch.additions, &batch.deductions, now.timestamp())
                    .unwrap();
                core.mark_flush_succeeded();
            }
        }

        let idle_now = t0 + Duration::seconds(360); // 2026-11-24 00:02:00
        let out = core.record_sample(idle_now, ForegroundSample::Idle { idle_secs: 300 });
        assert!(out.should_flush);
        let batch = core.take_flush_batch();
        db.flush_usage_delta(&batch.additions, &batch.deductions, idle_now.timestamp())
            .unwrap();

        let d1 = db.get_day_summary("2026-11-23").unwrap();
        let d2 = db.get_day_summary("2026-11-24").unwrap();
        assert_eq!(d1.total_secs, 60);
        assert_eq!(d1.hourly_secs[23], 60);
        assert_eq!(d2.total_secs, 0);
        assert_eq!(d2.hourly_secs[0], 0);
        assert!(d2.apps.is_empty(), "zeroed next-day bucket must be pruned cleanly");

        // Verify direct oversized SQLite deduction clamps cleanly at 0 (never negative)
        let mut oversized = std::collections::HashMap::new();
        oversized.insert(
            crate::database::UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 23,
                exe_name: "firefox.exe".to_string(),
            },
            9999,
        );
        db.flush_usage_delta(&std::collections::HashMap::new(), &oversized, idle_now.timestamp())
            .unwrap();
        let d1_after = db.get_day_summary("2026-11-23").unwrap();
        assert_eq!(d1_after.total_secs, 0);
        assert!(d1_after.apps.is_empty());
    }

    #[test]
    fn flush_serialization_prevents_deduction_overtaking_earlier_addition() {
        use super::flush_core_into_db_serialized;
        use std::sync::{Arc, Barrier, Mutex};
        use std::thread;

        let db = Arc::new(Database::open_in_memory(PathBuf::from("unused_cache")).unwrap());
        let core = Arc::new(Mutex::new(TrackerCore::new(false)));
        let flush_lock = Arc::new(Mutex::new(()));

        let t0 = Local
            .with_ymd_and_hms(2026, 11, 23, 14, 0, 0)
            .earliest()
            .unwrap();
        let chrome = ForegroundSample::Active(AppIdentity::normalized(
            Some("chrome.exe"),
            Some("C:\\Chrome\\chrome.exe"),
            Some("Google Chrome"),
        ));

        // Record 10s of inactive-tail additions in core
        {
            let mut g = core.lock().unwrap();
            for s in 0..=10 {
                g.record_sample(t0 + Duration::seconds(s), chrome.clone());
            }
        }

        // Thread 1 starts flushing additions (+10s), pauses BEFORE DB commit while holding flush_lock.
        // Thread 2 records Idle { idle_secs: 10 } (queuing -10s deduction) and attempts concurrent flush.
        // Because flush_lock serializes take->commit->mark, Thread 1 MUST commit +10s BEFORE
        // Thread 2 takes and commits -10s, yielding exact net 0s instead of clamping -10->0 then +10!
        let entered_hook = Arc::new(Barrier::new(2));
        let t1_core = Arc::clone(&core);
        let t1_db = Arc::clone(&db);
        let t1_lock = Arc::clone(&flush_lock);
        let t1_entered = Arc::clone(&entered_hook);

        let h1 = thread::spawn(move || {
            let _guard = t1_lock.lock().unwrap();
            flush_core_into_db_serialized(&t1_core, &t1_db, || {
                t1_entered.wait();
                // Yield briefly so Thread 2 definitely hits flush_lock while Thread 1 is mid-flush
                thread::sleep(std::time::Duration::from_millis(60));
            });
        });

        // Wait until Thread 1 has taken the +10s batch and is holding flush_lock
        entered_hook.wait();

        // Now record Idle(10s) -> queues 10s SQLite deduction in core
        {
            let mut g = core.lock().unwrap();
            let out = g.record_sample(
                t0 + Duration::seconds(11),
                ForegroundSample::Idle { idle_secs: 11 },
            );
            assert!(out.should_flush);
            assert_eq!(g.pending_deductions_total_secs(), 10);
        }

        // Thread 2 attempts concurrent flush under the same flush_lock
        let t2_core = Arc::clone(&core);
        let t2_db = Arc::clone(&db);
        let t2_lock = Arc::clone(&flush_lock);
        let h2 = thread::spawn(move || {
            let _guard = t2_lock.lock().unwrap();
            flush_core_into_db_serialized(&t2_core, &t2_db, || {});
        });

        h1.join().unwrap();
        h2.join().unwrap();

        let summary = db.get_day_summary("2026-11-23").unwrap();
        assert_eq!(
            summary.total_secs, 0,
            "deduction must commit AFTER earlier addition, leaving 0s instead of leaking +10s"
        );
    }

    #[test]
    fn failed_serialized_flush_restores_batch_and_retries_without_duplication() {
        use super::flush_core_into_db_serialized;
        use std::sync::Mutex;

        let dir = std::env::temp_dir().join(format!(
            "piku_flush_retry_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let db = Database::open(&dir).unwrap();
        let core = Mutex::new(TrackerCore::new(false));

        let t0 = Local
            .with_ymd_and_hms(2026, 11, 23, 15, 0, 0)
            .earliest()
            .unwrap();
        let code = ForegroundSample::Active(AppIdentity::normalized(
            Some("code.exe"),
            Some("C:\\VSCode\\Code.exe"),
            Some("Visual Studio Code"),
        ));

        {
            let mut g = core.lock().unwrap();
            g.record_sample(t0, code.clone());
            g.record_sample(t0 + Duration::seconds(4), code.clone());
            assert_eq!(g.pending_total_secs(), 4);
        }

        // Hold an exclusive lock on an external connection with short busy_timeout on DB to force flush Err
        let blocker = rusqlite::Connection::open(dir.join("piku.db")).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        db.set_busy_timeout_for_test(50);

        flush_core_into_db_serialized(&core, &db, || {});

        // Batch must have been restored to core with retry scheduled
        assert_eq!(core.lock().unwrap().pending_total_secs(), 4);
        assert_eq!(db.get_day_summary("2026-11-23").unwrap().total_secs, 0);

        // Release external lock and retry flush -> commits exact 4s once (no duplication)
        blocker.execute_batch("ROLLBACK;").unwrap();
        drop(blocker);
        db.set_busy_timeout_for_test(5000);

        flush_core_into_db_serialized(&core, &db, || {});
        assert_eq!(core.lock().unwrap().pending_total_secs(), 0);
        assert_eq!(db.get_day_summary("2026-11-23").unwrap().total_secs, 4);

        // Extra flush after success is a strict no-op (no duplicate write)
        flush_core_into_db_serialized(&core, &db, || {});
        assert_eq!(db.get_day_summary("2026-11-23").unwrap().total_secs, 4);

        drop(db);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
