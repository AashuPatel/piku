# Privacy

Piku is designed as a local-first desktop companion. Your screen-time journal stays on your computer in a local SQLite database.

---

## 1. What Piku Observes

To measure how long each application is in use, Piku's local Windows tracking worker samples the foreground window once per second and observes:

- **Active executable name** (for example, `chrome.exe` or `code.exe`).
- **Executable file path** (used locally to read the app's friendly display name and extract its icon into your local icon cache).
- **Friendly application name** (read from local Windows PE version metadata, falling back to a formatted filename).
- **Window class name** (checked in memory for `explorer.exe` only, so Desktop and Taskbar interactions are excluded from File Explorer usage).
- **System idle duration and lock-screen state** (via Win32 `GetLastInputInfo` and `OpenInputDesktop`, used only to pause tracking and roll back inactive time).

---

## 2. What Piku Does NOT Collect or Store

Piku does **not** record or inspect:

- Window titles or document names
- Website URLs or browser tabs
- Keystrokes, mouse coordinates outside pet drag/hover, or clipboard contents
- Screenshots or screen recordings
- Files, project contents, or personal identifiers

---

## 3. Network, Telemetry & Accounts

Verified against the application source code (`src/`, `src-tauri/src/`) and dependencies (`package.json`, `src-tauri/Cargo.toml`):

- **No accounts or sign-in.**
- **No cloud sync or backend servers.**
- **No analytics, telemetry, or crash-reporting SDKs.**
- **No runtime external network requests from Piku code.** Frontend `fetch()` calls load only local bundled sprite assets served from the application package.

### Transparent Platform Notes
- **Installer WebView2 check:** During installation only, if Microsoft Edge WebView2 Runtime is not already installed on Windows, the NSIS installer downloads Microsoft's official WebView2 Evergreen Bootstrapper (`https://go.microsoft.com/fwlink/p/?LinkId=2124703`). If WebView2 is already present (standard on Windows 11), this step is skipped.
- **WebView2 local runtime cache:** Tauri renders Piku's UI using Microsoft Edge WebView2, which maintains its own local runtime folder under `%LOCALAPPDATA%\com.piku.desktop\EBWebView\`.

---

## 4. User Control Over Data

- **Pause anytime:** Right-click the Piku tray icon → **Pause Tracking**, or toggle **Screen-time tracking** off in **Preferences**.
- **Inspect your data:** All usage records and settings live in a standard SQLite file at `%LOCALAPPDATA%\com.piku.desktop\piku.db`. You can inspect it with any SQLite viewer.
- **Keep or delete on uninstall:** Uninstalling Piku removes the application and cleans up its Windows startup Registry entry (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Piku`) while preserving `%LOCALAPPDATA%\com.piku.desktop\` by default. To remove all local data permanently, select the delete-app-data option during uninstall or delete `%LOCALAPPDATA%\com.piku.desktop\` manually.

See [docs/data-storage.md](data-storage.md) for the exact file layout and SQLite schema.
