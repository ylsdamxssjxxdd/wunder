---
name: 绩效OKR-KPI评估
description: "生成绩效/OKR/KPI 模板并汇总评分，输出 Markdown 与 CSV。"
---

# 绩效 / OKR / KPI 模板与评估汇总

## 概要
提供可直接落地的 OKR/KPI 模板，并基于评分数据生成绩效汇总报告，输出 Markdown 与 CSV，便于归档与复盘。

## 快速流程
1. 参考模板准备 `performance_input.json`。
2. 运行脚本生成 `performance_summary.md` 与 `performance_scores.csv`。
3. 根据评分结果校对权重与评语。

## 输入 JSON 结构
```json
{
  "employee": "张三",
  "period": "2026 Q1",
  "role": "后端工程师",
  "section_weights": {"okr": 0.6, "kpi": 0.3, "behavior": 0.1},
  "okr": [
    {
      "objective": "提升稳定性",
      "weight": 0.6,
      "key_results": [
        {"name": "故障数降低到 <=2", "weight": 0.6, "score": 0.9},
        {"name": "核心服务可用性 99.95%", "weight": 0.4, "score": 0.85}
      ]
    }
  ],
  "kpi": [
    {"name": "交付准时率", "weight": 0.5, "target": "95%", "actual": "96%", "score": 0.9},
    {"name": "缺陷密度", "weight": 0.5, "target": "<=0.3", "actual": "0.25", "score": 0.88}
  ],
  "behavior": [
    {"name": "协作与沟通", "weight": 1.0, "score": 0.95, "evidence": "跨团队推进发布流程"}
  ]
}
```

## 脚本用法
```bash
python scripts/performance_okr_kpi.py \
  --input examples/performance_input.json \
  --output-md examples/performance_summary.md \
  --output-csv examples/performance_scores.csv
```

## 输出说明
- `performance_summary.md`：汇总 OKR/KPI/行为评分与总评。
- `performance_scores.csv`：细项评分表，便于进一步分析。
- `templates/okr_kpi_template.md`：空白模板，可直接填写。
