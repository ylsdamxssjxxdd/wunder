---
name: 电子表格处理
description: "分析与可视化优先的电子表格处理技能：面向 .xlsx/.xlsm/.csv/.tsv 的结构体检、聚合分析、图表输出（PNG/SVG）。默认不修复原表。"
---

# 核心定位（重要）
- 本技能默认目标：**分析数据 + 绘制图表**。
- 默认模式：**只读分析模式（read-only）**，不修改原始表格。
- 仅当用户明确说“修复/回写/改表”时，才进入修复模式。

# 推荐先用脚本做体检（强烈建议）
先运行：
```bash
python skills/电子表格处理/scripts/spreadsheet_inspector.py <文件路径> --output-json temp_dir/spreadsheet_meta.json
```
脚本会自动：
- 检测文件是否可读、是否疑似加密/损坏/格式不匹配；
- 输出 sheet 级元信息、列画像、字段角色候选；
- 给出可执行建议，降低模型在未知结构下的误判风险。

# 一键分析出图脚本（推荐）
在体检后直接运行：
```bash
python skills/电子表格处理/scripts/analysis_chart_runner.py <文件路径> --meta-json temp_dir/spreadsheet_meta.json --out-dir temp_dir
```
默认产物：
- `*_analysis.png`：主看板 PNG
- `*_analysis.svg`：可编辑矢量图
- `*_analysis_chart_data.xlsx`：聚合层数据
- `*_analysis_summary.md`：字段映射、口径与洞察

常用参数：
```bash
# 指定 sheet
--sheet 人员信息

# 手动指定角色字段（逗号分隔）
--time-col 入职日期
--category-cols 一级部门,二级部门
--metric-cols 本月学习时长(小时),本月加班时长(小时)

# TopN 与时间粒度
--top-n 10
--period M

# 文本拥挤控制（强烈建议保留默认）
--max-x-ticks 12
--label-max-len 12
```

图表防拥挤策略（analysis_chart_runner 内置）：
- 时间轴标签过多时自动稀疏采样显示，不再全量挤在一起；
- 标签自动截断（保留前缀 + `…`）；
- 点位过多时趋势线自动去掉 marker；
- 环形图类别过多时自动改为图例展示，避免文字重叠。

# 适用范围
- 结构未知或字段命名不统一的电子表格分析。
- 生成统计摘要、洞察、图表看板（柱状图/折线图/占比图/热力图/散点图）。
- 输出 `PNG/SVG` 图片与 `chart_data` 聚合表。

# 硬性原则（必须遵守）
1. 先体检，后分析：先输出元信息，再做图。
2. 先映射，后计算：先确定字段角色，不写死列名。
3. 先聚合，后绘图：禁止直接用全量明细作图。
4. 先结论，后美化：先保证口径正确，再优化视觉。
5. 可复现：同输入 + 同参数 => 同输出。

# 机械执行流程（低能力模型也可照做）

## Step 0. 任务判定（30 秒）
输出并确认以下内容：
- 任务类型：`分析出图` / `修复回写` / `混合`。
- 目标文件：输入路径、输出路径。
- 是否允许修改原表：默认 `否`。

若用户未明确，默认：
- 只读分析；
- 输出图表和分析结论，不修改源文件。

## Step 1. 元信息体检（必须）
至少输出：
- 文件类型、sheet 列表、行列数。
- 每列 `dtype/null_ratio/sample`。
- 候选字段：时间列、分类列、数值列。

最小代码：
```python
from pathlib import Path
import pandas as pd

def inspect_table(path: str, sheet: str | None = None, nrows: int = 3000):
    p = Path(path)
    if p.suffix.lower() in {".xlsx", ".xlsm"}:
        xls = pd.ExcelFile(path)
        active = sheet or xls.sheet_names[0]
        df = pd.read_excel(path, sheet_name=active, nrows=nrows)
        meta = {"file_type": p.suffix.lower(), "sheet_names": xls.sheet_names, "active_sheet": active}
    else:
        df = pd.read_csv(path, nrows=nrows)
        meta = {"file_type": p.suffix.lower(), "sheet_names": [], "active_sheet": None}
    profile = pd.DataFrame({
        "column": df.columns,
        "dtype": [str(t) for t in df.dtypes],
        "null_ratio": df.isna().mean().round(4).values,
        "sample": [str(df[c].dropna().head(1).iloc[0]) if df[c].notna().any() else "" for c in df.columns],
    })
    return meta, profile
```

## Step 2. 字段角色映射（必须）
先给出角色映射，再分析：
```python
role_map = {
    "id_col": "...",              # 可选：主键/ID
    "time_col": "...",            # 可选：日期/月份
    "category_cols": ["..."],     # 至少 1 个分类维度
    "metric_cols": ["..."],       # 至少 1 个数值指标
}
```

若列名不一致：
- 先给候选映射 + 置信度；
- 低置信度映射（<0.8）先向用户确认再继续。

## Step 3. 标准化（最小必要）
- 时间列：`to_datetime(errors="coerce")`
- 数值列：`to_numeric(errors="coerce")`
- 分类列：`fillna("未分类").str.strip()`
- 不在原表上改，始终 `df.copy()`

## Step 4. 聚合层构建（必须）
至少构建两层：
- `by_category`：按主分类汇总指标；
- `by_time`：若有时间列，按日/周/月汇总趋势。

可选：
- `pivot_heatmap`：二维透视；
- `top_n`：TopN 贡献分析。

## Step 5. 图表类型自动决策（推荐）
按字段类型选图：
- 分类 + 单指标：柱状图（TopN）
- 时间 + 单指标：折线图（趋势）
- 分类占比：饼图/环形图（类别 <= 8）
- 二维矩阵：热力图
- 两指标关系：散点图

禁止：
- 类别 > 20 仍全部画柱图（应改 TopN + Others）
- 明细点过多直接散点（应采样或聚合）

## Step 6. 绘图与导出
输出建议：
- 图片：`PNG + SVG`
- 数据：`chart_data.xlsx`（仅聚合层）
- 报告：`summary.md`（口径 + 核心结论）

图表最低规范：
- 标题、坐标轴、单位齐全；
- 标签不重叠（旋转/换行/缩短）；
- 中文字体可显示；
- 色板统一，避免过饱和。

## Step 7. 质量校验（必须）
发布前检查：
- 图表值是否可追溯到聚合表；
- 占比图总和是否约等于 100%；
- 趋势图时间是否连续、有序；
- 是否存在异常值误导（极端值未标注）；
- 输出文件是否实际生成成功。

## Step 8. 交付格式（固定模板）
最终答复至少包含：
1) 字段映射结果  
2) 统计口径（分母、时间窗口、去重规则）  
3) 生成文件列表（带路径）  
4) 3~5 条关键洞察  
5) 风险与假设（若有）

# 特殊情况/错误处理手册（必须遵守）

## A. 文件打不开/读取失败
- 先检查路径、后缀、权限。
- Excel 尝试 `engine="openpyxl"`；CSV 尝试 `encoding="utf-8-sig"` 再 `gbk`。
- 仍失败：返回“无法读取原因 + 建议下一步”，不要瞎推断。

## B. 列名乱码/中文路径异常
- 使用 UTF-8 环境；
- Python 中优先 `Path` 对象，不手写转义路径；
- 禁止用“????”列名继续分析。
- 若出现大量 `Unnamed:*` 列，优先运行 `spreadsheet_inspector.py` 与 `analysis_chart_runner.py`，两者内置“标题行自动探测”（可识别首行是标题/说明文本导致的表头偏移）。

## C. 没有时间列
- 不做趋势图；
- 改做分类对比 + 占比 + 分布图，并在报告中说明“无时间维度”。

## D. 数值列是字符串（如 `12,345`、`37%`）
- 先去逗号和 `%` 再 `to_numeric`；
- 百分比统一转换口径（0~1 或 0~100），并在报告写明。

## E. 类别太多（>20）
- 默认 Top10 + Others；
- 或按业务分层（一级部门 -> 二级部门）。

## F. 图表中文乱码
- 设置字体回退链，如：
```python
import matplotlib as mpl
mpl.rcParams["font.sans-serif"] = ["Microsoft YaHei", "SimHei", "Noto Sans CJK SC", "Arial Unicode MS", "DejaVu Sans"]
mpl.rcParams["axes.unicode_minus"] = False
```

## G. 数据太大导致慢/内存高
- `usecols` 只读必要列；
- `dtype` 显式指定；
- CSV 用 `chunksize` 分块聚合；
- 先抽样做探索，再全量跑最终聚合。

## H. 用户需求模糊
- 必问 3 件事：时间窗口、核心指标、目标受众（管理层/业务/技术）。
- 若用户不回复：使用默认口径并明确写出假设。

# 通用代码骨架（分析出图模式）
```python
import pandas as pd

def normalize(df: pd.DataFrame, role_map: dict) -> pd.DataFrame:
    out = df.copy()
    if role_map.get("time_col"):
        out[role_map["time_col"]] = pd.to_datetime(out[role_map["time_col"]], errors="coerce")
    for col in role_map.get("metric_cols", []):
        out[col] = pd.to_numeric(out[col], errors="coerce")
    for col in role_map.get("category_cols", []):
        out[col] = out[col].fillna("未分类").astype(str).str.strip()
    return out

def build_aggregates(df: pd.DataFrame, role_map: dict):
    cat = role_map["category_cols"][0]
    metric = role_map["metric_cols"][0]
    by_cat = df.groupby(cat, dropna=False)[metric].sum().reset_index().sort_values(metric, ascending=False)
    by_time = None
    t = role_map.get("time_col")
    if t:
        by_time = (
            df.dropna(subset=[t])
              .assign(period=df[t].dt.to_period("M").astype(str))
              .groupby("period", dropna=False)[metric]
              .sum()
              .reset_index()
        )
    return by_cat, by_time
```

# 交付检查清单
- [ ] 已明确任务是“分析出图”还是“修复回写”。
- [ ] 已输出元信息和字段角色映射。
- [ ] 图表全部来自聚合层而非明细层。
- [ ] 每张图有标题/轴标签/单位。
- [ ] 已导出 PNG（推荐同时导出 SVG）。
- [ ] 已输出口径说明与关键结论。

# 修复模式声明（非默认）
仅当用户明确要求“修复统计值/回写 Excel”时启用，且必须：
1) 先输出差异报告（原值/应值/依据）；  
2) 再执行回写；  
3) 保留新文件，不覆盖原始文件。  
