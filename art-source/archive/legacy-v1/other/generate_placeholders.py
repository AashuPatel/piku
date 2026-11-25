#!/usr/bin/env python3
"""
Deterministic Placeholder Sprite & Icon Generator for Piku Desktop Pet.

Generates:
  - All 34 MVP animation frames (256x256 RGBA transparent PNGs) across the 6 MVP states:
      idle (6), blink (4), sleep (6), wave (6), happy (6), click (6)
  - public/assets/character/piku/manifest.json (single source of truth)
  - src-tauri/icons/{tray.png, 32x32.png, 128x128.png, 128x128@2x.png, icon.ico}
  - Optional 512x512 master frames & QC preview contact sheets (--emit-qc)

Usage (from project root):
  python scripts/assets/generate_placeholders.py
  python scripts/assets/generate_placeholders.py --emit-qc
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Iterable

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

# ---------------------------------------------------------------------------
# Approved Piku Palette (from assets/piku-character-sheet.png & design-system.md)
# ---------------------------------------------------------------------------
OUTLINE_RGB = (71, 53, 39)        # #473527 warm dark brown
OUTLINE_RGBA = (71, 53, 39, 255)
GRAD_TOP = np.array([227.0, 243.0, 150.0], dtype=np.float32)   # warm pastel yellow-lime
GRAD_MID = np.array([174.0, 232.0, 200.0], dtype=np.float32)   # #AEE8C8 soft mint
GRAD_BOT = np.array([153.0, 220.0, 241.0], dtype=np.float32)   # pastel aqua/sky
HIGHLIGHT_RGBA = (246, 248, 226, 92)                           # soft top-left cream glow
LEAF_FILL_RGBA = (206, 236, 114, 255)                          # fresh sprout yellow-green
LEAF_TIP_RGBA = (174, 232, 168, 255)
MOUTH_CORAL_RGBA = (233, 148, 130, 255)                        # #E99482 soft coral-peach
SPARK_YELLOW_RGBA = (242, 201, 76, 255)                        # #F2C94C warm golden spark

# Master 512x512 coordinate specification (divide by 2 for 256x256 runtime)
MASTER_SIZE = 512
RUNTIME_SIZE = 256
ANCHOR_X = 256.0
GROUND_Y = 432.0

MANIFEST_SPEC = {
    "character": "piku",
    "version": "1.0.0",
    "assetKind": "placeholder",
    "frameSize": {"width": RUNTIME_SIZE, "height": RUNTIME_SIZE},
    "anchor": {"x": 0.5, "y": round(GROUND_Y / MASTER_SIZE, 3)},
    "defaultState": "idle",
    "states": {
        "idle": {"frames": 6, "fps": 6, "loop": True, "path": "idle/idle_{nn}.png"},
        "blink": {"frames": 4, "fps": 10, "loop": False, "path": "blink/blink_{nn}.png", "next": "idle"},
        "sleep": {"frames": 6, "fps": 4, "loop": True, "path": "sleep/sleep_{nn}.png"},
        "wave": {"frames": 6, "fps": 9, "loop": False, "path": "wave/wave_{nn}.png", "next": "idle"},
        "happy": {"frames": 6, "fps": 10, "loop": False, "path": "happy/happy_{nn}.png", "next": "idle"},
        "click": {"frames": 6, "fps": 12, "loop": False, "path": "click/click_{nn}.png", "next": "idle"},
    },
}


# ---------------------------------------------------------------------------
# Premultiplied-alpha Lanczos downscale (prevents dark/white halo ringing)
# ---------------------------------------------------------------------------
def downscale_premultiplied(img: Image.Image, target_size: tuple[int, int]) -> Image.Image:
    arr = np.asarray(img.convert("RGBA"), dtype=np.float32)
    alpha = arr[..., 3:4] / 255.0
    premul = np.empty_like(arr)
    premul[..., :3] = arr[..., :3] * alpha
    premul[..., 3:4] = arr[..., 3:4]

    premul_img = Image.fromarray(np.clip(np.round(premul), 0, 255).astype(np.uint8), mode="RGBA")
    resized = premul_img.resize(target_size, Image.Resampling.LANCZOS)

    out = np.asarray(resized, dtype=np.float32)
    a_out = out[..., 3:4]
    safe_a = np.where(a_out > 0.5, a_out / 255.0, 1.0)
    rgb = np.where(a_out > 0.5, out[..., :3] / safe_a, 0.0)

    final = np.empty_like(out)
    final[..., :3] = np.clip(np.round(rgb), 0, 255)
    final[..., 3:4] = np.where(a_out > 0.5, np.clip(np.round(a_out), 0, 255), 0.0)
    return Image.fromarray(final.astype(np.uint8), mode="RGBA")


def rotate_point(x: float, y: float, cx: float, cy: float, deg: float) -> tuple[float, float]:
    rad = math.radians(deg)
    dx, dy = x - cx, y - cy
    cos_a, sin_a = math.cos(rad), math.sin(rad)
    return cx + dx * cos_a - dy * sin_a, cy + dx * sin_a + dy * cos_a


def draw_thick_polyline(
    draw: ImageDraw.ImageDraw,
    pts: list[tuple[float, float]],
    color: tuple[int, int, int, int],
    width: float,
) -> None:
    if len(pts) < 2:
        return
    r = width / 2.0
    w_int = max(1, int(round(width)))
    for i in range(len(pts) - 1):
        draw.line([pts[i], pts[i + 1]], fill=color, width=w_int)
    for px, py in pts:
        draw.ellipse((px - r, py - r, px + r, py + r), fill=color)


def cubic_bezier(
    p0: tuple[float, float],
    p1: tuple[float, float],
    p2: tuple[float, float],
    p3: tuple[float, float],
    steps: int = 18,
) -> list[tuple[float, float]]:
    pts = []
    for i in range(steps + 1):
        t = i / steps
        mt = 1.0 - t
        x = mt**3 * p0[0] + 3 * mt**2 * t * p1[0] + 3 * mt * t**2 * p2[0] + t**3 * p3[0]
        y = mt**3 * p0[1] + 3 * mt**2 * t * p1[1] + 3 * mt * t**2 * p2[1] + t**3 * p3[1]
        pts.append((x, y))
    return pts


def mochi_body_points(
    cx: float,
    cy: float,
    rx: float,
    ry: float,
    lean_deg: float = 0.0,
    samples: int = 144,
) -> list[tuple[float, float]]:
    """Generate Piku's slightly imperfect organic rounded mochi silhouette."""
    pts: list[tuple[float, float]] = []
    p = 2.12  # gentle mochi softness (not a rigid circle)
    for i in range(samples):
        theta = (2.0 * math.pi * i) / samples
        ct = math.cos(theta)
        st = math.sin(theta)
        # Slightly fuller lower cheeks & gently tapered crown
        width_mod = 1.0 + 0.048 * st - 0.014 * (ct * ct)
        x = math.copysign(abs(ct) ** (2.0 / p), ct) * rx * width_mod
        y = math.copysign(abs(st) ** (2.0 / p), st) * ry
        px, py = cx + x, cy + y
        if lean_deg != 0.0:
            px, py = rotate_point(px, py, cx, cy + ry * 0.85, lean_deg)
        pts.append((px, py))
    return pts


def puddle_body_points(
    cx: float,
    base_y: float,
    half_w: float,
    height: float,
    samples: int = 160,
) -> list[tuple[float, float]]:
    """Generate Piku's cozy rounded melted mochi-puddle mound resting flat along base_y."""
    pts: list[tuple[float, float]] = []
    # Plump, rounded dome with soft mochi shoulders (matches character sheet sleep puddle)
    top_n = samples // 2
    for i in range(top_n + 1):
        u = -1.0 + 2.0 * (i / top_n)  # -1..1
        x = cx + u * half_w
        # Smooth superellipse dome (full rounded sides, never triangular!)
        dome = max(0.0, 1.0 - (abs(u) ** 2.6)) ** 0.44
        # Gentle soft crown bump near center-left
        crest = 0.08 * math.exp(-(((u + 0.08) / 0.28) ** 2)) * (1.0 - u * u)
        y = base_y - height * (dome + crest)
        pts.append((x, y))
    # Flat cozy underside resting smoothly along base_y
    bot_n = samples // 4
    for i in range(1, bot_n):
        u = 1.0 - 2.0 * (i / bot_n)
        x = cx + u * (half_w * 0.96)
        edge_lift = 3.5 * (u**8)
        y = base_y - edge_lift
        pts.append((x, y))
    return pts


def fill_vertical_gradient_polygon(
    canvas: Image.Image,
    pts: list[tuple[float, float]],
    outline_width: float = 6.0,
    add_highlight: bool = True,
) -> None:
    """Fill polygon with Piku's lime -> mint -> aqua vertical gradient and warm brown outline."""
    w, h = canvas.size
    mask = Image.new("L", (w, h), 0)
    mdraw = ImageDraw.Draw(mask)
    mdraw.polygon(pts, fill=255)

    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    min_y, max_y = min(ys), max(ys)
    span_y = max(1.0, max_y - min_y)

    y_coords = np.arange(h, dtype=np.float32)[:, None]
    t = np.clip((y_coords - min_y) / span_y, 0.0, 1.0)

    # Piecewise smooth gradient: 0.0..0.52 (lime->mint), 0.52..1.0 (mint->aqua)
    mid_split = 0.52
    t1 = np.clip(t / mid_split, 0.0, 1.0)[..., None]
    t2 = np.clip((t - mid_split) / (1.0 - mid_split), 0.0, 1.0)[..., None]
    upper = GRAD_TOP * (1.0 - t1) + GRAD_MID * t1
    lower = GRAD_MID * (1.0 - t2) + GRAD_BOT * t2
    grad_col = np.where(t[..., None] <= mid_split, upper, lower)

    grad_rgb = np.broadcast_to(grad_col, (h, w, 3)).astype(np.uint8)
    mask_arr = np.asarray(mask, dtype=np.uint8)
    rgba = np.dstack([grad_rgb, mask_arr])
    body_layer = Image.fromarray(rgba, mode="RGBA")

    if add_highlight:
        # Soft creamy top-left inner glow clipped to the body mask
        hl = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        hdraw = ImageDraw.Draw(hl)
        min_x, max_x = min(xs), max(xs)
        bw, bh = max_x - min_x, max_y - min_y
        hdraw.ellipse(
            (
                min_x + bw * 0.14,
                min_y + bh * 0.09,
                min_x + bw * 0.54,
                min_y + bh * 0.42,
            ),
            fill=HIGHLIGHT_RGBA,
        )
        hl = hl.filter(ImageFilter.GaussianBlur(radius=10.0))
        hl_arr = np.asarray(hl, dtype=np.uint8).copy()
        hl_arr[..., 3] = ((hl_arr[..., 3].astype(np.uint16) * mask_arr.astype(np.uint16)) // 255).astype(np.uint8)
        body_layer = Image.alpha_composite(body_layer, Image.fromarray(hl_arr, mode="RGBA"))

    # Outline with crisp smooth rounded stroke
    odraw = ImageDraw.Draw(body_layer)
    closed = pts + [pts[0]]
    draw_thick_polyline(odraw, closed, OUTLINE_RGBA, outline_width)

    canvas.alpha_composite(body_layer)


def leaf_polygon(
    base_x: float,
    base_y: float,
    length: float,
    half_width: float,
    angle_deg: float,
    samples: int = 36,
) -> list[tuple[float, float]]:
    """Generate a smooth pointed oval sprout leaf anchored at (base_x, base_y)."""
    pts: list[tuple[float, float]] = []
    for i in range(samples):
        u = i / samples  # 0..1 along perimeter
        if u <= 0.5:
            s = u * 2.0  # 0..1 from base to tip
            local_x = s * length
            local_y = -math.sin(s * math.pi) ** 0.85 * half_width
        else:
            s = (1.0 - u) * 2.0  # 1..0 from tip to base
            local_x = s * length
            local_y = math.sin(s * math.pi) ** 0.85 * half_width
        px, py = rotate_point(base_x + local_x, base_y + local_y, base_x, base_y, angle_deg)
        pts.append((px, py))
    return pts


def draw_sprout(
    canvas: Image.Image,
    root_x: float,
    root_y: float,
    stem_len: float = 26.0,
    tilt_deg: float = 0.0,
    spread_delta_deg: float = 0.0,
    scale: float = 1.0,
    outline_width: float = 5.5,
) -> None:
    """Draw Piku's signature stem + two small pointed oval green sprout leaves."""
    draw = ImageDraw.Draw(canvas)
    top_x, top_y = rotate_point(root_x, root_y - stem_len * scale, root_x, root_y, tilt_deg)
    mid_x, mid_y = rotate_point(root_x + 1.5 * scale, root_y - stem_len * 0.5 * scale, root_x, root_y, tilt_deg)

    # Stem
    stem_pts = cubic_bezier(
        (root_x, root_y + 2.0),
        (mid_x, mid_y),
        (top_x, top_y + 4.0),
        (top_x, top_y),
        steps=10,
    )
    draw_thick_polyline(draw, stem_pts, OUTLINE_RGBA, outline_width * 0.95)

    # Left and right pointed oval leaves
    leaf_len = 46.0 * scale
    leaf_hw = 17.0 * scale
    left_angle = -152.0 - spread_delta_deg + tilt_deg
    right_angle = -28.0 + spread_delta_deg + tilt_deg

    for angle, fill_col in ((left_angle, LEAF_FILL_RGBA), (right_angle, LEAF_TIP_RGBA)):
        lpts = leaf_polygon(top_x, top_y, leaf_len, leaf_hw, angle)
        draw.polygon(lpts, fill=fill_col)
        draw_thick_polyline(draw, lpts + [lpts[0]], OUTLINE_RGBA, outline_width * 0.88)


def draw_feet(
    canvas: Image.Image,
    cx: float,
    body_bottom_y: float,
    foot_y: float,
    stance_half_w: float = 34.0,
    foot_out: float = 13.0,
    splay: float = 0.0,
    dangle: bool = False,
    outline_width: float = 6.0,
) -> None:
    """Draw Piku's two short dark brown curved stick legs with small rounded feet."""
    draw = ImageDraw.Draw(canvas)
    stroke_r = outline_width / 2.0
    # Account for stroke radius so the lowest opaque pixel lands cleanly at foot_y
    target_y = foot_y - stroke_r

    for side in (-1.0, 1.0):
        hip_x = cx + side * (stance_half_w + splay * 0.4)
        hip_y = body_bottom_y - 6.0
        ankle_x = cx + side * (stance_half_w + splay)
        ankle_y = target_y - (4.0 if dangle else 0.0)
        toe_x = ankle_x + side * (foot_out + (2.0 if dangle else 0.0))
        toe_y = target_y + (2.0 if dangle else 0.0)

        leg_pts = cubic_bezier(
            (hip_x, hip_y),
            (hip_x + side * 2.0, (hip_y + ankle_y) * 0.55),
            (ankle_x, ankle_y),
            (toe_x, toe_y),
            steps=14,
        )
        draw_thick_polyline(draw, leg_pts, OUTLINE_RGBA, outline_width)


def draw_arm(
    canvas: Image.Image,
    shoulder_x: float,
    shoulder_y: float,
    hand_x: float,
    hand_y: float,
    ctrl_x: float,
    ctrl_y: float,
    outline_width: float = 6.0,
) -> None:
    draw = ImageDraw.Draw(canvas)
    pts = cubic_bezier(
        (shoulder_x, shoulder_y),
        (ctrl_x, ctrl_y),
        ((ctrl_x + hand_x) * 0.5, (ctrl_y + hand_y) * 0.5),
        (hand_x, hand_y),
        steps=14,
    )
    draw_thick_polyline(draw, pts, OUTLINE_RGBA, outline_width)


def draw_face(
    canvas: Image.Image,
    fx: float,
    fy: float,
    eye_mode: str = "open",
    mouth_mode: str = "smile",
    lean_deg: float = 0.0,
    eye_spacing: float = 40.0,
    outline_width: float = 5.0,
) -> None:
    """Draw Piku's minimal warm dark brown facial features."""
    draw = ImageDraw.Draw(canvas)

    def pt(dx: float, dy: float) -> tuple[float, float]:
        x, y = fx + dx, fy + dy
        if lean_deg != 0.0:
            return rotate_point(x, y, fx, fy + 80.0, lean_deg)
        return x, y

    # Eyes
    for side in (-1.0, 1.0):
        ex, ey = pt(side * eye_spacing, 0.0)
        if eye_mode == "open":
            rx, ry = 6.5, 7.2
            draw.ellipse((ex - rx, ey - ry, ex + rx, ey + ry), fill=OUTLINE_RGBA)
        elif eye_mode == "soft":
            rx, ry = 6.8, 5.2
            draw.ellipse((ex - rx, ey - ry, ex + rx, ey + ry), fill=OUTLINE_RGBA)
        elif eye_mode == "half":
            pts = [
                (ex - 9.0, ey + 0.5),
                (ex, ey + 2.5),
                (ex + 9.0, ey + 0.5),
            ]
            draw_thick_polyline(draw, pts, OUTLINE_RGBA, outline_width * 0.85)
        elif eye_mode == "closed":
            pts = cubic_bezier(
                (ex - 10.0, ey - 1.5),
                (ex - 4.0, ey + 5.5),
                (ex + 4.0, ey + 5.5),
                (ex + 10.0, ey - 1.5),
                steps=10,
            )
            draw_thick_polyline(draw, pts, OUTLINE_RGBA, outline_width * 0.85)
        elif eye_mode == "squint":
            # Cute > < squeezed reaction eyes
            if side < 0:
                pts = [(ex - 8.5, ey - 6.5), (ex + 5.5, ey), (ex - 8.5, ey + 6.5)]
            else:
                pts = [(ex + 8.5, ey - 6.5), (ex - 5.5, ey), (ex + 8.5, ey + 6.5)]
            draw_thick_polyline(draw, pts, OUTLINE_RGBA, outline_width * 0.9)

    # Mouth
    mx, my = pt(0.0, 6.0)
    if mouth_mode == "smile":
        mpts = cubic_bezier(
            (mx - 12.0, my - 1.0),
            (mx - 5.0, my + 8.0),
            (mx + 5.0, my + 8.0),
            (mx + 12.0, my - 1.0),
            steps=10,
        )
        draw_thick_polyline(draw, mpts, OUTLINE_RGBA, outline_width * 0.82)
    elif mouth_mode == "happy_open":
        # Tiny D-shaped happy grin with soft coral-peach interior
        d_pts = cubic_bezier(
            (mx - 13.0, my - 2.0),
            (mx - 9.0, my + 16.0),
            (mx + 9.0, my + 16.0),
            (mx + 13.0, my - 2.0),
            steps=12,
        )
        draw.polygon(d_pts, fill=MOUTH_CORAL_RGBA)
        draw_thick_polyline(draw, d_pts + [d_pts[0]], OUTLINE_RGBA, outline_width * 0.78)
    elif mouth_mode == "tiny_o":
        draw.ellipse((mx - 6.5, my - 4.0, mx + 6.5, my + 8.5), fill=MOUTH_CORAL_RGBA)
        o_pts = [
            (mx + 6.5 * math.cos(2 * math.pi * i / 20), my + 2.25 + 6.25 * math.sin(2 * math.pi * i / 20))
            for i in range(21)
        ]
        draw_thick_polyline(draw, o_pts, OUTLINE_RGBA, outline_width * 0.72)
    elif mouth_mode == "sleep_smile":
        mpts = cubic_bezier(
            (mx - 9.0, my - 1.0),
            (mx - 3.5, my + 5.0),
            (mx + 3.5, my + 5.0),
            (mx + 9.0, my - 1.0),
            steps=8,
        )
        draw_thick_polyline(draw, mpts, OUTLINE_RGBA, outline_width * 0.78)


def draw_sparks(
    canvas: Image.Image,
    origin_x: float,
    origin_y: float,
    angles_deg: Iterable[float],
    inner_r: float = 12.0,
    outer_r: float = 34.0,
    width: float = 7.0,
    alpha: int = 255,
) -> None:
    """Draw warm golden-yellow motion/joy sparks (#F2C94C)."""
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    col = (SPARK_YELLOW_RGBA[0], SPARK_YELLOW_RGBA[1], SPARK_YELLOW_RGBA[2], alpha)
    for deg in angles_deg:
        rad = math.radians(deg)
        p0 = (origin_x + math.cos(rad) * inner_r, origin_y + math.sin(rad) * inner_r)
        p1 = (origin_x + math.cos(rad) * outer_r, origin_y + math.sin(rad) * outer_r)
        draw_thick_polyline(draw, [p0, p1], col, width)
    canvas.alpha_composite(layer)


def draw_z_glyph(
    canvas: Image.Image,
    cx: float,
    cy: float,
    size: float,
    alpha: int,
    width: float = 4.6,
) -> None:
    """Draw a clean hand-drawn warm dark brown Z letter for sleep frames."""
    if alpha <= 0:
        return
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    hs = size * 0.5
    col = (OUTLINE_RGB[0], OUTLINE_RGB[1], OUTLINE_RGB[2], alpha)
    pts = [
        (cx - hs, cy - hs),
        (cx + hs, cy - hs),
        (cx - hs, cy + hs),
        (cx + hs, cy + hs),
    ]
    draw_thick_polyline(draw, pts, col, width)
    canvas.alpha_composite(layer)


# ---------------------------------------------------------------------------
# Per-State Frame Renderers (512x512 masters, exact spec from Section 3)
# ---------------------------------------------------------------------------
def render_upright_frame(
    *,
    body_w: float = 224.0,
    body_h: float = 196.0,
    leg_h: float = 32.0,
    lift_y: float = 0.0,
    lean_deg: float = 0.0,
    sprout_tilt_deg: float = 0.0,
    sprout_spread_deg: float = 0.0,
    sprout_scale: float = 1.0,
    eye_mode: str = "open",
    mouth_mode: str = "smile",
    foot_splay: float = 0.0,
    dangle_feet: bool = False,
    left_arm: tuple[float, float, float, float] | None = None,
    right_arm: tuple[float, float, float, float] | None = None,
    sparks: list[tuple[float, float, list[float], float, float, int]] | None = None,
) -> Image.Image:
    canvas = Image.new("RGBA", (MASTER_SIZE, MASTER_SIZE), (0, 0, 0, 0))
    foot_y = GROUND_Y - lift_y
    body_bottom_y = foot_y - leg_h
    body_cy = body_bottom_y - body_h * 0.5
    rx, ry = body_w * 0.5, body_h * 0.5

    # 1. Feet behind body
    draw_feet(
        canvas,
        cx=ANCHOR_X,
        body_bottom_y=body_bottom_y,
        foot_y=foot_y,
        splay=foot_splay,
        dangle=dangle_feet,
    )

    # 2. Optional arms (tucked/hidden at rest; emerge in wave, happy, click spring)
    if left_arm is not None:
        hx, hy, cx_ctrl, cy_ctrl = left_arm
        sx, sy = rotate_point(ANCHOR_X - rx * 0.90, body_cy + 10.0, ANCHOR_X, body_bottom_y, lean_deg)
        draw_arm(canvas, sx, sy, hx, hy, cx_ctrl, cy_ctrl)
    if right_arm is not None:
        hx, hy, cx_ctrl, cy_ctrl = right_arm
        sx, sy = rotate_point(ANCHOR_X + rx * 0.90, body_cy + 10.0, ANCHOR_X, body_bottom_y, lean_deg)
        draw_arm(canvas, sx, sy, hx, hy, cx_ctrl, cy_ctrl)

    # 3. Organic mochi body
    pts = mochi_body_points(ANCHOR_X, body_cy, rx, ry, lean_deg=lean_deg)
    fill_vertical_gradient_polygon(canvas, pts, outline_width=6.0, add_highlight=True)

    # 4. Signature two-leaf sprout anchored to crown
    crown_x, crown_y = rotate_point(ANCHOR_X, body_cy - ry + 2.0, ANCHOR_X, body_bottom_y, lean_deg)
    draw_sprout(
        canvas,
        root_x=crown_x,
        root_y=crown_y,
        tilt_deg=lean_deg + sprout_tilt_deg,
        spread_delta_deg=sprout_spread_deg,
        scale=sprout_scale,
    )

    # 5. Minimal face
    face_x, face_y = rotate_point(ANCHOR_X, body_cy - ry * 0.05, ANCHOR_X, body_bottom_y, lean_deg)
    draw_face(
        canvas,
        fx=face_x,
        fy=face_y,
        eye_mode=eye_mode,
        mouth_mode=mouth_mode,
        lean_deg=lean_deg,
    )

    # 6. Optional golden sparks
    if sparks:
        for sx, sy, angles, ir, or_, alpha in sparks:
            draw_sparks(canvas, sx, sy, angles, inner_r=ir, outer_r=or_, alpha=alpha)

    return canvas


def build_idle_frames() -> list[Image.Image]:
    # 6-frame seamless breathing loop (feet locked on y=432)
    configs = [
        (224.0, 196.0, 0.0),   # 01 neutral
        (222.0, 200.0, 3.0),   # 02 early inhale
        (218.0, 204.0, 6.0),   # 03 inhale peak
        (222.0, 199.0, 2.5),   # 04 early exhale
        (230.0, 192.0, -3.0),  # 05 exhale settle
        (226.0, 195.0, -1.0),  # 06 recovery to neutral
    ]
    return [
        render_upright_frame(body_w=bw, body_h=bh, sprout_spread_deg=sp)
        for bw, bh, sp in configs
    ]


def build_blink_frames() -> list[Image.Image]:
    # 4-frame blink: body, sprout, feet pixel-identical to idle_01
    return [
        render_upright_frame(eye_mode="soft"),
        render_upright_frame(eye_mode="half"),
        render_upright_frame(eye_mode="closed"),
        render_upright_frame(eye_mode="soft"),
    ]


def build_sleep_frames() -> list[Image.Image]:
    # 6-frame seamless melted mochi-puddle breathing loop + drifting z Z
    configs = [
        # (half_w, height, sprout_tilt, z_list: (x, y, size, alpha))
        (138.0, 130.0, -16.0, [(358.0, 294.0, 14.0, 180)]),
        (136.0, 136.0, -13.0, [(376.0, 270.0, 18.0, 235)]),
        (133.0, 142.0, -10.0, [(396.0, 242.0, 23.0, 255), (350.0, 298.0, 13.0, 130)]),
        (135.0, 138.0, -12.0, [(412.0, 218.0, 25.0, 165), (368.0, 274.0, 17.0, 220)]),
        (138.0, 132.0, -15.0, [(426.0, 198.0, 26.0, 65), (386.0, 248.0, 21.0, 210)]),
        (141.0, 128.0, -18.0, [(402.0, 228.0, 22.0, 110)]),
    ]
    frames: list[Image.Image] = []
    for half_w, height, stilt, zs in configs:
        canvas = Image.new("RGBA", (MASTER_SIZE, MASTER_SIZE), (0, 0, 0, 0))
        # Align outer stroke bottom exactly on GROUND_Y = 432
        pts = puddle_body_points(ANCHOR_X, GROUND_Y - 3.0, half_w, height)
        fill_vertical_gradient_polygon(canvas, pts, outline_width=6.0, add_highlight=True)
        # Sprout resting gently tilted on the top crest
        draw_sprout(
            canvas,
            root_x=ANCHOR_X - 12.0,
            root_y=GROUND_Y - 3.0 - height * 1.07,
            stem_len=20.0,
            tilt_deg=stilt,
            spread_delta_deg=-6.0,
            scale=0.92,
        )
        # Peaceful sleeping face
        draw_face(
            canvas,
            fx=ANCHOR_X - 4.0,
            fy=GROUND_Y - height * 0.44,
            eye_mode="closed",
            mouth_mode="sleep_smile",
            eye_spacing=36.0,
        )
        for zx, zy, zsize, zalpha in zs:
            draw_z_glyph(canvas, zx, zy, zsize, zalpha)
        frames.append(canvas)
    return frames


def build_wave_frames() -> list[Image.Image]:
    # 6-frame friendly greeting wave (grounded on y=432, viewer-right arm waving)
    frames = [
        # 01 anticipation lean, arm emerging
        render_upright_frame(
            body_w=224.0, body_h=196.0, lean_deg=-3.0, sprout_tilt_deg=-2.0,
            right_arm=(384.0, 320.0, 372.0, 326.0),
        ),
        # 02 arm raises up
        render_upright_frame(
            body_w=222.0, body_h=198.0, lean_deg=-4.5, sprout_tilt_deg=-4.0, sprout_spread_deg=3.0,
            right_arm=(404.0, 256.0, 392.0, 292.0),
        ),
        # 03 wave peak 1 (outward) + golden sparks
        render_upright_frame(
            body_w=220.0, body_h=200.0, lean_deg=-5.0, sprout_tilt_deg=-5.0, sprout_spread_deg=5.0,
            right_arm=(416.0, 234.0, 404.0, 276.0),
            sparks=[(426.0, 218.0, [-55.0, -25.0, 5.0], 10.0, 28.0, 235)],
        ),
        # 04 wave inward sway
        render_upright_frame(
            body_w=222.0, body_h=198.0, lean_deg=1.5, sprout_tilt_deg=3.0, sprout_spread_deg=2.0,
            right_arm=(386.0, 232.0, 392.0, 278.0),
        ),
        # 05 wave peak 2 (enthusiastic) + 3 crisp sparks + joyful open smile
        render_upright_frame(
            body_w=220.0, body_h=201.0, lean_deg=-4.5, sprout_tilt_deg=-4.0, sprout_spread_deg=6.0,
            mouth_mode="happy_open",
            right_arm=(420.0, 230.0, 406.0, 274.0),
            sparks=[(430.0, 214.0, [-60.0, -30.0, 2.0], 10.0, 32.0, 255)],
        ),
        # 06 arm lowers & tucks back toward neutral
        render_upright_frame(
            body_w=224.0, body_h=196.0, lean_deg=-1.0, sprout_tilt_deg=-1.0,
            right_arm=(382.0, 322.0, 372.0, 326.0),
        ),
    ]
    return frames


def build_happy_frames() -> list[Image.Image]:
    # 6-frame happy bounce (grounded 01/05/06, airborne 02/03/04 with +28/+72/+36 px lift)
    frames = [
        # 01 anticipation crouch
        render_upright_frame(
            body_w=242.0, body_h=178.0, leg_h=28.0, lift_y=0.0,
            sprout_spread_deg=-4.0, foot_splay=6.0,
        ),
        # 02 upward takeoff (+28 px lift)
        render_upright_frame(
            body_w=214.0, body_h=206.0, leg_h=34.0, lift_y=28.0,
            sprout_spread_deg=-5.0, dangle_feet=True,
            left_arm=(118.0, 240.0, 134.0, 278.0),
            right_arm=(394.0, 240.0, 378.0, 278.0),
        ),
        # 03 airborne apex (+72 px lift, joyful V arms, open smile, golden sparks)
        render_upright_frame(
            body_w=218.0, body_h=200.0, leg_h=34.0, lift_y=72.0,
            sprout_spread_deg=8.0, mouth_mode="happy_open", dangle_feet=True,
            left_arm=(110.0, 178.0, 130.0, 224.0),
            right_arm=(402.0, 178.0, 382.0, 224.0),
            sparks=[
                (382.0, 126.0, [-55.0, -25.0], 10.0, 30.0, 255),
                (130.0, 126.0, [-125.0, -155.0], 10.0, 30.0, 255),
            ],
        ),
        # 04 descent (+36 px lift)
        render_upright_frame(
            body_w=218.0, body_h=202.0, leg_h=34.0, lift_y=36.0,
            sprout_spread_deg=5.0, dangle_feet=True,
            left_arm=(118.0, 218.0, 134.0, 258.0),
            right_arm=(394.0, 218.0, 378.0, 258.0),
            sparks=[(386.0, 166.0, [-50.0, -20.0], 10.0, 22.0, 140)],
        ),
        # 05 landing squash (grounded on y=432)
        render_upright_frame(
            body_w=238.0, body_h=182.0, leg_h=28.0, lift_y=0.0,
            sprout_spread_deg=-3.0, foot_splay=5.0,
            left_arm=(128.0, 338.0, 134.0, 322.0),
            right_arm=(384.0, 338.0, 378.0, 322.0),
        ),
        # 06 elastic settle (grounded on y=432)
        render_upright_frame(
            body_w=222.0, body_h=198.0, leg_h=32.0, lift_y=0.0,
            sprout_spread_deg=1.0,
        ),
    ]
    return frames


def build_click_frames() -> list[Image.Image]:
    # 6-frame click reaction: poke -> surprise squish -> max mochi compression -> spring pop -> land -> settle
    frames = [
        # 01 initial poke contact
        render_upright_frame(
            body_w=234.0, body_h=186.0, leg_h=30.0, lift_y=0.0,
            sprout_tilt_deg=-4.0, foot_splay=3.0,
        ),
        # 02 surprise squish (> < eyes + golden pop lines)
        render_upright_frame(
            body_w=252.0, body_h=168.0, leg_h=26.0, lift_y=0.0,
            sprout_tilt_deg=-10.0, sprout_spread_deg=-5.0,
            eye_mode="squint", mouth_mode="smile", foot_splay=8.0,
            sparks=[(390.0, 224.0, [-60.0, -30.0, 0.0], 10.0, 28.0, 240)],
        ),
        # 03 max mochi compression (> < eyes + tiny o mouth + splayed feet)
        render_upright_frame(
            body_w=268.0, body_h=154.0, leg_h=24.0, lift_y=0.0,
            sprout_tilt_deg=-14.0, sprout_spread_deg=-10.0, sprout_scale=0.92,
            eye_mode="squint", mouth_mode="tiny_o", foot_splay=14.0,
            sparks=[(402.0, 240.0, [-50.0, -20.0], 10.0, 20.0, 130)],
        ),
        # 04 elastic spring-up pop (+48 px lift, bright eyes, arms out, crisp sparks)
        render_upright_frame(
            body_w=212.0, body_h=208.0, leg_h=34.0, lift_y=48.0,
            sprout_spread_deg=8.0, mouth_mode="happy_open", dangle_feet=True,
            left_arm=(118.0, 218.0, 134.0, 256.0),
            right_arm=(394.0, 218.0, 378.0, 256.0),
            sparks=[(386.0, 148.0, [-65.0, -35.0, -5.0], 10.0, 32.0, 255)],
        ),
        # 05 soft rebound landing (grounded)
        render_upright_frame(
            body_w=232.0, body_h=188.0, leg_h=30.0, lift_y=0.0,
            sprout_spread_deg=-2.0, foot_splay=4.0,
        ),
        # 06 calm settle (grounded)
        render_upright_frame(
            body_w=222.0, body_h=198.0, leg_h=32.0, lift_y=0.0,
            sprout_spread_deg=1.0,
        ),
    ]
    return frames


# ---------------------------------------------------------------------------
# Icon Generator (High-Readability Piku Head + Sprout for Tray & App Icons)
# ---------------------------------------------------------------------------
def render_piku_icon_master(size: int = 256) -> Image.Image:
    """Crisp close-up Piku head + sprout filling ~84% of icon box for 16x16/32x32 legibility."""
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    scale = size / 256.0

    # Body centered slightly lower to give generous space for the signature two-leaf sprout
    pts = mochi_body_points(128.0 * scale, 152.0 * scale, 96.0 * scale, 84.0 * scale)
    fill_vertical_gradient_polygon(canvas, pts, outline_width=13.0 * scale, add_highlight=True)

    # Bold readable sprout
    draw_sprout(
        canvas,
        root_x=128.0 * scale,
        root_y=68.0 * scale,
        stem_len=24.0 * scale,
        scale=1.05 * scale,
        outline_width=11.0 * scale,
    )

    # Crisp dark-brown face scaled up slightly for small taskbar clarity
    draw = ImageDraw.Draw(canvas)
    for side in (-1.0, 1.0):
        ex = (128.0 + side * 34.0) * scale
        ey = 148.0 * scale
        r = 9.0 * scale
        draw.ellipse((ex - r, ey - r, ex + r, ey + r), fill=OUTLINE_RGBA)
    mpts = cubic_bezier(
        (114.0 * scale, 156.0 * scale),
        (122.0 * scale, 168.0 * scale),
        (134.0 * scale, 168.0 * scale),
        (142.0 * scale, 156.0 * scale),
        steps=10,
    )
    draw_thick_polyline(draw, mpts, OUTLINE_RGBA, 8.0 * scale)
    return canvas


def write_icons(project_root: Path) -> list[Path]:
    icons_dir = project_root / "src-tauri" / "icons"
    icons_dir.mkdir(parents=True, exist_ok=True)
    master = render_piku_icon_master(256)

    outputs: list[Path] = []
    targets = [
        ("tray.png", (32, 32)),
        ("32x32.png", (32, 32)),
        ("128x128.png", (128, 128)),
        ("128x128@2x.png", (256, 256)),
    ]
    for name, dims in targets:
        out_path = icons_dir / name
        img = master if dims == (256, 256) else downscale_premultiplied(master, dims)
        img.save(out_path, format="PNG", optimize=True)
        outputs.append(out_path)

    ico_path = icons_dir / "icon.ico"
    master.save(
        ico_path,
        format="ICO",
        sizes=[(16, 16), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    outputs.append(ico_path)
    return outputs


def make_qc_contact_sheet(
    state_frames: dict[str, list[Image.Image]],
    out_path: Path,
) -> None:
    """Create a dark+cream dual-backdrop visual contact sheet for human/agent QA."""
    cell = 128
    pad = 12
    cols = max(len(v) for v in state_frames.values())
    rows = len(state_frames)
    w = cols * (cell + pad) + pad
    row_h = cell + pad
    half_h = rows * row_h + pad

    sheet = Image.new("RGBA", (w, half_h * 2), (248, 246, 239, 255))
    draw = ImageDraw.Draw(sheet)
    # Top half: Piku Cloud Cream (#F8F6EF); Bottom half: Dark Windows wallpaper (#18181B)
    draw.rectangle((0, 0, w, half_h), fill=(248, 246, 239, 255))
    draw.rectangle((0, half_h, w, half_h * 2), fill=(24, 24, 27, 255))

    for r_idx, (_, frames) in enumerate(state_frames.items()):
        for c_idx, frame in enumerate(frames):
            thumb = downscale_premultiplied(frame, (cell, cell))
            x = pad + c_idx * (cell + pad)
            y_light = pad + r_idx * row_h
            y_dark = half_h + y_light
            sheet.alpha_composite(thumb, (x, y_light))
            sheet.alpha_composite(thumb, (x, y_dark))

    out_path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(out_path, format="PNG", optimize=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate deterministic Piku placeholder sprites & icons.")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path("."),
        help="Project root directory (default: current working directory)",
    )
    parser.add_argument(
        "--emit-qc",
        action="store_true",
        help="Also emit dual-backdrop QC contact sheet under art-source/qc-previews/",
    )
    args = parser.parse_args()
    root = args.root.resolve()

    char_dir = root / "public" / "assets" / "character" / "piku"
    char_dir.mkdir(parents=True, exist_ok=True)

    builders = {
        "idle": build_idle_frames,
        "blink": build_blink_frames,
        "sleep": build_sleep_frames,
        "wave": build_wave_frames,
        "happy": build_happy_frames,
        "click": build_click_frames,
    }

    runtime_frames_by_state: dict[str, list[Image.Image]] = {}
    total_frames = 0

    for state, builder in builders.items():
        masters = builder()
        expected = MANIFEST_SPEC["states"][state]["frames"]
        if len(masters) != expected:
            raise RuntimeError(f"State '{state}' generated {len(masters)} frames, expected {expected}")

        state_dir = char_dir / state
        state_dir.mkdir(parents=True, exist_ok=True)
        runtime_list: list[Image.Image] = []

        for idx, master_img in enumerate(masters, start=1):
            runtime_img = downscale_premultiplied(master_img, (RUNTIME_SIZE, RUNTIME_SIZE))
            out_path = state_dir / f"{state}_{idx:02d}.png"
            runtime_img.save(out_path, format="PNG", optimize=True)
            runtime_list.append(runtime_img)
            total_frames += 1

        runtime_frames_by_state[state] = runtime_list

    manifest_path = char_dir / "manifest.json"
    manifest_path.write_text(json.dumps(MANIFEST_SPEC, indent=2) + "\n", encoding="utf-8")

    icon_paths = write_icons(root)

    if args.emit_qc:
        qc_path = root / "art-source" / "qc-previews" / "placeholders_backdrop_qc.png"
        make_qc_contact_sheet(runtime_frames_by_state, qc_path)
        print(f"QC contact sheet: {qc_path.relative_to(root)}")

    print(f"Generated {total_frames} placeholder frames (256x256 RGBA) under {char_dir.relative_to(root)}")
    print(f"Manifest written: {manifest_path.relative_to(root)}")
    print(f"Icons written ({len(icon_paths)}): {', '.join(p.name for p in icon_paths)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
