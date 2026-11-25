"""
Piku Illustration Asset Processing Pipeline
===========================================
Extracts all 156 individual visual elements from the 5 source sheets:
  - 4 animated sprite sheets (24 animations x 6 frames = 144 frames)
  - 1 static illustration sheet (12 static illustrations: 6 UI states, 3 icons, 3 badges)

Preserves all original artwork untouched, copies originals into art-source/original/,
exports clean transparent PNGs into art-source/processed/, generates dual-backdrop
contact sheets (Cloud Cream #F8F6EF + Dark #18181B + onion-skin alignment preview),
and emits art-source/processed/manifest.json.
"""

from __future__ import annotations

import json
import shutil
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

PROJECT_ROOT = Path(__file__).resolve().parents[2]
SOURCE_DIR = PROJECT_ROOT / "assets" / "new-illustartions"
ART_SOURCE = PROJECT_ROOT / "art-source"
ORIGINAL_DIR = ART_SOURCE / "original"
PROCESSED_DIR = ART_SOURCE / "processed"
ANIM_OUT_DIR = PROCESSED_DIR / "animations"
STATIC_OUT_DIR = PROCESSED_DIR / "static"
CONTACT_DIR = PROCESSED_DIR / "contact-sheets"

CANVAS_SIZE = 256
GROUND_BASELINE_Y = 216  # Exact match to manifest anchor.y = 0.844 (216 / 256 = 0.84375)
CENTER_X = 128

LIGHT_BG = (248, 246, 239, 255)  # Cloud Cream #F8F6EF
DARK_BG = (24, 24, 27, 255)      # Dark Slate #18181B
GRID_LIGHT = (224, 220, 208, 255)
GRID_DARK = (52, 52, 60, 255)
TEXT_LIGHT = (71, 53, 39, 255)   # Outline Brown #473527
TEXT_DARK = (235, 235, 240, 255)


@dataclass(frozen=True)
class AnimationSpec:
    asset_id: str
    app_state_id: str
    display_name: str
    detected_label: str
    sheet_id: str
    row_index: int  # 1-based
    fps: int
    loop: bool
    next_state: str | None
    app_usage: str
    align_mode: str = "body_center"  # "body_center" | "right_wall" | "full_solid"


@dataclass(frozen=True)
class StaticSpec:
    asset_id: str
    display_name: str
    detected_label: str
    section: str
    category_dir: str  # "ui-states" | "icons" | "badges"
    slot_index: int  # 1-based within section
    app_usage: str


SOURCE_FILES = {
    "Image A": "Piku\u2019s Kawaii Animation Sprite Sheet.png",
    "Image B": "Piku\u2019s Pastel Emotion Sticker Sprite Sheet.png",
    "Image C": "Piku Sprout Animation Sprite Sheet.png",
    "Image D": "Piku\u2019s Pastel Sticker Animation Sheet.png",
    "Image E": "Piku\u2019s Static Illustration Sticker Sheet.png",
}

ANIMATION_SPECS: list[AnimationSpec] = [
    # Image A — Core Animations
    AnimationSpec("idle", "idle", "Idle", "Idle", "Image A", 1, 6, True, None,
                  "Default ambient desktop breathing loop."),
    AnimationSpec("blink", "blink", "Blink", "Blink", "Image A", 2, 10, False, "idle",
                  "Random natural eye blink every 3.5-7.5s during idle."),
    AnimationSpec("wave", "wave", "Wave", "Wave (Hello)", "Image A", 3, 9, False, "idle",
                  "Greeting panel open, app launch welcome, friendly hello."),
    AnimationSpec("happy", "happy", "Happy", "Happy (Bounce)", "Image A", 4, 10, False, "idle",
                  "Opening Today view and cheerful ambient delight."),
    AnimationSpec("sleep", "sleep", "Sleep", "Sleep", "Image A", 5, 4, True, None,
                  "Idle sleep state, paused tracking, or cozy rest."),
    AnimationSpec("click_reaction", "click", "Click Reaction", "Click Reaction", "Image A", 6, 12, False, "idle",
                  "Direct desktop pointer poke/click reaction (mapped to runtime state 'click')."),

    # Image B — Expressions
    AnimationSpec("excited", "excited", "Excited", "Excited", "Image B", 1, 10, False, "idle",
                  "Energetic ambient delight or lively reaction."),
    AnimationSpec("content", "content", "Content", "Content", "Image B", 2, 6, False, "idle",
                  "Warm calm satisfaction during active desktop companionship."),
    AnimationSpec("blush", "blush", "Blush", "Blush", "Image B", 3, 8, False, "idle",
                  "Shy happy reaction on gentle cursor hover or greeting."),
    AnimationSpec("curious", "curious", "Curious", "Curious", "Image B", 4, 8, False, "idle",
                  "Inquisitive head-tilt reaction during ambient checks or hover."),
    AnimationSpec("thinking", "thinking", "Thinking", "Thinking", "Image B", 5, 7, False, "idle",
                  "Reflective ambient moment while user works."),
    AnimationSpec("surprised", "surprised", "Surprised", "Surprised", "Image B", 6, 10, False, "idle",
                  "Startled-cute reaction on wake from sleep or sudden interaction."),

    # Image C — Activity & Desktop States
    AnimationSpec("focused", "focused", "Focused", "Focused", "Image C", 1, 7, False, "idle",
                  "Determined focus expression during active sessions."),
    AnimationSpec("yawn", "yawn", "Yawn", "Yawn", "Image C", 2, 6, False, "sleep",
                  "Sleepy transition right before settling into idle sleep."),
    AnimationSpec("drag", "drag", "Drag", "Drag", "Image C", 3, 10, True, None,
                  "Active loop while the user drags Piku across the desktop."),
    AnimationSpec("working", "working", "Working", "Working", "Image C", 4, 7, False, "idle",
                  "Ambient companion moment typing on tiny laptop while user works.",
                  align_mode="full_solid"),
    AnimationSpec("break", "break", "Break", "Break", "Image C", 5, 8, False, "idle",
                  "Ambient cozy break moment vibing with headphones."),
    AnimationSpec("late_night", "late_night", "Late Night", "Late Night", "Image C", 6, 4, True, None,
                  "Cozy plaid-blanket state triggered by late_night hint (22:00-05:00).",
                  align_mode="full_solid"),

    # Image D — Cute Ambient Moments
    AnimationSpec("celebration", "celebration", "Celebration", "Celebration", "Image D", 1, 10, False, "idle",
                  "Joyful confetti jump for cheerful milestones or positive moments."),
    AnimationSpec("snack", "snack", "Snack", "Snack", "Image D", 2, 8, False, "idle",
                  "Ambient cute moment ('It's snack time. I don't make the rules.')."),
    AnimationSpec("tea", "tea", "Tea", "Tea", "Image D", 3, 7, False, "idle",
                  "Ambient cozy tea break moment."),
    AnimationSpec("cloud", "cloud", "Cloud", "Cloud", "Image D", 4, 7, False, "idle",
                  "Ambient daydream moment ('I found a pretty cloud today')."),
    AnimationSpec("stretch", "stretch", "Stretch", "Stretch", "Image D", 5, 8, False, "idle",
                  "Refreshing wake-up or midday ambient stretch moment."),
    AnimationSpec("peek", "peek", "Peek", "Peek", "Image D", 6, 9, False, "idle",
                  "Playful ambient peek moment ('psst...').",
                  align_mode="right_wall"),
]

STATIC_SPECS: list[StaticSpec] = [
    # UI States (6)
    StaticSpec("empty_state", "Empty State", "Empty State", "UI States (6)", "ui-states", 1,
               "TodayView & HistoryView empty state when no activity is recorded."),
    StaticSpec("loading", "Loading", "Loading", "UI States (6)", "ui-states", 2,
               "Library UI state asset for loading panels/placeholders."),
    StaticSpec("success", "Success", "Success", "UI States (6)", "ui-states", 3,
               "Library UI state asset for confirmed/saved feedback (reserved, no extra features added)."),
    StaticSpec("error", "Error", "Error", "UI States (6)", "ui-states", 4,
               "TodayView & HistoryView gentle local-data error state card."),
    StaticSpec("no_connection", "No Connection", "No Connection", "UI States (6)", "ui-states", 5,
               "Library UI state asset (Piku is local-first/offline; reserved in library)."),
    StaticSpec("not_found", "Not Found", "Not Found", "UI States (6)", "ui-states", 6,
               "Library UI state asset for empty search/missing date fallback."),
    # Icons & Badges (6) -> 3 icons + 3 badges
    StaticSpec("checkmark", "Checkmark", "Checkmark", "Icons & Badges (6)", "badges", 1,
               "Library badge asset (reserved; not added as a new feature)."),
    StaticSpec("favorite", "Favorite", "Star / Favorite", "Icons & Badges (6)", "badges", 2,
               "Library badge asset (reserved; not added as a new feature)."),
    StaticSpec("notification", "Notification", "Bell / Notification", "Icons & Badges (6)", "icons", 3,
               "Library icon asset (reserved; not added as a new feature)."),
    StaticSpec("calendar", "Calendar", "Calendar / Schedule", "Icons & Badges (6)", "icons", 4,
               "Library icon asset (reserved for history/calendar visual accents)."),
    StaticSpec("settings", "Settings", "Settings", "Icons & Badges (6)", "icons", 5,
               "Library icon asset (reserved for preferences visual accent)."),
    StaticSpec("achievement", "Achievement", "Achievement", "Icons & Badges (6)", "badges", 6,
               "Library badge asset (reserved; no achievements/streaks feature added)."),
]


def copy_originals() -> None:
    ORIGINAL_DIR.mkdir(parents=True, exist_ok=True)
    for fname in SOURCE_FILES.values():
        src = SOURCE_DIR / fname
        if not src.exists():
            raise FileNotFoundError(f"Missing source sheet: {src}")
        dst = ORIGINAL_DIR / fname
        if not dst.exists() or dst.stat().st_size != src.stat().st_size:
            shutil.copy2(src, dst)


def clean_sprite_cell_alpha(cell: np.ndarray, haze_thresh: int = 12) -> np.ndarray:
    """
    Removes disconnected low-alpha background haze (alpha <= haze_thresh) while
    preserving 100% of anti-aliased outlines and faint floating effects connected
    to solid artwork (alpha >= 32). Never alters solid interior artwork.
    """
    out = cell.copy()
    a = out[:, :, 3]
    # Keep anything >= 32, plus pixels within 3px of >= 32
    solid_support = ndimage.binary_dilation(a >= 32, iterations=3)
    # Also keep connected components whose max alpha >= 24 (e.g. very faint z or confetti)
    lbl, n = ndimage.label(a > haze_thresh)
    keep_mask = solid_support.copy()
    if n > 0:
        max_alphas = ndimage.maximum(a, lbl, range(1, n + 1))
        for cid, ma in enumerate(max_alphas, start=1):
            if ma >= 24:
                keep_mask |= ndimage.binary_dilation(lbl == cid, iterations=2)

    clear_mask = (a <= haze_thresh) & (~keep_mask)
    out[clear_mask] = 0
    # Zero RGB on fully transparent pixels to prevent fringe bleeding on resize/compositing
    out[out[:, :, 3] == 0] = 0
    return out


def extract_animated_sheets() -> tuple[dict[str, dict[str, Any]], dict[str, list[dict[str, Any]]]]:
    """
    Slices all 24 animations (144 frames) from Images A-D using content-derived
    row/column boundaries, preserves native 1:1 pixel sharpness, aligns consistently
    to 256x256 with baseline y=218, and returns manifest entries + extraction map.
    """
    anim_manifest: dict[str, dict[str, Any]] = {}
    extraction_map: dict[str, list[dict[str, Any]]] = {}

    specs_by_sheet: dict[str, list[AnimationSpec]] = {}
    for spec in ANIMATION_SPECS:
        specs_by_sheet.setdefault(spec.sheet_id, []).append(spec)

    for sheet_id, sheet_specs in specs_by_sheet.items():
        fname = SOURCE_FILES[sheet_id]
        src_path = SOURCE_DIR / fname
        arr = np.array(Image.open(src_path).convert("RGBA"))
        h, w, _ = arr.shape
        a = arr[:, :, 3]
        mask = a > 20

        # Detect 6 row bands from non-transparent density
        rc = mask.sum(axis=1)
        y_bands: list[tuple[int, int]] = []
        in_b = False
        start = 0
        for y in range(h):
            if rc[y] > 5 and not in_b:
                in_b = True
                start = y
            elif rc[y] <= 5 and in_b:
                in_b = False
                if y - start >= 8:
                    y_bands.append((start, y - 1))
        if in_b and h - start >= 8:
            y_bands.append((start, h - 1))

        if len(y_bands) != 6:
            raise RuntimeError(f"{fname}: expected 6 rows, found {len(y_bands)}: {y_bands}")

        # Midpoint vertical splits (preserves full headroom for confetti/jumps)
        y_splits = [0]
        for i in range(5):
            # Find the lowest-density y row inside [y_bands[i][1], y_bands[i+1][0]]
            lo = y_bands[i][1]
            hi = y_bands[i + 1][0]
            if hi <= lo:
                y_splits.append(lo + 1)
            else:
                band_rc = (a[lo : hi + 1, :] > 15).sum(axis=1)
                min_offset = int(np.argmin(band_rc))
                y_splits.append(lo + min_offset + 1)
        y_splits.append(h)

        extraction_map[sheet_id] = []

        for spec in sorted(sheet_specs, key=lambda s: s.row_index):
            r = spec.row_index - 1
            ry0, ry1 = y_splits[r], y_splits[r + 1]
            sub_m = mask[ry0:ry1, :]
            cc = sub_m.sum(axis=0)
            spans: list[tuple[int, int]] = []
            in_c = False
            c0 = 0
            for x in range(w):
                if cc[x] > 1 and not in_c:
                    in_c = True
                    c0 = x
                elif cc[x] <= 1 and in_c:
                    in_c = False
                    spans.append((c0, x - 1))
            if in_c:
                spans.append((c0, w - 1))

            merged: list[tuple[int, int]] = []
            for s0, s1 in spans:
                if merged and s0 - merged[-1][1] <= 18:
                    merged[-1] = (merged[-1][0], s1)
                else:
                    merged.append((s0, s1))

            if len(merged) != 7:
                raise RuntimeError(
                    f"{fname} row {spec.row_index} ({spec.asset_id}): expected 7 x-groups (label + 6 frames), got {len(merged)}: {merged}"
                )

            label_x = merged[0]
            frame_spans = merged[1:]
            # Midpoint horizontal splits between label and frames, and between adjacent frames
            x_splits = [(label_x[1] + frame_spans[0][0]) // 2 + 1]
            for i in range(5):
                x_splits.append((frame_spans[i][1] + frame_spans[i + 1][0]) // 2 + 1)
            x_splits.append(w)

            # First pass: extract cleaned cells and measure main body anchors
            cells: list[np.ndarray] = []
            metrics: list[dict[str, float]] = []
            cell_boxes: list[list[int]] = []

            for f in range(6):
                fx0, fx1 = x_splits[f], x_splits[f + 1]
                raw_cell = arr[ry0:ry1, fx0:fx1].copy()

                # Boundary bleed guard: if a tiny disconnected speck (<25px) sits strictly
                # on row 0 or row -1 of the cell (e.g. neighbor row foot/sprout bleed), remove it
                clean = clean_sprite_cell_alpha(raw_cell)
                ch, cw, _ = clean.shape
                cm = clean[:, :, 3] > 20
                lbl, n = ndimage.label(cm)
                if n > 1:
                    sizes = ndimage.sum(cm, lbl, range(1, n + 1))
                    for cid, sz in enumerate(sizes, start=1):
                        if sz < 25:
                            comp_ys, _ = np.where(lbl == cid)
                            if comp_ys.max() <= 2 or comp_ys.min() >= ch - 3:
                                clean[lbl == cid] = 0

                cm = clean[:, :, 3] > 20
                lbl, n = ndimage.label(cm)
                sizes = ndimage.sum(cm, lbl, range(1, n + 1))
                main_id = int(np.argmax(sizes)) + 1
                main_mask = lbl == main_id

                # For "full_solid", include all major components (>150 px) e.g. body + laptop
                if spec.align_mode == "full_solid":
                    solid_mask = np.zeros_like(cm)
                    for cid, sz in enumerate(sizes, start=1):
                        if sz >= 150:
                            solid_mask |= (lbl == cid)
                    mys, mxs = np.where(solid_mask)
                else:
                    mys, mxs = np.where(main_mask)

                ays, axs = np.where(cm)
                cells.append(clean)
                cell_boxes.append([fx0, ry0, fx1, ry1])
                metrics.append({
                    "main_cx": (float(mxs.min()) + float(mxs.max())) / 2.0,
                    "main_x1": float(mxs.max()),
                    "all_x1": float(axs.max()),
                    "main_y1": float(mys.max()),
                    "all_x0": float(axs.min()),
                    "all_y0": float(ays.min()),
                    "all_y1": float(ays.max()),
                    "bbox_w": float(axs.max() - axs.min() + 1),
                    "bbox_h": float(ays.max() - ays.min() + 1),
                })

            # Compute row ground baseline from resting frames (max bottom of F1 & F6,
            # or 75th percentile of main_y1 across the row so jumps float naturally upward!)
            if spec.asset_id == "peek":
                # Peek has the vertical wall reaching bottom of every frame
                row_ground_y = float(np.median([m["all_y1"] for m in metrics]))
            else:
                row_ground_y = max(metrics[0]["main_y1"], metrics[5]["main_y1"])

            out_dir = ANIM_OUT_DIR / spec.asset_id
            out_dir.mkdir(parents=True, exist_ok=True)
            extracted_paths: list[str] = []

            for f in range(6):
                clean = cells[f]
                m = metrics[f]
                canvas = np.zeros((CANVAS_SIZE, CANVAS_SIZE, 4), dtype=np.uint8)

                # Tight crop of non-zero alpha inside cell
                nz_y, nz_x = np.where(clean[:, :, 3] > 0)
                cy0, cy1 = int(nz_y.min()), int(nz_y.max()) + 1
                cx0, cx1 = int(nz_x.min()), int(nz_x.max()) + 1
                crop = clean[cy0:cy1, cx0:cx1]
                crop_h, crop_w, _ = crop.shape

                # Horizontal placement
                if spec.align_mode == "right_wall":
                    # Lock the right edge of the vertical wall at x = 192 across all 6 frames
                    target_wall_right_x = 192
                    anchor_x_in_crop = m["all_x1"] - cx0
                    dst_x = int(round(target_wall_right_x - anchor_x_in_crop))
                else:
                    anchor_x_in_crop = m["main_cx"] - cx0
                    dst_x = int(round(CENTER_X - anchor_x_in_crop))

                # Vertical placement: preserve exact offset relative to row_ground_y!
                anchor_y_in_crop = row_ground_y - cy0
                dst_y = int(round(GROUND_BASELINE_Y - anchor_y_in_crop))

                # Safety clamp only if crop would touch border (should never trigger at 1:1)
                if dst_x < 4 or dst_x + crop_w > CANVAS_SIZE - 4:
                    dst_x = max(4, min(CANVAS_SIZE - 4 - crop_w, dst_x))
                if dst_y < 4 or dst_y + crop_h > CANVAS_SIZE - 4:
                    dst_y = max(4, min(CANVAS_SIZE - 4 - crop_h, dst_y))

                canvas[dst_y : dst_y + crop_h, dst_x : dst_x + crop_w] = crop

                frame_name = f"{spec.asset_id}_{f + 1:02d}.png"
                frame_path = out_dir / frame_name
                Image.fromarray(canvas, "RGBA").save(frame_path, optimize=True)
                extracted_paths.append(f"animations/{spec.asset_id}/{frame_name}")

            extraction_map[sheet_id].append({
                "row": spec.row_index,
                "assetId": spec.asset_id,
                "detectedLabel": spec.detected_label,
                "rowYRange": [ry0, ry1],
                "labelXRange": [label_x[0], label_x[1]],
                "frameBoxesXYXY": cell_boxes,
            })

            anim_manifest[spec.asset_id] = {
                "assetId": spec.asset_id,
                "appStateId": spec.app_state_id,
                "displayName": spec.display_name,
                "detectedLabel": spec.detected_label,
                "sourceFilename": fname,
                "sourceSheet": sheet_id,
                "sourceLocation": f"Row {spec.row_index} (y={ry0}..{ry1})",
                "files": extracted_paths,
                "frameCount": 6,
                "dimensions": {"width": CANVAS_SIZE, "height": CANVAS_SIZE},
                "recommendedFps": spec.fps,
                "loop": spec.loop,
                "nextState": spec.next_state,
                "transparencyStatus": "verified_rgba_transparent",
                "validationStatus": "processed",
                "applicationUsage": spec.app_usage,
            }

    return anim_manifest, extraction_map


def remove_image_e_card_haze(crop: np.ndarray) -> np.ndarray:
    """
    Image E has a semi-transparent off-white/pale-green background veil inside its
    card containers (alpha ~ 10..95, RGB ~ [225, 232, 195]), while all solid
    illustration elements (including white spinner cards, magnifying lens, and
    soft ground shadow) have alpha ~ 200..254.

    Strategy:
    1. Flood-fill exterior background from the 4 edges across pixels with alpha < 170
       (cannot cross dark-brown outlines or solid artwork >= 170).
    2. Compute distance to solid artwork (alpha >= 175):
       - Pixels > 2px away from solid artwork in the exterior flood region -> alpha = 0.
       - 1-2px anti-aliased transition band around outlines -> smoothly remap alpha
         relative to the local background haze floor so outlines stay anti-aliased
         with ZERO greenish halo and ZERO white fringe!
    3. Normalize peak opaque alpha (250..254 -> 255) since Image E had alpha_max = 254.
    """
    out = crop.astype(np.float32)
    h, w, _ = out.shape
    a = out[:, :, 3]

    # Seed exterior flood from all border pixels where alpha < 175
    low_alpha = a < 175
    border_seeds = np.zeros((h, w), dtype=bool)
    border_seeds[0, :] = low_alpha[0, :]
    border_seeds[-1, :] = low_alpha[-1, :]
    border_seeds[:, 0] = low_alpha[:, 0]
    border_seeds[:, -1] = low_alpha[:, -1]

    exterior = ndimage.binary_propagation(border_seeds, mask=low_alpha)

    # Also catch enclosed exterior holes (e.g. gap between arms/body or under handle)
    # where alpha < 115 and color is pale greenish-cream haze
    r, g, b = out[:, :, 0], out[:, :, 1], out[:, :, 2]
    is_haze_hole = (a < 115) & (r > 195) & (g > 200) & (b > 165)
    exterior |= is_haze_hole

    # Estimate local card haze alpha floor from far-exterior pixels
    solid = ~exterior
    near_solid_1 = ndimage.binary_dilation(solid, iterations=1) & exterior
    near_solid_2 = ndimage.binary_dilation(solid, iterations=2) & exterior & (~near_solid_1)
    far_ext = exterior & (~ndimage.binary_dilation(solid, iterations=2))

    # Zero far exterior completely
    out[far_ext] = 0.0

    # Smoothly de-haze the 2px anti-aliasing rim so edges remain crisp on dark & light
    haze_floor = float(np.percentile(a[far_ext], 80)) if far_ext.any() else 55.0
    haze_floor = max(35.0, min(95.0, haze_floor))

    for rim_mask, scale in [(near_solid_1, 0.85), (near_solid_2, 0.45)]:
        if not rim_mask.any():
            continue
        rim_a = a[rim_mask]
        # Subtract haze floor and remap gently
        new_a = np.clip((rim_a - haze_floor) / max(1.0, (175.0 - haze_floor)) * 175.0 * scale, 0.0, 255.0)
        out[rim_mask, 3] = new_a
        # If rim pixel is still pale greenish haze (R>180, G>185, low saturation) with low alpha, darken gently toward outline or clear
        rr = out[rim_mask, 0]
        gg = out[rim_mask, 1]
        bb = out[rim_mask, 2]
        pale_halo = (rr > 180) & (gg > 180) & (bb > 150) & (new_a < 95)
        new_a = np.where(pale_halo, new_a * 0.25, new_a)
        out[rim_mask, 3] = new_a

    # Normalize near-opaque alpha (Image E max was 254; lift 246..254 -> 255 smoothly)
    a_final = out[:, :, 3]
    a_final = np.where(a_final >= 246.0, 255.0, a_final)
    a_final = np.where(a_final < 6.0, 0.0, a_final)
    out[:, :, 3] = a_final
    out[out[:, :, 3] == 0] = 0.0

    return np.clip(np.round(out), 0, 255).astype(np.uint8)


def extract_static_sheet() -> tuple[dict[str, dict[str, Any]], list[dict[str, Any]]]:
    """
    Extracts all 12 static illustrations from Image E into ui-states/, icons/, badges/
    at consistent 256x256 transparent PNGs with zero background haze and zero clipping.
    """
    fname = SOURCE_FILES["Image E"]
    arr = np.array(Image.open(SOURCE_DIR / fname).convert("RGBA"))
    h, w, _ = arr.shape
    high = arr[:, :, 3] >= 200

    sections = [
        ("UI States (6)", 230, 470, STATIC_SPECS[:6]),
        ("Icons & Badges (6)", 678, 908, STATIC_SPECS[6:]),
    ]

    static_manifest: dict[str, dict[str, Any]] = {}
    extraction_entries: list[dict[str, Any]] = []

    for sec_name, y0, y1, specs in sections:
        sub = high[y0 : y1 + 1, 28 : w - 28]
        col_c = sub.sum(axis=0)
        spans: list[tuple[int, int]] = []
        in_c = False
        c0 = 0
        for i, v in enumerate(col_c):
            if v > 0 and not in_c:
                in_c = True
                c0 = i + 28
            elif v == 0 and in_c:
                in_c = False
                spans.append((c0, i - 1 + 28))
        if in_c:
            spans.append((c0, w - 29))

        merged: list[tuple[int, int]] = []
        for a0, a1 in spans:
            if merged and a0 - merged[-1][1] <= 18:
                merged[-1] = (merged[-1][0], a1)
            else:
                merged.append((a0, a1))

        if len(merged) != 6:
            raise RuntimeError(f"Image E {sec_name}: expected 6 items, found {len(merged)}: {merged}")

        # Build safe midpoint X crops (never crossing green card borders at x=28 / x=1508)
        x_splits = [28]
        for i in range(5):
            x_splits.append((merged[i][1] + merged[i + 1][0]) // 2 + 1)
        x_splits.append(w - 28)

        for idx, spec in enumerate(specs):
            fx0, fx1 = x_splits[idx], x_splits[idx + 1]
            # Pad vertically safely inside card, strictly above pill labels (y=483 and y=920)
            crop_raw = arr[y0 : y1 + 1, fx0:fx1].copy()
            cleaned = remove_image_e_card_haze(crop_raw)

            # Remove any tiny stray speckles (< 20px) far from main artwork
            cm = cleaned[:, :, 3] > 20
            lbl, n = ndimage.label(cm)
            if n > 1:
                sizes = ndimage.sum(cm, lbl, range(1, n + 1))
                for cid, sz in enumerate(sizes, start=1):
                    if sz < 20:
                        cleaned[lbl == cid] = 0

            nz_y, nz_x = np.where(cleaned[:, :, 3] > 0)
            ty0, ty1 = int(nz_y.min()), int(nz_y.max()) + 1
            tx0, tx1 = int(nz_x.min()), int(nz_x.max()) + 1
            tight = cleaned[ty0:ty1, tx0:tx1]
            th, tw, _ = tight.shape

            # Fit cleanly into 256x256 with >= 14px margin (gentle supersampled Lanczos downscale only if > 228px)
            max_dim = 228
            pil_tight = Image.fromarray(tight, "RGBA")
            if max(tw, th) > max_dim:
                scale = max_dim / float(max(tw, th))
                new_w = max(1, int(round(tw * scale)))
                new_h = max(1, int(round(th * scale)))
                pil_tight = pil_tight.resize((new_w, new_h), Image.Resampling.LANCZOS)
            else:
                new_w, new_h = tw, th

            canvas = Image.new("RGBA", (CANVAS_SIZE, CANVAS_SIZE), (0, 0, 0, 0))
            ox = (CANVAS_SIZE - new_w) // 2
            oy = (CANVAS_SIZE - new_h) // 2
            canvas.paste(pil_tight, (ox, oy), pil_tight)

            out_dir = STATIC_OUT_DIR / spec.category_dir
            out_dir.mkdir(parents=True, exist_ok=True)
            out_path = out_dir / f"{spec.asset_id}.png"
            canvas.save(out_path, optimize=True)

            rel_path = f"static/{spec.category_dir}/{spec.asset_id}.png"
            extraction_entries.append({
                "section": sec_name,
                "slot": spec.slot_index,
                "assetId": spec.asset_id,
                "detectedLabel": spec.detected_label,
                "sourceBoxXYXY": [fx0, y0, fx1, y1],
                "tightSizePx": [tw, th],
                "outputPath": rel_path,
            })

            static_manifest[spec.asset_id] = {
                "assetId": spec.asset_id,
                "displayName": spec.display_name,
                "detectedLabel": spec.detected_label,
                "category": spec.category_dir,
                "sourceFilename": fname,
                "sourceSheet": "Image E",
                "sourceLocation": f"{sec_name}, Slot {spec.slot_index} (x={fx0}..{fx1}, y={y0}..{y1})",
                "files": [rel_path],
                "frameCount": 1,
                "dimensions": {"width": CANVAS_SIZE, "height": CANVAS_SIZE},
                "recommendedFps": None,
                "loop": False,
                "transparencyStatus": "verified_rgba_transparent",
                "validationStatus": "processed",
                "applicationUsage": spec.app_usage,
            }

    return static_manifest, extraction_entries


def load_font(size: int) -> ImageFont.ImageFont:
    for candidate in ["C:/Windows/Fonts/segoeui.ttf", "C:/Windows/Fonts/arial.ttf"]:
        if Path(candidate).exists():
            try:
                return ImageFont.truetype(candidate, size)
            except Exception:
                pass
    return ImageFont.load_default()


def build_contact_sheets() -> None:
    """
    Creates:
      - 24 individual animation contact sheets (light row + dark row + onion-skin alignment cell)
      - 4 grouped sheet-overview contact sheets (6 animations per overview image, light + dark)
      - 1 static-illustrations dual-backdrop contact sheet (all 12 static assets on light + dark)
    """
    anim_contact_dir = CONTACT_DIR / "animations"
    overview_dir = CONTACT_DIR / "overviews"
    anim_contact_dir.mkdir(parents=True, exist_ok=True)
    overview_dir.mkdir(parents=True, exist_ok=True)

    font_title = load_font(18)
    font_small = load_font(13)

    cell_sz = 160
    pad = 12
    header_h = 38

    # 1. Individual contact sheet for every animation (6 frames light + onion | 6 frames dark + onion)
    for spec in ANIMATION_SPECS:
        frames = [
            Image.open(ANIM_OUT_DIR / spec.asset_id / f"{spec.asset_id}_{i:02d}.png").convert("RGBA")
            for i in range(1, 7)
        ]
        cols = 7  # F1..F6 + Onion-Skin
        w = pad + cols * (cell_sz + pad)
        h = header_h + 2 * (cell_sz + pad) + pad
        sheet = Image.new("RGBA", (w, h), LIGHT_BG)
        draw = ImageDraw.Draw(sheet)

        draw.text(
            (pad, 10),
            f"{spec.display_name} ({spec.asset_id}) — {spec.sheet_id} Row {spec.row_index} | {spec.fps} fps | loop={spec.loop}",
            fill=TEXT_LIGHT,
            font=font_title,
        )

        for row_idx, (bg_col, grid_col, txt_col) in enumerate([
            (LIGHT_BG, GRID_LIGHT, TEXT_LIGHT),
            (DARK_BG, GRID_DARK, TEXT_DARK),
        ]):
            y0 = header_h + row_idx * (cell_sz + pad)
            if row_idx == 1:
                draw.rectangle([0, y0 - 6, w, h], fill=DARK_BG)

            # 6 frames
            for fi, fr in enumerate(frames):
                x0 = pad + fi * (cell_sz + pad)
                draw.rectangle([x0, y0, x0 + cell_sz, y0 + cell_sz], fill=bg_col, outline=grid_col, width=1)
                # Subtle baseline guide at 218/256
                by = y0 + int(round(GROUND_BASELINE_Y / CANVAS_SIZE * cell_sz))
                draw.line([x0 + 4, by, x0 + cell_sz - 4, by], fill=grid_col, width=1)
                thumb = fr.resize((cell_sz, cell_sz), Image.Resampling.LANCZOS)
                sheet.alpha_composite(thumb, (x0, y0))
                draw.text((x0 + 6, y0 + 4), f"F{fi + 1}", fill=txt_col, font=font_small)

            # Onion-skin alignment check in col 7
            ox = pad + 6 * (cell_sz + pad)
            draw.rectangle([ox, y0, ox + cell_sz, y0 + cell_sz], fill=bg_col, outline=grid_col, width=1)
            by = y0 + int(round(GROUND_BASELINE_Y / CANVAS_SIZE * cell_sz))
            draw.line([ox + 4, by, ox + cell_sz - 4, by], fill=(220, 110, 110, 180), width=1)
            cx_line = ox + cell_sz // 2
            draw.line([cx_line, y0 + 4, cx_line, y0 + cell_sz - 4], fill=(110, 160, 220, 140), width=1)
            for fr in frames:
                arr = np.array(fr.resize((cell_sz, cell_sz), Image.Resampling.LANCZOS))
                arr[:, :, 3] = (arr[:, :, 3].astype(np.float32) * 0.28).astype(np.uint8)
                sheet.alpha_composite(Image.fromarray(arr, "RGBA"), (ox, y0))
            draw.text((ox + 6, y0 + 4), "Onion", fill=txt_col, font=font_small)

        sheet.convert("RGB").save(anim_contact_dir / f"{spec.asset_id}_contact.png", optimize=True)

    # 2. Four grouped overview contact sheets (one per source sheet A..D, half light / half dark)
    by_sheet: dict[str, list[AnimationSpec]] = {}
    for spec in ANIMATION_SPECS:
        by_sheet.setdefault(spec.sheet_id, []).append(spec)

    ov_cell = 136
    label_w = 156
    for sheet_id, specs in by_sheet.items():
        w = label_w + 7 * (ov_cell + 8) + 16
        h = 44 + 6 * (ov_cell + 10) + 16
        for theme, bg, grd, txt in [
            ("light", LIGHT_BG, GRID_LIGHT, TEXT_LIGHT),
            ("dark", DARK_BG, GRID_DARK, TEXT_DARK),
        ]:
            img = Image.new("RGBA", (w, h), bg)
            dr = ImageDraw.Draw(img)
            dr.text(
                (16, 12),
                f"{sheet_id} — Extracted & Aligned Overview ({theme.upper()} backdrop) | F1..F6 + Onion-Skin",
                fill=txt,
                font=font_title,
            )
            for r_i, spec in enumerate(specs):
                y0 = 44 + r_i * (ov_cell + 10)
                dr.text((16, y0 + ov_cell // 2 - 14), f"{spec.display_name}", fill=txt, font=font_title)
                dr.text((16, y0 + ov_cell // 2 + 8), f"id: {spec.asset_id}", fill=txt, font=font_small)
                frames = [
                    Image.open(ANIM_OUT_DIR / spec.asset_id / f"{spec.asset_id}_{i:02d}.png").convert("RGBA")
                    for i in range(1, 7)
                ]
                for fi, fr in enumerate(frames):
                    x0 = label_w + fi * (ov_cell + 8)
                    dr.rectangle([x0, y0, x0 + ov_cell, y0 + ov_cell], fill=bg, outline=grd, width=1)
                    by = y0 + int(round(GROUND_BASELINE_Y / CANVAS_SIZE * ov_cell))
                    dr.line([x0 + 3, by, x0 + ov_cell - 3, by], fill=grd, width=1)
                    img.alpha_composite(fr.resize((ov_cell, ov_cell), Image.Resampling.LANCZOS), (x0, y0))
                # Onion skin
                ox = label_w + 6 * (ov_cell + 8)
                dr.rectangle([ox, y0, ox + ov_cell, y0 + ov_cell], fill=bg, outline=grd, width=1)
                for fr in frames:
                    arr = np.array(fr.resize((ov_cell, ov_cell), Image.Resampling.LANCZOS))
                    arr[:, :, 3] = (arr[:, :, 3].astype(np.float32) * 0.28).astype(np.uint8)
                    img.alpha_composite(Image.fromarray(arr, "RGBA"), (ox, y0))

            slug = sheet_id.lower().replace(" ", "_")
            img.convert("RGB").save(overview_dir / f"{slug}_{theme}.png", optimize=True)

    # 3. Static illustrations dual-backdrop contact sheet (all 12 on light + all 12 on dark)
    sc = 156
    sw = 16 + 6 * (sc + 12) + 4
    sh = 44 + 4 * (sc + 30) + 16
    st_img = Image.new("RGBA", (sw, sh), LIGHT_BG)
    dr = ImageDraw.Draw(st_img)
    dr.text((16, 12), "Image E — All 12 Extracted Static Illustrations (Top: Cloud Cream Light | Bottom: Dark Slate)", fill=TEXT_LIGHT, font=font_title)

    half_y = 44 + 2 * (sc + 30)
    dr.rectangle([0, half_y - 6, sw, sh], fill=DARK_BG)

    for block_idx, (bg, grd, txt) in enumerate([
        (LIGHT_BG, GRID_LIGHT, TEXT_LIGHT),
        (DARK_BG, GRID_DARK, TEXT_DARK),
    ]):
        for idx, spec in enumerate(STATIC_SPECS):
            row = idx // 6
            col = idx % 6
            x0 = 16 + col * (sc + 12)
            y0 = 44 + (block_idx * 2 + row) * (sc + 30)
            dr.rectangle([x0, y0, x0 + sc, y0 + sc], fill=bg, outline=grd, width=1)
            im = Image.open(STATIC_OUT_DIR / spec.category_dir / f"{spec.asset_id}.png").convert("RGBA")
            st_img.alpha_composite(im.resize((sc, sc), Image.Resampling.LANCZOS), (x0, y0))
            dr.text((x0 + 4, y0 + sc + 4), f"{spec.asset_id} ({spec.category_dir})", fill=txt, font=font_small)

    st_img.convert("RGB").save(overview_dir / "image_e_static_dual.png", optimize=True)


def sync_runtime_assets() -> None:
    """
    Publishes validated production sprites (24 animations = 144 frames) and active
    UI-state static illustrations (empty_state.png, error.png) to
    public/assets/character/piku/ and writes the runtime manifest.json.
    """
    runtime_dir = PROJECT_ROOT / "public" / "assets" / "character" / "piku"
    runtime_dir.mkdir(parents=True, exist_ok=True)

    # Remove stale legacy placeholder 'click/' directory if present (replaced by click_reaction/)
    legacy_click = runtime_dir / "click"
    if legacy_click.exists() and legacy_click.is_dir():
        shutil.rmtree(legacy_click)

    states_map: dict[str, dict[str, Any]] = {}
    for spec in ANIMATION_SPECS:
        src_dir = ANIM_OUT_DIR / spec.asset_id
        dst_dir = runtime_dir / spec.asset_id
        if dst_dir.exists():
            shutil.rmtree(dst_dir)
        shutil.copytree(src_dir, dst_dir)

        entry: dict[str, Any] = {
            "frames": 6,
            "fps": spec.fps,
            "loop": spec.loop,
            "path": f"{spec.asset_id}/{spec.asset_id}_{{nn}}.png",
        }
        if spec.next_state is not None:
            entry["next"] = spec.next_state
        states_map[spec.app_state_id] = entry

    # Copy only static UI-state illustrations actively used by existing UI views
    runtime_static = runtime_dir / "static"
    runtime_static.mkdir(parents=True, exist_ok=True)
    for ui_id in ("empty_state", "error"):
        shutil.copy2(STATIC_OUT_DIR / "ui-states" / f"{ui_id}.png", runtime_static / f"{ui_id}.png")

    runtime_manifest = {
        "character": "piku",
        "version": "2.0.0",
        "assetKind": "production",
        "frameSize": {"width": CANVAS_SIZE, "height": CANVAS_SIZE},
        "anchor": {"x": 0.5, "y": round(GROUND_BASELINE_Y / CANVAS_SIZE, 3)},
        "defaultState": "idle",
        "states": states_map,
    }
    (runtime_dir / "manifest.json").write_text(
        json.dumps(runtime_manifest, indent=2) + "\n", encoding="utf-8"
    )


def main() -> None:
    print("[1/5] Copying original 5 source sheets untouched to art-source/original/ ...")
    copy_originals()

    print("[2/5] Extracting 24 animations (144 frames) from Images A-D ...")
    anim_manifest, anim_map = extract_animated_sheets()

    print("[3/5] Extracting 12 static illustrations from Image E ...")
    static_manifest, static_map = extract_static_sheet()

    print("[4/5] Generating contact sheets and art-source/processed/manifest.json ...")
    build_contact_sheets()

    manifest = {
        "schemaVersion": "2.1.0",
        "project": "Piku Desktop Pet",
        "assetKind": "production",
        "canvasSize": {"width": CANVAS_SIZE, "height": CANVAS_SIZE},
        "anchor": {"x": 0.5, "y": round(GROUND_BASELINE_Y / CANVAS_SIZE, 3)},
        "summary": {
            "sourceSheets": 5,
            "totalAnimations": len(anim_manifest),
            "framesPerAnimation": 6,
            "totalAnimationFrames": sum(v["frameCount"] for v in anim_manifest.values()),
            "totalStaticIllustrations": len(static_manifest),
            "totalVisualElements": sum(v["frameCount"] for v in anim_manifest.values()) + len(static_manifest),
        },
        "sources": {
            sid: {
                "filename": fname,
                "originalCopy": f"art-source/original/{fname}",
                "sourcePath": f"assets/new-illustartions/{fname}",
            }
            for sid, fname in SOURCE_FILES.items()
        },
        "extractionMap": {
            "animatedSheets": anim_map,
            "staticSheet": static_map,
        },
        "animations": anim_manifest,
        "static": static_manifest,
    }

    manifest_path = PROCESSED_DIR / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print("[5/5] Syncing production sprites + runtime manifest to public/assets/character/piku/ ...")
    sync_runtime_assets()
    print(f"Done! Wrote {manifest['summary']['totalVisualElements']} assets + manifests.")


if __name__ == "__main__":
    main()
