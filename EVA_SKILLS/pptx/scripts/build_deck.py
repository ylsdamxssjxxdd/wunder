#!/usr/bin/env python3
"""Build a PPTX deck from a template and a YAML/JSON outline."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Dict, Iterable, List, Tuple

import yaml
from pptx import Presentation
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_SHAPE, PP_PLACEHOLDER
from pptx.util import Inches


def load_outline(path: Path) -> Dict[str, Any]:
    raw = path.read_text(encoding="utf-8")
    if path.suffix.lower() in {".yaml", ".yml"}:
        return yaml.safe_load(raw)
    if path.suffix.lower() == ".json":
        return json.loads(raw)
    try:
        return yaml.safe_load(raw)
    except Exception:
        return json.loads(raw)


def resolve_path(value: str, base_dir: Path) -> Path:
    candidate = Path(value)
    if candidate.is_absolute():
        return candidate
    return (base_dir / candidate).resolve()


def emu_to_inches(value: int) -> float:
    return value / 914400.0


def parse_color(value: Any) -> RGBColor | None:
    if not value:
        return None
    if not isinstance(value, str):
        return None
    hex_value = value.strip().lstrip("#")
    if len(hex_value) != 6:
        return None
    try:
        red = int(hex_value[0:2], 16)
        green = int(hex_value[2:4], 16)
        blue = int(hex_value[4:6], 16)
    except ValueError:
        return None
    return RGBColor(red, green, blue)


def to_float(value: Any, default: float) -> float:
    try:
        return float(value)
    except (TypeError, ValueError):
        return default


def send_shape_to_back(slide, shape) -> None:
    shape_element = shape._element
    shape_element.getparent().remove(shape_element)
    slide.shapes._spTree.insert(2, shape_element)


def add_background_rect(
    slide,
    left_in: float,
    top_in: float,
    width_in: float,
    height_in: float,
    color: RGBColor,
) -> None:
    if width_in <= 0 or height_in <= 0:
        return
    shape = slide.shapes.add_shape(
        MSO_SHAPE.RECTANGLE,
        Inches(left_in),
        Inches(top_in),
        Inches(width_in),
        Inches(height_in),
    )
    shape.fill.solid()
    shape.fill.fore_color.rgb = color
    shape.line.fill.background()
    send_shape_to_back(slide, shape)


def apply_theme(slide, prs: Presentation, theme: Dict[str, Any], is_title: bool) -> None:
    if not theme:
        return
    background = parse_color(theme.get("background"))
    if background:
        fill = slide.background.fill
        fill.solid()
        fill.fore_color.rgb = background

    accent = parse_color(theme.get("accent"))
    if not accent:
        return
    accent_light = parse_color(theme.get("accent_light")) or accent

    slide_width_in = emu_to_inches(prs.slide_width)
    slide_height_in = emu_to_inches(prs.slide_height)
    bar_height_in = to_float(theme.get("accent_bar_height_in"), 0.32)
    add_background_rect(slide, 0, 0, slide_width_in, bar_height_in, accent)

    if is_title:
        block_height_in = to_float(theme.get("title_block_height_in"), 1.4)
        add_background_rect(
            slide,
            0,
            max(0.0, slide_height_in - block_height_in),
            slide_width_in,
            block_height_in,
            accent_light,
        )


def set_slide_ratio(prs: Presentation, ratio_value: Any) -> None:
    ratio = str(ratio_value).strip().lower() if ratio_value is not None else "16:9"
    ratio_aliases = {
        "16x9": "16:9",
        "widescreen": "16:9",
        "4x3": "4:3",
        "16x10": "16:10",
    }
    ratio = ratio_aliases.get(ratio, ratio)
    if ratio in {"template", "keep"}:
        return
    size_map = {
        "16:9": (13.333, 7.5),
        "4:3": (10.0, 7.5),
        "16:10": (10.0, 6.25),
    }
    if ratio not in size_map:
        raise ValueError(f"Unsupported slide_ratio: {ratio_value}")
    width_in, height_in = size_map[ratio]
    prs.slide_width = Inches(width_in)
    prs.slide_height = Inches(height_in)


def get_layout(prs: Presentation, layout_spec: Any):
    if layout_spec is None:
        raise ValueError("Slide layout is required")

    if isinstance(layout_spec, int) or str(layout_spec).isdigit():
        index = int(layout_spec)
        if index < 0 or index >= len(prs.slide_layouts):
            raise ValueError(f"Layout index out of range: {index}")
        return prs.slide_layouts[index]

    target = str(layout_spec).strip().lower()
    for layout in prs.slide_layouts:
        if layout.name.strip().lower() == target:
            return layout
    raise ValueError(f"Layout not found: {layout_spec}")


def normalize_bullets(items: Iterable[Any]) -> List[Tuple[str, int]]:
    bullets: List[Tuple[str, int]] = []
    for item in items:
        if isinstance(item, dict):
            text = str(item.get("text", ""))
            level = int(item.get("level", 0))
        else:
            text = str(item)
            level = 0
        bullets.append((text, level))
    return bullets


def apply_text(text_frame, content: Any) -> None:
    text_frame.clear()
    if isinstance(content, list):
        bullets = normalize_bullets(content)
        for idx, (text, level) in enumerate(bullets):
            paragraph = text_frame.paragraphs[0] if idx == 0 else text_frame.add_paragraph()
            paragraph.text = text
            paragraph.level = level
        return

    if isinstance(content, dict):
        if "bullets" in content:
            bullets = normalize_bullets(content["bullets"] or [])
            for idx, (text, level) in enumerate(bullets):
                paragraph = text_frame.paragraphs[0] if idx == 0 else text_frame.add_paragraph()
                paragraph.text = text
                paragraph.level = level
            return
        if "text" in content:
            text_frame.text = str(content["text"])
            return

    text_frame.text = "" if content is None else str(content)


def apply_placeholder(slide, placeholder, content: Any, strict: bool) -> None:
    if isinstance(content, dict) and "image" in content:
        image_path = Path(content["image"])
        if not image_path.exists():
            message = f"Image not found: {image_path}"
            if strict:
                raise FileNotFoundError(message)
            print(f"WARNING: {message}")
            return

        if hasattr(placeholder, "insert_picture"):
            placeholder.insert_picture(str(image_path))
        else:
            slide.shapes.add_picture(
                str(image_path),
                placeholder.left,
                placeholder.top,
                placeholder.width,
                placeholder.height,
            )
        return

    if not getattr(placeholder, "has_text_frame", False):
        message = f"Placeholder idx={placeholder.placeholder_format.idx} has no text frame"
        if strict:
            raise ValueError(message)
        print(f"WARNING: {message}")
        return

    apply_text(placeholder.text_frame, content)


def find_placeholder_by_types(slide, placeholder_types: Iterable[PP_PLACEHOLDER]):
    for placeholder in slide.placeholders:
        if placeholder.placeholder_format.type in placeholder_types:
            return placeholder
    return None


def apply_shorthand(slide, spec: Dict[str, Any], strict: bool) -> None:
    title = spec.get("title")
    subtitle = spec.get("subtitle")
    body = spec.get("body")
    bullets = spec.get("bullets")

    if title is not None:
        placeholder = find_placeholder_by_types(
            slide, (PP_PLACEHOLDER.TITLE, PP_PLACEHOLDER.CENTER_TITLE)
        )
        if placeholder is None:
            message = "TITLE placeholder not found for slide"
            if strict:
                raise ValueError(message)
            print(f"WARNING: {message}")
        else:
            apply_placeholder(slide, placeholder, title, strict)

    if subtitle is not None:
        placeholder = find_placeholder_by_types(slide, (PP_PLACEHOLDER.SUBTITLE,))
        if placeholder is None:
            message = "SUBTITLE placeholder not found for slide"
            if strict:
                raise ValueError(message)
            print(f"WARNING: {message}")
        else:
            apply_placeholder(slide, placeholder, subtitle, strict)

    if body is not None:
        placeholder = find_placeholder_by_types(
            slide, (PP_PLACEHOLDER.BODY, PP_PLACEHOLDER.OBJECT)
        )
        if placeholder is None:
            message = "BODY placeholder not found for slide"
            if strict:
                raise ValueError(message)
            print(f"WARNING: {message}")
        else:
            apply_placeholder(slide, placeholder, body, strict)

    if bullets is not None and body is None:
        placeholder = find_placeholder_by_types(
            slide, (PP_PLACEHOLDER.BODY, PP_PLACEHOLDER.OBJECT)
        )
        if placeholder is None:
            message = "BODY placeholder not found for slide bullets"
            if strict:
                raise ValueError(message)
            print(f"WARNING: {message}")
        else:
            apply_placeholder(slide, placeholder, {"bullets": bullets}, strict)


def main() -> None:
    parser = argparse.ArgumentParser(description="Build PPTX deck from outline.")
    parser.add_argument("--template", type=Path, help="Path to PPTX template")
    parser.add_argument("--outline", type=Path, required=True, help="Outline YAML/JSON")
    parser.add_argument("--output", type=Path, required=True, help="Output PPTX path")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail on missing placeholders or assets",
    )
    args = parser.parse_args()

    outline_path = args.outline.resolve()
    outline = load_outline(outline_path)
    outline_dir = outline_path.parent

    if not isinstance(outline, dict):
        raise ValueError("Outline must be a dict with 'slides' array")

    template_value = args.template
    template_base_dir = None
    if template_value is None:
        meta_template = outline.get("meta", {}).get("template")
        if meta_template:
            template_value = Path(meta_template)
            template_base_dir = outline_dir
    if template_value is None:
        raise ValueError("Template path is required via --template or meta.template")
    if template_base_dir is None:
        template_base_dir = Path.cwd()
    template_path = resolve_path(str(template_value), template_base_dir)
    if not template_path.exists():
        raise FileNotFoundError(f"Template not found: {template_path}")

    slides = outline.get("slides", [])
    if not isinstance(slides, list) or not slides:
        raise ValueError("Outline must include a non-empty 'slides' list")

    prs = Presentation(str(template_path))
    meta = outline.get("meta", {})
    set_slide_ratio(prs, meta.get("slide_ratio", "16:9"))
    theme = meta.get("theme", {})

    for slide_index, spec in enumerate(slides):
        if not isinstance(spec, dict):
            raise ValueError(f"Slide spec at index {slide_index} must be a dict")

        layout = get_layout(prs, spec.get("layout"))
        slide = prs.slides.add_slide(layout)
        apply_theme(slide, prs, theme, slide_index == 0)

        placeholders_spec = spec.get("placeholders")
        if placeholders_spec:
            placeholder_map = {
                ph.placeholder_format.idx: ph for ph in slide.placeholders
            }
            for raw_idx, content in placeholders_spec.items():
                try:
                    idx = int(raw_idx)
                except (TypeError, ValueError):
                    if args.strict:
                        raise ValueError(
                            f"Invalid placeholder index '{raw_idx}' on slide {slide_index}"
                        )
                    print(
                        f"WARNING: Invalid placeholder index '{raw_idx}' on slide {slide_index}"
                    )
                    continue

                placeholder = placeholder_map.get(idx)
                if placeholder is None:
                    message = f"Placeholder idx={idx} not found on slide {slide_index}"
                    if args.strict:
                        raise ValueError(message)
                    print(f"WARNING: {message}")
                    continue

                apply_placeholder(slide, placeholder, content, args.strict)
        else:
            apply_shorthand(slide, spec, args.strict)

        notes = spec.get("notes")
        if notes:
            slide.notes_slide.notes_text_frame.text = str(notes)

    output_path = args.output
    output_path.parent.mkdir(parents=True, exist_ok=True)
    prs.save(str(output_path))
    print(f"Saved presentation: {output_path}")


if __name__ == "__main__":
    main()
