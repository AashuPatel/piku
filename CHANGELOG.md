# Changelog

All notable changes to Piku are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Piku adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-24

First public controlled beta release for Windows x64.

### Added
- **Desktop companion:** Transparent, non-activating Windows desktop-layer pet (`pet` window) with 6 sprite animation states (`idle`, `blink`, `sleep`, `wave`, `happy`, `click`), drag-and-persist positioning, customizable pet size (Small / Normal / Large), and optional speech bubbles.
- **Screen-time tracking:** 1 Hz Win32 foreground application tracker with UWP child-process resolution, PE friendly display names, cached native app icons, Windows shell/desktop/taskbar exclusions, screen-lock detection, and live 300-second idle-tail rollback across flush, hour, and midnight boundaries.
- **Journal UI:** On-demand Greeting popup, Today summary with expandable application list, Day (24-hour rhythm) / Week / Month heatmap History views, and Preferences panel.
- **System tray & startup:** Native Windows tray menu (Hide/Show Piku, Pause/Resume Tracking, Preferences, Quit Piku), single-instance Windows named mutex, and per-user `HKCU\...\Run\Piku` startup reconciliation.
- **Local persistence & recovery:** Local SQLite database in WAL mode (`%LOCALAPPDATA%\com.piku.desktop\piku.db`), serialized transactional flushes, shutdown `wal_checkpoint(TRUNCATE)`, validated rolling local `VACUUM INTO` backups (up to 2 snapshots), conservative corruption quarantine, and schema version guarding (`PRAGMA user_version = 1`).
- **Panel optimization:** Conditional sprite loading (0 sprites and 0 animation timers in direct History/Preferences; 22 frames in Greeting/Today) and automatic animation suspension in non-visual views.
- **Windows packaging:** Per-user Windows x64 NSIS installer (`Piku_0.1.0_x64-setup.exe`) preserving user data on uninstall and cleaning up the startup Registry entry.

### Known Limitations (Controlled Beta)
- Unsigned Windows installer and executable (Windows Defender SmartScreen warning expected on downloaded copies).
- Validated on Windows 11 x64 single-monitor hardware; physical multi-monitor hardware, fresh-machine installs, true version upgrades, and 2-hour unattended soak runs have not yet been executed.
- Rolling local backups refresh on clean shutdown (not yet on an in-session daily schedule while continuously open across multiple days).
