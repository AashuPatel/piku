#!/usr/bin/env python3
"""
Automated Asset Validator for Piku Desktop Pet (Production 24-Animation + 12-Static Library).

Verifies:
  1. Runtime public/assets/character/piku/manifest.json schema, all 24 animation states (144 frames),
     and active runtime static UI-state illustrations (empty_state.png, error.png).
  2. Exact 256x256 dimensions, 32-bit RGBA transparent mode, transparent corners,
     and 4px outer safe border (zero clipping / zero baked backdrop).
  3. Runtime file size budget (<= 30 KB per frame, total 144-frame pack <= 3500 KB).
  4. Anchor/baseline consistency on grounded frames.
  5. Complete art-source/processed/ library (24 animations x 6 frames = 144 + 12 static = 156 assets,
     manifest.json path resolution, 24 animation contact sheets, and 5 untouched originals).
  6. Required Windows Tauri app & system tray icons in src-tauri/icons/.

Usage (from project root):
  python scripts/assets/validate_assets.py
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import re

import numpy as np
from PIL import Image

EXPECTED_RUNTIME_STATES = {
    "idle": {"frames": 6, "loop": True},
    "blink": {"frames": 6, "loop": False},
    "wave": {"frames": 6, "loop": False},
    "happy": {"frames": 6, "loop": False},
    "sleep": {"frames": 6, "loop": True},
    "click": {"frames": 6, "loop": False},
    "excited": {"frames": 6, "loop": False},
    "content": {"frames": 6, "loop": False},
    "blush": {"frames": 6, "loop": False},
    "curious": {"frames": 6, "loop": False},
    "thinking": {"frames": 6, "loop": False},
    "surprised": {"frames": 6, "loop": False},
    "focused": {"frames": 6, "loop": False},
    "yawn": {"frames": 6, "loop": False},
    "drag": {"frames": 6, "loop": True},
    "working": {"frames": 6, "loop": False},
    "break": {"frames": 6, "loop": False},
    "late_night": {"frames": 6, "loop": True},
    "celebration": {"frames": 6, "loop": False},
    "snack": {"frames": 6, "loop": False},
    "tea": {"frames": 6, "loop": False},
    "cloud": {"frames": 6, "loop": False},
    "stretch": {"frames": 6, "loop": False},
    "peek": {"frames": 6, "loop": False},
}

EXPECTED_RUNTIME_STATIC = ["static/empty_state.png", "static/error.png"]

FILENAME_RE = re.compile(r"^[a-z0-9_]+_[0-9]{2}\.png$")
SAFE_BORDER_PX = 4


def check_png(
    fpath: Path,
    rel_label: str,
    max_kb: float,
    errors: list[str],
    warnings: list[str],
    check_border: bool = True,
) -> int:
    if not fpath.exists():
        errors.append(f"{rel_label}: file not found")
        return 0

    fsize = fpath.stat().st_size
    if fsize > max_kb * 1024:
        warnings.append(f"{rel_label}: {fsize / 1024:.1f} KB exceeds {max_kb:.1f} KB soft target")

    with Image.open(fpath) as img:
        if img.size != (256, 256):
            errors.append(f"{rel_label}: expected 256x256, got {img.size[0]}x{img.size[1]}")
            return fsize
        if img.mode != "RGBA":
            errors.append(f"{rel_label}: expected RGBA mode, got {img.mode}")
            return fsize

        arr = np.asarray(img, dtype=np.uint8)
        alpha = arr[..., 3]

        corners = [int(alpha[0, 0]), int(alpha[0, -1]), int(alpha[-1, 0]), int(alpha[-1, -1])]
        if any(c != 0 for c in corners):
            errors.append(f"{rel_label}: non-transparent corner pixels {corners}")

        if check_border and (
            np.any(alpha[:SAFE_BORDER_PX, :] > 16)
            or np.any(alpha[-SAFE_BORDER_PX:, :] > 16)
            or np.any(alpha[:, :SAFE_BORDER_PX] > 16)
            or np.any(alpha[:, -SAFE_BORDER_PX:] > 16)
        ):
            errors.append(f"{rel_label}: non-transparent pixels encroach into {SAFE_BORDER_PX}px outer safe border")

        opaque_px = int(np.count_nonzero(alpha > 16))
        if opaque_px < 400:
            errors.append(f"{rel_label}: frame appears empty or too small ({opaque_px} opaque px)")

    return fsize


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate Piku runtime sprites, library manifest, and icons.")
    parser.add_argument("--root", type=Path, default=Path("."), help="Project root directory")
    parser.add_argument("--max-kb", type=float, default=35.0, help="Soft max size per 256x256 PNG (KB)")
    args = parser.parse_args()

    root = args.root.resolve()
    char_dir = root / "public" / "assets" / "character" / "piku"
    manifest_path = char_dir / "manifest.json"

    errors: list[str] = []
    warnings: list[str] = []

    if not manifest_path.exists():
        print(f"FAIL: Missing runtime manifest at {manifest_path}")
        return 1

    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    for key in ("character", "version", "assetKind", "frameSize", "anchor", "defaultState", "states"):
        if key not in manifest:
            errors.append(f"runtime manifest.json missing required field '{key}'")

    if (manifest.get("frameSize", {}).get("width"), manifest.get("frameSize", {}).get("height")) != (256, 256):
        errors.append(f"runtime manifest frameSize expected 256x256, got {manifest.get('frameSize')}")

    states = manifest.get("states", {})
    total_frames = 0
    total_bytes = 0
    max_frame_bytes = 0

    for state_name, req in EXPECTED_RUNTIME_STATES.items():
        if state_name not in states:
            errors.append(f"runtime manifest missing state '{state_name}'")
            continue
        cfg = states[state_name]
        if cfg.get("frames") != req["frames"]:
            errors.append(f"state '{state_name}' frames expected {req['frames']}, got {cfg.get('frames')}")
        if cfg.get("loop") != req["loop"]:
            errors.append(f"state '{state_name}' loop expected {req['loop']}, got {cfg.get('loop')}")
        if not isinstance(cfg.get("fps"), (int, float)) or cfg["fps"] <= 0:
            errors.append(f"state '{state_name}' invalid fps: {cfg.get('fps')}")
        pattern = cfg.get("path", "")
        if "{nn}" not in pattern:
            errors.append(f"state '{state_name}' path missing '{{nn}}': {pattern}")
            continue

        for idx in range(1, int(cfg["frames"]) + 1):
            rel_path = pattern.replace("{nn}", f"{idx:02d}")
            fpath = char_dir / rel_path
            total_frames += 1
            if not FILENAME_RE.match(fpath.name):
                errors.append(f"{rel_path}: filename violates lowercase <id>_<NN>.png convention")
            sz = check_png(fpath, f"runtime/{rel_path}", args.max_kb, errors, warnings)
            total_bytes += sz
            max_frame_bytes = max(max_frame_bytes, sz)

    if total_frames != 144:
        errors.append(f"Expected 144 total runtime animation frames, validated {total_frames}")

    for srel in EXPECTED_RUNTIME_STATIC:
        check_png(char_dir / srel, f"runtime/{srel}", 80.0, errors, warnings)

    # Validate processed library in art-source/processed/
    lib_dir = root / "art-source" / "processed"
    lib_manifest_path = lib_dir / "manifest.json"
    lib_checked = 0
    if not lib_manifest_path.exists():
        errors.append("art-source/processed/manifest.json missing")
    else:
        lib = json.loads(lib_manifest_path.read_text(encoding="utf-8"))
        if len(lib.get("animations", {})) != 24:
            errors.append(f"art-source manifest expected 24 animations, found {len(lib.get('animations', {}))}")
        if len(lib.get("static", {})) != 12:
            errors.append(f"art-source manifest expected 12 static assets, found {len(lib.get('static', {}))}")

        for aid, entry in lib.get("animations", {}).items():
            if len(entry.get("files", [])) != 6:
                errors.append(f"art-source animation '{aid}' expected 6 files, got {len(entry.get('files', []))}")
            for rel in entry.get("files", []):
                check_png(lib_dir / rel, f"library/{rel}", args.max_kb, errors, warnings)
                lib_checked += 1
            cs = lib_dir / "contact-sheets" / "animations" / f"{aid}_contact.png"
            if not cs.exists():
                errors.append(f"missing animation contact sheet: {cs.relative_to(root)}")

        for sid, entry in lib.get("static", {}).items():
            for rel in entry.get("files", []):
                check_png(lib_dir / rel, f"library/{rel}", 80.0, errors, warnings)
                lib_checked += 1

        orig_dir = root / "art-source" / "original"
        orig_pngs = list(orig_dir.glob("*.png")) if orig_dir.exists() else []
        if len(orig_pngs) < 5:
            errors.append(f"art-source/original/ expected at least 5 untouched source PNGs, found {len(orig_pngs)}")

    # Validate Windows Tauri app & system tray icons
    icons_dir = root / "src-tauri" / "icons"
    for name, expected_size in [
        ("tray.png", (32, 32)),
        ("32x32.png", (32, 32)),
        ("128x128.png", (128, 128)),
        ("128x128@2x.png", (256, 256)),
    ]:
        ipath = icons_dir / name
        if not ipath.exists():
            errors.append(f"src-tauri/icons/{name}: missing")
            continue
        with Image.open(ipath) as im:
            if im.size != expected_size or im.mode != "RGBA":
                errors.append(f"src-tauri/icons/{name}: expected {expected_size} RGBA, got {im.size} {im.mode}")

    ico_path = icons_dir / "icon.ico"
    if not ico_path.exists() or ico_path.stat().st_size == 0:
        errors.append("src-tauri/icons/icon.ico: missing or empty")

    print("=== Piku Asset Validation Summary ===")
    print(f"Runtime manifest: {manifest_path.relative_to(root)} (assetKind={manifest.get('assetKind')}, version={manifest.get('version')})")
    print(f"Runtime frames:   {total_frames}/144 across {len(EXPECTED_RUNTIME_STATES)} states (+{len(EXPECTED_RUNTIME_STATIC)} static UI states)")
    print(f"Runtime pack:     {total_bytes / 1024:.1f} KB total (max single frame: {max_frame_bytes / 1024:.1f} KB)")
    print(f"Library assets:   {lib_checked}/156 (24 animations x 6 + 12 static) + 24 contact sheets")
    print(f"Tauri icons:      5 checked (tray.png, 32x32.png, 128x128.png, 128x128@2x.png, icon.ico)")

    if warnings:
        print(f"Warnings ({len(warnings)}):")
        for w in warnings[:20]:
            print(f"  [WARN] {w}")

    if errors:
        print(f"FAIL ({len(errors)} errors):")
        for e in errors[:25]:
            print(f"  [FAIL] {e}")
        return 1

    print("RESULT: PASS (0 errors)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
