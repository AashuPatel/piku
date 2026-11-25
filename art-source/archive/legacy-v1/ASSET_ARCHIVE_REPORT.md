# Piku — Legacy Asset Archive Report (`legacy-v1`)

This report documents the lossless archival of Piku's v1.0.0 placeholder illustrations, historical character reference copies, v1 brand static copies, v1 QC sprite sheet, and superseded v1 asset scripts into `art-source/archive/legacy-v1/`.

No original artwork or historical files were permanently deleted. Authoritative production assets in `public/assets/character/piku/` and `art-source/processed/` remain untouched.

---

## 1. Assets Archived (`41` files, `2.52 MB` including `legacy-manifest.json`)

Full per-file metadata (original filename, original location, archived location, category, previous purpose, application reference status, archival reason, byte size, and `SHA-256`) is recorded in `art-source/archive/legacy-v1/legacy-manifest.json`.

| Category | Count | Operation | Contents |
| --- | :-: | --- | --- |
| `animations/` | `35` | Recovered verbatim from `v0.1.0` (`HEAD`) | Complete v1.0.0 placeholder animation pack: `manifest.json` (`assetKind: "placeholder"`) + `34` PNG frames (`idle` ×6, `blink` ×4, `sleep` ×6, `wave` ×6, `happy` ×6, `click` ×6) |
| `characters/` | `0` | Created (reserved) | No standalone single-character portrait files existed in v1 separate from `references/piku-character-sheet.png` and `animations/` |
| `references/` | `1` | Copied (original preserved) | `piku-character-sheet.png` — original v1 6-pose character reference sheet |
| `static/` | `2` | Copied (originals preserved) | `piku-logo.png`, `piku-icon.png` — v1 brand logo & icon artwork |
| `sprites/` | `1` | Copied (original preserved) | `placeholders_backdrop_qc.png` — v1 34-frame light/dark contact sheet |
| `other/` | `2` | Moved cleanly | `generate_placeholders.py`, `process_sheet.py` — superseded v1 generator & single-row slicer moved out of `scripts/assets/` to prevent accidental overwrite of v2 production sprites |

---

## 2. Original & New Locations Summary

| Original Location | Archived Location | Operation |
| --- | --- | --- |
| `public/assets/character/piku/manifest.json` (v1.0.0 `HEAD`) | `art-source/archive/legacy-v1/animations/manifest.json` | Recovered from `HEAD` |
| `public/assets/character/piku/{idle,blink,sleep,wave,happy,click}/*.png` (`HEAD`) | `art-source/archive/legacy-v1/animations/{idle,blink,sleep,wave,happy,click}/*.png` | Recovered from `HEAD` (`34` PNGs) |
| `assets/piku-character-sheet.png` | `art-source/archive/legacy-v1/references/piku-character-sheet.png` | Verified copy (original kept) |
| `assets/logo/piku-logo.png` | `art-source/archive/legacy-v1/static/piku-logo.png` | Verified copy (original kept) |
| `assets/logo/piku-icon.png` | `art-source/archive/legacy-v1/static/piku-icon.png` | Verified copy (original kept) |
| `.internal/test-results/qc-previews/placeholders_backdrop_qc.png` | `art-source/archive/legacy-v1/sprites/placeholders_backdrop_qc.png` | Verified copy (original kept) |
| `scripts/assets/generate_placeholders.py` | `art-source/archive/legacy-v1/other/generate_placeholders.py` | Moved (`0` external refs) |
| `scripts/assets/process_sheet.py` | `art-source/archive/legacy-v1/other/process_sheet.py` | Moved (`0` external refs) |

---

## 3. Files Deliberately Left in Place

| Path | Classification | Reason Left in Place |
| --- | --- | --- |
| `public/assets/character/piku/` (`147` files, `3.74 MB`) | **A — Production Library** | Authoritative runtime v2.0.0 sprites (`144` animation PNGs + `2` static UI PNGs + `manifest.json`). |
| `art-source/processed/` (`191` files, `10.50 MB`) | **A — Production Library** | Authoritative processed 156-asset library, full `manifest.json`, validation report, and review contact sheets. |
| `scripts/assets/extract_illustration_library.py` & `validate_assets.py` | **A — Production Tooling** | Active v2 extraction and validation scripts (`validate_assets.py` referenced by `docs/building-from-source.md`). |
| `art-source/original/` (`5` files, `6.65 MB`) | **C — Original Source Artwork** | Canonical byte-for-byte copies of the 5 new source illustration sheets. |
| `assets/new-illustartions/` (`5` files, `6.65 MB`) | **C — Original Source Artwork** | Original user-dropped source sheets; never deleted automatically. |
| `assets/piku-character-sheet.png` (`1.57 MB`) | **C — Original Reference Artwork** | Referenced by `docs/piku-illustration-spec.json` (`referenceSheet`) and explicitly kept public by project owner. |
| `src-tauri/icons/` (`5` files) | **D — Active Runtime Icons** | Required by `src-tauri/tauri.conf.json`, Windows NSIS installer, and system tray (`tray.png`, `icon.ico`, `32x32.png`, `128x128.png`, `128x128@2x.png`). |
| `assets/logo/piku-logo.png` & `piku-icon.png` | **D — Public README / Brand** | `assets/logo/piku-logo.png` is actively rendered in `README.md`. |
| `assets/demo/piku-demo.gif` & `assets/screenshots/*.png` (`6` files) | **D — Public Documentation** | Actively rendered in the `README.md` screenshot gallery and demo showcase. |
| `docs/*` (`architecture.md`, `building-from-source.md`, `data-storage.md`, `privacy.md`, `piku-illustration-spec.json`) | **D — Project Documentation** | Active repository documentation and illustration specification. |
| `.internal/` | **D — Local Archive (Gitignored)** | Local historical QA logs and phase reports; untouched. |

---

## 4. Unresolved References

**None (`0` broken or unresolved references).**

- All runtime code references (`src/animation/spriteStore.ts`, `src/components/panel/TodayView.tsx`, `src/components/history/HistoryView.tsx`), Tauri icon paths (`src-tauri/tauri.conf.json`), `README.md` media links, `docs/building-from-source.md`, and `docs/piku-illustration-spec.json` resolve cleanly to existing files on disk.
- Neither moved legacy script (`generate_placeholders.py`, `process_sheet.py`) was referenced by application code, `package.json`, Cargo/Tauri configs, or active docs.

---

## 5. Git Tracking Recommendations & Proposed `.gitignore` Review

`.gitignore` was **NOT modified** automatically. Current on-disk sizes for review:

| Path | Files | Size | Recommendation |
| --- | :-: | :-: | --- |
| `public/assets/character/piku/` | `147` | `3.74 MB` | **Track** — required production runtime sprites & manifest. |
| `art-source/original/` | `5` | `6.65 MB` | **Track** — canonical untouched master illustration sheets. |
| `art-source/processed/` (manifests + `156` assets) | `158` | `4.34 MB` | **Track** — full processed library, `manifest.json`, and `ASSET_VALIDATION_REPORT.md`. |
| `art-source/processed/contact-sheets/` | `33` | `6.23 MB` | **Track (or optional ignore)** — useful for visual GitHub review; regenerable via `extract_illustration_library.py` if you want a smaller repo. |
| `art-source/archive/legacy-v1/` | `43` | `2.53 MB` | **Track** — preserves full v1.0.0 historical archive + `legacy-manifest.json` + this report. |
| `scripts/assets/` (`extract_illustration_library.py`, `validate_assets.py`) | `2` | `50 KB` | **Track** — reproducible extraction & validation tooling. |
| `docs/piku-illustration-spec.json` | `1` | `31 KB` | **Track** — master illustration specification. |
| `assets/new-illustartions/` | `5` | `6.65 MB` | **Optional ignore (duplicate of `art-source/original/`)** — byte-for-byte identical to `art-source/original/`; ignoring it avoids committing two 6.65 MB copies of the same 5 sheets without deleting your local folder. |

### Proposed `.gitignore` Addition (For Review Only — Not Applied)

```gitignore
# Python bytecode from asset scripts
scripts/assets/__pycache__/
*.pyc

# Ephemeral art working folders (do NOT ignore original/, processed/, or archive/)
art-source/tmp/
art-source/work/

# Optional: uncomment if you want to avoid tracking the duplicate raw drop folder
# (canonical untouched copies are already tracked in art-source/original/)
# assets/new-illustartions/

# Optional: uncomment if you prefer regenerating contact sheets instead of tracking 6.2 MB of previews
# art-source/processed/contact-sheets/
```

---

## 6. Final Verification Results

| Check | Result | Details |
| --- | :---: | --- |
| Archived file existence & `SHA-256` integrity | **PASS** | `41/41` archived files verified on disk with exact `SHA-256` match against sources (`HEAD` / original files). |
| Before/after file counts | **PASS** | `scripts/assets/`: `4 → 2` (2 legacy scripts moved cleanly to `legacy-v1/other/`); `art-source/original/`: `5/5` untouched (`SHA-256` identical to `assets/new-illustartions/`); `public/assets/character/piku/`: `147` files intact; `art-source/processed/`: `156` assets + `33` contact sheets intact. |
| Runtime & documentation references | **PASS** | `20/20` checked code, Tauri icon, README, and doc paths resolve on disk (`0` broken). |
| `python scripts/assets/validate_assets.py` | **PASS** | `0` errors (`144/144` runtime frames, `2/2` runtime static UI states, `156/156` library assets, `24` contact sheets, `5/5` Tauri icons). |
| `pnpm run typecheck` (`tsc --noEmit`) | **PASS** | `0` TypeScript errors. |
| `pnpm run build` (`tsc --noEmit && vite build`) | **PASS** | Built cleanly into `dist/` in `10.00s`. |
| Git safety | **PASS** | Zero Git mutations (`git add`, `commit`, `push`, `tag`, `reset`) performed. |
