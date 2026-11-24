# Building from Source

Piku currently targets **Windows 10 / 11 (x64)** because its foreground tracking, desktop-layer windowing, and startup integration use native Win32 APIs.

---

## 1. Prerequisites

| Requirement | Notes / Tested Version |
| --- | --- |
| **OS** | Windows 10 or Windows 11 (64-bit / x64) |
| **Node.js** | Node.js 20+ (verified on `v24.22.3`) |
| **pnpm** | Required package manager (`pnpm-lock.yaml`; verified on `12.3.4`) |
| **Rust toolchain** | Stable `rustc` + `cargo` via [rustup](https://rustup.rs/) (`x86_64-pc-windows-msvc`; verified on `1.100.0`) |
| **C++ Build Tools** | Visual Studio Build Tools with the **Desktop development with C++** workload |
| **WebView2 Runtime** | Microsoft Edge WebView2 Evergreen Runtime (pre-installed on Windows 11) |

---

## 2. Install Dependencies

```powershell
pnpm install
```

Rust crate dependencies are fetched automatically by Cargo on first build (including bundled SQLite via `rusqlite` — no separate system SQLite installation is required).

---

## 3. Run in Development Mode

Starts the Vite frontend dev server (`http://localhost:1420`) and launches the Tauri desktop shell:

```powershell
pnpm tauri dev
```

Frontend-only commands from `package.json`:

```powershell
pnpm dev        # Vite dev server only
pnpm typecheck  # TypeScript typecheck (tsc --noEmit)
pnpm build      # Typecheck + production Vite bundle into dist/
```

---

## 4. Run Tests

Run the Rust unit and integration test suite (tracking core, idle-tail rollback, SQLite integrity/recovery/backups, flush serialization, multi-monitor geometry, and startup reconciliation):

```powershell
cd src-tauri
cargo test
```

> Note: `startup::windows_tests::startup_reconciliation_handles_all_path_and_quoting_cases` tests against a temporary value (`PikuStartupUnitTestTemp`) in `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and cleans it up automatically.

---

## 5. Production Build & Windows Installer

Build the optimized release executable and per-user Windows NSIS installer (`currentUser`, LZMA compression):

```powershell
pnpm tauri build --bundles nsis
```

Outputs:
- **Release executable:** `src-tauri/target/release/piku.exe`
- **Windows x64 NSIS installer:** `src-tauri/target/release/bundle/nsis/Piku_0.1.0_x64-setup.exe`

*(Note: Tauri downloads NSIS automatically on first bundle build if not already cached locally. Release builds are unsigned by default.)*

---

## 6. Character Sprite Validation (Optional)

If you modify or inspect character sprite assets under `public/assets/character/piku/`, you can validate them against `manifest.json` using Python 3 + Pillow:

```powershell
python scripts/assets/validate_assets.py
```
