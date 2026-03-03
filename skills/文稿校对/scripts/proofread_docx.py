#!/usr/bin/env python3
"""Proofread DOCX for common Chinese official document format and typo issues."""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Optional

try:
    from docx import Document
    from docx.enum.text import WD_ALIGN_PARAGRAPH
    from docx.oxml.ns import qn
except Exception as exc:  # pragma: no cover
    raise SystemExit(
        "python-docx is required. Please install it first: pip install python-docx\n"
        f"detail: {exc}"
    )


EXPECTED = {
    "page_width_cm": 21.0,
    "page_height_cm": 29.7,
    "margin_top_cm": 3.7,
    "margin_bottom_cm": 3.5,
    "margin_left_cm": 2.8,
    "margin_right_cm": 2.6,
    "title_font_keywords": ("方正小标宋", "小标宋"),
    "title_size_pt": 22.0,
    "heading1_font_keywords": ("黑体",),
    "heading2_font_keywords": ("楷体",),
    "heading3_font_keywords": ("仿宋",),
    "body_font_keywords": ("仿宋",),
    "body_size_pt": 16.0,
    "line_spacing_pt": 28.9,
    "first_line_indent_pt": 32.0,
}

TOLERANCE = {
    "page_cm": 0.2,
    "margin_cm": 0.2,
    "size_pt": 1.0,
    "line_spacing_pt": 1.2,
    "indent_pt": 4.0,
}

COMMON_TYPO_MAP = {
    "部暑": "部署",
    "布署": "部署",
    "必需": "必须",
    "做为": "作为",
    "以经": "已经",
    "即然": "既然",
    "按排": "安排",
    "决对": "绝对",
    "在职工做": "在职工作",
    "通迅": "通讯",
    "凭添": "平添",
    "渡过难关": "度过难关",
    "再接再励": "再接再厉",
    "一愁莫展": "一筹莫展",
    "默守成规": "墨守成规",
    "相形见拙": "相形见绌",
    "随声附合": "随声附和",
    "黙契": "默契",
    "按步就班": "按部就班",
    "安份守己": "安分守己",
    "重蹈复辙": "重蹈覆辙",
    "既往不究": "既往不咎",
    "冒然": "贸然",
    "针贬": "针砭",
    "精减": "精简",
    "幅射": "辐射",
}

DOUBLE_CHAR_TYPOS = {
    "的的": "的",
    "了了": "了",
    "在在": "在",
    "和和": "和",
    "是是": "是",
}

HEADING_PATTERNS = (
    ("heading1", re.compile(r"^[一二三四五六七八九十百千]+、")),
    ("heading2", re.compile(r"^（[一二三四五六七八九十百千]+）")),
    ("heading3", re.compile(r"^[0-9]+[\\.．]")),
    ("heading4", re.compile(r"^（[0-9]+）")),
)

STRUCTURE_PREFIXES = ("主送：", "附件：", "落款：", "抄送：", "印发：", "签发人：", "发文字号：")


@dataclass
class Issue:
    severity: str
    category: str
    rule: str
    location: str
    snippet: str
    expected: str
    actual: str
    suggestion: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Proofread DOCX document formatting and typos.")
    parser.add_argument("docx", help="Input .docx path")
    parser.add_argument("--output-json", dest="output_json", help="Write result JSON to file")
    parser.add_argument("--output-md", dest="output_md", help="Write markdown report to file")
    parser.add_argument(
        "--max-findings",
        type=int,
        default=200,
        help="Maximum findings returned in issues/typo list (default: 200)",
    )
    return parser.parse_args()


def is_close(actual: Optional[float], target: float, tolerance: float) -> bool:
    if actual is None:
        return False
    return abs(actual - target) <= tolerance


def to_pt(value) -> Optional[float]:
    if value is None:
        return None
    if hasattr(value, "pt"):
        try:
            return float(value.pt)
        except Exception:
            return None
    try:
        return float(value)
    except Exception:
        return None


def normalize_text(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def clip_text(text: str, limit: int = 48) -> str:
    text = normalize_text(text)
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "…"


def paragraph_text_items(document) -> list[tuple[int, str, object]]:
    items = []
    for idx, paragraph in enumerate(document.paragraphs, start=1):
        text = normalize_text(paragraph.text)
        if text:
            items.append((idx, text, paragraph))
    return items


def paragraph_level(text: str) -> Optional[str]:
    for level, pattern in HEADING_PATTERNS:
        if pattern.search(text):
            return level
    return None


def extract_run_font_name(run) -> Optional[str]:
    names = []
    if run.font and run.font.name:
        names.append(run.font.name)
    r_pr = getattr(run._element, "rPr", None)
    if r_pr is not None and getattr(r_pr, "rFonts", None) is not None:
        for key in ("eastAsia", "ascii", "hAnsi", "cs"):
            value = r_pr.rFonts.get(qn(f"w:{key}"))
            if value:
                names.append(value)
    for name in names:
        cleaned = str(name).strip()
        if cleaned:
            return cleaned
    return None


def dominant_font(paragraph) -> Optional[str]:
    weights = {}
    for run in paragraph.runs:
        text_len = len(run.text.strip())
        if text_len == 0:
            continue
        font_name = extract_run_font_name(run)
        if not font_name:
            continue
        weights[font_name] = weights.get(font_name, 0) + text_len
    if not weights:
        style = getattr(paragraph, "style", None)
        if style is not None and style.font is not None and style.font.name:
            return str(style.font.name).strip()
        return None
    return max(weights.items(), key=lambda item: item[1])[0]


def dominant_size_pt(paragraph) -> Optional[float]:
    weights = {}
    for run in paragraph.runs:
        text_len = len(run.text.strip())
        if text_len == 0:
            continue
        size = to_pt(run.font.size)
        if size is None:
            continue
        rounded = round(size, 1)
        weights[rounded] = weights.get(rounded, 0) + text_len
    if weights:
        return max(weights.items(), key=lambda item: item[1])[0]
    style = getattr(paragraph, "style", None)
    if style is not None and style.font is not None:
        return to_pt(style.font.size)
    return None


def paragraph_line_spacing_pt(paragraph) -> Optional[float]:
    value = to_pt(paragraph.paragraph_format.line_spacing)
    if value is not None:
        return value
    style = getattr(paragraph, "style", None)
    if style is not None and style.paragraph_format is not None:
        return to_pt(style.paragraph_format.line_spacing)
    return None


def paragraph_first_indent_pt(paragraph) -> Optional[float]:
    value = to_pt(paragraph.paragraph_format.first_line_indent)
    if value is not None:
        return value
    style = getattr(paragraph, "style", None)
    if style is not None and style.paragraph_format is not None:
        return to_pt(style.paragraph_format.first_line_indent)
    return None


def font_matches(font_name: Optional[str], keywords: Iterable[str]) -> bool:
    if not font_name:
        return False
    lowered = font_name.lower()
    return any(keyword.lower() in lowered for keyword in keywords)


def add_issue(
    issues: list[Issue],
    severity: str,
    category: str,
    rule: str,
    location: str,
    snippet: str,
    expected: str,
    actual: str,
    suggestion: str,
) -> None:
    issues.append(
        Issue(
            severity=severity,
            category=category,
            rule=rule,
            location=location,
            snippet=snippet,
            expected=expected,
            actual=actual,
            suggestion=suggestion,
        )
    )


def locate_title(items: list[tuple[int, str, object]]) -> Optional[int]:
    if not items:
        return None
    for index, (_, text, _) in enumerate(items):
        if text.startswith(STRUCTURE_PREFIXES):
            continue
        return index
    return 0


def check_layout(document, issues: list[Issue]) -> None:
    if not document.sections:
        return
    section = document.sections[0]
    layout_checks = [
        ("页面宽度", section.page_width.cm, EXPECTED["page_width_cm"], TOLERANCE["page_cm"], "A4 宽度 21.0cm"),
        ("页面高度", section.page_height.cm, EXPECTED["page_height_cm"], TOLERANCE["page_cm"], "A4 高度 29.7cm"),
        ("上边距", section.top_margin.cm, EXPECTED["margin_top_cm"], TOLERANCE["margin_cm"], "3.7cm"),
        ("下边距", section.bottom_margin.cm, EXPECTED["margin_bottom_cm"], TOLERANCE["margin_cm"], "3.5cm"),
        ("左边距", section.left_margin.cm, EXPECTED["margin_left_cm"], TOLERANCE["margin_cm"], "2.8cm"),
        ("右边距", section.right_margin.cm, EXPECTED["margin_right_cm"], TOLERANCE["margin_cm"], "2.6cm"),
    ]
    for rule, actual, target, tolerance, expected_text in layout_checks:
        if not is_close(actual, target, tolerance):
            add_issue(
                issues,
                severity="high",
                category="格式",
                rule=rule,
                location="页面设置",
                snippet="",
                expected=expected_text,
                actual=f"{actual:.2f}cm",
                suggestion=f"将{rule}调整为 {target:.1f}cm。",
            )


def check_title(items: list[tuple[int, str, object]], title_index: int, issues: list[Issue]) -> None:
    paragraph_no, text, paragraph = items[title_index]
    location = f"第{paragraph_no}段"
    align = paragraph.alignment
    if align != WD_ALIGN_PARAGRAPH.CENTER:
        add_issue(
            issues,
            severity="high",
            category="格式",
            rule="标题对齐",
            location=location,
            snippet=clip_text(text),
            expected="居中对齐",
            actual=f"{align}",
            suggestion="将标题段落设置为居中对齐。",
        )
    font_name = dominant_font(paragraph)
    if not font_matches(font_name, EXPECTED["title_font_keywords"]):
        add_issue(
            issues,
            severity="high",
            category="格式",
            rule="标题字体",
            location=location,
            snippet=clip_text(text),
            expected="方正小标宋简体（或小标宋）",
            actual=font_name or "未检测到",
            suggestion="将标题字体调整为方正小标宋简体。",
        )
    size_pt = dominant_size_pt(paragraph)
    if not is_close(size_pt, EXPECTED["title_size_pt"], TOLERANCE["size_pt"]):
        add_issue(
            issues,
            severity="medium",
            category="格式",
            rule="标题字号",
            location=location,
            snippet=clip_text(text),
            expected="2号（约 22pt）",
            actual="未检测到" if size_pt is None else f"{size_pt:.1f}pt",
            suggestion="将标题字号调整为 2号（22pt 左右）。",
        )


def check_body_and_headings(
    items: list[tuple[int, str, object]],
    title_index: int,
    issues: list[Issue],
) -> None:
    for idx, (paragraph_no, text, paragraph) in enumerate(items):
        if idx == title_index:
            continue
        location = f"第{paragraph_no}段"
        if text.startswith(STRUCTURE_PREFIXES):
            continue
        level = paragraph_level(text)
        font_name = dominant_font(paragraph)
        size_pt = dominant_size_pt(paragraph)

        if level == "heading1" and not font_matches(font_name, EXPECTED["heading1_font_keywords"]):
            add_issue(
                issues,
                "medium",
                "格式",
                "一级标题字体",
                location,
                clip_text(text),
                "黑体三号",
                font_name or "未检测到",
                "将一级标题字体设为黑体。",
            )
        elif level == "heading2" and not font_matches(font_name, EXPECTED["heading2_font_keywords"]):
            add_issue(
                issues,
                "medium",
                "格式",
                "二级标题字体",
                location,
                clip_text(text),
                "楷体_GB2312 三号",
                font_name or "未检测到",
                "将二级标题字体设为楷体_GB2312。",
            )
        elif level in {"heading3", "heading4"} and not font_matches(font_name, EXPECTED["heading3_font_keywords"]):
            add_issue(
                issues,
                "low",
                "格式",
                "三级/四级标题字体",
                location,
                clip_text(text),
                "仿宋_GB2312 三号",
                font_name or "未检测到",
                "将三级/四级标题字体设为仿宋_GB2312。",
            )
        elif level is None:
            if not font_matches(font_name, EXPECTED["body_font_keywords"]):
                add_issue(
                    issues,
                    "low",
                    "格式",
                    "正文字体",
                    location,
                    clip_text(text),
                    "仿宋_GB2312 三号",
                    font_name or "未检测到",
                    "将正文字体调整为仿宋_GB2312。",
                )
            if not is_close(size_pt, EXPECTED["body_size_pt"], TOLERANCE["size_pt"]):
                add_issue(
                    issues,
                    "low",
                    "格式",
                    "正文字号",
                    location,
                    clip_text(text),
                    "三号（约 16pt）",
                    "未检测到" if size_pt is None else f"{size_pt:.1f}pt",
                    "将正文字号调整为三号（16pt 左右）。",
                )
            line_spacing = paragraph_line_spacing_pt(paragraph)
            if not is_close(line_spacing, EXPECTED["line_spacing_pt"], TOLERANCE["line_spacing_pt"]):
                add_issue(
                    issues,
                    "medium",
                    "格式",
                    "正文行距",
                    location,
                    clip_text(text),
                    "固定值 28.9pt",
                    "未检测到" if line_spacing is None else f"{line_spacing:.1f}pt",
                    "将正文行距设置为固定值 28.9pt。",
                )
            first_indent = paragraph_first_indent_pt(paragraph)
            if not is_close(first_indent, EXPECTED["first_line_indent_pt"], TOLERANCE["indent_pt"]):
                add_issue(
                    issues,
                    "medium",
                    "格式",
                    "正文首行缩进",
                    location,
                    clip_text(text),
                    "首行缩进 2 字（约 32pt）",
                    "未检测到" if first_indent is None else f"{first_indent:.1f}pt",
                    "将正文首行缩进设置为 2 字。",
                )


def detect_typos(items: list[tuple[int, str, object]]) -> list[Issue]:
    findings: list[Issue] = []
    for paragraph_no, text, _ in items:
        location = f"第{paragraph_no}段"
        for wrong, correct in COMMON_TYPO_MAP.items():
            start = 0
            while True:
                idx = text.find(wrong, start)
                if idx < 0:
                    break
                snippet = clip_text(text[max(0, idx - 10) : idx + len(wrong) + 10])
                add_issue(
                    findings,
                    "medium",
                    "错别字",
                    "常见误写词",
                    location,
                    snippet,
                    f"建议使用“{correct}”",
                    f"检测到“{wrong}”",
                    f"将“{wrong}”改为“{correct}”。",
                )
                start = idx + len(wrong)

        for wrong, correct in DOUBLE_CHAR_TYPOS.items():
            if wrong in text:
                add_issue(
                    findings,
                    "low",
                    "错别字",
                    "重复字",
                    location,
                    clip_text(text),
                    f"建议改为“{correct}”",
                    f"检测到“{wrong}”",
                    f"删除多余字符，建议改为“{correct}”。",
                )

        for match in re.finditer(r"([，。；：、！？])\1+", text):
            punct = match.group(0)
            add_issue(
                findings,
                "low",
                "文本",
                "重复标点",
                location,
                clip_text(text),
                "单个标点",
                f"检测到“{punct}”",
                "删除重复标点，保留一个。",
            )

        for match in re.finditer(r"([\\u4e00-\\u9fff])\\1\\1+", text):
            repeated = match.group(0)
            add_issue(
                findings,
                "low",
                "错别字",
                "疑似重复字",
                location,
                clip_text(text),
                "避免连续 3 个及以上相同汉字",
                f"检测到“{repeated}”",
                "检查是否为输入重复，必要时删除多余字符。",
            )

        for match in re.finditer(r"([\\u4e00-\\u9fff])[,:;]", text):
            punct = match.group(0)[-1]
            add_issue(
                findings,
                "low",
                "文本",
                "半角标点",
                location,
                clip_text(text),
                "中文语境建议使用全角标点（，：；）",
                f"检测到半角“{punct}”",
                "将半角标点替换为中文全角标点。",
            )
    return findings


def deduplicate_issues(issues: list[Issue]) -> list[Issue]:
    seen = set()
    output = []
    for issue in issues:
        key = (
            issue.category,
            issue.rule,
            issue.location,
            issue.expected,
            issue.actual,
            issue.snippet,
        )
        if key in seen:
            continue
        seen.add(key)
        output.append(issue)
    return output


def sort_issues(issues: list[Issue]) -> list[Issue]:
    priority = {"high": 0, "medium": 1, "low": 2}
    return sorted(
        issues,
        key=lambda item: (
            priority.get(item.severity, 9),
            item.category,
            item.rule,
            item.location,
        ),
    )


def evaluate_grade(issues: list[Issue]) -> tuple[int, str, bool]:
    weight = {"high": 10, "medium": 5, "low": 2}
    score = 100 - sum(weight.get(issue.severity, 2) for issue in issues)
    score = max(0, score)
    if score >= 90:
        grade = "A"
    elif score >= 75:
        grade = "B"
    else:
        grade = "C"
    can_publish = not any(issue.severity == "high" for issue in issues)
    return score, grade, can_publish


def markdown_report(
    doc_path: str,
    score: int,
    grade: str,
    can_publish: bool,
    format_issues: list[Issue],
    typo_issues: list[Issue],
) -> str:
    lines = [
        "# 文稿校对结果",
        "",
        "## 一、总体结论",
        f"- 合规等级：**{grade}**",
        f"- 评分：**{score}/100**",
        f"- 是否建议直接发文：**{'是' if can_publish else '否'}**",
        f"- 校对文档：`{doc_path}`",
        "",
        "## 二、格式问题",
    ]
    if not format_issues:
        lines.append("- 未检出明显格式问题。")
    else:
        for issue in format_issues:
            lines.append(
                f"- [{issue.severity}] {issue.location} {issue.rule}："
                f"{issue.actual}（建议：{issue.suggestion}）"
            )

    lines.extend(["", "## 三、错别字与文本问题"])
    if not typo_issues:
        lines.append("- 未检出明显错别字或文本问题。")
    else:
        for issue in typo_issues:
            lines.append(
                f"- [{issue.severity}] {issue.location} {issue.rule}："
                f"{issue.actual}（建议：{issue.suggestion}）"
            )

    lines.extend(["", "## 四、优先修复清单（Top 3）"])
    top3 = [*format_issues, *typo_issues][:3]
    if not top3:
        lines.append("- 无。")
    else:
        for issue in top3:
            lines.append(f"- {issue.location} {issue.rule}：{issue.suggestion}")

    lines.extend(
        [
            "",
            "## 五、复核建议",
            "- 修订后建议再次运行同一脚本进行二次校对。",
            "- 本报告用于初筛，正式发文前建议人工终审。",
            "",
        ]
    )
    return "\n".join(lines)


def analyze_docx(path: Path, max_findings: int) -> dict:
    document = Document(str(path))
    items = paragraph_text_items(document)
    issues: list[Issue] = []
    check_layout(document, issues)

    title_idx = locate_title(items)
    if title_idx is None:
        add_issue(
            issues,
            "high",
            "结构",
            "标题缺失",
            "文档整体",
            "",
            "存在明确公文标题",
            "未检测到标题段落",
            "在文档前部补充标题，并设置为小标宋二号居中。",
        )
    else:
        check_title(items, title_idx, issues)
        check_body_and_headings(items, title_idx, issues)

    typo_issues = detect_typos(items)
    all_issues = deduplicate_issues([*issues, *typo_issues])
    all_issues = sort_issues(all_issues)[: max(1, max_findings)]

    format_issues = [issue for issue in all_issues if issue.category == "格式" or issue.category == "结构"]
    typo_or_text_issues = [issue for issue in all_issues if issue.category in {"错别字", "文本"}]
    score, grade, can_publish = evaluate_grade(all_issues)

    md = markdown_report(
        doc_path=str(path),
        score=score,
        grade=grade,
        can_publish=can_publish,
        format_issues=format_issues,
        typo_issues=typo_or_text_issues,
    )
    severity_count = {"high": 0, "medium": 0, "low": 0}
    for issue in all_issues:
        severity_count[issue.severity] = severity_count.get(issue.severity, 0) + 1

    return {
        "ok": True,
        "document": {
            "path": str(path),
            "paragraph_count": len(document.paragraphs),
            "non_empty_paragraph_count": len(items),
        },
        "summary": {
            "score": score,
            "grade": grade,
            "can_publish_directly": can_publish,
            "issue_count": len(all_issues),
            "severity_count": severity_count,
            "format_issue_count": len(format_issues),
            "typo_issue_count": len(typo_or_text_issues),
        },
        "issues": [asdict(issue) for issue in all_issues],
        "standard_report_markdown": md,
    }


def write_text(path: str, content: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content, encoding="utf-8")


def write_json(path: str, payload: dict) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


def main() -> int:
    args = parse_args()
    docx_path = Path(args.docx)

    if not docx_path.exists():
        print(
            json.dumps(
                {"ok": False, "error": f"文件不存在: {docx_path}"},
                ensure_ascii=False,
            )
        )
        return 2
    if docx_path.suffix.lower() != ".docx":
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": "仅支持 .docx 文件，请先将 .doc 另存为 .docx 后再校对。",
                },
                ensure_ascii=False,
            )
        )
        return 2

    result = analyze_docx(docx_path, max_findings=args.max_findings)
    if args.output_json:
        write_json(args.output_json, result)
    if args.output_md:
        write_text(args.output_md, result["standard_report_markdown"])
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
