use crate::database::{PendingBucketEntry, UsageBucketKey};
use chrono::{DateTime, Duration, Local, Timelike};
use std::collections::{HashMap, VecDeque};

/// Maximum elapsed seconds credited from a single tick.
/// Prevents sleep/hibernate/clock jumps from inflating screen time.
pub const MAX_TICK_DELTA_SECS: i64 = 5;

/// Flush accumulated active usage to SQLite every 30 seconds while active.
pub const PERIODIC_FLUSH_SECS: i64 = 30;

/// Maximum idle tail that can be rolled back upon entering Idle (matches max `idle_threshold_secs`).
pub const MAX_IDLE_ROLLBACK_SECS: u64 = 3600;

/// Bound recent credit history slightly above the maximum configurable idle threshold.
const MAX_RECENT_CREDIT_HISTORY_SECS: i64 = 3660;

/// Normalized identity of an observed foreground Windows application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIdentity {
    pub exe_name: String,
    pub exe_path: Option<String>,
    pub display_name: String,
}

impl AppIdentity {
    /// Normalize raw executable metadata safely, falling back cleanly on empty/malformed values.
    pub fn normalized(
        raw_exe_name: Option<&str>,
        raw_exe_path: Option<&str>,
        raw_display_name: Option<&str>,
    ) -> Self {
        let clean_path = raw_exe_path
            .map(sanitize_text)
            .filter(|s| !s.is_empty());

        let derived_exe = raw_exe_name
            .map(sanitize_text)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                clean_path.as_deref().and_then(|p| {
                    p.rsplit(['\\', '/'])
                        .next()
                        .map(sanitize_text)
                        .filter(|s| !s.is_empty())
                })
            })
            .unwrap_or_else(|| "unknown_app".to_string())
            .to_lowercase();

        let clean_display = raw_display_name
            .map(sanitize_text)
            .filter(|s| !s.is_empty() && s.len() <= 120)
            .unwrap_or_else(|| friendly_name_from_exe(&derived_exe));

        Self {
            exe_name: derived_exe,
            exe_path: clean_path,
            display_name: clean_display,
        }
    }
}

/// Strip control characters and collapse surrounding whitespace.
pub fn sanitize_text(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Turn an executable filename like `code.exe` or `my_cool-app.exe` into `Code` / `My Cool App`.
pub fn friendly_name_from_exe(exe_name: &str) -> String {
    let lower = exe_name.trim().to_lowercase();
    if lower.is_empty() || lower == "unknown_app" {
        return "Unknown App".to_string();
    }
    let stem = lower.strip_suffix(".exe").unwrap_or(&lower);
    let parts: Vec<String> = stem
        .split(|c: char| c == '_' || c == '-' || c == '.' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();

    if parts.is_empty() {
        "Unknown App".to_string()
    } else {
        parts.join(" ")
    }
}

/// Processes excluded from user screen time (Piku itself, lock screen, desktop shell chrome).
pub fn is_excluded_process(exe_name_lower: &str) -> bool {
    matches!(
        exe_name_lower,
        "piku.exe"
            | "lockapp.exe"
            | "logonui.exe"
            | "shellexperiencehost.exe"
            | "shellhost.exe"
            | "pickerhost.exe"
            | "searchhost.exe"
            | "searchapp.exe"
            | "startmenuexperiencehost.exe"
            | "screenclippinghost.exe"
            | "textinputhost.exe"
            | "applicationframehost.exe"
            | "idle"
    )
}

/// Windows Explorer shell chrome window classes (Desktop, Taskbar, Tray overflow, Thumbnail previews)
/// that must not be attributed as active File Explorer usage.
/// Real File Explorer windows (`CabinetWClass`) and unrecognized classes remain tracked.
pub fn is_excluded_explorer_window_class(class_name: &str) -> bool {
    matches!(
        class_name,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "NotifyIconOverflowWindow"
            | "TaskListThumbnailWnd"
            | "TopLevelWindowForOverflowXamlIsland"
            | "XamlExplorerHostIslandWindow"
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundSample {
    Active(AppIdentity),
    Idle { idle_secs: u64 },
    Locked,
    SelfOrExcluded,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PetStateHint {
    Active,
    IdleSleep,
    Paused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickOutcome {
    pub should_flush: bool,
    pub state_hint_changed: Option<PetStateHint>,
}

/// Transactional batch of additions and idle-tail deductions ready for SQLite.
#[derive(Debug, Clone, Default)]
pub struct FlushBatch {
    pub additions: HashMap<UsageBucketKey, PendingBucketEntry>,
    pub deductions: HashMap<UsageBucketKey, i64>,
}

impl FlushBatch {
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.deductions.is_empty()
    }
}

/// Contiguous credited usage slice `[start_ts, end_ts)` for exact idle-tail rollback.
#[derive(Debug, Clone)]
struct CreditedSlice {
    start_ts: i64,
    end_ts: i64,
    key: UsageBucketKey,
}

fn retry_delay_ticks(consecutive_failures: u32) -> u8 {
    match consecutive_failures {
        0 => 0,
        1 => 2,  // prompt retry (~2s) after first transient SQLite failure
        2 => 5,  // ~5s after second consecutive failure
        _ => 10, // capped ~10s backoff to prevent busy-looping SQLite
    }
}

/// Pure, OS-independent tracking accumulator and state machine.
pub struct TrackerCore {
    last_tick: Option<DateTime<Local>>,
    current_app: Option<AppIdentity>,
    paused: bool,
    last_hint: Option<PetStateHint>,
    pending: HashMap<UsageBucketKey, PendingBucketEntry>,
    pending_deductions: HashMap<UsageBucketKey, i64>,
    recent_credits: VecDeque<CreditedSlice>,
    secs_since_flush: i64,
    consecutive_flush_failures: u32,
    ticks_until_retry: u8,
}

impl TrackerCore {
    pub fn new(initial_paused: bool) -> Self {
        Self {
            last_tick: None,
            current_app: None,
            paused: initial_paused,
            last_hint: None,
            pending: HashMap::new(),
            pending_deductions: HashMap::new(),
            recent_credits: VecDeque::new(),
            secs_since_flush: 0,
            consecutive_flush_failures: 0,
            ticks_until_retry: 0,
        }
    }

    pub fn has_unflushed_data(&self) -> bool {
        !self.pending.is_empty() || !self.pending_deductions.is_empty()
    }

    pub fn set_paused(&mut self, paused: bool) -> TickOutcome {
        self.paused = paused;
        if paused {
            self.current_app = None;
            self.last_tick = None;
            self.recent_credits.clear();
        }
        let hint = if paused {
            PetStateHint::Paused
        } else {
            PetStateHint::Active
        };
        let changed = self.update_hint(hint);
        TickOutcome {
            should_flush: paused
                && self.consecutive_flush_failures == 0
                && self.has_unflushed_data(),
            state_hint_changed: changed,
        }
    }

    /// Advance tracking by one sample at local timestamp `now`.
    pub fn record_sample(&mut self, now: DateTime<Local>, sample: ForegroundSample) -> TickOutcome {
        let retry_due = self.advance_retry_countdown();

        if self.paused {
            self.last_tick = None;
            self.current_app = None;
            self.recent_credits.clear();
            let changed = self.update_hint(PetStateHint::Paused);
            let should_flush = if self.consecutive_flush_failures > 0 {
                retry_due
            } else {
                self.has_unflushed_data()
            };
            return TickOutcome {
                should_flush,
                state_hint_changed: changed,
            };
        }

        let prev_tick = self.last_tick;
        self.last_tick = Some(now);

        // Compute clamped positive delta (prevents sleep/hibernate/clock-jump spikes)
        let raw_delta = prev_tick
            .map(|prev| now.timestamp() - prev.timestamp())
            .unwrap_or(0);
        let delta_secs = if raw_delta <= 0 || raw_delta > MAX_TICK_DELTA_SECS {
            if raw_delta > MAX_TICK_DELTA_SECS {
                // Wake from sleep or large forward jump: do not credit gap and clear rollback history
                // so pre-sleep active work cannot be retroactively rolled back.
                self.recent_credits.clear();
            }
            0
        } else {
            raw_delta
        };

        let mut should_flush = retry_due;

        // Credit the elapsed interval to the app that was active during [prev_tick, now),
        // only when the current sample confirms the user wasn't locked/idle across the tick.
        if delta_secs > 0 {
            if let Some(prev_start) = prev_tick {
                if let Some(active_app) = self.current_app.clone() {
                    if !matches!(
                        sample,
                        ForegroundSample::Idle { .. } | ForegroundSample::Locked
                    ) {
                        self.credit_interval(prev_start, delta_secs, &active_app);
                        self.secs_since_flush += delta_secs;
                    }
                }
            }
        }

        let hint = match sample {
            ForegroundSample::Active(app) => {
                let switched = self
                    .current_app
                    .as_ref()
                    .map(|prev| prev.exe_name != app.exe_name)
                    .unwrap_or(false);
                if switched && self.consecutive_flush_failures == 0 && self.has_unflushed_data() {
                    should_flush = true;
                }
                self.current_app = Some(app);
                PetStateHint::Active
            }
            ForegroundSample::Idle { idle_secs } => {
                let rolled_back = self.rollback_idle_tail(now.timestamp(), idle_secs);
                if (self.current_app.is_some() || rolled_back)
                    && self.consecutive_flush_failures == 0
                    && self.has_unflushed_data()
                {
                    should_flush = true;
                }
                self.current_app = None;
                PetStateHint::IdleSleep
            }
            ForegroundSample::Locked => {
                if self.current_app.is_some()
                    && self.consecutive_flush_failures == 0
                    && self.has_unflushed_data()
                {
                    should_flush = true;
                }
                self.current_app = None;
                PetStateHint::IdleSleep
            }
            ForegroundSample::SelfOrExcluded | ForegroundSample::None => {
                // Do not count Piku itself, shell chrome, or unresolved transitional frames,
                // while keeping Piku awake when the user interacts with them.
                if self.current_app.is_some()
                    && self.consecutive_flush_failures == 0
                    && self.has_unflushed_data()
                {
                    should_flush = true;
                }
                self.current_app = None;
                PetStateHint::Active
            }
        };

        if self.consecutive_flush_failures == 0
            && self.secs_since_flush >= PERIODIC_FLUSH_SECS
            && self.has_unflushed_data()
        {
            should_flush = true;
        }

        let state_hint_changed = self.update_hint(hint);
        TickOutcome {
            should_flush,
            state_hint_changed,
        }
    }

    /// Split `[start, start + delta_secs)` across exact local date & hour boundaries
    /// and record timestamped slices so a subsequent Idle transition can roll back
    /// only the exact inactive tail without touching legitimate active usage.
    fn credit_interval(&mut self, start: DateTime<Local>, delta_secs: i64, app: &AppIdentity) {
        let mut last_ts = start.timestamp();
        for offset in 0..delta_secs {
            let instant = start + Duration::seconds(offset);
            let sec_ts = instant.timestamp();
            last_ts = sec_ts + 1;

            let local_date = instant.format("%Y-%m-%d").to_string();
            let local_hour = instant.hour() as u8;
            let key = UsageBucketKey {
                local_date,
                local_hour,
                exe_name: app.exe_name.clone(),
            };
            self.pending
                .entry(key.clone())
                .and_modify(|e| {
                    e.active_secs += 1;
                    if e.exe_path.is_none() && app.exe_path.is_some() {
                        e.exe_path = app.exe_path.clone();
                    }
                })
                .or_insert_with(|| PendingBucketEntry {
                    exe_path: app.exe_path.clone(),
                    display_name: app.display_name.clone(),
                    active_secs: 1,
                });

            self.push_recent_credit(sec_ts, key);
        }

        let prune_before = last_ts - MAX_RECENT_CREDIT_HISTORY_SECS;
        while let Some(front) = self.recent_credits.front() {
            if front.end_ts <= prune_before {
                self.recent_credits.pop_front();
            } else {
                break;
            }
        }
    }

    fn push_recent_credit(&mut self, sec_ts: i64, key: UsageBucketKey) {
        if let Some(last) = self.recent_credits.back_mut() {
            if last.key == key && last.end_ts == sec_ts {
                last.end_ts = sec_ts + 1;
                return;
            }
        }
        self.recent_credits.push_back(CreditedSlice {
            start_ts: sec_ts,
            end_ts: sec_ts + 1,
            key,
        });
    }

    /// Remove credited seconds falling within `[now_ts - idle_secs, now_ts)` from pending
    /// buffers first and queue any already-flushed remainder as SQLite bucket deductions.
    /// Idempotent across consecutive Idle ticks because rolled-back slices are trimmed.
    fn rollback_idle_tail(&mut self, now_ts: i64, idle_secs: u64) -> bool {
        if idle_secs == 0 || self.recent_credits.is_empty() {
            return false;
        }
        let clamped_idle = idle_secs.min(MAX_IDLE_ROLLBACK_SECS) as i64;
        let idle_start_ts = now_ts - clamped_idle;

        let mut retained = VecDeque::with_capacity(self.recent_credits.len());
        let mut deductions: Vec<(UsageBucketKey, i64)> = Vec::new();

        while let Some(slice) = self.recent_credits.pop_front() {
            if slice.end_ts <= idle_start_ts {
                retained.push_back(slice);
                continue;
            }

            // Preserve any active prefix strictly before idle_start_ts
            if slice.start_ts < idle_start_ts {
                retained.push_back(CreditedSlice {
                    start_ts: slice.start_ts,
                    end_ts: idle_start_ts,
                    key: slice.key.clone(),
                });
            }

            let overlap_start = slice.start_ts.max(idle_start_ts);
            let overlap_end = slice.end_ts.min(now_ts);
            let overlap_secs = overlap_end - overlap_start;
            if overlap_secs > 0 {
                deductions.push((slice.key, overlap_secs));
            }
        }

        self.recent_credits = retained;

        let mut any_deducted = false;
        for (key, secs) in deductions {
            self.apply_bucket_deduction(key, secs);
            any_deducted = true;
        }
        any_deducted
    }

    fn apply_bucket_deduction(&mut self, key: UsageBucketKey, deduct_secs: i64) {
        let mut remaining = deduct_secs;
        if let Some(entry) = self.pending.get_mut(&key) {
            let from_pending = entry.active_secs.min(remaining);
            entry.active_secs -= from_pending;
            remaining -= from_pending;
            if entry.active_secs <= 0 {
                self.pending.remove(&key);
            }
        }
        if remaining > 0 {
            *self.pending_deductions.entry(key).or_insert(0) += remaining;
        }
    }

    fn advance_retry_countdown(&mut self) -> bool {
        if self.consecutive_flush_failures == 0 || !self.has_unflushed_data() {
            self.ticks_until_retry = 0;
            return false;
        }
        if self.ticks_until_retry > 1 {
            self.ticks_until_retry -= 1;
            false
        } else {
            self.ticks_until_retry = 0;
            true
        }
    }

    pub fn take_flush_batch(&mut self) -> FlushBatch {
        self.secs_since_flush = 0;
        self.ticks_until_retry = 0;
        FlushBatch {
            additions: std::mem::take(&mut self.pending),
            deductions: std::mem::take(&mut self.pending_deductions),
        }
    }

    pub fn mark_flush_succeeded(&mut self) {
        self.consecutive_flush_failures = 0;
        self.ticks_until_retry = 0;
    }

    pub fn restore_failed_flush(&mut self, failed: FlushBatch) {
        for (k, v) in failed.additions {
            self.pending
                .entry(k)
                .and_modify(|e| e.active_secs += v.active_secs)
                .or_insert(v);
        }
        for (k, secs) in failed.deductions {
            if secs > 0 {
                self.apply_bucket_deduction(k, secs);
            }
        }
        if self.has_unflushed_data() {
            self.consecutive_flush_failures = self.consecutive_flush_failures.saturating_add(1);
            self.ticks_until_retry = retry_delay_ticks(self.consecutive_flush_failures);
        }
    }

    #[cfg(test)]
    pub fn take_pending(&mut self) -> HashMap<UsageBucketKey, PendingBucketEntry> {
        self.take_flush_batch().additions
    }

    #[cfg(test)]
    pub fn restore_pending(&mut self, failed: HashMap<UsageBucketKey, PendingBucketEntry>) {
        self.restore_failed_flush(FlushBatch {
            additions: failed,
            deductions: HashMap::new(),
        });
    }

    #[cfg(test)]
    pub fn pending_total_secs(&self) -> i64 {
        self.pending.values().map(|v| v.active_secs).sum()
    }

    #[cfg(test)]
    pub fn pending_deductions_total_secs(&self) -> i64 {
        self.pending_deductions.values().sum()
    }

    fn update_hint(&mut self, next: PetStateHint) -> Option<PetStateHint> {
        if self.last_hint == Some(next) {
            None
        } else {
            self.last_hint = Some(next);
            Some(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local_dt(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, m, d, h, min, s)
            .earliest()
            .expect("valid local datetime")
    }

    fn app(exe: &str, name: &str) -> ForegroundSample {
        ForegroundSample::Active(AppIdentity::normalized(Some(exe), None, Some(name)))
    }

    #[test]
    fn attributes_active_time_and_handles_app_switching() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 10, 0, 0);

        core.record_sample(t0, app("code.exe", "VS Code"));
        core.record_sample(t0 + Duration::seconds(2), app("code.exe", "VS Code"));
        // Switch to Chrome at t0+3 -> credits [t0+2, t0+3) to VS Code and triggers flush
        let out = core.record_sample(t0 + Duration::seconds(3), app("chrome.exe", "Chrome"));
        assert!(out.should_flush);
        core.record_sample(t0 + Duration::seconds(5), app("chrome.exe", "Chrome"));

        let pending = core.take_pending();
        let code = pending
            .get(&UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 10,
                exe_name: "code.exe".to_string(),
            })
            .unwrap();
        let chrome = pending
            .get(&UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 10,
                exe_name: "chrome.exe".to_string(),
            })
            .unwrap();
        assert_eq!(code.active_secs, 3);
        assert_eq!(chrome.active_secs, 2);
    }

    #[test]
    fn excludes_idle_lock_and_piku_self_time() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 14, 0, 0);

        core.record_sample(t0, app("code.exe", "VS Code"));
        core.record_sample(t0 + Duration::seconds(2), app("code.exe", "VS Code"));

        // Enter idle -> emits IdleSleep, does not credit idle gap (idle_secs=0 tests pure non-crediting)
        let idle_out = core.record_sample(
            t0 + Duration::seconds(4),
            ForegroundSample::Idle { idle_secs: 0 },
        );
        assert_eq!(idle_out.state_hint_changed, Some(PetStateHint::IdleSleep));
        assert!(idle_out.should_flush);

        // Stay locked -> 0 seconds credited
        core.record_sample(t0 + Duration::seconds(6), ForegroundSample::Locked);

        // User clicks Piku itself -> awake, 0 seconds credited to Piku
        let self_out =
            core.record_sample(t0 + Duration::seconds(8), ForegroundSample::SelfOrExcluded);
        assert_eq!(self_out.state_hint_changed, Some(PetStateHint::Active));
        core.record_sample(t0 + Duration::seconds(10), ForegroundSample::SelfOrExcluded);

        assert_eq!(core.pending_total_secs(), 2);
    }

    #[test]
    fn splits_midnight_and_hour_boundaries_cleanly() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 23, 59, 58);

        core.record_sample(t0, app("firefox.exe", "Firefox"));
        // 5-second tick crossing 23:59:58 -> 00:00:03 next day
        core.record_sample(t0 + Duration::seconds(5), app("firefox.exe", "Firefox"));

        let pending = core.take_pending();
        let prev_day = pending
            .get(&UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 23,
                exe_name: "firefox.exe".to_string(),
            })
            .unwrap();
        let next_day = pending
            .get(&UsageBucketKey {
                local_date: "2026-11-24".to_string(),
                local_hour: 0,
                exe_name: "firefox.exe".to_string(),
            })
            .unwrap();
        assert_eq!(prev_day.active_secs, 2); // 23:59:58, 23:59:59
        assert_eq!(next_day.active_secs, 3); // 00:00:00, 00:00:01, 00:00:02
    }

    #[test]
    fn clamps_sleep_or_hibernate_time_jumps() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 11, 0, 0);

        core.record_sample(t0, app("code.exe", "VS Code"));
        core.record_sample(t0 + Duration::seconds(1), app("code.exe", "VS Code"));
        // Simulate 2-hour laptop sleep jump -> must credit 0 seconds for the jump!
        core.record_sample(t0 + Duration::hours(2), app("code.exe", "VS Code"));
        // Next normal 1s tick resumes tracking cleanly
        core.record_sample(
            t0 + Duration::hours(2) + Duration::seconds(1),
            app("code.exe", "VS Code"),
        );

        assert_eq!(core.pending_total_secs(), 2);
    }

    #[test]
    fn recovers_gracefully_from_malformed_or_unknown_app_metadata() {
        let empty = AppIdentity::normalized(None, None, None);
        assert_eq!(empty.exe_name, "unknown_app");
        assert_eq!(empty.display_name, "Unknown App");

        let noisy = AppIdentity::normalized(
            Some("  \u{0000}\nmy_custom-tool.EXE\r\n "),
            Some("C:\\Tools\\my_custom-tool.EXE"),
            Some("\u{0007}   "),
        );
        assert_eq!(noisy.exe_name, "my_custom-tool.exe");
        assert_eq!(noisy.display_name, "My Custom Tool");

        assert!(is_excluded_process("piku.exe"));
        assert!(is_excluded_process("lockapp.exe"));
        assert!(!is_excluded_process("code.exe"));
    }

    #[test]
    fn restores_pending_buffer_on_failed_flush() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 15, 0, 0);
        core.record_sample(t0, app("code.exe", "VS Code"));
        core.record_sample(t0 + Duration::seconds(4), app("code.exe", "VS Code"));

        let taken = core.take_pending();
        assert_eq!(core.pending_total_secs(), 0);
        core.restore_pending(taken);
        assert_eq!(core.pending_total_secs(), 4);
    }

    #[test]
    fn idle_rollback_removes_in_memory_tail_without_subtracting_active_usage() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 10, 0, 0);

        // 20s active + 10s inactive tail before Idle(idle_secs=10)
        for s in 0..=29 {
            core.record_sample(t0 + Duration::seconds(s), app("chrome.exe", "Chrome"));
        }
        assert_eq!(core.pending_total_secs(), 29);

        let out = core.record_sample(
            t0 + Duration::seconds(30),
            ForegroundSample::Idle { idle_secs: 10 },
        );
        assert_eq!(out.state_hint_changed, Some(PetStateHint::IdleSleep));
        assert!(out.should_flush);
        // Active [0..20) preserved; idle tail [20..29) removed from pending cleanly
        assert_eq!(core.pending_total_secs(), 20);
        assert_eq!(core.pending_deductions_total_secs(), 0);

        // Consecutive Idle tick is strictly idempotent (no double deduction)
        let out2 = core.record_sample(
            t0 + Duration::seconds(31),
            ForegroundSample::Idle { idle_secs: 11 },
        );
        assert_eq!(out2.state_hint_changed, None);
        assert_eq!(core.pending_total_secs(), 20);
        assert_eq!(core.pending_deductions_total_secs(), 0);
    }

    #[test]
    fn failed_flush_retries_promptly_with_backoff_and_no_busy_loop() {
        let mut core = TrackerCore::new(false);
        let t0 = local_dt(2026, 11, 23, 16, 0, 0);

        core.record_sample(t0, app("code.exe", "VS Code"));
        core.record_sample(t0 + Duration::seconds(2), app("code.exe", "VS Code"));
        let batch = core.take_flush_batch();
        assert_eq!(batch.additions.values().map(|v| v.active_secs).sum::<i64>(), 2);

        // Simulate first DB flush failure -> retry should become due in 2 ticks (~2s), NOT 30s, NOT 1s busy-loop
        core.restore_failed_flush(batch);
        assert_eq!(core.pending_total_secs(), 2);

        let tick1 = core.record_sample(t0 + Duration::seconds(3), app("code.exe", "VS Code"));
        assert!(!tick1.should_flush, "must not busy-loop on immediate next tick");

        let tick2 = core.record_sample(t0 + Duration::seconds(4), app("code.exe", "VS Code"));
        assert!(tick2.should_flush, "retry must fire promptly on 2nd tick (~2s)");

        // Simulate second consecutive failure -> backs off to 5 ticks (no busy-loop)
        let batch2 = core.take_flush_batch();
        core.restore_failed_flush(batch2);
        for offset in 5..=8 {
            let tick = core.record_sample(t0 + Duration::seconds(offset), app("code.exe", "VS Code"));
            assert!(!tick.should_flush, "must back off during consecutive failures (tick {offset})");
        }
        let tick_retry2 = core.record_sample(t0 + Duration::seconds(9), app("code.exe", "VS Code"));
        assert!(tick_retry2.should_flush, "second failure retry should trigger on 5th tick");

        // After success, failure count resets cleanly
        let _ = core.take_flush_batch();
        core.mark_flush_succeeded();
        let after_ok = core.record_sample(t0 + Duration::seconds(10), app("code.exe", "VS Code"));
        assert!(!after_ok.should_flush);
    }

    #[test]
    fn excludes_shellhost_pickerhost_and_desktop_explorer_classes() {
        assert!(is_excluded_process("shellhost.exe"));
        assert!(is_excluded_process("pickerhost.exe"));
        assert!(is_excluded_process("applicationframehost.exe"));
        assert!(!is_excluded_process("explorer.exe"));
        assert!(!is_excluded_process("chrome.exe"));

        assert!(is_excluded_explorer_window_class("Progman"));
        assert!(is_excluded_explorer_window_class("WorkerW"));
        assert!(is_excluded_explorer_window_class("Shell_TrayWnd"));
        assert!(is_excluded_explorer_window_class("Shell_SecondaryTrayWnd"));
        assert!(is_excluded_explorer_window_class("NotifyIconOverflowWindow"));
        assert!(is_excluded_explorer_window_class("TaskListThumbnailWnd"));
        assert!(!is_excluded_explorer_window_class("CabinetWClass"));
    }
}
