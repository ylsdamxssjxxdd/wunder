#!/usr/bin/env python3
"""Regenerate the 舵机 (servo) icon set from the same gear the terminal banner draws.

The CLI paints its welcome banner as braille from analytic tubes (see
crates/wunder-cli/empty_state_animation/paths.rs). This script keeps the icon and that
banner the same object: one table of path data feeds both the SVG writer and the
rasteriser, whose ring/capsule distance and light vector mirror geometry.rs and
lighting.rs. Change the gear in one place, regenerate, and both stay in step.

Outputs into images/:
  servo-gear.svg   vector master, transparent
  servo-gear.png   256 px, transparent
  servo-gear.ico   16..256, seated on a navy plate for small-size legibility
"""

from __future__ import annotations

import math
import os
from dataclasses import dataclass

import numpy as np
from PIL import Image, ImageDraw

# --- path data, mirrored from empty_state_animation/paths.rs --------------------

TOOTH_COUNT = 12
RIM_RADIUS = 0.700
RIM_TUBE = 0.085
TOOTH_INNER = 0.700
TOOTH_OUTER = 0.950
TOOTH_TUBE = 0.062
TOOTH_TIP_TUBE = 0.050
HOOP_RADIUS = 0.430
HOOP_TUBE = 0.072

LETTER_HALF_WIDTH = 0.290
LETTER_HALF_HEIGHT = 0.200
LETTER_TUBE = 0.050

# Model half-width the icon canvas covers; a tooth tip reaches 1.000.
GEAR_SPAN = 1.05

# Light and material, mirrored from lighting.rs and the banner's Rust-orange default.
LIGHT = np.array([-0.40, -0.56, 0.73], dtype=np.float64)
LIGHT /= np.linalg.norm(LIGHT)
AMBIENT = 0.13
SPECULAR_POWER = 7.0
SPECULAR_GAIN = 0.34

ORANGE_DARK = np.array([92, 26, 2], dtype=np.float64)
ORANGE = np.array([247, 76, 0], dtype=np.float64)
ORANGE_LIGHT = np.array([255, 202, 150], dtype=np.float64)
OUTLINE = np.array([46, 14, 2], dtype=np.float64)
PLATE_TOP = np.array([22, 32, 56], dtype=np.float64)
PLATE_BOTTOM = np.array([8, 12, 22], dtype=np.float64)


@dataclass(frozen=True)
class Ring:
    radius: float
    tube: float


@dataclass(frozen=True)
class Bar:
    a: tuple[float, float]
    b: tuple[float, float]
    tube: float


def polar(radius: float, angle: float) -> tuple[float, float]:
    return radius * math.cos(angle), radius * math.sin(angle)


def letter_w() -> list[Bar]:
    """The hub W as four strokes, matching paths.rs::letter_w."""
    w, h = LETTER_HALF_WIDTH, LETTER_HALF_HEIGHT
    crest_left = (-w, -h)
    valley_left = (-w * 0.5, h)
    peak_mid = (0.0, -h * 0.30)
    valley_right = (w * 0.5, h)
    crest_right = (w, -h)
    points = [crest_left, valley_left, peak_mid, valley_right, crest_right]
    return [
        Bar(points[i], points[i + 1], LETTER_TUBE) for i in range(len(points) - 1)
    ]


def gear() -> list[Ring | Bar]:
    prims: list[Ring | Bar] = [
        Ring(RIM_RADIUS, RIM_TUBE),
        Ring(HOOP_RADIUS, HOOP_TUBE),
    ]
    for index in range(TOOTH_COUNT):
        direction = index * 2.0 * math.pi / TOOTH_COUNT
        prims.append(
            Bar(
                polar(TOOTH_INNER, direction),
                polar(TOOTH_OUTER, direction),
                TOOTH_TUBE,
            )
        )
        prims.append(Bar(polar(TOOTH_OUTER, direction), polar(TOOTH_OUTER, direction), TOOTH_TIP_TUBE))
    prims.extend(letter_w())
    return prims


# --- surface field, mirrored from geometry.rs ------------------------------------


def frontmost(points: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Return (coverage, normal_xy, depth) for the union of tubes at `points`."""
    best_depth = np.zeros(points.shape[:2], dtype=np.float64)
    # Worse than any point inside a tube, so the first covering prim always wins.
    best_ratio = np.full(points.shape[:2], 2.0, dtype=np.float64)
    best_normal = np.zeros_like(points)

    for prim in gear():
        if isinstance(prim, Ring):
            length = np.hypot(points[..., 0], points[..., 1])
            radial = np.where(
                length[..., None] > 1e-6,
                points / np.maximum(length, 1e-6)[..., None],
                np.array([1.0, 0.0]),
            )
            distance = np.abs(length - prim.radius)
            # Outer flank faces away from the centre, inner flank toward it.
            radial = radial * np.sign(length - prim.radius)[..., None]
        else:
            a = np.array(prim.a, dtype=np.float64)
            b = np.array(prim.b, dtype=np.float64)
            ab = b - a
            span = float(ab @ ab)
            if span <= 1e-12:
                along = np.zeros(points.shape[:2], dtype=np.float64)
            else:
                along = np.clip(((points - a) @ ab) / span, 0.0, 1.0)
            offset = points - (a + along[..., None] * ab)
            distance = np.hypot(offset[..., 0], offset[..., 1])
            radial = np.where(
                distance[..., None] > 1e-5,
                offset / np.maximum(distance, 1e-5)[..., None],
                np.array([0.0, -1.0]),
            )

        ratio = distance / prim.tube
        takes = (ratio < best_ratio + 1e-9) & (ratio < 1.0)
        best_ratio = np.where(takes, ratio, best_ratio)
        best_normal = np.where(takes[..., None], radial * ratio[..., None], best_normal)
        depth = np.sqrt(np.clip(1.0 - np.clip(ratio, 0.0, 1.0) ** 2, 0.0, 1.0))
        best_depth = np.where(takes, depth, best_depth)

    coverage = np.clip(1.0 - smoothstep(0.62, 1.0, np.minimum(best_ratio, 1.0)), 0.0, 1.0)
    return coverage, best_normal, best_depth


def smoothstep(edge0: float, edge1: float, value: np.ndarray) -> np.ndarray:
    t = np.clip((value - edge0) / (edge1 - edge0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def shade(points: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    coverage, normal_xy, depth = frontmost(points)
    normals = np.dstack([normal_xy, depth[..., None] * 0.86])
    normals /= np.maximum(np.linalg.norm(normals, axis=-1, keepdims=True), 1e-9)

    diffuse = np.clip(normals @ LIGHT, 0.0, 1.0)
    specular = diffuse**SPECULAR_POWER * SPECULAR_GAIN
    intensity = np.clip(AMBIENT + (1.0 - AMBIENT) * diffuse + specular, 0.0, 1.0)

    low = intensity < 0.56
    mix = np.where(low, intensity / 0.56, (intensity - 0.56) / 0.44)
    from_a = np.where(low[..., None], ORANGE_DARK, ORANGE)
    to_a = np.where(low[..., None], ORANGE, ORANGE_LIGHT)
    rgb = from_a + (to_a - from_a) * mix[..., None]
    rgb = rgb + ORANGE_LIGHT * specular[..., None] * np.array([1.0, 0.86, 0.68])

    # A dark rim keeps the silhouette readable on a light taskbar.
    rim = 1.0 - smoothstep(0.0, 0.32, coverage)
    rgb = rgb + (OUTLINE - rgb) * rim[..., None] * 0.75
    return np.clip(rgb, 0.0, 255.0), coverage


def render(size: int, supersample: int = 3) -> Image.Image:
    samples = size * supersample
    axes = (np.arange(samples) + 0.5) / samples * 2.0 - 1.0
    grid_x, grid_y = np.meshgrid(axes, axes, indexing="xy")
    points = np.dstack([grid_x * GEAR_SPAN, grid_y * GEAR_SPAN])

    rgb, coverage = shade(points)
    alpha = np.clip(coverage * 255.0, 0.0, 255.0)
    image = Image.fromarray(np.dstack([rgb, alpha]).astype(np.uint8), "RGBA")
    if supersample > 1:
        image = image.resize((size, size), Image.LANCZOS)
    return image


def with_plate(gear_image: Image.Image) -> Image.Image:
    """Seat the gear on a navy rounded square.

    The gear is line art, so on a light taskbar at 16 px it nearly vanishes; the plate is
    what gives the exe icon a silhouette. The PNG and SVG stay transparent masters.
    """
    size = gear_image.size[0]
    radius = max(2, size // 5)
    mask = Image.new("L", gear_image.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, size - 1, size - 1], radius=radius, fill=255)

    ramp = np.linspace(0.0, 1.0, size, dtype=np.float64)[:, None]
    rows = (PLATE_TOP + (PLATE_BOTTOM - PLATE_TOP) * ramp).astype(np.uint8)
    plate = Image.fromarray(np.repeat(rows[:, None, :], size, axis=1), "RGB").convert("RGBA")
    plate.putalpha(mask)
    plate.alpha_composite(gear_image)

    ImageDraw.Draw(plate).rounded_rectangle(
        [1, 1, size - 2, size - 2],
        radius=max(1, radius - 1),
        outline=(247, 130, 40, 200),
        width=max(1, size // 64),
    )
    return plate


# --- SVG master ------------------------------------------------------------------


def svg_document() -> str:
    parts = [
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="-110 -110 220 220" '
        'width="220" height="220" role="img">',
        "  <title>wunder 舵机</title>",
        f'  <g fill="none" stroke="#f74c00" stroke-linecap="round">',
        f'    <circle r="{RIM_RADIUS * 100:.4g}" stroke-width="{RIM_TUBE * 200:.4g}"/>',
        f'    <circle r="{HOOP_RADIUS * 100:.4g}" stroke-width="{HOOP_TUBE * 200:.4g}"/>',
    ]
    for index in range(TOOTH_COUNT):
        direction = index * 2.0 * math.pi / TOOTH_COUNT
        x1, y1 = polar(TOOTH_INNER, direction)
        x2, y2 = polar(TOOTH_OUTER, direction)
        parts.append(
            f'    <line x1="{x1 * 100:.4g}" y1="{y1 * 100:.4g}" '
            f'x2="{x2 * 100:.4g}" y2="{y2 * 100:.4g}" '
            f'stroke-width="{TOOTH_TUBE * 200:.4g}"/>'
        )
        parts.append(
            f'    <circle cx="{x2 * 100:.4g}" cy="{y2 * 100:.4g}" '
            f'r="{TOOTH_TIP_TUBE * 100:.4g}" fill="#f74c00" stroke="none"/>'
        )
    points = " ".join(
        f"{x * 100:.4g},{y * 100:.4g}" for x, y in [p for b in [
            [(-LETTER_HALF_WIDTH, -LETTER_HALF_HEIGHT),
             (-LETTER_HALF_WIDTH * 0.5, LETTER_HALF_HEIGHT),
             (0.0, -LETTER_HALF_HEIGHT * 0.30),
             (LETTER_HALF_WIDTH * 0.5, LETTER_HALF_HEIGHT),
             (LETTER_HALF_WIDTH, -LETTER_HALF_HEIGHT)]
        ] for p in b]
    )
    parts.append(
        f'    <polyline points="{points}" '
        f'stroke-width="{LETTER_TUBE * 200:.4g}"/>'
    )
    parts.append("  </g>")
    parts.append("</svg>")
    return "\n".join(parts) + "\n"


def main() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    images = os.path.join(os.path.dirname(here), "images")
    os.makedirs(images, exist_ok=True)

    with open(os.path.join(images, "servo-gear.svg"), "w", encoding="utf-8") as handle:
        handle.write(svg_document())

    master = render(256)
    master.save(os.path.join(images, "servo-gear.png"))
    with_plate(master).save(
        os.path.join(images, "servo-gear.ico"),
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print(f"wrote servo-gear.svg, .png and .ico into {images}")


if __name__ == "__main__":
    main()
