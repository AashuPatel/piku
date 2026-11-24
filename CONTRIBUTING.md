# Contributing to Piku

Thank you for your interest in contributing to Piku! Piku is meant to stay small, warm, local-first, and unobtrusive—focused PRs and clear bug reports help keep it that way.

---

## Getting Started

1. Fork the repository and clone your fork.
2. Follow [docs/building-from-source.md](docs/building-from-source.md) to set up Node.js, `pnpm`, Rust, and Windows C++ Build Tools.
3. Verify everything builds and passes before making changes:

```powershell
pnpm install
pnpm typecheck
pnpm build
cd src-tauri
cargo test
```

---

## Guidelines

- **Keep Piku local-first and calm:** Avoid adding telemetry, accounts, cloud dependencies, or noisy dashboard clutter.
- **Keep PRs focused:** One bug fix or small, well-scoped improvement per pull request.
- **Test your changes:**
  - Run `pnpm typecheck` and `pnpm build` for frontend changes.
  - Run `cargo test` in `src-tauri/` for Rust changes.
  - Verify real Windows desktop behavior (`pnpm tauri dev`) for windowing, tray, or tracking changes.
- **Never commit personal data:** Do not commit `piku.db`, WAL/SHM files, screenshots containing personal files, or files from `.internal/`.

---

## Reporting Bugs & Requesting Features

- For bugs, use the **Bug Report** issue template and include your Windows version, display/DPI setup, and clear reproduction steps.
- For feature ideas, use the **Feature Request** issue template and explain how it fits Piku's calm, local-first design.
