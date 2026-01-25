#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import csv
import json
from pathlib import Path
from typing import Any, Dict, List, Tuple


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate OKR/KPI performance summary.")
    parser.add_argument("--input", required=True, help="Input JSON path.")
    parser.add_argument("--output-md", required=True, help="Output Markdown path.")
    parser.add_argument("--output-csv", required=True, help="Output CSV path.")
    return parser.parse_args()


def normalize_weights(weights: List[float]) -> List[float]:
    total = sum(weights)
    if total <= 0:
        count = len(weights)
        return [1 / count for _ in weights] if count else []
    return [w / total for w in weights]


def weighted_score(items: List[Dict[str, Any]], score_key: str = "score") -> float:
    if not items:
        return 0.0
    weights = [float(item.get("weight", 0)) for item in items]
    norm_weights = normalize_weights(weights)
    scores = [float(item.get(score_key, 0)) for item in items]
    return sum(w * s for w, s in zip(norm_weights, scores))


def compute_okr_score(okr: List[Dict[str, Any]]) -> Tuple[float, List[Dict[str, Any]]]:
    if not okr:
        return 0.0, []
    objective_weights = [float(item.get("weight", 0)) for item in okr]
    obj_norm_weights = normalize_weights(objective_weights)

    objective_scores = []
    for obj, obj_weight in zip(okr, obj_norm_weights):
        krs = obj.get("key_results", []) if isinstance(obj.get("key_results"), list) else []
        kr_score = weighted_score(krs)
        objective_scores.append({
            "objective": obj.get("objective", ""),
            "weight": obj_weight,
            "score": kr_score,
            "key_results": krs,
        })

    okr_score = sum(item["weight"] * item["score"] for item in objective_scores)
    return okr_score, objective_scores


def rating_label(score: float) -> str:
    if score >= 0.9:
        return "优秀"
    if score >= 0.8:
        return "良好"
    if score >= 0.7:
        return "达标"
    return "待改进"


def build_markdown(data: Dict[str, Any]) -> str:
    employee = data.get("employee", "")
    period = data.get("period", "")
    role = data.get("role", "")

    okr_score, okr_detail = compute_okr_score(data.get("okr", []))
    kpi_score = weighted_score(data.get("kpi", []))
    behavior_score = weighted_score(data.get("behavior", []))

    section_weights = data.get(
        "section_weights",
        {"okr": 0.6, "kpi": 0.3, "behavior": 0.1},
    )
    weights = normalize_weights(
        [
            float(section_weights.get("okr", 0.6)),
            float(section_weights.get("kpi", 0.3)),
            float(section_weights.get("behavior", 0.1)),
        ]
    )
    okr_w, kpi_w, behavior_w = weights
    overall = okr_score * okr_w + kpi_score * kpi_w + behavior_score * behavior_w

    lines: List[str] = ["# 绩效评估汇总", ""]
    if employee:
        lines.append(f"- 员工：{employee}")
    if period:
        lines.append(f"- 周期：{period}")
    if role:
        lines.append(f"- 岗位：{role}")
    lines.append("")

    lines.append("## 汇总评分")
    lines.append("")
    lines.append("| 模块 | 权重 | 得分 |")
    lines.append("| --- | --- | --- |")
    lines.append(f"| OKR | {okr_w:.2f} | {okr_score:.2f} |")
    lines.append(f"| KPI | {kpi_w:.2f} | {kpi_score:.2f} |")
    lines.append(f"| 行为 | {behavior_w:.2f} | {behavior_score:.2f} |")
    lines.append("")
    lines.append(f"综合评分：{overall:.2f}（{rating_label(overall)}）")
    lines.append("")

    lines.append("## OKR 明细")
    if okr_detail:
        for obj in okr_detail:
            lines.append("")
            lines.append(f"### {obj['objective']}（权重 {obj['weight']:.2f} / 得分 {obj['score']:.2f}）")
            lines.append("| KR | 权重 | 评分 |")
            lines.append("| --- | --- | --- |")
            krs = obj.get("key_results", [])
            if krs:
                kr_weights = normalize_weights([float(kr.get("weight", 0)) for kr in krs])
                for kr, kr_weight in zip(krs, kr_weights):
                    lines.append(
                        f"| {kr.get('name', '')} | {kr_weight:.2f} | {float(kr.get('score', 0)):.2f} |"
                    )
            else:
                lines.append("| （暂无） |  |  |")
    else:
        lines.append("（暂无）")
    lines.append("")

    lines.append("## KPI 明细")
    kpi_items = data.get("kpi", []) if isinstance(data.get("kpi"), list) else []
    if kpi_items:
        lines.append("| 指标 | 权重 | 目标 | 实际 | 评分 |")
        lines.append("| --- | --- | --- | --- | --- |")
        kpi_weights = normalize_weights([float(item.get("weight", 0)) for item in kpi_items])
        for item, weight in zip(kpi_items, kpi_weights):
            lines.append(
                f"| {item.get('name', '')} | {weight:.2f} | {item.get('target', '')} | {item.get('actual', '')} | {float(item.get('score', 0)):.2f} |"
            )
    else:
        lines.append("（暂无）")
    lines.append("")

    lines.append("## 行为/价值观")
    behavior_items = data.get("behavior", []) if isinstance(data.get("behavior"), list) else []
    if behavior_items:
        lines.append("| 维度 | 权重 | 评分 | 证据 |")
        lines.append("| --- | --- | --- | --- |")
        behavior_weights = normalize_weights([float(item.get("weight", 0)) for item in behavior_items])
        for item, weight in zip(behavior_items, behavior_weights):
            lines.append(
                f"| {item.get('name', '')} | {weight:.2f} | {float(item.get('score', 0)):.2f} | {item.get('evidence', '')} |"
            )
    else:
        lines.append("（暂无）")

    return "\n".join(lines).strip() + "\n"


def write_csv(data: Dict[str, Any], path: Path) -> None:
    rows: List[Dict[str, str]] = []
    okr_items = data.get("okr", []) if isinstance(data.get("okr"), list) else []
    for obj in okr_items:
        krs = obj.get("key_results", []) if isinstance(obj.get("key_results"), list) else []
        for kr in krs:
            rows.append(
                {
                    "type": "OKR-KR",
                    "objective": obj.get("objective", ""),
                    "name": kr.get("name", ""),
                    "weight": str(kr.get("weight", "")),
                    "score": str(kr.get("score", "")),
                    "target": str(kr.get("target", "")),
                    "actual": str(kr.get("actual", "")),
                }
            )
    kpi_items = data.get("kpi", []) if isinstance(data.get("kpi"), list) else []
    for kpi in kpi_items:
        rows.append(
            {
                "type": "KPI",
                "objective": "",
                "name": kpi.get("name", ""),
                "weight": str(kpi.get("weight", "")),
                "score": str(kpi.get("score", "")),
                "target": str(kpi.get("target", "")),
                "actual": str(kpi.get("actual", "")),
            }
        )
    behavior_items = data.get("behavior", []) if isinstance(data.get("behavior"), list) else []
    for item in behavior_items:
        rows.append(
            {
                "type": "Behavior",
                "objective": "",
                "name": item.get("name", ""),
                "weight": str(item.get("weight", "")),
                "score": str(item.get("score", "")),
                "target": "",
                "actual": item.get("evidence", ""),
            }
        )

    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["type", "objective", "name", "weight", "score", "target", "actual"],
        )
        writer.writeheader()
        writer.writerows(rows)


def main() -> int:
    args = parse_args()
    data = json.loads(Path(args.input).read_text(encoding="utf-8-sig"))

    output_md = Path(args.output_md)
    output_csv = Path(args.output_csv)
    output_md.parent.mkdir(parents=True, exist_ok=True)
    output_csv.parent.mkdir(parents=True, exist_ok=True)

    output_md.write_text(build_markdown(data), encoding="utf-8")
    write_csv(data, output_csv)

    print(f"Saved: {output_md}")
    print(f"Saved: {output_csv}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
