---
name: 大数据处理
description: "通用数据分析技能：当需要基于 `db_query` 系列数据库工具或本地 Excel/CSV 文件做结构探测、聚合分析、趋势判断与专业图表方案设计时使用；适用于字段探索、全景分析、专题下钻与中文字段检索验证。"
---

# 核心目标
- 产出可复核的分析结论，而不是只给“看起来合理”的描述。
- 先分析再建议，不擅自修改数据库或原始文件。
- 仅基于当前可见资源工作：`db_query` 系列工具、或本地表格文件。

# 执行策略（弹性，不写死）
1) 先确认任务口径，再执行查询。  
2) 先探测字段和样本，再做聚合和趋势。  
3) 先覆盖核心维度，再按用户问题下钻。  
4) 每条结论都要附证据，缺字段就明确限制。  
5) 如果可以检索相关政策法规的，要优先去检索，不要盲目分析。

# “全面分析”默认最小覆盖包
当用户表达“全面分析/整体分析/给专业报告”时，若数据允许，至少覆盖 4 个维度：
- 规模与状态：总量、在职/离职（或等价状态）占比。
- 组织结构：按部门/团队分布，优先映射名称而不是只报 ID。
- 岗位结构：职位/职能 TopN 与占比。
- 时间结构：按月/季度趋势（如入职趋势、业务趋势）。
- 数值分布：薪资/金额等指标的均值、极值、离散度（支持时补充分位数）。

若任一维度缺字段，不跳过说明：在结果里写清“缺口字段 + 可替代口径 + 对结论影响”。

# 数据库分析指南（当可见 `db_query` 工具）
## A. 轻量字段探测
```sql
SELECT * FROM employees LIMIT 3;
```
```sql
SELECT * FROM departments LIMIT 3;
```

## B. 维度分析示例（按实际字段改名）
> 下面示例按中文字段演示；如果实际字段不同，先以探测结果为准。

### 1) 规模与状态
```sql
SELECT
  COUNT(*) AS 总人数,
  SUM(CASE WHEN `是否在职` = 1 THEN 1 ELSE 0 END) AS 在职人数,
  SUM(CASE WHEN `是否在职` = 0 THEN 1 ELSE 0 END) AS 离职人数
FROM employees;
```

### 2) 组织结构（优先映射部门名称）
```sql
SELECT
  e.`部门ID`,
  d.`部门名称`,
  COUNT(*) AS 人数,
  ROUND(AVG(CAST(e.`薪资` AS DECIMAL(12,2))), 2) AS 平均薪资
FROM employees e
LEFT JOIN departments d ON e.`部门ID` = d.`部门ID`
GROUP BY e.`部门ID`, d.`部门名称`
ORDER BY 人数 DESC
LIMIT 50;
```

### 3) 岗位结构
```sql
SELECT
  `职位`,
  COUNT(*) AS 人数
FROM employees
GROUP BY `职位`
ORDER BY 人数 DESC
LIMIT 20;
```

### 4) 时间结构（入职趋势）
```sql
SELECT
  DATE_FORMAT(`入职日期`, '%Y-%m') AS 月份,
  COUNT(*) AS 入职人数
FROM employees
GROUP BY 月份
ORDER BY 月份
LIMIT 240;
```

### 5) 数值分布（薪资）
```sql
SELECT
  ROUND(AVG(CAST(`薪资` AS DECIMAL(12,2))), 2) AS 平均薪资,
  ROUND(MIN(CAST(`薪资` AS DECIMAL(12,2))), 2) AS 最低薪资,
  ROUND(MAX(CAST(`薪资` AS DECIMAL(12,2))), 2) AS 最高薪资,
  ROUND(STDDEV_POP(CAST(`薪资` AS DECIMAL(12,2))), 2) AS 薪资标准差
FROM employees;
```

## C. 防撞墙规则（SQL）
- 单条 SQL 失败时，不重复硬撞：先简化再重试。
- 优先拆分查询，不强求一条 SQL 做完全部逻辑。
- 返回结果优先聚合表，限制行数（如 `LIMIT 20/50/200`）。
- 当方言不支持某函数时，先退回通用写法（分组、聚合、CASE）。

# 本地表格分析指南（当用户给 Excel/CSV）
## 1) 先体检
```bash
python {{SKILL_ROOT}}/scripts/dataset_inspector.py <文件路径> --output-json temp_dir/dataset_meta.json
```

## 2) 再分析与图表（可选）
```bash
python {{SKILL_ROOT}}/scripts/data_analysis_chart_runner.py <文件路径> --meta-json temp_dir/dataset_meta.json --out-dir temp_dir
```

## 3) 产物
- `*_analysis.png/.svg`
- `*_analysis_chart_data.xlsx`
- `*_analysis_summary.md`

# 图表输出标准（专业且节省上下文）
- “全面分析”默认建议至少 3 张图，覆盖趋势 + 结构 + 分布。
- 每张图必须写明：图类型、X/Y、聚合口径、筛选条件、一句话解读。
- 没有绘图能力时，输出“图表设计说明 + 对应聚合数据表头”，不伪造图片路径。
- 不把整段原始数据塞进回复，只给必要聚合结果和关键数字。
- 生成绘图脚本时，图表数据必须直接来自查询结果；不要手写“中间省略”的数组。

# 结论与证据规则
- 每条核心洞察后附“证据来源”（对应 SQL 或聚合结果）。
- 没有离职时间字段时，不下“离职趋势稳定/下降”结论。
- 没有部门名称映射时，明确“当前仅能按部门ID分析”。
- 存在口径风险时，显式写“假设与限制”。

# 上下文预算策略
- 同一指标口径只保留一次查询结果，避免重复查询和重复粘贴。
- 单个证据表默认不超过 20 行；超长结果只保留 TopN 或关键分段。
- 时间序列很长时，正文只放关键区间与汇总，完整序列作为附录口径说明。

# 推荐输出结构
1) 分析口径与数据范围  
2) 字段确认与数据质量  
3) 分维度结果（规模/结构/趋势/分布）  
4) 核心洞察（结论-证据配对）  
5) 图表方案（不少于 3 张，若用户要求全面）  
6) 限制与下一步验证建议  

