#!/usr/bin/env python3
# -*- coding: utf-8 -*-

"""Run spreadsheet analysis + chart generation with minimal manual mapping.

Expected workflow:
1) Run spreadsheet_inspector.py to produce metadata JSON.
2) Run this script with data file + metadata JSON.
3) Consume generated PNG/SVG, chart_data.xlsx, and summary.md.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import warnings
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional

import matplotlib as mpl
import matplotlib.pyplot as plt
import numpy as np
import pandas as pd


@dataclass
class RoleMap:
    id_col: Optional[str]
    time_col: Optional[str]
    category_cols: List[str]
    metric_cols: List[str]


def parse_csv_list(value: Optional[str]) -> List[str]:
    if not value:
        return []
    return [x.strip() for x in value.split(",") if x.strip()]


def read_meta(meta_json: Optional[Path]) -> Dict[str, Any]:
    if not meta_json:
        return {}
    if not meta_json.exists():
        return {}
    try:
        return json.loads(meta_json.read_text(encoding="utf-8"))
    except Exception:
        return {}


def select_sheet(meta: Dict[str, Any], forced_sheet: Optional[str]) -> Optional[str]:
    if forced_sheet:
        return forced_sheet
    sheets = meta.get("meta", {}).get("sheets", [])
    if not sheets:
        return None

    best_name = None
    best_score = -1
    best_rows = -1
    for s in sheets:
        profile = s.get("profile", {})
        roles = profile.get("role_candidates", {})
        score = (
            len(roles.get("metric_cols", [])) * 3
            + len(roles.get("category_cols", [])) * 2
            + len(roles.get("time_cols", []))
        )
        rows = int(s.get("sampled_rows", 0))
        if score > best_score or (score == best_score and rows > best_rows):
            best_score = score
            best_rows = rows
            best_name = s.get("name")
    return best_name


def _header_candidate_score(columns: List[str]) -> tuple[float, int, float, int]:
    if not columns:
        return (1.0, 10**9, 1.0, -1)
    col_s = pd.Series(columns)
    unnamed_ratio = float(col_s.str.lower().str.startswith("unnamed").mean())
    dup_count = int(col_s.duplicated().sum())
    numeric_like_ratio = float(col_s.str.fullmatch(r"\d+(\.\d+)?").fillna(False).mean())
    unique_count = int(col_s.nunique(dropna=False))
    return (unnamed_ratio, dup_count, numeric_like_ratio, -unique_count)


def detect_best_header_row(path: Path, sheet_name: str, sample_rows: int = 2000, max_scan_rows: int = 5) -> int:
    best_header = 0
    best_score: Optional[tuple[float, int, float, int]] = None
    for header_row in range(0, max_scan_rows + 1):
        try:
            df = pd.read_excel(
                path,
                sheet_name=sheet_name,
                nrows=sample_rows,
                dtype=object,
                engine="openpyxl",
                header=header_row,
            )
        except Exception:
            continue
        score = _header_candidate_score([str(c) for c in df.columns])
        if best_score is None or score < best_score:
            best_score = score
            best_header = header_row
    return best_header


def _safe_read_text_table(path: Path, meta: Dict[str, Any]) -> pd.DataFrame:
    meta_cfg = meta.get("meta", {})
    enc = meta_cfg.get("encoding", "utf-8-sig")
    sep = meta_cfg.get("delimiter", "," if path.suffix.lower() == ".csv" else "\t")
    tried = []
    for encoding in [enc, "utf-8-sig", "utf-8", "gb18030", "gbk", "latin1"]:
        try:
            return pd.read_csv(path, sep=sep, dtype=object, encoding=encoding, engine="python")
        except Exception as exc:
            tried.append(f"{encoding}:{exc}")
    raise RuntimeError(f"Failed to read text table: {' | '.join(tried[:3])}")


def load_dataframe(path: Path, sheet: Optional[str], meta: Dict[str, Any]) -> tuple[pd.DataFrame, Optional[str], int]:
    suffix = path.suffix.lower()
    if suffix in {".xlsx", ".xlsm", ".xltx", ".xltm", ".xls"}:
        if not sheet:
            xls = pd.ExcelFile(path)
            sheet = xls.sheet_names[0]
        sheet_meta = _meta_sheet_entry(meta, sheet)
        header_row = int(sheet_meta.get("header_row_detected", 0))
        df = pd.read_excel(path, sheet_name=sheet, dtype=object, engine="openpyxl", header=header_row)

        # If metadata is missing or stale and header still looks broken, auto-detect.
        cols = [str(c) for c in df.columns]
        unnamed_ratio = float(pd.Series(cols).str.lower().str.startswith("unnamed").mean()) if cols else 1.0
        if unnamed_ratio > 0.5:
            header_row = detect_best_header_row(path, sheet_name=sheet)
            df = pd.read_excel(path, sheet_name=sheet, dtype=object, engine="openpyxl", header=header_row)
        return df, sheet, header_row
    if suffix in {".csv", ".tsv"}:
        df = _safe_read_text_table(path, meta)
        return df, "table", 0
    raise ValueError(f"Unsupported file suffix: {suffix}")


def _meta_sheet_entry(meta: Dict[str, Any], sheet_name: Optional[str]) -> Dict[str, Any]:
    if not sheet_name:
        return {}
    for s in meta.get("meta", {}).get("sheets", []):
        if s.get("name") == sheet_name:
            return s
    return {}


def _numeric_ratio(series: pd.Series) -> float:
    non_null = series.dropna()
    if non_null.empty:
        return 0.0
    text = non_null.astype(str).str.replace(",", "", regex=False).str.replace("%", "", regex=False).str.strip()
    num = pd.to_numeric(text, errors="coerce")
    return float(num.notna().mean())


def _metric_score(df: pd.DataFrame, col: str) -> float:
    s = _to_numeric_clean(df[col]).dropna()
    if s.empty:
        return -1e9
    name = str(col).lower()
    score = 0.0

    # Prefer additive business-like metrics over demographics/rates.
    if any(k in name for k in ["amount", "revenue", "sales", "count", "qty", "volume", "hours", "days"]):
        score += 4.0
    if any(k in name for k in ["金额", "收入", "销量", "总", "数量", "人数", "时长", "天数", "次数"]):
        score += 4.0
    if any(k in name for k in ["age", "年龄", "编号", "id"]):
        score -= 3.0
    if any(k in name for k in ["rate", "ratio", "占比", "比例", "率"]):
        score -= 1.5

    # Data-based preference.
    positive_ratio = float((s >= 0).mean())
    uniq_ratio = float(s.nunique() / max(len(s), 1))
    std = float(s.std()) if len(s) > 1 else 0.0
    score += positive_ratio * 1.0
    score += min(std, 1000.0) / 1000.0
    score += (1.0 - abs(uniq_ratio - 0.3))  # mild preference for non-trivial spread
    return score


def _category_score(df: pd.DataFrame, col: str) -> float:
    s = df[col].dropna().astype(str)
    if s.empty:
        return -1e9
    nunique = int(s.nunique())
    total = int(s.shape[0])
    unique_ratio = nunique / max(total, 1)
    name = str(col).lower()
    score = 0.0

    # Prefer dimensions with manageable cardinality for chart readability.
    if 3 <= nunique <= 20:
        score += 5.0
    elif 2 <= nunique <= 40:
        score += 3.5
    elif nunique > 100:
        score -= 2.0

    if unique_ratio > 0.9:
        score -= 4.0
    if any(k in name for k in ["id", "编号", "姓名", "name"]):
        score -= 4.0
    if any(k in name for k in ["部门", "城市", "区域", "团队", "类别", "品类", "渠道"]):
        score += 2.0
    return score


def _time_ratio(series: pd.Series) -> float:
    non_null = series.dropna()
    if non_null.empty:
        return 0.0
    text = non_null.astype(str)
    has_date_tokens = float(text.str.contains(r"[-/年月日Tt:]", regex=True).mean())
    if has_date_tokens < 0.2:
        return 0.0
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", category=UserWarning)
        dt = pd.to_datetime(non_null, errors="coerce")
    return float(dt.notna().mean())


def infer_role_map(
    df: pd.DataFrame,
    meta: Dict[str, Any],
    sheet_name: Optional[str],
    id_col: Optional[str],
    time_col: Optional[str],
    category_cols: List[str],
    metric_cols: List[str],
) -> RoleMap:
    columns = [str(c) for c in df.columns]
    sheet_meta = _meta_sheet_entry(meta, sheet_name)
    candidates = sheet_meta.get("profile", {}).get("role_candidates", {})

    inferred_id = id_col
    inferred_time = time_col
    inferred_category = category_cols[:]
    inferred_metric = metric_cols[:]

    if not inferred_id:
        id_candidates = [c for c in candidates.get("id_cols", []) if c in columns]
        inferred_id = id_candidates[0] if id_candidates else None
    if not inferred_id:
        for c in columns:
            name = str(c)
            if not re.search(r"(id|编号|工号|员工号|订单号|流水号)", name, flags=re.IGNORECASE):
                continue
            s = df[c].dropna().astype(str)
            if s.empty:
                continue
            unique_ratio = float(s.nunique() / max(len(s), 1))
            if unique_ratio >= 0.7:
                inferred_id = c
                break

    if not inferred_time:
        time_candidates = [c for c in candidates.get("time_cols", []) if c in columns]
        if time_candidates:
            inferred_time = time_candidates[0]
        else:
            maybe_time = [c for c in columns if _time_ratio(df[c]) >= 0.8]
            inferred_time = maybe_time[0] if maybe_time else None

    if not inferred_category:
        category_candidates = [c for c in candidates.get("category_cols", []) if c in columns]
        if category_candidates:
            ranked = sorted(category_candidates, key=lambda c: _category_score(df, c), reverse=True)
            inferred_category = ranked[:2]
        else:
            guessed = []
            for c in columns:
                s = df[c]
                if _numeric_ratio(s) < 0.5:
                    nunique = s.dropna().nunique()
                    if 2 <= nunique <= 200:
                        guessed.append(c)
            ranked = sorted(guessed, key=lambda c: _category_score(df, c), reverse=True)
            inferred_category = ranked[:2]

    if not inferred_metric:
        metric_candidates = [c for c in candidates.get("metric_cols", []) if c in columns]
        if metric_candidates:
            ranked = sorted(metric_candidates, key=lambda c: _metric_score(df, c), reverse=True)
            inferred_metric = ranked[:3]
        else:
            guessed = [c for c in columns if _numeric_ratio(df[c]) >= 0.8]
            ranked = sorted(guessed, key=lambda c: _metric_score(df, c), reverse=True)
            inferred_metric = ranked[:3]

    inferred_category = [c for c in inferred_category if c in columns]
    inferred_metric = [c for c in inferred_metric if c in columns]
    if inferred_time and inferred_time not in columns:
        inferred_time = None
    if inferred_id and inferred_id not in columns:
        inferred_id = None

    if not inferred_metric:
        raise ValueError("Cannot infer metric column. Please pass --metric-cols explicitly.")
    if not inferred_category:
        # Fallback synthetic category to keep chart pipeline running.
        synthetic = "__all__"
        df[synthetic] = "All"
        inferred_category = [synthetic]

    return RoleMap(
        id_col=inferred_id,
        time_col=inferred_time,
        category_cols=inferred_category,
        metric_cols=inferred_metric,
    )


def _to_numeric_clean(series: pd.Series) -> pd.Series:
    text = series.astype(str).str.replace(",", "", regex=False).str.strip()
    has_pct = float(text.str.contains("%", regex=False).mean()) >= 0.3
    if has_pct:
        text = text.str.replace("%", "", regex=False)
    num = pd.to_numeric(text, errors="coerce")
    if has_pct:
        num = num / 100.0
    return num


def normalize(df: pd.DataFrame, role_map: RoleMap) -> pd.DataFrame:
    out = df.copy()
    if role_map.time_col:
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", category=UserWarning)
            out[role_map.time_col] = pd.to_datetime(out[role_map.time_col], errors="coerce")
    for col in role_map.metric_cols:
        out[col] = _to_numeric_clean(out[col])
    for col in role_map.category_cols:
        out[col] = out[col].fillna("Uncategorized").astype(str).str.strip()
    if role_map.id_col and role_map.id_col in out.columns:
        out = out.drop_duplicates(subset=[role_map.id_col])
    return out


def build_aggregates(
    df: pd.DataFrame,
    role_map: RoleMap,
    period: str,
    top_n: int,
) -> Dict[str, Any]:
    cat_col = role_map.category_cols[0]
    metric = role_map.metric_cols[0]

    work = df.copy()
    work = work[work[metric].notna()]
    if work.empty:
        raise ValueError(f"Metric column '{metric}' has no valid numeric values.")

    by_category = (
        work.groupby(cat_col, dropna=False)[metric]
        .sum()
        .reset_index()
        .sort_values(metric, ascending=False)
        .reset_index(drop=True)
    )

    by_category_top = by_category.copy()
    if top_n > 0 and by_category.shape[0] > top_n:
        head = by_category.iloc[:top_n].copy()
        tail_sum = float(by_category.iloc[top_n:][metric].sum())
        others = pd.DataFrame([{cat_col: "Others", metric: tail_sum}])
        by_category_top = pd.concat([head, others], ignore_index=True)

    share = by_category_top.copy()
    total_value = float(share[metric].sum())
    share["share_ratio"] = share[metric] / total_value if total_value else 0.0

    by_time = None
    heatmap = None
    if role_map.time_col and role_map.time_col in work.columns:
        tcol = role_map.time_col
        valid = work.dropna(subset=[tcol]).copy()
        if not valid.empty:
            valid["period_obj"] = valid[tcol].dt.to_period(period)
            by_time = (
                valid.groupby("period_obj")[metric]
                .sum()
                .reset_index()
                .sort_values("period_obj")
                .reset_index(drop=True)
            )
            by_time["period"] = by_time["period_obj"].astype(str)
            by_time = by_time[["period", metric]]

            # Heatmap: top categories x latest periods
            top_cat_values = by_category.iloc[: min(8, len(by_category))][cat_col].tolist()
            h = valid[valid[cat_col].isin(top_cat_values)].copy()
            pivot = pd.pivot_table(
                h,
                index=cat_col,
                columns="period_obj",
                values=metric,
                aggfunc="sum",
                fill_value=0.0,
            )
            if not pivot.empty:
                pivot = pivot.reindex(top_cat_values, fill_value=0.0)
                if pivot.shape[1] > 12:
                    pivot = pivot.iloc[:, -12:]
                pivot.columns = [str(x) for x in pivot.columns]
                heatmap = pivot

    return {
        "metric": metric,
        "category_col": cat_col,
        "by_category": by_category,
        "by_category_top": by_category_top,
        "share": share,
        "by_time": by_time,
        "heatmap": heatmap,
    }


def compute_insights(df: pd.DataFrame, agg: Dict[str, Any], role_map: RoleMap) -> List[str]:
    insights: List[str] = []
    metric = agg["metric"]
    cat_col = agg["category_col"]
    by_category = agg["by_category"]
    by_time = agg["by_time"]

    total = float(by_category[metric].sum()) if not by_category.empty else 0.0
    if not by_category.empty:
        top = by_category.iloc[0]
        ratio = (float(top[metric]) / total * 100.0) if total else 0.0
        insights.append(
            f"Top category is '{top[cat_col]}' with {top[metric]:,.2f} ({ratio:.2f}% of total {metric})."
        )

    if by_category.shape[0] >= 3:
        top3 = float(by_category.iloc[:3][metric].sum())
        top3_ratio = (top3 / total * 100.0) if total else 0.0
        insights.append(f"Top 3 categories contribute {top3_ratio:.2f}% of total {metric}.")

    if by_time is not None and by_time.shape[0] >= 2:
        first_v = float(by_time.iloc[0][metric])
        last_v = float(by_time.iloc[-1][metric])
        if abs(first_v) > 1e-12:
            change = (last_v - first_v) / abs(first_v) * 100.0
            direction = "up" if change >= 0 else "down"
            insights.append(
                f"Trend from first to last period is {direction} {abs(change):.2f}% ({first_v:,.2f} -> {last_v:,.2f})."
            )

    if len(role_map.metric_cols) >= 2:
        m2 = role_map.metric_cols[1]
        if m2 in df.columns:
            corr_df = df[[metric, m2]].dropna()
            if corr_df.shape[0] >= 10:
                corr = corr_df[metric].corr(corr_df[m2])
                if pd.notna(corr):
                    insights.append(f"Correlation between {metric} and {m2} is {corr:.3f}.")

    if not insights:
        insights.append("No strong numeric insight found from current mapping.")
    return insights[:5]


def configure_font() -> None:
    mpl.rcParams["font.sans-serif"] = [
        "Microsoft YaHei",
        "SimHei",
        "Noto Sans CJK SC",
        "Arial Unicode MS",
        "DejaVu Sans",
    ]
    mpl.rcParams["axes.unicode_minus"] = False


def _shorten_label(text: Any, max_len: int) -> str:
    s = str(text)
    if max_len <= 0 or len(s) <= max_len:
        return s
    if max_len <= 2:
        return s[:max_len]
    return f"{s[:max_len - 1]}…"


def _apply_sparse_xticks(ax: Any, labels: List[str], max_ticks: int, rotation: int = 35, ha: str = "right") -> None:
    n = len(labels)
    if n == 0:
        return
    max_ticks = max(2, max_ticks)
    step = max(1, math.ceil(n / max_ticks))
    idx = np.arange(0, n, step, dtype=int)
    if idx[-1] != n - 1:
        if (n - 1 - idx[-1]) < max(1, step // 2):
            idx[-1] = n - 1
        else:
            idx = np.append(idx, n - 1)
    ax.set_xticks(idx)
    ax.set_xticklabels([labels[i] for i in idx], rotation=rotation, ha=ha)


def draw_dashboard(
    agg: Dict[str, Any],
    df: pd.DataFrame,
    role_map: RoleMap,
    png_path: Path,
    svg_path: Path,
    max_x_ticks: int,
    label_max_len: int,
) -> None:
    configure_font()
    metric = agg["metric"]
    cat_col = agg["category_col"]
    by_category_top = agg["by_category_top"]
    by_time = agg["by_time"]
    heatmap = agg["heatmap"]

    fig, axes = plt.subplots(2, 2, figsize=(15, 10), dpi=140)
    fig.patch.set_facecolor("white")

    # Panel 1: bar chart
    ax = axes[0, 0]
    x = np.arange(len(by_category_top))
    y = by_category_top[metric].to_numpy(dtype=float)
    colors = plt.cm.Blues(np.linspace(0.45, 0.85, len(y)))
    bars = ax.bar(x, y, color=colors)
    ax.set_title(f"Top categories by {metric}")
    labels = [_shorten_label(v, label_max_len) for v in by_category_top[cat_col].astype(str).tolist()]
    ax.set_xticks(x)
    ax.set_xticklabels(labels, rotation=25, ha="right")
    ax.grid(axis="y", linestyle="--", alpha=0.25)
    show_bar_labels = len(bars) <= 12
    for b in bars:
        if not show_bar_labels:
            continue
        ax.text(
            b.get_x() + b.get_width() / 2,
            b.get_height(),
            f"{b.get_height():.0f}",
            ha="center",
            va="bottom",
            fontsize=8,
        )

    # Panel 2: trend or distribution
    ax = axes[0, 1]
    if by_time is not None and by_time.shape[0] >= 2:
        xt = np.arange(len(by_time))
        yt = by_time[metric].to_numpy(dtype=float)
        marker = "o" if len(by_time) <= 48 else None
        marker_size = 4 if len(by_time) <= 48 else 0
        ax.plot(xt, yt, color="#1f77b4", linewidth=2.0, marker=marker, markersize=marker_size)
        ax.fill_between(xt, yt, color="#1f77b4", alpha=0.18)
        period_labels = [_shorten_label(v, label_max_len) for v in by_time["period"].astype(str).tolist()]
        _apply_sparse_xticks(ax, period_labels, max_ticks=max_x_ticks, rotation=35, ha="right")
        ax.set_title(f"Trend of {metric}")
    else:
        vals = df[metric].dropna().to_numpy(dtype=float)
        ax.hist(vals, bins=min(30, max(8, int(math.sqrt(max(1, len(vals)))))), color="#4caf50", alpha=0.8)
        ax.set_title(f"Distribution of {metric}")
    ax.grid(linestyle="--", alpha=0.25)

    # Panel 3: donut share
    ax = axes[1, 0]
    share = agg["share"]
    vals = share[metric].to_numpy(dtype=float)
    labels_full = share[cat_col].astype(str).tolist()
    labels_short = [_shorten_label(v, label_max_len) for v in labels_full]
    pie_colors = plt.cm.viridis(np.linspace(0.2, 0.9, len(vals)))
    pie_show_labels = len(labels_short) <= 8
    wedges, _, autotexts = ax.pie(
        vals,
        labels=labels_short if pie_show_labels else None,
        autopct="%1.1f%%",
        startangle=90,
        pctdistance=0.78,
        colors=pie_colors,
        wedgeprops={"width": 0.42, "edgecolor": "white"},
        textprops={"fontsize": 8},
    )
    for t in autotexts:
        t.set_fontsize(7)
    if not pie_show_labels:
        ax.legend(
            wedges,
            labels_short,
            loc="center left",
            bbox_to_anchor=(1.0, 0.5),
            fontsize=7,
            frameon=False,
            title=_shorten_label(cat_col, max(8, label_max_len)),
            title_fontsize=8,
        )
    ax.set_title(f"Share of {metric}")

    # Panel 4: heatmap / scatter / fallback text
    ax = axes[1, 1]
    if heatmap is not None and heatmap.shape[0] > 0 and heatmap.shape[1] > 0:
        im = ax.imshow(heatmap.to_numpy(dtype=float), cmap="magma", aspect="auto")
        heat_labels = [_shorten_label(v, label_max_len) for v in heatmap.columns.astype(str).tolist()]
        _apply_sparse_xticks(ax, heat_labels, max_ticks=max_x_ticks, rotation=35, ha="right")
        ax.set_yticks(np.arange(len(heatmap.index)))
        ax.set_yticklabels([_shorten_label(v, label_max_len) for v in heatmap.index.astype(str).tolist()])
        ax.set_title(f"Heatmap ({cat_col} x period)")
        fig.colorbar(im, ax=ax, fraction=0.046, pad=0.04)
    elif len(role_map.metric_cols) >= 2:
        m2 = role_map.metric_cols[1]
        pts = df[[metric, m2]].dropna()
        if not pts.empty:
            ax.scatter(pts[metric], pts[m2], alpha=0.6, s=20, c="#ff7f0e", edgecolors="none")
            ax.set_xlabel(metric)
            ax.set_ylabel(m2)
            ax.set_title(f"Scatter: {metric} vs {m2}")
            ax.grid(linestyle="--", alpha=0.25)
        else:
            ax.text(0.5, 0.5, "No data for panel 4", ha="center", va="center")
            ax.set_axis_off()
    else:
        ax.text(0.5, 0.5, "No data for panel 4", ha="center", va="center")
        ax.set_axis_off()

    fig.suptitle("Spreadsheet Analysis Dashboard", fontsize=15, fontweight="bold")
    fig.tight_layout(rect=(0, 0, 1, 0.97))
    fig.savefig(png_path, bbox_inches="tight")
    fig.savefig(svg_path, bbox_inches="tight")
    plt.close(fig)


def write_chart_data(path: Path, agg: Dict[str, Any]) -> None:
    with pd.ExcelWriter(path, engine="openpyxl") as writer:
        agg["by_category"].to_excel(writer, sheet_name="by_category", index=False)
        agg["by_category_top"].to_excel(writer, sheet_name="by_category_top", index=False)
        agg["share"].to_excel(writer, sheet_name="share", index=False)
        if agg["by_time"] is not None:
            agg["by_time"].to_excel(writer, sheet_name="by_time", index=False)
        if agg["heatmap"] is not None:
            agg["heatmap"].reset_index().to_excel(writer, sheet_name="heatmap", index=False)


def write_summary(
    path: Path,
    input_path: Path,
    sheet_name: Optional[str],
    header_row: int,
    row_count: int,
    role_map: RoleMap,
    insights: List[str],
    outputs: Dict[str, Path],
    render_options: Dict[str, Any],
) -> None:
    lines = [
        "# Analysis Summary",
        "",
        "## Input",
        f"- file: `{input_path}`",
        f"- sheet: `{sheet_name}`",
        f"- header_row_detected: {header_row + 1}",
        f"- rows: {row_count}",
        "",
        "## Role Map",
        f"- id_col: `{role_map.id_col}`",
        f"- time_col: `{role_map.time_col}`",
        f"- category_cols: `{role_map.category_cols}`",
        f"- metric_cols: `{role_map.metric_cols}`",
        "",
        "## Key Insights",
    ]
    for i in insights:
        lines.append(f"- {i}")
    lines.extend(
        [
            "",
            "## Outputs",
            f"- png: `{outputs['png']}`",
            f"- svg: `{outputs['svg']}`",
            f"- chart_data: `{outputs['chart_data']}`",
            "",
            "## Render Options",
            f"- max_x_ticks: {render_options['max_x_ticks']}",
            f"- label_max_len: {render_options['label_max_len']}",
        ]
    )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def build_output_base_name(data_path: Path, sheet_name: Optional[str]) -> str:
    sheet_part = sheet_name if sheet_name else "table"
    safe_sheet = "".join(ch if ch.isalnum() or ch in ("-", "_") else "_" for ch in sheet_part)
    return f"{data_path.stem}_{safe_sheet}_analysis"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Auto analysis + chart runner for spreadsheets.")
    parser.add_argument("path", help="Input file path.")
    parser.add_argument("--meta-json", default=None, help="Metadata JSON generated by spreadsheet_inspector.py.")
    parser.add_argument("--sheet", default=None, help="Force specific sheet (Excel only).")
    parser.add_argument("--id-col", default=None, help="Force ID column.")
    parser.add_argument("--time-col", default=None, help="Force time column.")
    parser.add_argument("--category-cols", default=None, help="Force category columns, comma-separated.")
    parser.add_argument("--metric-cols", default=None, help="Force metric columns, comma-separated.")
    parser.add_argument("--top-n", type=int, default=10, help="Top-N categories for bar/share charts.")
    parser.add_argument("--period", default="M", help="Time period for trend aggregation. e.g. D/W/M/Q")
    parser.add_argument("--max-x-ticks", type=int, default=12, help="Maximum x-axis tick labels to display per chart.")
    parser.add_argument("--label-max-len", type=int, default=12, help="Maximum label text length before truncation.")
    parser.add_argument("--out-dir", default="temp_dir", help="Output directory.")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    path = Path(args.path)
    if not path.exists():
        raise FileNotFoundError(f"Input file not found: {path}")

    meta_json = Path(args.meta_json) if args.meta_json else None
    meta = read_meta(meta_json)

    sheet = select_sheet(meta, args.sheet)
    df, selected_sheet, header_row = load_dataframe(path, sheet, meta)
    if df.empty:
        raise ValueError("Input table is empty.")

    role_map = infer_role_map(
        df=df,
        meta=meta,
        sheet_name=selected_sheet,
        id_col=args.id_col,
        time_col=args.time_col,
        category_cols=parse_csv_list(args.category_cols),
        metric_cols=parse_csv_list(args.metric_cols),
    )

    normalized = normalize(df, role_map)
    agg = build_aggregates(normalized, role_map, period=args.period, top_n=max(1, args.top_n))
    insights = compute_insights(normalized, agg, role_map)

    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    base = build_output_base_name(path, selected_sheet)
    png_path = out_dir / f"{base}.png"
    svg_path = out_dir / f"{base}.svg"
    chart_data_path = out_dir / f"{base}_chart_data.xlsx"
    summary_path = out_dir / f"{base}_summary.md"

    draw_dashboard(
        agg,
        normalized,
        role_map,
        png_path,
        svg_path,
        max_x_ticks=max(2, args.max_x_ticks),
        label_max_len=max(4, args.label_max_len),
    )
    write_chart_data(chart_data_path, agg)
    write_summary(
        summary_path,
        input_path=path,
        sheet_name=selected_sheet,
        header_row=header_row,
        row_count=int(normalized.shape[0]),
        role_map=role_map,
        insights=insights,
        outputs={"png": png_path, "svg": svg_path, "chart_data": chart_data_path},
        render_options={"max_x_ticks": max(2, args.max_x_ticks), "label_max_len": max(4, args.label_max_len)},
    )

    print("analysis_done")
    print(f"input={path}")
    print(f"sheet={selected_sheet}")
    print(f"header_row_detected={header_row + 1}")
    print(f"role_map={role_map}")
    print(f"png={png_path}")
    print(f"svg={svg_path}")
    print(f"chart_data={chart_data_path}")
    print(f"summary={summary_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
