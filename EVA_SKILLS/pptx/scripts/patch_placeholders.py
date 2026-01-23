#!/usr/bin/env python3
"""Patch PPTX templates to add title/body placeholders to layouts."""

from __future__ import annotations

import argparse
import shutil
import tempfile
import zipfile
from copy import deepcopy
from pathlib import Path
from typing import Iterable, List
import posixpath
import xml.etree.ElementTree as ET

from pptx import Presentation

P_NS = "http://schemas.openxmlformats.org/presentationml/2006/main"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"

ET.register_namespace("a", "http://schemas.openxmlformats.org/drawingml/2006/main")
ET.register_namespace("r", R_NS)
ET.register_namespace("p", P_NS)


def build_base_layouts() -> tuple[bytes, bytes]:
    """Create a base PPTX and return title + title/content layout XML."""
    temp_file = tempfile.NamedTemporaryFile(delete=False, suffix=".pptx")
    temp_path = Path(temp_file.name)
    temp_file.close()
    try:
        Presentation().save(temp_path)
        with zipfile.ZipFile(temp_path, "r") as zf:
            title_layout = zf.read("ppt/slideLayouts/slideLayout1.xml")
            content_layout = zf.read("ppt/slideLayouts/slideLayout2.xml")
    finally:
        temp_path.unlink(missing_ok=True)
    return title_layout, content_layout


def extract_placeholders(layout_xml: bytes, modes: set[str]) -> List[ET.Element]:
    tree = ET.fromstring(layout_xml)
    sp_tree = tree.find(f".//{{{P_NS}}}spTree")
    if sp_tree is None:
        return []
    shapes: List[ET.Element] = []
    for sp in sp_tree.findall(f"{{{P_NS}}}sp"):
        ph = sp.find(f".//{{{P_NS}}}ph")
        if ph is None:
            continue
        ph_type = ph.get("type")
        ph_idx = ph.get("idx")
        if "title" in modes and ph_type in {"ctrTitle", "title"}:
            shapes.append(deepcopy(sp))
            continue
        if "subtitle" in modes and ph_type == "subTitle":
            shapes.append(deepcopy(sp))
            continue
        if "body" in modes and ph_type is None and ph_idx == "1":
            shapes.append(deepcopy(sp))
            continue
    return shapes


def insert_placeholders(layout_xml: bytes, shapes: Iterable[ET.Element]) -> bytes:
    tree = ET.fromstring(layout_xml)
    sp_tree = tree.find(f".//{{{P_NS}}}spTree")
    if sp_tree is None:
        return layout_xml

    for sp in list(sp_tree.findall(f"{{{P_NS}}}sp")):
        if sp.find(f".//{{{P_NS}}}ph") is not None:
            sp_tree.remove(sp)

    insert_index = 0
    for idx, child in enumerate(list(sp_tree)):
        if child.tag in {f"{{{P_NS}}}nvGrpSpPr", f"{{{P_NS}}}grpSpPr"}:
            insert_index = idx + 1

    for shape in shapes:
        sp_tree.insert(insert_index, deepcopy(shape))
        insert_index += 1

    return ET.tostring(tree, encoding="utf-8", xml_declaration=True)


def resolve_layout_paths(zf: zipfile.ZipFile) -> List[str]:
    pres_xml = ET.fromstring(zf.read("ppt/presentation.xml"))
    pres_rels_xml = ET.fromstring(zf.read("ppt/_rels/presentation.xml.rels"))

    pres_rel_map = {
        rel.get("Id"): rel.get("Target")
        for rel in pres_rels_xml.findall(
            ".//{http://schemas.openxmlformats.org/package/2006/relationships}Relationship"
        )
    }
    master_ids = pres_xml.findall(f".//{{{P_NS}}}sldMasterId")
    if not master_ids:
        return []
    master_rid = master_ids[0].get(f"{{{R_NS}}}id")
    master_target = pres_rel_map.get(master_rid)
    if not master_target:
        return []
    if not master_target.startswith("ppt/"):
        master_target = f"ppt/{master_target}"
    master_target = posixpath.normpath(master_target)

    master_xml = ET.fromstring(zf.read(master_target))
    master_dir = posixpath.dirname(master_target)
    master_rels_path = (
        Path(master_target).parent / "_rels" / f"{Path(master_target).name}.rels"
    )
    master_rels_xml = ET.fromstring(zf.read(str(master_rels_path).replace("\\", "/")))
    master_rel_map = {
        rel.get("Id"): rel.get("Target")
        for rel in master_rels_xml.findall(
            ".//{http://schemas.openxmlformats.org/package/2006/relationships}Relationship"
        )
    }

    layout_ids = master_xml.findall(f".//{{{P_NS}}}sldLayoutId")
    layout_paths: List[str] = []
    for layout in layout_ids:
        r_id = layout.get(f"{{{R_NS}}}id")
        target = master_rel_map.get(r_id)
        if not target:
            continue
        target = posixpath.normpath(posixpath.join(master_dir, target))
        layout_paths.append(target)
    return layout_paths


def patch_template(path: Path, cover_index: int, content_index: int) -> None:
    title_layout_xml, content_layout_xml = build_base_layouts()
    cover_shapes = extract_placeholders(title_layout_xml, {"title", "subtitle"})
    content_shapes = extract_placeholders(content_layout_xml, {"title", "body"})

    temp_path = path.with_suffix(".patched.pptx")
    with zipfile.ZipFile(path, "r") as zin, zipfile.ZipFile(
        temp_path, "w", compression=zipfile.ZIP_DEFLATED
    ) as zout:
        layout_paths = resolve_layout_paths(zin)
        cover_layout = layout_paths[cover_index] if cover_index < len(layout_paths) else None
        content_layout = layout_paths[content_index] if content_index < len(layout_paths) else None

        for item in zin.infolist():
            data = zin.read(item.filename)
            if item.filename == cover_layout:
                data = insert_placeholders(data, cover_shapes)
            elif item.filename == content_layout:
                data = insert_placeholders(data, content_shapes)
            zout.writestr(item, data)

    shutil.move(str(temp_path), str(path))


def main() -> None:
    parser = argparse.ArgumentParser(description="Patch PPTX templates with placeholders.")
    parser.add_argument(
        "templates",
        nargs="*",
        help="Template paths to patch (default: templates directory).",
    )
    parser.add_argument(
        "--templates-dir",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "templates",
        help="Directory to scan when no templates are provided.",
    )
    parser.add_argument("--cover-index", type=int, default=0, help="Cover layout index")
    parser.add_argument(
        "--content-index", type=int, default=1, help="Content layout index"
    )
    args = parser.parse_args()

    if args.templates:
        targets = [Path(t) for t in args.templates]
    else:
        targets = sorted(args.templates_dir.glob("*.pptx"))

    if not targets:
        raise SystemExit("No templates found to patch.")

    for template in targets:
        patch_template(template, args.cover_index, args.content_index)
        print(f"patched: {template}")


if __name__ == "__main__":
    main()
