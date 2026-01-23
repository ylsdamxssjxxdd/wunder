#!/usr/bin/env python3
"""Inspect PPTX layouts and placeholders to help build outline mappings."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from pptx import Presentation
from pptx.enum.shapes import PP_PLACEHOLDER


def placeholder_type_name(placeholder) -> str:
    try:
        return placeholder.placeholder_format.type.name
    except Exception:
        return "UNKNOWN"


def placeholder_type_hint(placeholder) -> str:
    if not hasattr(placeholder, "has_text_frame") or not placeholder.has_text_frame:
        return "non-text"
    if placeholder.placeholder_format.type == PP_PLACEHOLDER.PICTURE:
        return "picture"
    return "text"


def normalize_text(value: str) -> str:
    return value.replace("\u00a0", " ")


def main() -> None:
    parser = argparse.ArgumentParser(description="Inspect PPTX template layouts.")
    parser.add_argument("template", type=Path, help="Path to PPTX template")
    args = parser.parse_args()

    if not args.template.exists():
        raise FileNotFoundError(f"Template not found: {args.template}")

    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")

    prs = Presentation(str(args.template))

    for idx, layout in enumerate(prs.slide_layouts):
        layout_name = normalize_text(layout.name)
        print(f'Layout[{idx}] "{layout_name}"')
        placeholders = list(layout.placeholders)
        if not placeholders:
            print("  (no placeholders)")
            continue
        for ph in placeholders:
            ph_idx = ph.placeholder_format.idx
            ph_type = placeholder_type_name(ph)
            hint = placeholder_type_hint(ph)
            name = normalize_text(ph.name)
            print(f"  - idx={ph_idx} type={ph_type} hint={hint} name={name}")
        print("")


if __name__ == "__main__":
    main()
