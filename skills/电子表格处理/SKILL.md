---
name: 电子表格处理
description: "通用电子表格处理技能：面向 .xlsx/.xlsm/.csv/.tsv 的读取、清洗、分析、回写与可视化（含 matplotlib 高质量图表）。"
---

# 适用范围
- 新建或修改电子表格。
- 读取并分析结构未知或字段命名不统一的数据。
- 生成统计图、趋势图、占比图、组合图，并导出到图片或回写 Excel。

# 关键原则
1. **先元信息，后处理**：先读取表格元信息（sheet、列、类型、缺失、样本），再决定字段映射和计算逻辑。
2. **字段按“角色”映射，不写死列名**：用 `id_col/time_col/category_cols/metric_cols` 这类角色驱动流程。
3. **先聚合再出图**：图表引用聚合结果，不直接对明细全量作图。
4. **可复现**：相同输入与参数，产出应一致。

# 标准流程（建议照此执行）
1. 获取元信息（结构体检）。
2. 建立字段角色映射（用户确认口径）。
3. 清洗与标准化（类型、缺失、时间、异常值）。
4. 计算指标与聚合（明细 -> 统计表）。
5. 绘图（matplotlib）。
6. 输出（PNG/SVG/Excel）。
7. 质量校验（口径、范围、空值、可读性）。

# 1) 元信息获取（必须先做）
```python
from pathlib import Path
import pandas as pd

def inspect_table(path: str, sheet: str | None = None, sample_n: int = 5):
    p = Path(path)
    if p.suffix.lower() in {".xlsx", ".xlsm"}:
        xls = pd.ExcelFile(path)
        target_sheet = sheet or xls.sheet_names[0]
        df = pd.read_excel(path, sheet_name=target_sheet, nrows=2000)
        meta = {"file_type": p.suffix.lower(), "sheet_names": xls.sheet_names, "active_sheet": target_sheet}
    else:
        df = pd.read_csv(path, nrows=2000)
        meta = {"file_type": p.suffix.lower(), "sheet_names": [], "active_sheet": None}

    profile = pd.DataFrame({
        "column": df.columns,
        "dtype": [str(t) for t in df.dtypes],
        "null_ratio": df.isna().mean().round(4).values,
        "sample": [str(df[c].dropna().head(1).iloc[0]) if df[c].dropna().shape[0] else "" for c in df.columns],
    })
    return meta, profile

# 用法
# meta, profile = inspect_table("input.xlsx")
# print(meta)
# print(profile)
```

# 2) 字段角色映射（通用模板）
不要直接假设列名；先把“业务字段”映射到“角色字段”。

```python
role_map = {
    "id_col": "...",              # 主键/人员ID/订单号
    "time_col": "...",            # 日期/月份/时间戳（可选）
    "category_cols": ["..."],     # 部门/区域/产品线等分类字段
    "metric_cols": ["..."],       # 需要统计的数值字段
}
```

当字段名不一致时，优先做“候选别名表”，再由用户确认最终映射。

# 3) 清洗与标准化（通用模板）
```python
import pandas as pd

def normalize(df: pd.DataFrame, role_map: dict) -> pd.DataFrame:
    out = df.copy()

    # 时间列
    t = role_map.get("time_col")
    if t:
        out[t] = pd.to_datetime(out[t], errors="coerce")

    # 数值列
    for col in role_map.get("metric_cols", []):
        out[col] = pd.to_numeric(out[col], errors="coerce").fillna(0.0)

    # 分类列
    for col in role_map.get("category_cols", []):
        out[col] = out[col].fillna("未分类").astype(str).str.strip()

    # 去重（如果有主键）
    k = role_map.get("id_col")
    if k and k in out.columns:
        out = out.drop_duplicates(subset=[k])

    return out
```

# 4) 聚合（示例）
```python
def build_aggregates(df, role_map):
    cat = role_map["category_cols"][0]
    metric = role_map["metric_cols"][0]

    # 分类汇总
    by_cat = df.groupby(cat, dropna=False)[metric].sum().reset_index()
    by_cat = by_cat.sort_values(metric, ascending=False)

    # 时间趋势（若有）
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

# 5) matplotlib 高质量图表（详细示例）
下面示例演示：**渐变柱状图 + 趋势折线图 + 环形占比图 + 热力图**（2x2 面板）。

```python
import matplotlib.pyplot as plt
import matplotlib as mpl
import numpy as np

def draw_dashboard(by_cat, by_time, share_df, heat_values, heat_x, heat_y, out_png="dashboard.png"):
    # 主题（深色风格）
    plt.style.use("dark_background")
    mpl.rcParams["figure.dpi"] = 140
    mpl.rcParams["axes.unicode_minus"] = False

    fig = plt.figure(figsize=(16, 10), constrained_layout=True)
    gs = fig.add_gridspec(2, 2)

    # 1) 渐变柱状图
    ax1 = fig.add_subplot(gs[0, 0])
    x = np.arange(len(by_cat))
    y = by_cat.iloc[:, 1].to_numpy()
    colors = plt.cm.viridis(np.linspace(0.2, 0.95, len(y)))
    bars = ax1.bar(x, y, color=colors, edgecolor="white", linewidth=0.6)
    ax1.set_title("Top Categories", fontsize=13, fontweight="bold")
    ax1.set_xticks(x)
    ax1.set_xticklabels(by_cat.iloc[:, 0].astype(str), rotation=35, ha="right")
    ax1.grid(axis="y", alpha=0.25, linestyle="--")
    for b in bars:
        ax1.text(b.get_x() + b.get_width() / 2, b.get_height(), f"{b.get_height():.0f}",
                 ha="center", va="bottom", fontsize=8)

    # 2) 趋势折线图（含面积）
    ax2 = fig.add_subplot(gs[0, 1])
    if by_time is not None and len(by_time) > 0:
        xt = np.arange(len(by_time))
        yt = by_time.iloc[:, 1].to_numpy()
        ax2.plot(xt, yt, color="#4cc9f0", linewidth=2.6, marker="o")
        ax2.fill_between(xt, yt, color="#4cc9f0", alpha=0.2)
        ax2.set_xticks(xt)
        ax2.set_xticklabels(by_time.iloc[:, 0].astype(str), rotation=35, ha="right")
    ax2.set_title("Trend", fontsize=13, fontweight="bold")
    ax2.grid(alpha=0.25, linestyle="--")

    # 3) 环形占比图（Donut）
    ax3 = fig.add_subplot(gs[1, 0])
    vals = share_df.iloc[:, 1].to_numpy()
    labels = share_df.iloc[:, 0].astype(str).to_list()
    pie_colors = plt.cm.plasma(np.linspace(0.15, 0.9, len(vals)))
    wedges, texts, autotexts = ax3.pie(
        vals, labels=labels, autopct="%1.1f%%", startangle=90,
        colors=pie_colors, pctdistance=0.78, wedgeprops={"width": 0.42, "edgecolor": "black"}
    )
    for t in autotexts:
        t.set_fontsize(8)
    ax3.set_title("Share", fontsize=13, fontweight="bold")

    # 4) 热力图（二维矩阵）
    ax4 = fig.add_subplot(gs[1, 1])
    im = ax4.imshow(heat_values, cmap="magma", aspect="auto")
    ax4.set_xticks(np.arange(len(heat_x)))
    ax4.set_xticklabels(heat_x, rotation=35, ha="right")
    ax4.set_yticks(np.arange(len(heat_y)))
    ax4.set_yticklabels(heat_y)
    ax4.set_title("Heatmap", fontsize=13, fontweight="bold")
    fig.colorbar(im, ax=ax4, fraction=0.046, pad=0.04)

    fig.suptitle("Analytics Dashboard", fontsize=16, fontweight="bold")
    fig.savefig(out_png, bbox_inches="tight")
    plt.close(fig)
```

# 6) 导出建议
- 图表：同时导出 `PNG + SVG`（汇报 + 二次编辑）。
- 表格：保留 `raw_data`、`chart_data`、`charts` 三层。
- 命名：`<主题>_<YYYYMMDD>_v1`，便于版本管理。

# 性能建议（大数据时启用）
- `usecols` 只读必要列。
- 明确 `dtype`，减少推断开销。
- 超大 CSV 用 `chunksize` 分块聚合。
- Excel 大文件优先“读分析 -> 写结果”两段式，不在明细层做重操作。

# 交付检查清单
- [ ] 元信息与字段映射已明确（可复查）。
- [ ] 统计口径有说明（分母/时间窗口/去重规则）。
- [ ] 图表引用的是聚合层而非明细全列。
- [ ] 图表标题、轴标签、单位完整。
- [ ] 输出文件/图片可复现，且无明显异常值误导。

# 常见失败点
- 没先做元信息检查，直接假设列名。
- 口径混用（全量 vs 去重 vs 在岗等）。
- 图表直接绑明细层，导致卡顿和范围错误。
