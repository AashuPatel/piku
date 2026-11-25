#!/usr/bin/env python3
"""
Deterministic AI Sprite-Sheet Extractor, Normalizer & QC Sheet Generator for Piku.

Takes a 3x2 (1536x1024) or 2x2 (1024x1024) sprite sheet and:
  1. Partitions it into uniform cell tiles and flags any tile boundary bleed.
  2. Extracts clean transparency (native RGBA OR exterior BFS matte removal +
     1px outline color decontamination toward #473527, protecting interior highlights).
  3. Removes tiny stray dust speckles (< 12 px^2) while keeping intentional FX (sparks, z Z).
  4. Aligns frames CONSERVATIVELY using integer pixel translation ONLY (never warps
     or auto-rescales Piku!):
       - Body-core horizontal center -> x = 256 (512 master)
       - Grounded bottom foot/puddle -> y = 432 (512 master)
       - Airborne frames (happy 02-04, click 04) -> documented vertical lift offsets
         (or untouched artist Y via --preserve-y)
  5. Flags suspicious frames (scale variance > 8%, edge clipping, tile bleed, low pixel area).
  6. Exports 512x512 masters, premultiplied-alpha Lanczos 256x256 runtime PNGs,
     an onion-skin alignment preview, and a dark/cream backdrop QC contact sheet.

Usage (from project root):
  python scripts/assets/process_sheet.py --state idle --input art-source/raw-sheets/sheet_idle_v1.png --grid 3x2
  python scripts/assets/process_sheet.py --state blink --input art-source/raw-sheets/sheet_blink_v1.png --grid 2x2
"""

from __future__ import annotations

import argparse
from collections import deque
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

MASTER_SIZE = 512
RUNTIME_SIZE = 256
ANCHOR_X = 256
GROUND_Y = 432
SAFE_MARGIN_MASTER = 8
OUTLINE_RGB = np.array([71.0, 53.0, 39.0], dtype=np.float32)  # #473527

# Documented airborne vertical lifts on 512x512 master (from docs/PIKU_ASSET_REQUIREMENTS.md §6.5)
AIRBORNE_LIFTS: dict[tuple[str, int], int] = {
    ("happy", 2): 28,
    ("happy", 3): 72,
    ("happy", 4): 36,
    ("click", 4): 48,
}


def downscale_premultiplied(img: Image.Image, target_size: tuple[int, int]) -> Image.Image:
    arr = np.asarray(img.convert("RGBA"), dtype=np.float32)
    alpha = arr[..., 3:4] / 255.0
    premul = np.empty_like(arr)
    premul[..., :3] = arr[..., :3] * alpha
    premul[..., 3:4] = arr[..., 3:4]

    resized = Image.fromarray(np.clip(np.round(premul), 0, 255).astype(np.uint8), mode="RGBA").resize(
        target_size, Image.Resampling.LANCZOS
    )
    out = np.asarray(resized, dtype=np.float32)
    a_out = out[..., 3:4]
    safe_a = np.where(a_out > 0.5, a_out / 255.0, 1.0)
    rgb = np.where(a_out > 0.5, out[..., :3] / safe_a, 0.0)

    final = np.empty_like(out)
    final[..., :3] = np.clip(np.round(rgb), 0, 255)
    final[..., 3:4] = np.where(a_out > 0.5, np.clip(np.round(a_out), 0, 255), 0.0)
    return Image.fromarray(final.astype(np.uint8), mode="RGBA")


def exterior_matte_mask(rgb: np.ndarray, matte_rgb: np.ndarray, tolerance: float) -> np.ndarray:
    """BFS flood-fill from image border to identify exterior background pixels only."""
    h, w, _ = rgb.shape
    dist = np.max(np.abs(rgb.astype(np.float32) - matte_rgb[None, None, :]), axis=2)
    candidate = dist <= tolerance

    bg = np.zeros((h, w), dtype=bool)
    queue: deque[tuple[int, int]] = deque()

    for x in range(w):
        if candidate[0, x] and not bg[0, x]:
            bg[0, x] = True
            queue.append((0, x))
        if candidate[h - 1, x] and not bg[h - 1, x]:
            bg[h - 1, x] = True
            queue.append((h - 1, x))
    for y in range(h):
        if candidate[y, 0] and not bg[y, 0]:
            bg[y, 0] = True
            queue.append((y, 0))
        if candidate[y, w - 1] and not bg[y, w - 1]:
            bg[y, w - 1] = True
            queue.append((y, w - 1))

    while queue:
        cy, cx = queue.popleft()
        if cy > 0 and candidate[cy - 1, cx] and not bg[cy - 1, cx]:
            bg[cy - 1, cx] = True
            queue.append((cy - 1, cx))
        if cy + 1 < h and candidate[cy + 1, cx] and not bg[cy + 1, cx]:
            bg[cy + 1, cx] = True
            queue.append((cy + 1, cx))
        if cx > 0 and candidate[cy, cx - 1] and not bg[cy, cx - 1]:
            bg[cy, cx - 1] = True
            queue.append((cy, cx - 1))
        if cx + 1 < w and candidate[cy, cx + 1] and not bg[cy, cx + 1]:
            bg[cy, cx + 1] = True
            queue.append((cy, cx + 1))

    return bg


def dilate_1px(mask: np.ndarray) -> np.ndarray:
    out = mask.copy()
    out[1:, :] |= mask[:-1, :]
    out[:-1, :] |= mask[1:, :]
    out[:, 1:] |= mask[:, :-1]
    out[:, :-1] |= mask[:, 1:]
    return out


def remove_matte_and_defringe(
    tile: Image.Image,
    matte_mode: str = "auto",
    tolerance: float = 28.0,
) -> tuple[np.ndarray, list[str]]:
    """Return RGBA uint8 array with clean transparent background & edge defringing."""
    warnings: list[str] = []
    arr = np.asarray(tile.convert("RGBA"), dtype=np.uint8).copy()
    h, w, _ = arr.shape
    corners = np.array([arr[0, 0], arr[0, w - 1], arr[h - 1, 0], arr[h - 1, w - 1]], dtype=np.float32)

    use_transparent = matte_mode == "transparent" or (
        matte_mode == "auto" and np.all(corners[:, 3] < 10)
    )

    if use_transparent:
        # Check for semi-transparent corner haze
        if np.any((corners[:, 3] > 0) & (corners[:, 3] < 245)):
            warnings.append("semi_transparent_corner_haze")
        arr[arr[..., 3] < 4, :3] = 0
        arr[arr[..., 3] < 4, 3] = 0
        return arr, warnings

    # Solid matte removal (exterior-connected only — protects interior highlights)
    matte_rgb = (
        np.array([255.0, 255.0, 255.0], dtype=np.float32)
        if matte_mode == "white"
        else np.median(corners[:, :3], axis=0)
    )
    bg_mask = exterior_matte_mask(arr[..., :3], matte_rgb, tolerance=tolerance)
    arr[bg_mask, 3] = 0
    arr[bg_mask, :3] = 0

    # 2px exterior outline decontamination against warm dark brown (#473527)
    ring1 = dilate_1px(bg_mask) & (~bg_mask)
    ring2 = dilate_1px(bg_mask | ring1) & (~(bg_mask | ring1))
    border_ring = ring1 | ring2
    if np.any(border_ring):
        ring_rgb = arr[border_ring, :3].astype(np.float32)
        matte_vec = matte_rgb - OUTLINE_RGB
        denom = float(np.dot(matte_vec, matte_vec))
        if denom > 1.0:
            proj = np.clip(np.sum((ring_rgb - OUTLINE_RGB) * matte_vec, axis=1) / denom, 0.0, 1.0)
            reconstructed = OUTLINE_RGB + proj[:, None] * matte_vec
            line_err = np.max(np.abs(ring_rgb - reconstructed), axis=1)
            is_fringe = (line_err < 52.0) & (proj > 0.06)
            if np.any(is_fringe):
                new_alpha = np.clip(np.round((1.0 - proj) * 255.0), 0, 255).astype(np.uint8)
                ring_indices = np.where(border_ring)
                fringe_y = ring_indices[0][is_fringe]
                fringe_x = ring_indices[1][is_fringe]
                arr[fringe_y, fringe_x, :3] = OUTLINE_RGB.astype(np.uint8)
                arr[fringe_y, fringe_x, 3] = np.minimum(arr[fringe_y, fringe_x, 3], new_alpha[is_fringe])

    arr[arr[..., 3] == 0, :3] = 0
    return arr, warnings


def connected_components(mask: np.ndarray) -> list[np.ndarray]:
    """Return list of (N, 2) arrays of (y, x) coordinates for each 4-connected component."""
    h, w = mask.shape
    visited = np.zeros((h, w), dtype=bool)
    comps: list[np.ndarray] = []

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
        comps.append(np.array(pts, dtype=np.int32))

    comps.sort(key=lambda c: len(c), reverse=True)
    return comps


def prune_dust_speckles(arr: np.ndarray, min_area: int = 12) -> tuple[np.ndarray, list[np.ndarray]]:
    """Remove tiny detached dust speckles (< min_area px) while keeping Piku & intentional FX."""
    mask = arr[..., 3] > 16
    comps = connected_components(mask)
    kept: list[np.ndarray] = []
    for comp in comps:
        if len(comp) < min_area:
            arr[comp[:, 0], comp[:, 1], :] = 0
        else:
            kept.append(comp)
    return arr, kept


def align_frame_conservatively(
    arr: np.ndarray,
    main_comp: np.ndarray,
    *,
    target_x: int = ANCHOR_X,
    target_bottom_y: int = GROUND_Y,
    preserve_y: bool = False,
) -> tuple[np.ndarray, dict[str, int]]:
    """Translate frame by integer dx, dy only (never distorts or rescales Piku)."""
    ys = main_comp[:, 0]
    xs = main_comp[:, 1]
    min_y, max_y = int(ys.min()), int(ys.max())
    height = max(1, max_y - min_y + 1)

    # Compute body-core horizontal center from middle 35%..75% vertical slice
    # (ignores outstretched waving arms & detached sparks)
    band_top = min_y + int(height * 0.35)
    band_bot = min_y + int(height * 0.75)
    band_mask = (ys >= band_top) & (ys <= band_bot)
    core_xs = xs[band_mask] if np.any(band_mask) else xs
    core_cx = int(round(float(np.mean(core_xs))))

    dx = target_x - core_cx
    dy = 0 if preserve_y else (target_bottom_y - max_y)

    h, w, _ = arr.shape
    out = np.zeros((MASTER_SIZE, MASTER_SIZE, 4), dtype=np.uint8)

    # Shift from tile coordinates into centered MASTER_SIZE canvas
    src_y0 = max(0, -dy)
    src_y1 = min(h, MASTER_SIZE - dy)
    src_x0 = max(0, -dx)
    src_x1 = min(w, MASTER_SIZE - dx)

    dst_y0 = max(0, dy)
    dst_y1 = dst_y0 + (src_y1 - src_y0)
    dst_x0 = max(0, dx)
    dst_x1 = dst_x0 + (src_x1 - src_x0)

    if src_y1 > src_y0 and src_x1 > src_x0:
        out[dst_y0:dst_y1, dst_x0:dst_x1] = arr[src_y0:src_y1, src_x0:src_x1]

    bbox_w = int(xs.max() - xs.min() + 1)
    bbox_h = height
    return out, {"dx": dx, "dy": dy, "bbox_w": bbox_w, "bbox_h": bbox_h, "area": int(len(main_comp))}


def build_onionskin(frames: list[Image.Image], out_path: Path) -> None:
    acc = np.zeros((MASTER_SIZE, MASTER_SIZE, 4), dtype=np.float32)
    weight = 1.0 / max(1, len(frames))
    for frame in frames:
        arr = np.asarray(frame, dtype=np.float32)
        acc += arr * weight

    base = Image.new("RGBA", (MASTER_SIZE, MASTER_SIZE), (248, 246, 239, 255))
    overlay = Image.fromarray(np.clip(np.round(acc), 0, 255).astype(np.uint8), mode="RGBA")
    base.alpha_composite(overlay)

    draw = ImageDraw.Draw(base)
    # Crosshair at canonical anchor (x=256, y=432) and 8px safe border
    draw.line((ANCHOR_X, 0, ANCHOR_X, MASTER_SIZE), fill=(180, 70, 70, 140), width=1)
    draw.line((0, GROUND_Y, MASTER_SIZE, GROUND_Y), fill=(180, 70, 70, 180), width=2)
    draw.rectangle(
        (SAFE_MARGIN_MASTER, SAFE_MARGIN_MASTER, MASTER_SIZE - SAFE_MARGIN_MASTER - 1, MASTER_SIZE - SAFE_MARGIN_MASTER - 1),
        outline=(110, 140, 200, 140),
        width=1,
    )
    out_path.parent.mkdir(parents=True, exist_ok=True)
    base.save(out_path, format="PNG", optimize=True)


def build_backdrop_qc(frames: list[Image.Image], out_path: Path) -> None:
    cell = 160
    pad = 10
    w = len(frames) * (cell + pad) + pad
    h_half = cell + pad * 2
    sheet = Image.new("RGBA", (w, h_half * 2), (248, 246, 239, 255))
    draw = ImageDraw.Draw(sheet)
    draw.rectangle((0, 0, w, h_half), fill=(248, 246, 239, 255))
    draw.rectangle((0, h_half, w, h_half * 2), fill=(24, 24, 27, 255))

    for i, frame in enumerate(frames):
        thumb = downscale_premultiplied(frame, (cell, cell))
        x = pad + i * (cell + pad)
        sheet.alpha_composite(thumb, (x, pad))
        sheet.alpha_composite(thumb, (x, h_half + pad))

    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path, format="PNG", optimize=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Slice, clean, align, and QC an AI sprite sheet for Piku.")
    parser.add_argument("--state", required=True, help="Animation state name (idle|blink|sleep|wave|happy|click)")
    parser.add_argument("--input", required=True, type=Path, help="Input sprite sheet PNG path")
    parser.add_argument("--grid", default="3x2", choices=["3x2", "2x2"], help="Sprite sheet grid layout (default: 3x2)")
    parser.add_argument("--matte", default="auto", choices=["auto", "white", "transparent"], help="Background matte mode")
    parser.add_argument("--tolerance", type=float, default=28.0, help="Matte color flood-fill tolerance (default: 28)")
    parser.add_argument("--preserve-y", action="store_true", help="Keep artist-authored Y offset instead of snapping baseline")
    parser.add_argument("--root", type=Path, default=Path("."), help="Project root directory")
    parser.add_argument(
        "--dry-run-dir",
        type=Path,
        default=None,
        help="Write outputs into an isolated directory instead of overwriting public/assets/character/piku/",
    )
    args = parser.parse_args()

    root = args.root.resolve()
    sheet_path = args.input if args.input.is_absolute() else (root / args.input)
    if not sheet_path.exists():
        print(f"ERROR: Sheet not found: {sheet_path}")
        return 1

    cols, rows = (3, 2) if args.grid == "3x2" else (2, 2)
    sheet = Image.open(sheet_path).convert("RGBA")
    sw, sh = sheet.size
    if sw % cols != 0 or sh % rows != 0:
        print(f"WARNING: Sheet size {sw}x{sh} is not cleanly divisible by {cols}x{rows}; integer floor crop will be used.")

    tile_w, tile_h = sw // cols, sh // rows
    master_frames: list[Image.Image] = []
    frame_reports: list[dict[str, object]] = []
    flagged_count = 0

    for idx in range(cols * rows):
        frame_num = idx + 1
        r = idx // cols
        c = idx % cols
        tile = sheet.crop((c * tile_w, r * tile_h, (c + 1) * tile_w, (r + 1) * tile_h))
        if tile.size != (MASTER_SIZE, MASTER_SIZE):
            tile = downscale_premultiplied(tile, (MASTER_SIZE, MASTER_SIZE))

        cleaned, flags = remove_matte_and_defringe(tile, matte_mode=args.matte, tolerance=args.tolerance)

        # Check tile boundary bleed before alignment
        border_alpha = (
            np.any(cleaned[0, :, 3] > 16)
            or np.any(cleaned[-1, :, 3] > 16)
            or np.any(cleaned[:, 0, 3] > 16)
            or np.any(cleaned[:, -1, 3] > 16)
        )
        if border_alpha:
            flags.append("tile_boundary_bleed")

        cleaned, comps = prune_dust_speckles(cleaned, min_area=12)
        if not comps or len(comps[0]) < 500:
            flags.append("empty_or_tiny_frame")
            aligned_arr = cleaned
            stats = {"dx": 0, "dy": 0, "bbox_w": 0, "bbox_h": 0, "area": int(len(comps[0])) if comps else 0}
        else:
            lift = AIRBORNE_LIFTS.get((args.state, frame_num), 0)
            target_y = GROUND_Y - lift
            aligned_arr, stats = align_frame_conservatively(
                cleaned,
                comps[0],
                target_x=ANCHOR_X,
                target_bottom_y=target_y,
                preserve_y=args.preserve_y,
            )

            # Safe-margin clip check on final 512x512 master
            alpha_mask = aligned_arr[..., 3] > 0
            if (
                np.any(alpha_mask[:SAFE_MARGIN_MASTER, :])
                or np.any(alpha_mask[-SAFE_MARGIN_MASTER:, :])
                or np.any(alpha_mask[:, :SAFE_MARGIN_MASTER])
                or np.any(alpha_mask[:, -SAFE_MARGIN_MASTER:])
            ):
                flags.append("safe_margin_clip")

        master_frames.append(Image.fromarray(aligned_arr, mode="RGBA"))
        frame_reports.append({"frame": frame_num, "stats": stats, "flags": flags})

    # Conservative scale-drift check across frames (flag > 8% variance vs median, do NOT warp!)
    valid_areas = [r["stats"]["area"] for r in frame_reports if r["stats"]["area"] >= 500]
    if valid_areas:
        median_area = float(np.median(valid_areas))
        for rep in frame_reports:
            area = rep["stats"]["area"]
            if area >= 500:
                rel_diff = abs(area - median_area) / max(1.0, median_area)
                rep["stats"]["area_drift_pct"] = round(rel_diff * 100.0, 2)
                # Note: sqrt(area) roughly tracks linear scale; 16.6% area ~ 8% linear scale
                if rel_diff > 0.17:
                    rep["flags"].append(f"scale_drift_{rep['stats']['area_drift_pct']}%_vs_median")

    # Determine output roots
    if args.dry_run_dir is not None:
        base_out = args.dry_run_dir if args.dry_run_dir.is_absolute() else (root / args.dry_run_dir)
        masters_dir = base_out / "masters-512" / args.state
        runtime_dir = base_out / "runtime-256" / args.state
        qc_dir = base_out / "qc-previews"
    else:
        masters_dir = root / "art-source" / "masters-512" / args.state
        runtime_dir = root / "public" / "assets" / "character" / "piku" / args.state
        qc_dir = root / "art-source" / "qc-previews"

    masters_dir.mkdir(parents=True, exist_ok=True)
    runtime_dir.mkdir(parents=True, exist_ok=True)

    for idx, (master_img, rep) in enumerate(zip(master_frames, frame_reports), start=1):
        fname = f"{args.state}_{idx:02d}.png"
        master_img.save(masters_dir / fname, format="PNG", optimize=True)
        runtime_img = downscale_premultiplied(master_img, (RUNTIME_SIZE, RUNTIME_SIZE))
        runtime_img.save(runtime_dir / fname, format="PNG", optimize=True)
        if rep["flags"]:
            flagged_count += 1

    onionskin_path = qc_dir / f"{args.state}_onionskin.png"
    backdrop_path = qc_dir / f"{args.state}_backdrop_qc.png"
    report_path = qc_dir / f"{args.state}_report.json"
    build_onionskin(master_frames, onionskin_path)
    build_backdrop_qc(master_frames, backdrop_path)
    report_path.write_text(json.dumps({"state": args.state, "frames": frame_reports}, indent=2) + "\n", encoding="utf-8")

    print(f"Processed {len(master_frames)} frames for state '{args.state}' ({args.grid})")
    print(f"  Masters (512x512): {masters_dir}")
    print(f"  Runtime (256x256): {runtime_dir}")
    print(f"  QC previews:       {onionskin_path.name}, {backdrop_path.name}, {report_path.name}")
    if flagged_count:
        print(f"  FLAGGED FRAMES ({flagged_count}): inspect report before promoting to production!")
        for rep in frame_reports:
            if rep["flags"]:
                print(f"    Frame {rep['frame']:02d}: {', '.join(rep['flags'])}")
    else:
        print("  Suspicious frame flags: 0 (clean)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
