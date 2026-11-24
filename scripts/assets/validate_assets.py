#!/usr/bin/env python3
"""
Automated Asset Validator for Piku Desktop Pet.

Verifies every technical gate from docs/PIKU_ASSET_REQUIREMENTS.md:
  1. public/assets/character/piku/manifest.json schema, MVP states, and frame count (34 total).
  2. Exact 256x256 dimensions and 32-bit RGBA transparent mode for every frame.
  3. Clean transparent corners and 4px outer safe border (zero clipping / zero baked backdrop).
  4. Runtime file size budget (<= 25 KB per frame, total pack <= 850 KB).
  5. Anchor/baseline consistency on grounded frames (prevents jitter).
  6. Required Windows Tauri app & system tray icons in src-tauri/icons/.

Usage (from project root):
  python scripts/assets/validate_assets.py
  python scripts/assets/validate_assets.py --max-kb 25
"""

from __future__ import annotations

import argparse
from collections import deque
import json
from pathlib import Path
import re

import numpy as np
from PIL import Image

EXPECTED_MVP_STATES = {
    "idle": {"frames": 6, "loop": True},
    "blink": {"frames": 4, "loop": False},
    "sleep": {"frames": 6, "loop": True},
    "wave": {"frames": 6, "loop": False},
    "happy": {"frames": 6, "loop": False},
    "click": {"frames": 6, "loop": False},
}

# Airborne frames intentionally lift off the ground baseline (y=216 at 256x256)
AIRBORNE_FRAMES = {
    ("happy", 2),
    ("happy", 3),
    ("happy", 4),
    ("click", 4),
}

FILENAME_RE = re.compile(r"^[a-z0-9_]+_[0-9]{2}\.png$")
SAFE_BORDER_PX = 4
EXPECTED_BASELINE_Y = 216
EXPECTED_CENTER_X = 128


def largest_component_bounds(alpha: np.ndarray) -> tuple[int, int, int, int, float] | None:
    """Find (min_x, max_x, min_y, max_y, core_cx) of the largest 4-connected opaque component."""
    mask = alpha > 16
    if not np.any(mask):
        return None
    h, w = mask.shape
    visited = np.zeros((h, w), dtype=bool)
    best: list[tuple[int, int]] = []

    ys, xs = np.nonzero(mask)
    for y0, x0 in zip(ys.tolist(), xs.tolist()):
        if visited[y0, x0]:
            continue
        visited[y0, x0] = True
        q: deque[tuple[int, int]] = deque([(y0, x0)])
        pts: list[tuple[int, int]] = []
        while q:
            cy, cx = q.popleft()
            pts.append((cy, cx))
            if cy > 0 and mask[cy - 1, cx] and not visited[cy - 1, cx]:
                visited[cy - 1, cx] = True
                q.append((cy - 1, cx))
            if cy + 1 < h and mask[cy + 1, cx] and not visited[cy + 1, cx]:
                visited[cy + 1, cx] = True
                q.append((cy + 1, cx))
            if cx > 0 and mask[cy, cx - 1] and not visited[cy, cx - 1]:
                visited[cy, cx - 1] = True
                q.append((cy, cx - 1))
            if cx + 1 < w and mask[cy, cx + 1] and not visited[cy, cx + 1]:
                visited[cy, cx + 1] = True
                q.append((cy, cx + 1))
        if len(pts) > len(best):
            best = pts

    arr = np.array(best, dtype=np.int32)
    by, bx = arr[:, 0], arr[:, 1]
    min_y, max_y = int(by.min()), int(by.max())
    height = max(1, max_y - min_y + 1)
    band = (by >= min_y + int(height * 0.35)) & (by <= min_y + int(height * 0.75))
    core_cx = float(np.mean(bx[band])) if np.any(band) else float(np.mean(bx))
    return int(bx.min()), int(bx.max()), min_y, max_y, core_cx


def main() -> int:
    parser = argparse.ArgumentParser(description="Validate Piku runtime sprites, manifest, and icons.")
    parser.add_argument("--root", type=Path, default=Path("."), help="Project root directory")
    parser.add_argument("--max-kb", type=float, default=25.0, help="Max allowed size per 256x256 PNG (KB)")
    args = parser.parse_args()

    root = args.root.resolve()
    char_dir = root / "public" / "assets" / "character" / "piku"
    manifest_path = char_dir / "manifest.json"

    errors: list[str] = []
    warnings: list[str] = []

    if not manifest_path.exists():
        print(f"FAIL: Missing manifest at {manifest_path}")
        return 1

    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        print(f"FAIL: Invalid JSON in {manifest_path}: {exc}")
        return 1

    # 1. Validate manifest schema
    for key in ("character", "version", "assetKind", "frameSize", "anchor", "defaultState", "states"):
        if key not in manifest:
            errors.append(f"manifest.json missing required field '{key}'")

    frame_w = manifest.get("frameSize", {}).get("width")
    frame_h = manifest.get("frameSize", {}).get("height")
    if (frame_w, frame_h) != (256, 256):
        errors.append(f"manifest.json frameSize expected 256x256, got {frame_w}x{frame_h}")

    states = manifest.get("states", {})
    total_frames = 0
    total_bytes = 0
    max_frame_bytes = 0

    for state_name, req in EXPECTED_MVP_STATES.items():
        if state_name not in states:
            errors.append(f"manifest.json missing required MVP state '{state_name}'")
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
            errors.append(f"state '{state_name}' path pattern missing '{{nn}}': {pattern}")
            continue

        for idx in range(1, int(cfg.get("frames", 0)) + 1):
            rel_path = pattern.replace("{nn}", f"{idx:02d}")
            fpath = char_dir / rel_path
            total_frames += 1

            if not FILENAME_RE.match(fpath.name):
                errors.append(f"{rel_path}: filename violates lowercase <state>_<NN>.png convention")

            if not fpath.exists():
                errors.append(f"{rel_path}: file not found")
                continue

            fsize = fpath.stat().st_size
            total_bytes += fsize
            max_frame_bytes = max(max_frame_bytes, fsize)
            if fsize > args.max_kb * 1024:
                errors.append(f"{rel_path}: {fsize / 1024:.1f} KB exceeds {args.max_kb:.1f} KB budget")

            with Image.open(fpath) as img:
                if img.size != (256, 256):
                    errors.append(f"{rel_path}: expected 256x256, got {img.size[0]}x{img.size[1]}")
                    continue
                if img.mode != "RGBA":
                    errors.append(f"{rel_path}: expected RGBA mode, got {img.mode}")
                    continue

                arr = np.asarray(img, dtype=np.uint8)
                alpha = arr[..., 3]

                # Transparent corners & 4px outer safe border check
                if (
                    np.any(alpha[:SAFE_BORDER_PX, :] > 0)
                    or np.any(alpha[-SAFE_BORDER_PX:, :] > 0)
                    or np.any(alpha[:, :SAFE_BORDER_PX] > 0)
                    or np.any(alpha[:, -SAFE_BORDER_PX:] > 0)
                ):
                    errors.append(f"{rel_path}: non-transparent pixels encroach into 4px outer safe border")

                opaque_px = int(np.count_nonzero(alpha > 16))
                if opaque_px < 400:
                    errors.append(f"{rel_path}: frame appears empty or too small ({opaque_px} opaque px)")
                    continue

                # Anchor/baseline consistency check on grounded frames
                bounds = largest_component_bounds(alpha)
                if bounds is not None and (state_name, idx) not in AIRBORNE_FRAMES:
                    _, _, _, max_y, core_cx = bounds
                    if abs(max_y - EXPECTED_BASELINE_Y) > 4:
                        errors.append(
                            f"{rel_path}: grounded baseline y={max_y} drifts from canonical y={EXPECTED_BASELINE_Y} (+/-4 px)"
                        )
                    if abs(core_cx - EXPECTED_CENTER_X) > 10.0:
                        warnings.append(
                            f"{rel_path}: horizontal core center x={core_cx:.1f} shifted from x={EXPECTED_CENTER_X}"
                        )

    if total_frames != 34:
        errors.append(f"Expected 34 total MVP frames, validated {total_frames}")

    if total_bytes > 850 * 1024:
        errors.append(f"Total runtime pack size {total_bytes / 1024:.1f} KB exceeds 850 KB budget")

    # 2. Validate Windows Tauri app & system tray icons
    icons_dir = root / "src-tauri" / "icons"
    icon_checks = [
        ("tray.png", (32, 32), "RGBA"),
        ("32x32.png", (32, 32), "RGBA"),
        ("128x128.png", (128, 128), "RGBA"),
        ("128x128@2x.png", (256, 256), "RGBA"),
    ]
    for name, expected_size, expected_mode in icon_checks:
        ipath = icons_dir / name
        if not ipath.exists():
            errors.append(f"src-tauri/icons/{name}: missing")
            continue
        with Image.open(ipath) as im:
            if im.size != expected_size:
                errors.append(f"src-tauri/icons/{name}: expected {expected_size}, got {im.size}")
            if im.mode != expected_mode:
                errors.append(f"src-tauri/icons/{name}: expected {expected_mode}, got {im.mode}")
            arr = np.asarray(im, dtype=np.uint8)
            if arr[0, 0, 3] != 0 or arr[0, -1, 3] != 0:
                warnings.append(f"src-tauri/icons/{name}: corner pixels are not transparent")

    ico_path = icons_dir / "icon.ico"
    if not ico_path.exists() or ico_path.stat().st_size == 0:
        errors.append("src-tauri/icons/icon.ico: missing or empty")

    # Summary output
    print("=== Piku Asset Validation Summary ===")
    print(f"Manifest:       {manifest_path.relative_to(root)} (assetKind={manifest.get('assetKind')})")
    print(f"Frames checked: {total_frames}/34")
    print(f"Pack size:      {total_bytes / 1024:.1f} KB total (max single frame: {max_frame_bytes / 1024:.1f} KB)")
    print(f"Icons checked:  5 (tray.png, 32x32.png, 128x128.png, 128x128@2x.png, icon.ico)")

    if warnings:
        print(f"Warnings ({len(warnings)}):")
        for w in warnings:
            print(f"  [WARN] {w}")

    if errors:
        print(f"FAIL ({len(errors)} errors):")
        for e in errors:
            print(f"  [FAIL] {e}")
        return 1

    print("RESULT: PASS (0 errors)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
