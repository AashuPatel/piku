# Local Data Storage & Recovery

Piku stores all screen-time history, cached application icons, settings, and local recovery backups inside your per-user Windows local application data directory.

---

## 1. Storage Location

```text
%LOCALAPPDATA%\com.piku.desktop\
├── piku.db              # Primary SQLite database (WAL mode)
├── piku.db-wal          # Write-Ahead Log sidecar (truncated to 0 B on clean shutdown)
├── piku.db-shm          # SQLite shared-memory index (runtime sidecar)
├── backups/             # Validated rolling local backups (up to 2 snapshots)
│   ├── piku.db.bak.1    # Most recent clean-shutdown snapshot
│   └── piku.db.bak.2    # Previous clean-shutdown snapshot
├── icon-cache/          # Locally extracted PNG application icons
├── quarantine/          # Created only if a corrupted database set must be preserved
├── EBWebView/           # Microsoft Edge WebView2 runtime cache
└── pet_state.json       # Legacy pre-SQLite state file (migrated once if present)
```

> The application binaries themselves are installed separately in `%LOCALAPPDATA%\Piku\`.

---

## 2. SQLite Configuration & Schema

Piku opens `piku.db` with:

- `PRAGMA journal_mode = WAL;`
- `PRAGMA synchronous = NORMAL;`
- `PRAGMA foreign_keys = ON;`
- `PRAGMA busy_timeout = 5000;`
- `PRAGMA user_version = 1;` (guards against silently mutating a database created by a newer Piku version)

### Tables

1. **`apps`** — Known applications seen on this machine:
   - `id`, `exe_name` (unique lowercase executable name), `exe_path`, `display_name`, `icon_hash`, `first_seen_at`
2. **`usage_hourly`** — Local-time hourly screen-time totals:
   - `local_date` (`YYYY-MM-DD`), `local_hour` (`0..23`), `app_id` (foreign key → `apps.id`), `active_secs` (`>= 0`)
   - Primary key `(local_date, local_hour, app_id)` with index `idx_usage_date`
3. **`settings`** — Key/value user preferences and pet position:
   - `tracking_paused`, `pet_visible`, `pet_x`, `pet_y`, `has_saved_position`, `pet_size`, `idle_threshold_secs`, `cute_moments_enabled`, `cute_frequency`, `launch_on_startup`

Runtime pet updates (`AppState::persist()`) update only the five runtime-owned keys (`tracking_paused`, `pet_visible`, `has_saved_position`, `pet_x`, `pet_y`) inside a single transaction without rewriting unrelated preferences.

---

## 3. Flush Lifecycle & Idle Rollback

- Active foreground usage accumulates in memory and flushes transactionally to SQLite every **30 seconds**, as well as immediately on **application switch**, **idle**, **screen lock**, **tracking pause**, or **shutdown**.
- All flushes are serialized by a dedicated `flush_lock` mutex so concurrent callers cannot commit an idle-tail deduction ahead of an earlier addition.
- When OS idle reaches `idle_threshold_secs` (`300s` default), previously buffered and flushed idle-tail seconds are deducted cleanly (clamping at `0` and pruning zeroed hourly rows).
- If a flush fails (for example, temporary file lock), the pending batch is restored and retried automatically with capped backoff.

---

## 4. Shutdown Checkpoints, Backups & Corruption Recovery

- **Clean shutdown (`Tray Quit`, `CloseRequested`, `WM_ENDSESSION`, `RunEvent::Exit`):**
  1. Stops the tracking thread and runs a final transactional flush.
  2. Persists runtime pet state.
  3. Runs `PRAGMA wal_checkpoint(TRUNCATE)` so committed WAL frames are folded into `piku.db`.
  4. Creates a standalone validated snapshot via `VACUUM INTO` (`backups/piku.db.bak.1`), validates it with `PRAGMA integrity_check` and `PRAGMA foreign_key_check`, and rotates at most 2 snapshots (`bak.1` → `bak.2`).
  - *Note:* Rolling backups refresh on clean shutdown (and on initial startup if `bak.1` does not exist yet). A daily in-session backup scheduler while Piku stays open across multiple days is not yet implemented.
- **Startup recovery:**
  1. Checks the 16-byte SQLite header before opening read-write so sidecar WAL/SHM files are never unlinked prior to quarantine.
  2. Refuses to open or modify databases with `PRAGMA user_version > 1`.
  3. Runs `PRAGMA integrity_check` and `PRAGMA foreign_key_check` on open.
  4. If `piku.db` is corrupt, Piku **first** copies the original `piku.db`, `-wal`, and `-shm` files into a timestamped folder under `quarantine/`, then attempts restoration from the newest validated backup (`bak.1` → `bak.2`), falling back to a fresh database only if no valid backup exists.

---

## 5. Exporting, Backing Up, or Deleting Your Data

- **Manual backup:** Quit Piku cleanly from the system tray, then copy `%LOCALAPPDATA%\com.piku.desktop\piku.db` (and optionally `backups/`).
- **Uninstall behavior:** Standard uninstall removes `%LOCALAPPDATA%\Piku\` and `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Piku` while keeping `%LOCALAPPDATA%\com.piku.desktop\` intact so reinstalling restores your history and preferences.
- **Full removal:** Delete `%LOCALAPPDATA%\com.piku.desktop\` after quitting or uninstalling Piku.
