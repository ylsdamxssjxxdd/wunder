#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import importlib.util
import sys
import types
import unittest
from pathlib import Path


SCRIPT_PATH = Path(__file__).resolve().parent / "convert_markdown_to_docx.py"
if "docx" not in sys.modules:
    fake_docx = types.ModuleType("docx")
    fake_docx.Document = object
    sys.modules["docx"] = fake_docx

    enum_style = types.ModuleType("docx.enum.style")
    enum_style.WD_STYLE_TYPE = object
    sys.modules["docx.enum.style"] = enum_style

    enum_table = types.ModuleType("docx.enum.table")
    enum_table.WD_TABLE_ALIGNMENT = types.SimpleNamespace(LEFT="LEFT", CENTER="CENTER", RIGHT="RIGHT")
    enum_table.WD_CELL_VERTICAL_ALIGNMENT = types.SimpleNamespace(CENTER="CENTER")
    sys.modules["docx.enum.table"] = enum_table

    enum_text = types.ModuleType("docx.enum.text")
    enum_text.WD_ALIGN_PARAGRAPH = types.SimpleNamespace(
        LEFT="LEFT",
        CENTER="CENTER",
        RIGHT="RIGHT",
        MULTIPLE="MULTIPLE",
    )
    enum_text.WD_LINE_SPACING = types.SimpleNamespace(EXACTLY="EXACTLY", SINGLE="SINGLE", MULTIPLE="MULTIPLE")
    sys.modules["docx.enum.text"] = enum_text

    oxml_module = types.ModuleType("docx.oxml")
    oxml_module.OxmlElement = lambda name: {"name": name}
    sys.modules["docx.oxml"] = oxml_module

    oxml_ns = types.ModuleType("docx.oxml.ns")
    oxml_ns.qn = lambda value: value
    sys.modules["docx.oxml.ns"] = oxml_ns

    shared_module = types.ModuleType("docx.shared")
    shared_module.Cm = lambda value: value
    shared_module.Pt = lambda value: value
    shared_module.RGBColor = lambda r, g, b: (r, g, b)
    sys.modules["docx.shared"] = shared_module

SPEC = importlib.util.spec_from_file_location("convert_markdown_to_docx", SCRIPT_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


class ConvertMarkdownToDocxTests(unittest.TestCase):
    def test_normalize_markdown_layout_splits_inline_heading_and_drops_thematic_break(self):
        source = (
            "# 标题\n\n"
            "第一段内容 ---\n"
            "## 第二节\n"
            "| 列1 | 列2 |\n"
            "| --- | --- |\n"
            "| A | B |\n"
        )
        normalized = MODULE.normalize_markdown_layout(source)
        self.assertNotIn(" ---\n## 第二节", normalized)
        self.assertNotIn("\n---\n", normalized)
        self.assertIn("第一段内容", normalized)
        self.assertIn("\n\n## 第二节\n\n| 列1 | 列2 |", normalized)

    def test_markdown_residue_patterns_detect_expected_cases(self):
        self.assertIsNotNone(MODULE.DOCX_MARKDOWN_HEADING_RE.match("## 一、残留标题"))
        self.assertIsNotNone(MODULE.DOCX_PIPE_TABLE_RE.search("| --- | --- |"))
        self.assertIsNotNone(MODULE.DOCX_FENCE_RE.match("```python"))

    def test_parse_markdown_link_keeps_wrapped_url(self):
        source = "[Aliyun](https://\n  developer.aliyun.com/article/1724368)"
        parsed = MODULE.parse_markdown_link(source, 0)
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed[0], "Aliyun")
        self.assertEqual(parsed[1], "https://developer.aliyun.com/article/1724368")

    def test_promote_document_title_shifts_following_headings(self):
        source = "示例标题\n# 第一部分\n## 第二部分\n"
        promoted = MODULE.promote_document_title(source)
        lines = promoted.splitlines()
        self.assertEqual(lines[0], "# 示例标题")
        self.assertEqual(lines[1], "## 第一部分")
        self.assertEqual(lines[2], "### 第二部分")

    def test_promote_document_title_keeps_explicit_title_input(self):
        source = "# 示例标题\n# 第一部分\n## 第二部分\n"
        promoted = MODULE.promote_document_title(source)
        lines = promoted.splitlines()
        self.assertEqual(lines[0], "# 示例标题")
        self.assertEqual(lines[1], "# 第一部分")
        self.assertEqual(lines[2], "## 第二部分")

    def test_normalize_title_and_field_blocks_breaks_dense_fields(self):
        source = (
            "赛题标题\n"
            "[命题需求单位]：\n"
            "[命题需求单位专家]：\n"
            "[命题负责人联系方式]：\n"
            "正文开始\n"
        )
        normalized = MODULE.normalize_title_and_field_blocks(source)
        self.assertIn(
            "[命题需求单位]：\n\n[命题需求单位专家]：\n\n[命题负责人联系方式]：\n\n正文开始",
            normalized,
        )

    def test_normalize_markdown_layout_keeps_table_block_together(self):
        source = (
            "字段说明：\n\n"
            "| 字段名 | 类型 | 说明 |\n"
            "| --- | --- | --- |\n"
            "| a | b | c |\n"
            "下一段\n"
        )
        normalized = MODULE.normalize_markdown_layout(source)
        self.assertIn("| 字段名 | 类型 | 说明 |\n| --- | --- | --- |\n| a | b | c |", normalized)
        self.assertIn("| a | b | c |\n\n下一段", normalized)


if __name__ == "__main__":
    unittest.main()
