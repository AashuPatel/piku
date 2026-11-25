# Changelog

All notable changes to Piku are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Piku adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-11-25

Production hand-illustrated Piku character library, expanded desktop animation states, and startup defaults.

### Added
- **Hand-illustrated production sprite library (`24` animations / `144` frames):** Replaced the initial v0.1.0 programmatic placeholder sprites with 24 aligned `256×256` RGBA hand-illustrated animations:
  - **Core (`6`):** `idle`, `blink` (upgraded from 4 to 6 frames), `wave`, `happy`, `sleep`, and `click` (`click_reaction`).
  - **Expressions (`6`):** `excited`, `content`, `blush`, `curious`, `thinking`, `surprised`.
  - **Desktop & activity states (`6`):** `focused`, `yawn`, `drag`, `working`, `break`, `late_night`.
  - **Ambient moments (`6`):** `celebration`, `snack`, `tea`, `cloud`, `stretch`, `peek`.
- **Expanded runtime animation wiring:** Connected live pointer dragging to a steady static `drag_03` frame (restoring the exact previous state on release), `idle_sleep` transitions to `yawn → sleep`, `late_night` state hints to the plaid-blanket `late_night` loop, and all ambient expressions/moments to `AnimationController`.
- **Static UI-state illustrations:** Integrated transparent `empty_state.png` and `error.png` illustrations into the existing Today and History empty/error cards.
- **Asset library & legacy archive:** Organized the complete 156-element processed library (`art-source/processed/`), extraction/validation scripts (`scripts/assets/`), and a lossless `legacy-v1` archive (`art-source/archive/legacy-v1/`) preserving the v0.1.0 placeholder pack and reference sheet.

### Changed
- **Launch on startup default:** Enabled `launch_on_startup` by default on fresh installations (`AppSettings::default().launch_on_startup = true`), while preserving explicit user preferences on upgrades from v0.1.0 (`PRAGMA user_version = 1` unchanged).
- **On-demand sprite loading:** `PetWindow` preloads the 9 core interactive/lifecycle states (`54` frames) at startup and lazily decodes the 15 ambient/expression states on first use; History and Preferences views continue loading `0` animated sprites and `0` timers.
- **Dynamic grounding shadows:** Calibrated `PetCanvas` dynamic grounding shadows to the airborne, wide, and edge-peeking frames of the 24 production animations.
- **Windows packaging:** Per-user Windows x64 NSIS installer updated to `Piku_0.2.0_x64-setup.exe`.

### Fixed
- **Mid-yawn wake transition:** Fixed an FSM edge case in `AnimationController` where waking up (`active`) while `yawn` was mid-clip could transition into `sleep` via manifest `next` instead of returning to the authoritative `idle` base state.

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
