---
name: 报表汇总与图表
description: "基于 CSV 数据生成报表汇总与图表（SVG），输出 Markdown 报告。"
---

# 报表汇总与图表输出

## 概要
从 CSV 数据生成关键指标汇总，并输出可编辑的 SVG 图表与 Markdown 报告，便于汇报与归档。

## 快速流程
1. 准备 CSV 数据（至少包含数值列）。
2. 运行脚本生成 `report_summary.md` 与 `report_chart.svg`。
3. 根据业务需要微调摘要文字与图表标题。

## 输入 CSV 示例
```csv
date,category,value
2026-01-01,渠道A,120
2026-01-01,渠道B,80
2026-01-02,渠道A,150
```

## 输出说明
- `report_summary.md`：关键指标与分组统计。
- `report_chart.svg`：默认生成分组汇总柱状图（可直接用于 PPT）。

## 脚本用法
```bash
python scripts/report_summary.py \
  --input examples/sales_report.csv \
  --value-column value \
  --group-by category \
  --output-md examples/report_summary.md \
  --output-svg examples/report_chart.svg
```

## 注意事项
- 数值列为空或非数值时会自动跳过。
- 若需要时序图表，可将 `--group-by` 替换为 `--date-column`。
