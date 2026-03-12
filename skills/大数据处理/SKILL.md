---
name: 大数据处理
description: "通用数据分析技能：优先使用可见的 db_query 工具或本地表格文件完成只读分析，输出结论、图表方案与可复核的数据依据。"
---

# 核心定位
- 目标：快速完成**可复核的数据分析**与**专业图表设计建议**。
- 默认：先分析再建议，不擅自修改数据库或原始文件。
- 能力输入以模型“当前可见资源”为准，通常是：
  - `db_query` / `db_query_xxx` 工具
  - 本地 `.xlsx/.xlsm/.csv/.tsv` 文件

# 推荐执行策略（弹性，不要机械）
## 1) 先判断数据来源
- 若有 `db_query` 工具：优先走 SQL 聚合分析。
- 若用户给的是本地表格：优先做结构体检再分析。
- 若两者都有：先用数据库做主结论，再用本地文件做补充或交叉验证。

## 2) 先拿结构，再做统计
- 数据库：先做 1~2 条轻量探测 SQL（字段、样例、时间范围）。
- 表格：先运行 `dataset_inspector.py` 生成元信息，再进入分析。

## 3) 先聚合后细化
- 先拿总量、分组、趋势，再按需要下钻明细。
- 明细查询控制行数，避免把大批原始记录直接塞进上下文。

## 4) 结论要可追溯
- 每条关键结论附上“依据来源”（SQL 结果摘要或聚合表）。
- 对口径不确定的地方，显式写“假设/限制”，不要硬猜。

# 数据库分析指南（当可见 db_query 工具）
## 轻量探测模板
```sql
SELECT * FROM employees LIMIT 1
```
```sql
SELECT * FROM departments LIMIT 1
```

## 常用聚合模板（按需选用）
```sql
SELECT COUNT(*) AS total_count
FROM employees
```
```sql
SELECT
  department_id,
  COUNT(*) AS headcount,
  SUM(CASE WHEN is_active = 1 THEN 1 ELSE 0 END) AS active_count,
  ROUND(AVG(salary), 2) AS avg_salary
FROM employees
GROUP BY department_id
ORDER BY headcount DESC
LIMIT 200
```
```sql
SELECT
  DATE_FORMAT(hired_at, '%Y-%m') AS ym,
  COUNT(*) AS hires
FROM employees
GROUP BY ym
ORDER BY ym
LIMIT 240
```

## SQL 使用建议（避免撞墙）
- 优先单条 SQL、只读 SQL。
- 如果工具报“表限制/语法限制”，改为：
  - 分步查询（拆成两条或多条）
  - 简化函数（先不用复杂窗口函数）
  - 先聚合再关联（在模型侧做轻量映射）
- 不在一条 SQL 里追求“全做完”。

# 本地表格分析指南（当用户给 Excel/CSV）
## 先体检
```bash
python {{SKILL_ROOT}}/scripts/dataset_inspector.py <文件路径> --output-json temp_dir/dataset_meta.json
```

## 再分析出图（可选）
```bash
python {{SKILL_ROOT}}/scripts/data_analysis_chart_runner.py <文件路径> --meta-json temp_dir/dataset_meta.json --out-dir temp_dir
```

## 产物
- `*_analysis.png/.svg`
- `*_analysis_chart_data.xlsx`
- `*_analysis_summary.md`

# 输出模板（建议）
1. 分析范围与口径（时间窗、数据源、统计定义）
2. 关键指标（总量、结构、趋势）
3. 核心洞察（3~5 条）
4. 图表建议（图类型 + X/Y + 聚合口径 + 一句话解读）
5. 风险与后续（数据缺口、进一步验证建议）

# 异常处理（简洁版）
- 查询失败：先缩小 SQL 复杂度，再重试。
- 字段不确定：先探测样例，不猜列名。
- 返回过大：改聚合、降维度、加限制。
- 结果矛盾：先做口径校验，再给结论。

# 模型提示词
- 可直接注入：`skills/大数据处理/MCP_MODEL_PROMPT.md`

