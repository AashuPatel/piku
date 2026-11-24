use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Current SQLite schema version stored in `PRAGMA user_version`.
pub const CURRENT_SCHEMA_VERSION: i32 = 1;

const BACKUP_DIR_NAME: &str = "backups";
const QUARANTINE_DIR_NAME: &str = "quarantine";
const BACKUP_PRIMARY_NAME: &str = "piku.db.bak.1";
const BACKUP_SECONDARY_NAME: &str = "piku.db.bak.2";
const BACKUP_TEMP_NAME: &str = "piku.db.bak.tmp";

/// Runtime-owned pet state persisted by `AppState::persist()` without touching unrelated preferences.
#[derive(Debug, Clone, Copy)]
pub struct RuntimePetSettings {
    pub tracking_paused: bool,
    pub pet_visible: bool,
    pub has_saved_position: bool,
    pub pet_x: i32,
    pub pet_y: i32,
}

/// Result reported by SQLite `PRAGMA wal_checkpoint(TRUNCATE)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalCheckpointStatus {
    pub busy: i64,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

enum OpenAttemptError {
    NewerSchemaVersion { found: i32, supported: i32 },
    CorruptOrFailed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub app_id: i64,
    pub exe_name: String,
    pub display_name: String,
    pub active_secs: i64,
    pub icon_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaySummary {
    pub date: String,
    pub total_secs: i64,
    pub apps: Vec<AppUsage>,
    pub hourly_secs: Vec<i64>, // length 24 (local hours 0..23)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTotal {
    pub date: String,
    pub total_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub tracking_paused: bool,
    pub pet_visible: bool,
    pub pet_x: i32,
    pub pet_y: i32,
    pub has_saved_position: bool,
    pub pet_size: u32,
    pub idle_threshold_secs: u64,
    pub cute_moments_enabled: bool,
    pub cute_frequency: String, // "low" | "normal" | "off"
    pub launch_on_startup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tracking_paused: false,
            pet_visible: true,
            pet_x: 0,
            pet_y: 0,
            has_saved_position: false,
            pet_size: 148,
            idle_threshold_secs: 300,
            cute_moments_enabled: true,
            cute_frequency: "normal".to_string(),
            launch_on_startup: false,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub tracking_paused: Option<bool>,
    pub pet_visible: Option<bool>,
    pub pet_x: Option<i32>,
    pub pet_y: Option<i32>,
    pub pet_size: Option<u32>,
    pub idle_threshold_secs: Option<u64>,
    pub cute_moments_enabled: Option<bool>,
    pub cute_frequency: Option<String>,
    pub launch_on_startup: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UsageBucketKey {
    pub local_date: String,
    pub local_hour: u8,
    pub exe_name: String,
}

#[derive(Debug, Clone)]
pub struct PendingBucketEntry {
    pub exe_path: Option<String>,
    pub display_name: String,
    pub active_secs: i64,
}

pub struct Database {
    conn: Mutex<Connection>,
    app_data_dir: Option<PathBuf>,
    icon_cache_dir: PathBuf,
}

impl Database {
    pub fn open(app_data_dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(app_data_dir).map_err(|e| format!("create app data dir: {e}"))?;
        let icon_cache_dir = app_data_dir.join("icon-cache");
        let _ = fs::create_dir_all(&icon_cache_dir);

        let db_path = app_data_dir.join("piku.db");

        // Attempt normal open FIRST so SQLite can natively replay valid WAL frames.
        match Self::try_open_and_validate(
            &db_path,
            Some(app_data_dir.to_path_buf()),
            icon_cache_dir.clone(),
        ) {
            Ok(db) => {
                db.migrate_legacy_pet_state_if_needed(app_data_dir);
                let primary_bak = app_data_dir.join(BACKUP_DIR_NAME).join(BACKUP_PRIMARY_NAME);
                if !primary_bak.exists() {
                    if let Err(err) = db.create_rolling_backup() {
                        eprintln!("[Piku db] initial rolling backup skipped: {err}");
                    }
                }
                return Ok(db);
            }
            Err(OpenAttemptError::NewerSchemaVersion { found, supported }) => {
                // Never quarantine or overwrite a database created by a newer Piku version.
                return Err(format!(
                    "Database schema version {found} is newer than supported version {supported}; please update Piku."
                ));
            }
            Err(OpenAttemptError::CorruptOrFailed(reason)) => {
                eprintln!("[Piku recovery] database open/integrity check failed: {reason}");
                Self::recover_from_corruption(app_data_dir, &db_path, icon_cache_dir, &reason)
            }
        }
    }

    fn try_open_and_validate(
        db_path: &Path,
        app_data_dir: Option<PathBuf>,
        icon_cache_dir: PathBuf,
    ) -> Result<Self, OpenAttemptError> {
        // Check non-empty existing files for the 16-byte SQLite magic header BEFORE opening
        // read-write so SQLite cannot unlink/modify sidecar WAL/SHM files prior to quarantine.
        if db_path.exists() {
            let meta = fs::metadata(db_path).map_err(|e| {
                OpenAttemptError::CorruptOrFailed(format!("stat database file: {e}"))
            })?;
            if meta.len() > 0 {
                const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";
                let mut header = [0u8; 16];
                let read_ok = fs::File::open(db_path)
                    .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut header));
                if read_ok.is_err() || &header != SQLITE_MAGIC {
                    return Err(OpenAttemptError::CorruptOrFailed(
                        "file is not a valid SQLite database (header mismatch)".to_string(),
                    ));
                }
            }
        }

        let conn = Connection::open(db_path)
            .map_err(|e| OpenAttemptError::CorruptOrFailed(format!("open sqlite: {e}")))?;

        // Check schema version BEFORE executing any DDL so newer databases are never mutated.
        let user_version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| OpenAttemptError::CorruptOrFailed(format!("read user_version: {e}")))?;
        if user_version > CURRENT_SCHEMA_VERSION {
            drop(conn);
            return Err(OpenAttemptError::NewerSchemaVersion {
                found: user_version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }

        let db = Self::init_connection(conn, app_data_dir, icon_cache_dir)
            .map_err(OpenAttemptError::CorruptOrFailed)?;

        if let Err(err) = db.verify_integrity() {
            drop(db);
            return Err(OpenAttemptError::CorruptOrFailed(err));
        }

        Ok(db)
    }

    fn recover_from_corruption(
        app_data_dir: &Path,
        db_path: &Path,
        icon_cache_dir: PathBuf,
        reason: &str,
    ) -> Result<Self, String> {
        // Step 1: Preserve original piku.db + WAL + SHM as a timestamped quarantine set FIRST.
        // If preserving the original files fails, abort immediately so user data is never overwritten.
        let quarantine_dir = quarantine_database_set(app_data_dir, db_path)?;
        eprintln!(
            "[Piku recovery] quarantined original database set to {} (reason: {reason})",
            quarantine_dir.display()
        );

        // Step 2: Validate rolling backups in newest-first order before restoring.
        let backups_dir = app_data_dir.join(BACKUP_DIR_NAME);
        for name in [BACKUP_PRIMARY_NAME, BACKUP_SECONDARY_NAME] {
            let candidate = backups_dir.join(name);
            if !candidate.exists() {
                continue;
            }
            match validate_backup_file(&candidate) {
                Ok(()) => {
                    let restore_tmp = app_data_dir.join("piku.db.restore.tmp");
                    let _ = fs::remove_file(&restore_tmp);
                    if let Err(err) = fs::copy(&candidate, &restore_tmp)
                        .and_then(|_| fs::rename(&restore_tmp, db_path))
                    {
                        let _ = fs::remove_file(&restore_tmp);
                        eprintln!(
                            "[Piku recovery] failed to restore backup {}: {err}",
                            candidate.display()
                        );
                        continue;
                    }
                    match Self::try_open_and_validate(
                        db_path,
                        Some(app_data_dir.to_path_buf()),
                        icon_cache_dir.clone(),
                    ) {
                        Ok(db) => {
                            eprintln!(
                                "[Piku recovery] restored database successfully from validated backup {}",
                                candidate.display()
                            );
                            return Ok(db);
                        }
                        Err(OpenAttemptError::NewerSchemaVersion { found, supported }) => {
                            let _ = fs::remove_file(db_path);
                            return Err(format!(
                                "Backup schema version {found} is newer than supported version {supported}"
                            ));
                        }
                        Err(OpenAttemptError::CorruptOrFailed(open_err)) => {
                            let _ = fs::remove_file(db_path);
                            eprintln!(
                                "[Piku recovery] restored backup {} failed open validation: {open_err}",
                                candidate.display()
                            );
                        }
                    }
                }
                Err(err) => {
                    eprintln!(
                        "[Piku recovery] skipping invalid backup {}: {err}",
                        candidate.display()
                    );
                }
            }
        }

        // Step 3: Safe first-run fallback ONLY after original files are safely quarantined.
        let db = Self::try_open_and_validate(
            db_path,
            Some(app_data_dir.to_path_buf()),
            icon_cache_dir,
        )
        .map_err(|e| match e {
            OpenAttemptError::NewerSchemaVersion { found, supported } => {
                format!("Fresh database schema mismatch: {found} > {supported}")
            }
            OpenAttemptError::CorruptOrFailed(msg) => {
                format!("Failed to initialize fresh fallback database after quarantine: {msg}")
            }
        })?;
        eprintln!(
            "[Piku recovery] no valid backup found; original files preserved in {}, initialized fresh database",
            quarantine_dir.display()
        );
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_in_memory(icon_cache_dir: PathBuf) -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("open in-memory sqlite: {e}"))?;
        Self::init_connection(conn, None, icon_cache_dir)
    }

    fn init_connection(
        conn: Connection,
        app_data_dir: Option<PathBuf>,
        icon_cache_dir: PathBuf,
    ) -> Result<Self, String> {
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS apps (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                exe_name      TEXT NOT NULL UNIQUE,
                exe_path      TEXT,
                display_name  TEXT NOT NULL,
                icon_hash     TEXT,
                first_seen_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS usage_hourly (
                local_date    TEXT NOT NULL,
                local_hour    INTEGER NOT NULL CHECK (local_hour >= 0 AND local_hour <= 23),
                app_id        INTEGER NOT NULL REFERENCES apps(id),
                active_secs   INTEGER NOT NULL DEFAULT 0 CHECK (active_secs >= 0),
                PRIMARY KEY (local_date, local_hour, app_id)
            );

            CREATE INDEX IF NOT EXISTS idx_usage_date ON usage_hourly(local_date);

            CREATE TABLE IF NOT EXISTS settings (
                key           TEXT PRIMARY KEY,
                value         TEXT NOT NULL
            );
            ",
        )
        .map_err(|e| format!("init sqlite schema: {e}"))?;

        let current_ver: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| format!("read user_version: {e}"))?;
        if current_ver < CURRENT_SCHEMA_VERSION {
            conn.execute_batch(&format!("PRAGMA user_version = {CURRENT_SCHEMA_VERSION};"))
                .map_err(|e| format!("set user_version: {e}"))?;
        }

        Ok(Self {
            conn: Mutex::new(conn),
            app_data_dir,
            icon_cache_dir,
        })
    }

    pub fn verify_integrity(&self) -> Result<(), String> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        verify_connection_integrity(&conn)
    }

    /// Runs `PRAGMA wal_checkpoint(TRUNCATE)` and strictly validates the returned status.
    /// Returns `Err` if SQLite reported busy or left frames un-checkpointed.
    pub fn checkpoint_wal(&self) -> Result<WalCheckpointStatus, String> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let (busy, log_frames, checkpointed_frames): (i64, i64, i64) = conn
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(|e| format!("wal_checkpoint(TRUNCATE): {e}"))?;

        let status = WalCheckpointStatus {
            busy,
            log_frames,
            checkpointed_frames,
        };
        if busy != 0 || (log_frames > 0 && checkpointed_frames < log_frames) {
            return Err(format!(
                "WAL checkpoint incomplete (busy={busy}, log={log_frames}, checkpointed={checkpointed_frames})"
            ));
        }
        Ok(status)
    }

    /// Creates a SQLite-consistent standalone backup snapshot via `VACUUM INTO` (capturing active
    /// WAL data safely under `self.conn` lock), validates it with `integrity_check` and
    /// `foreign_key_check`, and atomically promotes it while retaining at most 2 snapshots.
    pub fn create_rolling_backup(&self) -> Result<PathBuf, String> {
        let Some(ref data_dir) = self.app_data_dir else {
            return Err("rolling backup unavailable for in-memory database".to_string());
        };

        let backups_dir = data_dir.join(BACKUP_DIR_NAME);
        fs::create_dir_all(&backups_dir).map_err(|e| format!("create backups dir: {e}"))?;

        let tmp_path = backups_dir.join(BACKUP_TEMP_NAME);
        let bak1_path = backups_dir.join(BACKUP_PRIMARY_NAME);
        let bak2_path = backups_dir.join(BACKUP_SECONDARY_NAME);

        // Hold DB connection lock during VACUUM INTO so no concurrent mutation runs during snapshot.
        {
            let conn = self.conn.lock().expect("db mutex poisoned");
            if tmp_path.exists() {
                let _ = fs::remove_file(&tmp_path);
            }
            let tmp_str = tmp_path
                .to_str()
                .ok_or_else(|| "backup temp path is not valid UTF-8".to_string())?;
            conn.execute("VACUUM INTO ?1", params![tmp_str])
                .map_err(|e| format!("VACUUM INTO backup: {e}"))?;
        }

        // Validate temporary snapshot before touching any existing valid backup.
        if let Err(err) = validate_backup_file(&tmp_path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(format!("backup validation failed (existing backups untouched): {err}"));
        }

        // Rotate: bak.1 -> bak.2 (max 2 snapshots), then promote validated tmp -> bak.1.
        if bak1_path.exists() {
            if bak2_path.exists() {
                let _ = fs::remove_file(&bak2_path);
            }
            fs::rename(&bak1_path, &bak2_path)
                .map_err(|e| format!("rotate primary backup to secondary: {e}"))?;
        }
        fs::rename(&tmp_path, &bak1_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            format!("promote validated backup: {e}")
        })?;

        Ok(bak1_path)
    }

    pub fn icon_cache_dir(&self) -> &Path {
        &self.icon_cache_dir
    }

    #[cfg(test)]
    pub fn set_busy_timeout_for_test(&self, ms: i64) {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let _ = conn.execute_batch(&format!("PRAGMA busy_timeout = {ms};"));
    }

    /// One-time migration of Phase 2's pet_state.json into SQLite settings.
    fn migrate_legacy_pet_state_if_needed(&self, app_data_dir: &Path) {
        let legacy_path = app_data_dir.join("pet_state.json");
        if !legacy_path.exists() {
            return;
        }
        let has_settings: bool = {
            let conn = self.conn.lock().expect("db mutex poisoned");
            conn.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get::<_, i64>(0))
                .map(|c| c > 0)
                .unwrap_or(false)
        };
        if has_settings {
            return;
        }

        #[derive(Deserialize)]
        struct LegacyState {
            x: i32,
            y: i32,
            tracking_paused: bool,
            pet_visible: bool,
        }

        if let Ok(raw) = fs::read_to_string(&legacy_path) {
            if let Ok(legacy) = serde_json::from_str::<LegacyState>(&raw) {
                let mut s = AppSettings::default();
                s.pet_x = legacy.x;
                s.pet_y = legacy.y;
                s.has_saved_position = true;
                s.tracking_paused = legacy.tracking_paused;
                s.pet_visible = legacy.pet_visible;
                let _ = self.save_settings(&s);
            }
        }
    }

    /// Transactionally flush accumulated hourly usage buckets to SQLite.
    /// Returns list of (exe_name, exe_path) for newly seen apps that still need icon caching.
    #[cfg(test)]
    pub fn flush_usage_buckets(
        &self,
        buckets: &HashMap<UsageBucketKey, PendingBucketEntry>,
        now_unix: i64,
    ) -> Result<Vec<(String, String)>, String> {
        let empty_deductions: HashMap<UsageBucketKey, i64> = HashMap::new();
        self.flush_usage_delta(buckets, &empty_deductions, now_unix)
    }

    /// Transactionally apply both active-usage additions and idle-tail deductions in one commit.
    /// Deductions clamp safely at 0 and prune zeroed rows so totals can never become negative.
    pub fn flush_usage_delta(
        &self,
        additions: &HashMap<UsageBucketKey, PendingBucketEntry>,
        deductions: &HashMap<UsageBucketKey, i64>,
        now_unix: i64,
    ) -> Result<Vec<(String, String)>, String> {
        if additions.is_empty() && deductions.is_empty() {
            return Ok(Vec::new());
        }

        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin flush tx: {e}"))?;

        let mut app_id_cache: HashMap<String, i64> = HashMap::new();
        let mut needs_icon: Vec<(String, String)> = Vec::new();

        for (key, entry) in additions {
            if entry.active_secs <= 0 || key.local_hour > 23 {
                continue;
            }

            let app_id = if let Some(&id) = app_id_cache.get(&key.exe_name) {
                id
            } else {
                let existing: Option<(i64, Option<String>, String, Option<String>)> = tx
                    .query_row(
                        "SELECT id, exe_path, display_name, icon_hash FROM apps WHERE exe_name = ?1",
                        params![key.exe_name],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()
                    .map_err(|e| format!("lookup app: {e}"))?;

                let id = if let Some((id, old_path, old_name, icon_hash)) = existing {
                    let should_update_path =
                        entry.exe_path.is_some() && entry.exe_path != old_path;
                    let should_update_name =
                        !entry.display_name.is_empty()
                            && entry.display_name != "Unknown App"
                            && old_name == "Unknown App";

                    if should_update_path || should_update_name {
                        let new_path = entry.exe_path.as_ref().or(old_path.as_ref());
                        let new_name = if should_update_name {
                            &entry.display_name
                        } else {
                            &old_name
                        };
                        tx.execute(
                            "UPDATE apps SET exe_path = ?1, display_name = ?2 WHERE id = ?3",
                            params![new_path, new_name, id],
                        )
                        .map_err(|e| format!("update app metadata: {e}"))?;
                    }

                    if icon_hash.is_none() {
                        if let Some(p) = entry.exe_path.as_ref().or(old_path.as_ref()) {
                            needs_icon.push((key.exe_name.clone(), p.clone()));
                        }
                    }
                    id
                } else {
                    tx.execute(
                        "INSERT INTO apps (exe_name, exe_path, display_name, icon_hash, first_seen_at)
                         VALUES (?1, ?2, ?3, NULL, ?4)",
                        params![key.exe_name, entry.exe_path, entry.display_name, now_unix],
                    )
                    .map_err(|e| format!("insert app: {e}"))?;
                    let new_id = tx.last_insert_rowid();
                    if let Some(p) = &entry.exe_path {
                        needs_icon.push((key.exe_name.clone(), p.clone()));
                    }
                    new_id
                };
                app_id_cache.insert(key.exe_name.clone(), id);
                id
            };

            tx.execute(
                "INSERT INTO usage_hourly (local_date, local_hour, app_id, active_secs)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(local_date, local_hour, app_id)
                 DO UPDATE SET active_secs = active_secs + excluded.active_secs",
                params![key.local_date, key.local_hour as i64, app_id, entry.active_secs],
            )
            .map_err(|e| format!("upsert usage_hourly: {e}"))?;
        }

        for (key, &deduct_secs) in deductions {
            if deduct_secs <= 0 || key.local_hour > 23 {
                continue;
            }

            let app_id: Option<i64> = if let Some(&id) = app_id_cache.get(&key.exe_name) {
                Some(id)
            } else {
                let id = tx
                    .query_row(
                        "SELECT id FROM apps WHERE exe_name = ?1",
                        params![key.exe_name],
                        |r| r.get::<_, i64>(0),
                    )
                    .optional()
                    .map_err(|e| format!("lookup app for deduction: {e}"))?;
                if let Some(found) = id {
                    app_id_cache.insert(key.exe_name.clone(), found);
                }
                id
            };

            let Some(app_id) = app_id else {
                continue;
            };

            tx.execute(
                "UPDATE usage_hourly
                 SET active_secs = MAX(0, active_secs - ?1)
                 WHERE local_date = ?2 AND local_hour = ?3 AND app_id = ?4",
                params![deduct_secs, key.local_date, key.local_hour as i64, app_id],
            )
            .map_err(|e| format!("deduct usage_hourly: {e}"))?;

            tx.execute(
                "DELETE FROM usage_hourly
                 WHERE local_date = ?1 AND local_hour = ?2 AND app_id = ?3 AND active_secs <= 0",
                params![key.local_date, key.local_hour as i64, app_id],
            )
            .map_err(|e| format!("prune empty usage_hourly: {e}"))?;
        }

        tx.commit().map_err(|e| format!("commit flush tx: {e}"))?;
        Ok(needs_icon)
    }

    pub fn set_app_icon_hash(&self, exe_name: &str, icon_hash: &str) -> Result<(), String> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        conn.execute(
            "UPDATE apps SET icon_hash = ?1 WHERE exe_name = ?2",
            params![icon_hash, exe_name],
        )
        .map_err(|e| format!("update icon_hash: {e}"))?;
        Ok(())
    }

    pub fn get_day_summary(&self, date: &str) -> Result<DaySummary, String> {
        validate_iso_date(date)?;
        let conn = self.conn.lock().expect("db mutex poisoned");

        let mut app_stmt = conn
            .prepare(
                "SELECT a.id, a.exe_name, a.display_name, a.icon_hash, SUM(u.active_secs) AS total
                 FROM usage_hourly u
                 INNER JOIN apps a ON a.id = u.app_id
                 WHERE u.local_date = ?1
                 GROUP BY a.id
                 HAVING total > 0
                 ORDER BY total DESC, a.display_name ASC",
            )
            .map_err(|e| format!("prepare day apps: {e}"))?;

        let rows = app_stmt
            .query_map(params![date], |r| {
                let icon_hash: Option<String> = r.get(3)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    icon_hash,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| format!("query day apps: {e}"))?;

        let mut apps = Vec::new();
        let mut total_secs: i64 = 0;
        for row in rows {
            let (app_id, exe_name, display_name, icon_hash, active_secs) =
                row.map_err(|e| format!("read day app row: {e}"))?;
            total_secs += active_secs;
            let icon_data_url = icon_hash
                .as_deref()
                .and_then(|h| self.read_icon_data_url(h));
            apps.push(AppUsage {
                app_id,
                exe_name,
                display_name,
                active_secs,
                icon_data_url,
            });
        }

        let mut hourly_secs = vec![0i64; 24];
        let mut hour_stmt = conn
            .prepare(
                "SELECT local_hour, SUM(active_secs)
                 FROM usage_hourly
                 WHERE local_date = ?1
                 GROUP BY local_hour",
            )
            .map_err(|e| format!("prepare hourly: {e}"))?;

        let h_rows = hour_stmt
            .query_map(params![date], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| format!("query hourly: {e}"))?;

        for row in h_rows {
            let (hour, secs) = row.map_err(|e| format!("read hourly row: {e}"))?;
            if (0..24).contains(&hour) {
                hourly_secs[hour as usize] = secs;
            }
        }

        Ok(DaySummary {
            date: date.to_string(),
            total_secs,
            apps,
            hourly_secs,
        })
    }

    pub fn get_week_summary(&self, end_date_str: Option<&str>) -> Result<Vec<DailyTotal>, String> {
        let end = match end_date_str {
            Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map_err(|_| format!("invalid endDate '{s}', expected YYYY-MM-DD"))?,
            None => Local::now().date_naive(),
        };
        let start = end - Duration::days(6);
        self.get_daily_totals_range(start, end)
    }

    pub fn get_month_summary(&self, year: i32, month: u32) -> Result<Vec<DailyTotal>, String> {
        if !(1..=12).contains(&month) {
            return Err(format!("invalid month {month}, expected 1..=12"));
        }
        let start = NaiveDate::from_ymd_opt(year, month, 1)
            .ok_or_else(|| format!("invalid year/month {year}-{month}"))?;
        let next_month = if month == 12 {
            NaiveDate::from_ymd_opt(year + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(year, month + 1, 1)
        }
        .ok_or_else(|| "date overflow".to_string())?;
        let end = next_month - Duration::days(1);
        self.get_daily_totals_range(start, end)
    }

    fn get_daily_totals_range(
        &self,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<DailyTotal>, String> {
        let start_s = start.format("%Y-%m-%d").to_string();
        let end_s = end.format("%Y-%m-%d").to_string();

        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut stmt = conn
            .prepare(
                "SELECT local_date, SUM(active_secs)
                 FROM usage_hourly
                 WHERE local_date >= ?1 AND local_date <= ?2
                 GROUP BY local_date",
            )
            .map_err(|e| format!("prepare range query: {e}"))?;

        let rows = stmt
            .query_map(params![start_s, end_s], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| format!("query range totals: {e}"))?;

        let mut map: HashMap<String, i64> = HashMap::new();
        for row in rows {
            let (d, s) = row.map_err(|e| format!("read range row: {e}"))?;
            map.insert(d, s);
        }

        let mut out = Vec::new();
        let mut cur = start;
        while cur <= end {
            let key = cur.format("%Y-%m-%d").to_string();
            let total_secs = *map.get(&key).unwrap_or(&0);
            out.push(DailyTotal {
                date: key,
                total_secs,
            });
            cur += Duration::days(1);
        }
        Ok(out)
    }

    pub fn load_settings(&self) -> AppSettings {
        match self.try_load_settings() {
            Ok(s) => s,
            Err(err) => {
                eprintln!("[Piku settings] load_settings failed (returning defaults for read): {err}");
                AppSettings::default()
            }
        }
    }

    /// Strict settings read that propagates SQLite errors rather than substituting defaults,
    /// ensuring write paths can never overwrite user preferences after a failed read.
    pub fn try_load_settings(&self) -> Result<AppSettings, String> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        let mut settings = AppSettings::default();

        let mut stmt = conn
            .prepare("SELECT key, value FROM settings")
            .map_err(|e| format!("prepare load_settings: {e}"))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| format!("query load_settings: {e}"))?;

        for row in rows {
            let (k, v) = row.map_err(|e| format!("read setting row: {e}"))?;
            match k.as_str() {
                "tracking_paused" => {
                    if let Ok(b) = v.parse() {
                        settings.tracking_paused = b;
                    }
                }
                "pet_visible" => {
                    if let Ok(b) = v.parse() {
                        settings.pet_visible = b;
                    }
                }
                "pet_x" => {
                    if let Ok(n) = v.parse() {
                        settings.pet_x = n;
                    }
                }
                "pet_y" => {
                    if let Ok(n) = v.parse() {
                        settings.pet_y = n;
                    }
                }
                "has_saved_position" => {
                    if let Ok(b) = v.parse() {
                        settings.has_saved_position = b;
                    }
                }
                "pet_size" => {
                    if let Ok(n) = v.parse::<u32>() {
                        settings.pet_size = n.clamp(100, 160);
                    }
                }
                "idle_threshold_secs" => {
                    if let Ok(n) = v.parse::<u64>() {
                        settings.idle_threshold_secs = n.clamp(30, 3600);
                    }
                }
                "cute_moments_enabled" => {
                    if let Ok(b) = v.parse() {
                        settings.cute_moments_enabled = b;
                    }
                }
                "cute_frequency" => {
                    if matches!(v.as_str(), "low" | "normal" | "off") {
                        settings.cute_frequency = v;
                    }
                }
                "launch_on_startup" => {
                    if let Ok(b) = v.parse() {
                        settings.launch_on_startup = b;
                    }
                }
                _ => {}
            }
        }
        Ok(settings)
    }

    /// Transactionally persist ONLY the 5 runtime-owned keys (`tracking_paused`, `pet_visible`,
    /// `has_saved_position`, `pet_x`, `pet_y`) without reading or overwriting unrelated preferences.
    pub fn save_runtime_settings(&self, s: &RuntimePetSettings) -> Result<(), String> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin runtime settings tx: {e}"))?;
        let pairs: [(&str, String); 5] = [
            ("tracking_paused", s.tracking_paused.to_string()),
            ("pet_visible", s.pet_visible.to_string()),
            ("has_saved_position", s.has_saved_position.to_string()),
            ("pet_x", s.pet_x.to_string()),
            ("pet_y", s.pet_y.to_string()),
        ];

        for (k, v) in pairs {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![k, v],
            )
            .map_err(|e| format!("save runtime setting {k}: {e}"))?;
        }

        tx.commit()
            .map_err(|e| format!("commit runtime settings tx: {e}"))?;
        Ok(())
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<(), String> {
        let mut conn = self.conn.lock().expect("db mutex poisoned");
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin settings tx: {e}"))?;
        let pairs: [(&str, String); 10] = [
            ("tracking_paused", s.tracking_paused.to_string()),
            ("pet_visible", s.pet_visible.to_string()),
            ("pet_x", s.pet_x.to_string()),
            ("pet_y", s.pet_y.to_string()),
            ("has_saved_position", s.has_saved_position.to_string()),
            ("pet_size", s.pet_size.clamp(100, 160).to_string()),
            (
                "idle_threshold_secs",
                s.idle_threshold_secs.clamp(30, 3600).to_string(),
            ),
            ("cute_moments_enabled", s.cute_moments_enabled.to_string()),
            ("cute_frequency", s.cute_frequency.clone()),
            ("launch_on_startup", s.launch_on_startup.to_string()),
        ];

        for (k, v) in pairs {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![k, v],
            )
            .map_err(|e| format!("save setting {k}: {e}"))?;
        }

        tx.commit().map_err(|e| format!("commit settings tx: {e}"))?;
        Ok(())
    }

    pub fn update_settings(&self, patch: AppSettingsPatch) -> Result<AppSettings, String> {
        // Strict read: fail instead of substituting defaults if SQLite cannot be read.
        let mut current = self.try_load_settings()?;
        if let Some(v) = patch.tracking_paused {
            current.tracking_paused = v;
        }
        if let Some(v) = patch.pet_visible {
            current.pet_visible = v;
        }
        if let Some(v) = patch.pet_x {
            current.pet_x = v;
            current.has_saved_position = true;
        }
        if let Some(v) = patch.pet_y {
            current.pet_y = v;
            current.has_saved_position = true;
        }
        if let Some(v) = patch.pet_size {
            current.pet_size = v.clamp(100, 160);
        }
        if let Some(v) = patch.idle_threshold_secs {
            current.idle_threshold_secs = v.clamp(30, 3600);
        }
        if let Some(v) = patch.cute_moments_enabled {
            current.cute_moments_enabled = v;
        }
        if let Some(v) = patch.cute_frequency {
            if matches!(v.as_str(), "low" | "normal" | "off") {
                current.cute_frequency = v;
            }
        }
        if let Some(v) = patch.launch_on_startup {
            current.launch_on_startup = v;
        }
        self.save_settings(&current)?;
        Ok(current)
    }

    fn read_icon_data_url(&self, icon_hash: &str) -> Option<String> {
        // Basic path traversal guard
        if icon_hash.contains('/') || icon_hash.contains('\\') || icon_hash.contains("..") {
            return None;
        }
        let path = self.icon_cache_dir.join(icon_hash);
        let bytes = fs::read(path).ok()?;
        if bytes.is_empty() || bytes.len() > 64 * 1024 {
            return None;
        }
        Some(format!("data:image/png;base64,{}", encode_base64(&bytes)))
    }
}

fn verify_connection_integrity(conn: &Connection) -> Result<(), String> {
    let mut stmt = conn
        .prepare("PRAGMA integrity_check")
        .map_err(|e| format!("prepare integrity_check: {e}"))?;
    let results: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| format!("query integrity_check: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read integrity_check: {e}"))?;
    if results.len() != 1 || results[0] != "ok" {
        return Err(format!("integrity_check failed: {}", results.join("; ")));
    }

    let mut fk_stmt = conn
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| format!("prepare foreign_key_check: {e}"))?;
    let fk_count = fk_stmt
        .query_map([], |_r| Ok(()))
        .map_err(|e| format!("query foreign_key_check: {e}"))?
        .count();
    if fk_count > 0 {
        return Err(format!("foreign_key_check found {fk_count} violation(s)"));
    }
    Ok(())
}

fn validate_backup_file(path: &Path) -> Result<(), String> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("open backup read-only: {e}"))?;
    let user_ver: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| format!("read backup user_version: {e}"))?;
    if user_ver > CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "backup schema version {user_ver} exceeds supported {CURRENT_SCHEMA_VERSION}"
        ));
    }
    verify_connection_integrity(&conn)?;

    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('apps', 'usage_hourly', 'settings')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| format!("check backup tables: {e}"))?;
    if table_count != 3 {
        return Err(format!("backup missing core tables (found {table_count}/3)"));
    }
    Ok(())
}

fn quarantine_database_set(app_data_dir: &Path, db_path: &Path) -> Result<PathBuf, String> {
    let quarantine_root = app_data_dir.join(QUARANTINE_DIR_NAME);
    fs::create_dir_all(&quarantine_root).map_err(|e| format!("create quarantine dir: {e}"))?;

    let base_ts = Local::now().timestamp();
    let mut target_dir = quarantine_root.join(format!("corrupt-{base_ts}"));
    let mut suffix = 1u32;
    while target_dir.exists() {
        target_dir = quarantine_root.join(format!("corrupt-{base_ts}-{suffix}"));
        suffix += 1;
    }
    fs::create_dir_all(&target_dir).map_err(|e| format!("create quarantine subdir: {e}"))?;

    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("piku.db");
    let candidates = [
        (db_path.to_path_buf(), file_name.to_string()),
        (
            app_data_dir.join(format!("{file_name}-wal")),
            format!("{file_name}-wal"),
        ),
        (
            app_data_dir.join(format!("{file_name}-shm")),
            format!("{file_name}-shm"),
        ),
    ];

    for (src, name) in candidates {
        if !src.exists() {
            continue;
        }
        let dst = target_dir.join(&name);
        if let Err(rename_err) = fs::rename(&src, &dst) {
            // Fallback copy+remove only if copy succeeds completely
            fs::copy(&src, &dst)
                .map_err(|copy_err| format!("quarantine {name} failed (rename: {rename_err}; copy: {copy_err})"))?;
            fs::remove_file(&src)
                .map_err(|rm_err| format!("quarantine cleanup {name} failed: {rm_err}"))?;
        }
    }

    Ok(target_dir)
}

pub fn validate_iso_date(date: &str) -> Result<(), String> {
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map(|d| {
            let _ = d.year();
        })
        .map_err(|_| format!("invalid date '{date}', expected YYYY-MM-DD"))
}

fn encode_base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(((input.len() + 2) / 3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache_dir() -> PathBuf {
        std::env::temp_dir().join("piku_test_icon_cache")
    }

    #[test]
    fn database_flushes_aggregates_and_queries_day_week_month() {
        let db = Database::open_in_memory(temp_cache_dir()).unwrap();
        let mut buckets = HashMap::new();
        buckets.insert(
            UsageBucketKey {
                local_date: "2026-11-22".to_string(),
                local_hour: 23,
                exe_name: "code.exe".to_string(),
            },
            PendingBucketEntry {
                exe_path: Some("C:\\Apps\\Code.exe".to_string()),
                display_name: "Visual Studio Code".to_string(),
                active_secs: 120,
            },
        );
        buckets.insert(
            UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 0,
                exe_name: "code.exe".to_string(),
            },
            PendingBucketEntry {
                exe_path: Some("C:\\Apps\\Code.exe".to_string()),
                display_name: "Visual Studio Code".to_string(),
                active_secs: 45,
            },
        );
        buckets.insert(
            UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 9,
                exe_name: "chrome.exe".to_string(),
            },
            PendingBucketEntry {
                exe_path: None,
                display_name: "Google Chrome".to_string(),
                active_secs: 300,
            },
        );

        let needs_icon = db.flush_usage_buckets(&buckets, 1_700_000_000).unwrap();
        assert_eq!(needs_icon.len(), 1);
        assert_eq!(needs_icon[0].0, "code.exe");

        // Second flush accumulates onto existing bucket
        let mut second = HashMap::new();
        second.insert(
            UsageBucketKey {
                local_date: "2026-11-23".to_string(),
                local_hour: 0,
                exe_name: "code.exe".to_string(),
            },
            PendingBucketEntry {
                exe_path: Some("C:\\Apps\\Code.exe".to_string()),
                display_name: "Visual Studio Code".to_string(),
                active_secs: 15,
            },
        );
        db.flush_usage_buckets(&second, 1_700_000_100).unwrap();

        let day = db.get_day_summary("2026-11-23").unwrap();
        assert_eq!(day.total_secs, 360);
        assert_eq!(day.hourly_secs.len(), 24);
        assert_eq!(day.hourly_secs[0], 60);
        assert_eq!(day.hourly_secs[9], 300);
        assert_eq!(day.apps.len(), 2);
        assert_eq!(day.apps[0].exe_name, "chrome.exe");
        assert_eq!(day.apps[0].active_secs, 300);
        assert_eq!(day.apps[1].exe_name, "code.exe");
        assert_eq!(day.apps[1].active_secs, 60);

        let week = db.get_week_summary(Some("2026-11-23")).unwrap();
        assert_eq!(week.len(), 7);
        assert_eq!(week[5].date, "2026-11-22");
        assert_eq!(week[5].total_secs, 120);
        assert_eq!(week[6].date, "2026-11-23");
        assert_eq!(week[6].total_secs, 360);

        let month = db.get_month_summary(2026, 11).unwrap();
        assert_eq!(month.len(), 30);
        assert_eq!(month[21].total_secs, 120);
        assert_eq!(month[22].total_secs, 360);
    }

    #[test]
    fn database_persists_across_reopen_and_saves_settings() {
        let dir = std::env::temp_dir().join(format!("piku_db_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        {
            let db = Database::open(&dir).unwrap();
            let updated = db
                .update_settings(AppSettingsPatch {
                    tracking_paused: Some(true),
                    pet_x: Some(420),
                    pet_y: Some(310),
                    pet_size: Some(128),
                    idle_threshold_secs: Some(180),
                    ..Default::default()
                })
                .unwrap();
            assert!(updated.tracking_paused);
            assert_eq!(updated.pet_x, 420);
            assert_eq!(updated.pet_size, 128);

            let mut buckets = HashMap::new();
            buckets.insert(
                UsageBucketKey {
                    local_date: "2026-11-23".to_string(),
                    local_hour: 10,
                    exe_name: "notepad.exe".to_string(),
                },
                PendingBucketEntry {
                    exe_path: None,
                    display_name: "Notepad".to_string(),
                    active_secs: 90,
                },
            );
            db.flush_usage_buckets(&buckets, 1_700_000_000).unwrap();
        }

        // Reopen database from disk and verify WAL persistence
        {
            let db2 = Database::open(&dir).unwrap();
            let loaded = db2.load_settings();
            assert!(loaded.tracking_paused);
            assert_eq!(loaded.pet_x, 420);
            assert_eq!(loaded.pet_y, 310);
            assert_eq!(loaded.pet_size, 128);
            assert_eq!(loaded.idle_threshold_secs, 180);

            let day = db2.get_day_summary("2026-11-23").unwrap();
            assert_eq!(day.total_secs, 90);
            assert_eq!(day.apps[0].display_name, "Notepad");
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_invalid_dates_and_months() {
        let db = Database::open_in_memory(temp_cache_dir()).unwrap();
        assert!(db.get_day_summary("not-a-date").is_err());
        assert!(db.get_month_summary(2026, 0).is_err());
        assert!(db.get_month_summary(2026, 13).is_err());
    }

    fn unique_test_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "piku_p551_{tag}_{}_{}",
            std::process::id(),
            Local::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn seed_usage(db: &Database, exe: &str, name: &str, secs: i64) {
        let mut buckets = HashMap::new();
        buckets.insert(
            UsageBucketKey {
                local_date: "2026-11-24".to_string(),
                local_hour: 10,
                exe_name: exe.to_string(),
            },
            PendingBucketEntry {
                exe_path: Some(format!("C:\\Apps\\{exe}")),
                display_name: name.to_string(),
                active_secs: secs,
            },
        );
        db.flush_usage_buckets(&buckets, 1_700_000_000).unwrap();
    }

    #[test]
    fn runtime_settings_persist_preserves_unrelated_preferences_and_fails_safely_when_locked() {
        let dir = unique_test_dir("settings_preserve");
        let db = Database::open(&dir).unwrap();

        // Set non-default user preferences
        db.update_settings(AppSettingsPatch {
            pet_size: Some(160),
            idle_threshold_secs: Some(180),
            cute_moments_enabled: Some(false),
            cute_frequency: Some("low".to_string()),
            launch_on_startup: Some(true),
            ..Default::default()
        })
        .unwrap();

        // Targeted runtime persist updates ONLY the 5 runtime keys
        db.save_runtime_settings(&RuntimePetSettings {
            tracking_paused: true,
            pet_visible: false,
            has_saved_position: true,
            pet_x: 512,
            pet_y: 384,
        })
        .unwrap();

        let loaded = db.try_load_settings().unwrap();
        assert!(loaded.tracking_paused);
        assert!(!loaded.pet_visible);
        assert_eq!((loaded.pet_x, loaded.pet_y), (512, 384));
        // Unrelated preferences must be 100% preserved
        assert_eq!(loaded.pet_size, 160);
        assert_eq!(loaded.idle_threshold_secs, 180);
        assert!(!loaded.cute_moments_enabled);
        assert_eq!(loaded.cute_frequency, "low");
        assert!(loaded.launch_on_startup);

        // Lock DB externally so write/read fails -> update_settings must propagate Err
        // and must NOT substitute defaults or overwrite valid settings.
        let blocker = Connection::open(dir.join("piku.db")).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
        db.set_busy_timeout_for_test(50);

        let err = db.update_settings(AppSettingsPatch {
            pet_x: Some(999),
            ..Default::default()
        });
        assert!(err.is_err(), "update_settings must return Err on DB lock");
        let rt_err = db.save_runtime_settings(&RuntimePetSettings {
            tracking_paused: false,
            pet_visible: true,
            has_saved_position: true,
            pet_x: 999,
            pet_y: 999,
        });
        assert!(rt_err.is_err(), "save_runtime_settings must return Err on DB lock");

        blocker.execute_batch("ROLLBACK;").unwrap();
        drop(blocker);
        db.set_busy_timeout_for_test(5000);

        // Verify original preferences remain completely intact
        let after = db.try_load_settings().unwrap();
        assert_eq!(after.pet_size, 160);
        assert_eq!(after.idle_threshold_secs, 180);
        assert!(!after.cute_moments_enabled);
        assert!(after.launch_on_startup);
        assert_eq!((after.pet_x, after.pet_y), (512, 384));

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wal_checkpoint_reports_success_when_unblocked_and_error_when_busy() {
        let dir = unique_test_dir("wal_checkpoint");
        let db = Database::open(&dir).unwrap();
        seed_usage(&db, "chrome.exe", "Google Chrome", 120);

        // Hold an active reader transaction so TRUNCATE checkpoint cannot complete
        let reader = Connection::open(dir.join("piku.db")).unwrap();
        reader
            .execute_batch("BEGIN; SELECT COUNT(*) FROM usage_hourly;")
            .unwrap();
        let busy_res = db.checkpoint_wal();
        assert!(
            busy_res.is_err(),
            "checkpoint_wal must return Err when WAL is busy/pinned by reader"
        );

        reader.execute_batch("COMMIT;").unwrap();
        drop(reader);

        // Once reader releases, TRUNCATE checkpoint succeeds cleanly and empties WAL
        let ok_status = db.checkpoint_wal().expect("unblocked checkpoint should succeed");
        assert_eq!(ok_status.busy, 0);
        let wal_len = fs::metadata(dir.join("piku.db-wal"))
            .map(|m| m.len())
            .unwrap_or(0);
        assert_eq!(wal_len, 0, "TRUNCATE checkpoint should truncate WAL to 0 bytes");

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rolling_backup_captures_active_wal_validates_and_retains_at_most_two_snapshots() {
        let dir = unique_test_dir("backup_wal");
        let db = Database::open(&dir).unwrap();

        // Disable autocheckpoint so writes stay in active WAL
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch("PRAGMA wal_autocheckpoint = 0;").unwrap();
        }
        seed_usage(&db, "code.exe", "VS Code", 321);
        assert!(
            fs::metadata(dir.join("piku.db-wal"))
                .map(|m| m.len())
                .unwrap_or(0)
                > 0,
            "expected un-checkpointed WAL frames"
        );

        // Backup 1 captures active WAL data
        let b1 = db.create_rolling_backup().unwrap();
        assert!(b1.exists());
        validate_backup_file(&b1).unwrap();
        {
            let c = Connection::open_with_flags(&b1, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let sum: i64 = c
                .query_row("SELECT COALESCE(SUM(active_secs),0) FROM usage_hourly", [], |r| r.get(0))
                .unwrap();
            assert_eq!(sum, 321, "backup must include committed active WAL data");
        }

        // Add more usage and run 2 more backups -> retain at most 2 snapshots (bak.1 + bak.2)
        seed_usage(&db, "chrome.exe", "Chrome", 100);
        db.create_rolling_backup().unwrap();
        seed_usage(&db, "chrome.exe", "Chrome", 50);
        db.create_rolling_backup().unwrap();

        let backups_dir = dir.join(BACKUP_DIR_NAME);
        let mut names: Vec<String> = fs::read_dir(&backups_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![BACKUP_PRIMARY_NAME.to_string(), BACKUP_SECONDARY_NAME.to_string()],
            "must retain at most two validated snapshots and leave no temp files"
        );

        // bak.1 has latest 471s, bak.2 has previous 421s
        let sum1: i64 = Connection::open_with_flags(backups_dir.join(BACKUP_PRIMARY_NAME), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT SUM(active_secs) FROM usage_hourly", [], |r| r.get(0))
            .unwrap();
        let sum2: i64 = Connection::open_with_flags(backups_dir.join(BACKUP_SECONDARY_NAME), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
            .query_row("SELECT SUM(active_secs) FROM usage_hourly", [], |r| r.get(0))
            .unwrap();
        assert_eq!(sum1, 471);
        assert_eq!(sum2, 421);

        drop(db);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_db_quarantines_full_set_and_recovers_from_valid_or_secondary_backup() {
        let dir = unique_test_dir("corrupt_recover");
        {
            let db = Database::open(&dir).unwrap();
            seed_usage(&db, "chrome.exe", "Chrome", 200);
            db.create_rolling_backup().unwrap(); // -> bak.1 (200s)
            seed_usage(&db, "chrome.exe", "Chrome", 50);
            db.create_rolling_backup().unwrap(); // -> bak.1 (250s), bak.2 (200s)
        }

        // Corrupt bak.1 so recovery must reject invalid bak.1 and restore valid bak.2 (200s)!
        fs::write(dir.join(BACKUP_DIR_NAME).join(BACKUP_PRIMARY_NAME), b"CORRUPT_BAK1").unwrap();

        // Corrupt main piku.db header AND place dummy WAL/SHM to verify entire set is quarantined
        fs::write(dir.join("piku.db"), vec![0xFFu8; 4096]).unwrap();
        fs::write(dir.join("piku.db-wal"), b"WAL_EVIDENCE").unwrap();
        fs::write(dir.join("piku.db-shm"), b"SHM_EVIDENCE").unwrap();

        let recovered = Database::open(&dir).expect("should recover from valid bak.2");
        let day = recovered.get_day_summary("2026-11-24").unwrap();
        assert_eq!(day.total_secs, 200, "must skip corrupt bak.1 and restore valid bak.2");

        // Verify original corrupted db + wal + shm were preserved in quarantine
        let q_entries: Vec<PathBuf> = fs::read_dir(dir.join(QUARANTINE_DIR_NAME))
            .unwrap()
            .filter_map(|e| e.ok().map(|x| x.path()))
            .collect();
        assert_eq!(q_entries.len(), 1);
        assert!(q_entries[0].join("piku.db").exists());
        assert_eq!(fs::read(q_entries[0].join("piku.db-wal")).unwrap(), b"WAL_EVIDENCE");
        assert_eq!(fs::read(q_entries[0].join("piku.db-shm")).unwrap(), b"SHM_EVIDENCE");

        drop(recovered);

        // Now test corruption with NO valid backups -> preserves quarantine & creates safe fresh DB
        let dir2 = unique_test_dir("corrupt_no_backup");
        fs::create_dir_all(dir2.join(BACKUP_DIR_NAME)).unwrap();
        fs::write(dir2.join("piku.db"), b"NOT_A_SQLITE_FILE").unwrap();
        fs::write(dir2.join(BACKUP_DIR_NAME).join(BACKUP_PRIMARY_NAME), b"BAD_BACKUP").unwrap();

        let fresh = Database::open(&dir2).expect("should fall back safely after quarantining");
        assert_eq!(fresh.get_day_summary("2026-11-24").unwrap().total_secs, 0);
        let q2: Vec<PathBuf> = fs::read_dir(dir2.join(QUARANTINE_DIR_NAME))
            .unwrap()
            .filter_map(|e| e.ok().map(|x| x.path()))
            .collect();
        assert_eq!(q2.len(), 1);
        assert_eq!(fs::read(q2[0].join("piku.db")).unwrap(), b"NOT_A_SQLITE_FILE");

        drop(fresh);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }

    #[test]
    fn wal_assisted_recovery_replays_valid_wal_and_ignores_corrupted_wal_tail() {
        let dir = unique_test_dir("wal_recovery");
        let db = Database::open(&dir).unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute_batch("PRAGMA wal_autocheckpoint = 0;").unwrap();
        }
        seed_usage(&db, "firefox.exe", "Firefox", 180);

        // Snapshot main + wal while connection is still open (simulates crash with un-checkpointed WAL)
        let dir_reopen = unique_test_dir("wal_recovery_reopen");
        fs::create_dir_all(&dir_reopen).unwrap();
        fs::copy(dir.join("piku.db"), dir_reopen.join("piku.db")).unwrap();
        fs::copy(dir.join("piku.db-wal"), dir_reopen.join("piku.db-wal")).unwrap();
        // Append malformed garbage bytes to WAL tail (SQLite must recover valid frames before tail)
        {
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(dir_reopen.join("piku.db-wal"))
                .unwrap();
            f.write_all(&[0xDE, 0xAD, 0xBE, 0xEF, 0x11, 0x22, 0x33, 0x44]).unwrap();
        }
        drop(db);

        let reopened = Database::open(&dir_reopen).expect("SQLite should replay valid WAL frames");
        assert_eq!(
            reopened.get_day_summary("2026-11-24").unwrap().total_secs,
            180,
            "WAL-assisted recovery must preserve committed WAL frames without discarding WAL"
        );
        assert!(
            !dir_reopen.join(QUARANTINE_DIR_NAME).exists(),
            "healthy WAL replay must not quarantine database"
        );

        drop(reopened);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir_reopen);
    }

    #[test]
    fn schema_version_stamps_current_and_rejects_newer_version_without_quarantine() {
        let dir = unique_test_dir("schema_version");
        {
            let db = Database::open(&dir).unwrap();
            let ver: i32 = db
                .conn
                .lock()
                .unwrap()
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(ver, CURRENT_SCHEMA_VERSION);
            seed_usage(&db, "chrome.exe", "Chrome", 75);
            db.checkpoint_wal().unwrap();
        }

        // Simulate a database created by a newer Piku version (user_version = 99)
        {
            let c = Connection::open(dir.join("piku.db")).unwrap();
            c.execute_batch("PRAGMA user_version = 99;").unwrap();
        }

        let res = Database::open(&dir);
        assert!(res.is_err(), "must refuse to open newer schema version");
        let msg = res.err().unwrap();
        assert!(
            msg.contains("99"),
            "error message should mention newer schema version, got: {msg}"
        );
        assert!(
            !dir.join(QUARANTINE_DIR_NAME).exists(),
            "newer schema version must NEVER be quarantined or overwritten"
        );
        assert!(dir.join("piku.db").exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
