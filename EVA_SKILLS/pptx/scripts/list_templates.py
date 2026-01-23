#!/usr/bin/env python3
"""List available PPTX templates in this skill package."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any, Dict, List

import yaml


def load_manifest(path: Path) -> Dict[str, Any]:
    if not path.exists():
        raise FileNotFoundError(f"Manifest not found: {path}")
    data = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict) or "templates" not in data:
        raise ValueError("Invalid manifest format: missing 'templates'")
    return data


def render_text(templates: List[Dict[str, Any]]) -> None:
    for template in templates:
        template_id = template.get("id", "unknown")
        name = template.get("name", "Unnamed")
        file_path = template.get("file", "")
        tags = ", ".join(template.get("tags", []) or [])
        license_name = template.get("license", "unknown")
        print(f"{template_id}: {name}")
        print(f"  file: {file_path}")
        if tags:
            print(f"  tags: {tags}")
        print(f"  license: {license_name}")
        print("")


def main() -> None:
    parser = argparse.ArgumentParser(description="List PPTX templates.")
    parser.add_argument(
        "--manifest",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "templates" / "manifest.yaml",
        help="Path to templates manifest",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output manifest as JSON",
    )
    args = parser.parse_args()

    manifest = load_manifest(args.manifest)
    templates = manifest.get("templates", [])

    if args.json:
        print(json.dumps(manifest, indent=2, ensure_ascii=False))
    else:
        render_text(templates)


if __name__ == "__main__":
    main()
