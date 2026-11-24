# Architecture

Piku is a local-first Windows desktop application built with **Tauri 2**, **Rust**, **React**, **TypeScript**, and **SQLite**. All operating-system probing, window management, persistence, and crash recovery live in the Rust backend (`src-tauri/`); the React frontend (`src/`) renders the animated pet canvas and on-demand journal panels via Tauri IPC and never accesses SQLite or Windows APIs directly.

---

## 1. High-Level Structure

```text
src-tauri/src/
├── main.rs / lib.rs       # Single-instance mutex, Tauri bootstrap, window & exit hooks
├── tracking/
│   ├── win_probe.rs       # Win32 foreground, UWP child, PE friendly name, idle & lock probe
│   ├── tracker_core.rs    # Time delta clamping, exclusions, idle-tail rollback, hourly buckets
│   └── mod.rs             # 1 Hz worker thread, serialized flush_lock, async icon extraction
├── database/mod.rs        # SQLite WAL store, transactional delta flushes, backups, quarantine
├── windowing.rs           # Desktop-layer Z-order, drag/DPI/display recovery, popups, WM_ENDSESSION
├── tray.rs                # Native Windows system tray menu & left-click panel toggle
├── startup.rs             # Native named mutex & HKCU\...\Run reconciliation
├── icons.rs               # Cached Windows executable icon extraction (PNG)
├── state.rs               # Shared runtime state, targeted persistence, idempotent shutdown
└── commands.rs            # Tauri IPC command handlers exposed to the React UI

src/
├── animation/
│   ├── spriteStore.ts     # Manifest-driven ImageBitmap loader & alpha hit-mask builder
│   └── controller.ts      # Pure setTimeout animation FSM (no 60 fps requestAnimationFrame loop)
└── components/
    ├── pet/               # Transparent pet window canvas & speech bubble
    ├── panel/             # Greeting, Today summary, and Preferences views
    └── history/           # Day (24h rhythm), Week, and Month heatmap views
```

---

## 2. Tracking Pipeline

1. **1 Hz Windows Probe (`src-tauri/src/tracking/win_probe.rs`)**
   - Checks screen-lock state via `OpenInputDesktop`.
   - Reads OS idle duration via `GetLastInputInfo` + `GetTickCount`.
   - Identifies the active window via `GetForegroundWindow` → `GetWindowThreadProcessId` → `QueryFullProcessImageNameW`.
   - Resolves UWP / packaged apps hosted under `applicationframehost.exe` by enumerating child windows (`EnumChildWindows`) to find the real child process executable.
   - Reads window class names (`GetClassNameW`) for `explorer.exe` so Desktop and Taskbar chrome (`Progman`, `WorkerW`, `Shell_TrayWnd`, `Shell_SecondaryTrayWnd`, overflow/thumbnail windows) are excluded while real File Explorer windows remain tracked.
   - Looks up friendly display names from PE version resources (`GetFileVersionInfoW`), cached in memory.

2. **Tracker Core (`src-tauri/src/tracking/tracker_core.rs`)**
   - Clamps individual tick deltas to `MAX_TICK_DELTA_SECS = 5` seconds so system sleep/hibernate jumps cannot inflate screen time.
   - Excludes Piku itself (`piku.exe`) and Windows shell/lock/search/input overlays.
   - Splits active seconds cleanly across local-time hour and midnight boundaries into `(local_date, local_hour, exe_name)` buckets.
   - Buffers active usage in memory and schedules flushes every `30` seconds, as well as immediately on application switch, idle, lock, pause, or shutdown.
   - **Idle-tail rollback:** When OS idle reaches the configured threshold (`300s` default), Piku deducts the inactive tail recorded prior to threshold crossing—both from the in-memory buffer and transactionally from already-flushed SQLite rows—clamping safely at `0` so usage never goes negative.
   - Failed flushes restore both pending additions and deductions into the buffer and retry with capped backoff.

3. **Serialized SQLite Persistence (`src-tauri/src/tracking/mod.rs` & `database/mod.rs`)**
   - A dedicated `flush_lock` mutex serializes `take_flush_batch() → SQLite transaction → mark/restore`, preventing concurrent callers (periodic worker, UI reads, pause, shutdown) from inverting additions and idle-tail deductions.
   - Newly observed applications trigger asynchronous icon extraction (`piku-icon-cache` worker thread) so tracking ticks never stall on disk I/O.

---

## 3. Native Windows Integration & Windowing

Piku manages three lightweight Tauri windows and a system tray icon:

- **Desktop Pet (`pet`):** Transparent, frameless, non-activating (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`, `WM_MOUSEACTIVATE → MA_NOACTIVATE`, `WS_MINIMIZEBOX | WS_MAXIMIZEBOX` stripped). Placed directly above the Windows desktop wallpaper layer (`Progman` / `WorkerW`) and behind normal application windows via a Win32 window subclass and `SetWinEventHook`. Handles `Win + D` show-desktop reconciliation and coalesced deferred recovery on `WM_DISPLAYCHANGE`, `WM_DPICHANGED`, and `SPI_SETWORKAREA`.
- **Greeting / Today / History / Preferences Panel (`panel`):** Created on demand, positioned beside Piku using the pet's target monitor work area and DPI, and destroyed on close or focus loss.
- **Speech Bubble (`bubble`):** Transient non-activating overlay anchored above Piku when ambient interactions are enabled.
- **System Tray (`piku-tray`):** Left-click toggles the panel; right-click provides **Hide/Show Piku**, **Pause/Resume Tracking**, **Preferences**, and **Quit Piku**.
- **Single-Instance & Shutdown Safety:** A native per-session mutex (`Local\com.piku.desktop.single_instance`) prevents duplicate instances. `AppState::shutdown_cleanly()` runs idempotently on Tray Quit, `CloseRequested`, `WM_ENDSESSION`, and `RunEvent::Exit` to unhook Win32 listeners, flush pending usage, persist runtime settings, truncate the WAL, and refresh the rolling backup.

---

## 4. Rendering & Resource Efficiency

- **Timer-driven sprite FSM (`src/animation/controller.ts`):** Uses `setTimeout` at each animation clip's native frame rate (4–12 fps) instead of a continuous 60 fps `requestAnimationFrame` loop.
- **Conditional panel sprite loading (`src/components/panel/GreetingPanel.tsx` & `spriteStore.ts`):**
  - The desktop pet loads all 6 clips (34 frames) plus a 64×64 alpha hit-test mask.
  - Greeting and Today load only the 4 clips needed by the panel (`idle`, `blink`, `wave`, `happy` — 22 frames) with no hit-mask canvas.
  - Opening **History** or **Preferences** directly loads **0 sprites** and starts **0 animation timers**.
  - Switching from Today into History or Preferences immediately suspends the panel `AnimationController` (clearing all timers) and resumes cleanly on return. Closing the panel disposes all `ImageBitmap` objects via `ImageBitmap.close()`.

---

## 5. Tauri IPC Boundary (`src-tauri/src/commands.rs`)

All frontend↔backend communication passes through explicit Tauri commands:

- **Pet & windowing:** `get_pet_runtime`, `begin_pet_drag`, `move_pet_drag`, `end_pet_drag`, `sync_pet_position`, `open_greeting_panel`, `open_preferences_panel`, `close_panel`, `resize_panel`, `show_bubble`
- **Usage queries:** `get_today_summary`, `get_day_detail`, `get_week_summary`, `get_month_summary`
- **Preferences:** `get_settings`, `update_settings`
