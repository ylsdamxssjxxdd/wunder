#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
from pathlib import Path

from docx import Document
from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_LINE_SPACING
from docx.shared import Pt


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Normalize image paragraphs in a DOCX to prevent text overlap.",
    )
    parser.add_argument("input", help="Input DOCX path.")
    parser.add_argument(
        "--output",
        default="",
        help="Output DOCX path (default: overwrite input).",
    )
    parser.add_argument(
        "--space-pt",
        type=float,
        default=6.0,
        help="Space before/after image paragraphs (pt).",
    )
    return parser.parse_args()


def normalize_image_paragraphs(doc: Document, space_pt: float) -> int:
    count = 0
    for paragraph in doc.paragraphs:
        if not paragraph._p.xpath(".//w:drawing"):
            continue
        fmt = paragraph.paragraph_format
        fmt.line_spacing_rule = WD_LINE_SPACING.MULTIPLE
        fmt.line_spacing = 1.0
        fmt.space_before = Pt(space_pt)
        fmt.space_after = Pt(space_pt)
        fmt.first_line_indent = Pt(0)
        fmt.left_indent = None
        paragraph.alignment = WD_ALIGN_PARAGRAPH.CENTER
        count += 1
    return count


def main() -> int:
    args = parse_args()
    input_path = Path(args.input).resolve()
    if not input_path.exists():
        raise SystemExit(f"Input DOCX not found: {input_path}")
    output_path = Path(args.output).resolve() if args.output else input_path

    doc = Document(input_path)
    normalize_image_paragraphs(doc, args.space_pt)
    doc.save(output_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
