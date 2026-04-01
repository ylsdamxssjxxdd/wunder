#!/usr/bin/env python3
# -*- coding: utf-8 -*-

from __future__ import annotations

import argparse
from pathlib import Path
from typing import Dict, List

import matplotlib as mpl
import matplotlib.pyplot as plt
import pandas as pd
import pymysql


def configure_plot_style() -> None:
    plt.style.use("seaborn-v0_8-whitegrid")
    mpl.rcParams["font.sans-serif"] = [
        "Microsoft YaHei",
        "SimHei",
        "Noto Sans CJK SC",
        "Arial Unicode MS",
        "DejaVu Sans",
    ]
    mpl.rcParams["axes.unicode_minus"] = False


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Build professional HR charts from MySQL personnel database.")
    parser.add_argument("--host", default="127.0.0.1", help="MySQL host.")
    parser.add_argument("--port", type=int, default=3307, help="MySQL port.")
    parser.add_argument("--user", default="root", help="MySQL username.")
    parser.add_argument("--password", default="rootpass123!", help="MySQL password.")
    parser.add_argument("--database", default="personnel", help="MySQL database.")
    parser.add_argument("--out-dir", default="temp_dir/mysql_personnel_dashboard", help="Output directory.")
    return parser.parse_args()


def load_data(args: argparse.Namespace) -> tuple[pd.DataFrame, pd.DataFrame]:
    conn = pymysql.connect(
        host=args.host,
        port=args.port,
        user=args.user,
        password=args.password,
        database=args.database,
        charset="utf8mb4",
    )
    try:
        with conn.cursor(pymysql.cursors.DictCursor) as cursor:
            cursor.execute(
                """
                SELECT
                    e.id,
                    e.name,
                    e.title,
                    e.hired_at,
                    e.salary,
                    e.is_active,
                    COALESCE(d.name, '未分配部门') AS department,
                    COALESCE(d.location, '未知地点') AS location
                FROM employees e
                LEFT JOIN departments d ON d.id = e.department_id
                """
            )
            employees = pd.DataFrame(cursor.fetchall())
            cursor.execute("SELECT id, name, location, budget FROM departments")
            departments = pd.DataFrame(cursor.fetchall())
    finally:
        conn.close()
    return employees, departments


def prepare_metrics(employees: pd.DataFrame) -> Dict[str, pd.DataFrame]:
    df = employees.copy()
    if df.empty:
        raise ValueError("employees 表为空，无法绘图。")

    df["hired_at"] = pd.to_datetime(df["hired_at"], errors="coerce")
    df["salary"] = pd.to_numeric(df["salary"], errors="coerce").fillna(0.0)
    df["is_active"] = pd.to_numeric(df["is_active"], errors="coerce").fillna(0).astype(int)
    today = pd.Timestamp.today().normalize()
    df["tenure_years"] = ((today - df["hired_at"]).dt.days / 365.25).clip(lower=0)
    df["active_label"] = df["is_active"].map({1: "在职/试用", 0: "离职"}).fillna("未知")

    dept_stats = (
        df.groupby("department", dropna=False)
        .agg(
            人员数=("id", "count"),
            在职人数=("is_active", "sum"),
            平均薪资=("salary", "mean"),
            中位薪资=("salary", "median"),
            薪资总额=("salary", "sum"),
        )
        .reset_index()
    )
    dept_stats["在职率"] = (dept_stats["在职人数"] / dept_stats["人员数"]).fillna(0.0)
    dept_stats = dept_stats.sort_values("人员数", ascending=False)

    hires = (
        df.dropna(subset=["hired_at"])
        .assign(月份=lambda x: x["hired_at"].dt.to_period("M").astype(str))
        .groupby("月份", dropna=False)["id"]
        .count()
        .reset_index(name="入职人数")
        .sort_values("月份")
    )

    title_top = (
        df.groupby("title", dropna=False)["id"]
        .count()
        .reset_index(name="人数")
        .sort_values("人数", ascending=False)
        .head(12)
    )

    location_stats = (
        df.groupby("location", dropna=False)["id"]
        .count()
        .reset_index(name="人数")
        .sort_values("人数", ascending=False)
    )

    tenure = df["tenure_years"].fillna(0)
    bins = [-0.1, 1, 3, 5, 8, 100]
    labels = ["≤1年", "1-3年", "3-5年", "5-8年", "8年以上"]
    tenure_dist = (
        pd.cut(tenure, bins=bins, labels=labels)
        .value_counts(dropna=False)
        .reindex(labels, fill_value=0)
        .reset_index()
    )
    tenure_dist.columns = ["司龄区间", "人数"]

    active_dist = (
        df.groupby("active_label", dropna=False)["id"]
        .count()
        .reset_index(name="人数")
        .sort_values("人数", ascending=False)
    )

    return {
        "employee_detail": df,
        "dept_stats": dept_stats,
        "hires": hires,
        "title_top": title_top,
        "location_stats": location_stats,
        "tenure_dist": tenure_dist,
        "active_dist": active_dist,
    }


def plot_overview(metrics: Dict[str, pd.DataFrame], out_png: Path, out_svg: Path) -> None:
    dept_stats = metrics["dept_stats"].copy()
    hires = metrics["hires"].copy()
    location_stats = metrics["location_stats"].copy().head(8)
    active_dist = metrics["active_dist"].copy()

    fig, axes = plt.subplots(2, 2, figsize=(16, 10), constrained_layout=True)
    fig.suptitle("人员结构总览（MySQL 数据）", fontsize=18, fontweight="bold")

    ax = axes[0, 0]
    ax.bar(dept_stats["department"], dept_stats["人员数"], color="#4C78A8", alpha=0.9, label="人员数")
    ax.set_title("部门人员规模")
    ax.set_xlabel("部门")
    ax.set_ylabel("人数")
    ax.tick_params(axis="x", rotation=25)
    ax2 = ax.twinx()
    ax2.plot(dept_stats["department"], dept_stats["在职率"] * 100, color="#F58518", marker="o", label="在职率(%)")
    ax2.set_ylabel("在职率(%)")
    ax2.set_ylim(0, 105)

    ax = axes[0, 1]
    if not hires.empty:
        ax.plot(hires["月份"], hires["入职人数"], color="#54A24B", linewidth=2)
        ax.fill_between(hires["月份"], hires["入职人数"], alpha=0.15, color="#54A24B")
        ax.set_title("月度入职趋势")
        ax.set_xlabel("月份")
        ax.set_ylabel("入职人数")
        tick_step = max(1, len(hires) // 12)
        for idx, label in enumerate(ax.get_xticklabels()):
            if idx % tick_step != 0:
                label.set_visible(False)
        ax.tick_params(axis="x", rotation=25)
    else:
        ax.text(0.5, 0.5, "无可用入职日期数据", ha="center", va="center")
        ax.set_axis_off()

    ax = axes[1, 0]
    ax.barh(location_stats["location"], location_stats["人数"], color="#B279A2", alpha=0.9)
    ax.set_title("办公地人数分布（Top8）")
    ax.set_xlabel("人数")
    ax.set_ylabel("办公地")

    ax = axes[1, 1]
    ax.pie(
        active_dist["人数"],
        labels=active_dist["active_label"],
        autopct="%1.1f%%",
        startangle=120,
        colors=["#72B7B2", "#E45756", "#A0A0A0"][: len(active_dist)],
        wedgeprops={"width": 0.45, "edgecolor": "white"},
    )
    ax.set_title("人员状态占比")

    fig.savefig(out_png, dpi=220)
    fig.savefig(out_svg)
    plt.close(fig)


def plot_compensation(metrics: Dict[str, pd.DataFrame], out_png: Path, out_svg: Path) -> None:
    dept_stats = metrics["dept_stats"].copy().sort_values("平均薪资", ascending=False)
    detail = metrics["employee_detail"].copy()
    title_top = metrics["title_top"].copy()
    tenure_dist = metrics["tenure_dist"].copy()

    fig, axes = plt.subplots(2, 2, figsize=(16, 10), constrained_layout=True)
    fig.suptitle("薪酬与岗位结构（MySQL 数据）", fontsize=18, fontweight="bold")

    ax = axes[0, 0]
    ax.bar(dept_stats["department"], dept_stats["平均薪资"], color="#4C78A8")
    ax.set_title("部门平均薪资")
    ax.set_xlabel("部门")
    ax.set_ylabel("平均薪资")
    ax.tick_params(axis="x", rotation=25)

    ax = axes[0, 1]
    ordered_depts: List[str] = detail.groupby("department")["salary"].mean().sort_values(ascending=False).index.tolist()
    salary_groups = [detail.loc[detail["department"] == dep, "salary"].dropna() for dep in ordered_depts]
    if salary_groups and any(len(group) > 0 for group in salary_groups):
        ax.boxplot(salary_groups, tick_labels=ordered_depts, patch_artist=True, showfliers=False)
        for patch in ax.artists:
            patch.set_facecolor("#72B7B2")
        ax.set_title("部门薪资分布（箱线图）")
        ax.set_xlabel("部门")
        ax.set_ylabel("薪资")
        ax.tick_params(axis="x", rotation=25)
    else:
        ax.text(0.5, 0.5, "无可用薪资数据", ha="center", va="center")
        ax.set_axis_off()

    ax = axes[1, 0]
    ax.barh(title_top["title"], title_top["人数"], color="#F58518")
    ax.set_title("岗位人数 Top12")
    ax.set_xlabel("人数")
    ax.set_ylabel("岗位名称")

    ax = axes[1, 1]
    ax.bar(tenure_dist["司龄区间"], tenure_dist["人数"], color="#54A24B")
    ax.set_title("司龄分布")
    ax.set_xlabel("司龄区间")
    ax.set_ylabel("人数")

    fig.savefig(out_png, dpi=220)
    fig.savefig(out_svg)
    plt.close(fig)


def write_outputs(
    metrics: Dict[str, pd.DataFrame],
    departments: pd.DataFrame,
    out_dir: Path,
    output_files: Dict[str, Path],
) -> None:
    xlsx_path = output_files["xlsx"]
    with pd.ExcelWriter(xlsx_path, engine="openpyxl") as writer:
        departments.to_excel(writer, sheet_name="departments", index=False)
        metrics["dept_stats"].to_excel(writer, sheet_name="dept_stats", index=False)
        metrics["hires"].to_excel(writer, sheet_name="monthly_hires", index=False)
        metrics["title_top"].to_excel(writer, sheet_name="title_top", index=False)
        metrics["location_stats"].to_excel(writer, sheet_name="location_stats", index=False)
        metrics["tenure_dist"].to_excel(writer, sheet_name="tenure_dist", index=False)
        metrics["active_dist"].to_excel(writer, sheet_name="active_dist", index=False)

    detail = metrics["employee_detail"]
    dept_stats = metrics["dept_stats"]
    total = int(len(detail))
    active = int(detail["is_active"].sum())
    inactive = total - active
    active_rate = active / total if total else 0.0
    avg_salary = float(detail["salary"].mean()) if total else 0.0
    median_salary = float(detail["salary"].median()) if total else 0.0
    top_dept = dept_stats.iloc[0]["department"] if not dept_stats.empty else "N/A"
    top_dept_count = int(dept_stats.iloc[0]["人员数"]) if not dept_stats.empty else 0
    top_salary_dept = (
        dept_stats.sort_values("平均薪资", ascending=False).iloc[0]["department"] if not dept_stats.empty else "N/A"
    )
    latest_hires = metrics["hires"].iloc[-1]["入职人数"] if not metrics["hires"].empty else 0
    latest_month = metrics["hires"].iloc[-1]["月份"] if not metrics["hires"].empty else "N/A"

    lines = [
        "# MySQL 人员数据分析摘要",
        "",
        "## 数据范围",
        f"- 数据源：`{detail.shape[0]}` 条人员记录，`{departments.shape[0]}` 条部门记录。",
        "- 数据库：`personnel`（表：`employees`、`departments`）",
        "",
        "## 核心指标",
        f"- 人员总数：**{total}**",
        f"- 在职/试用：**{active}**，离职：**{inactive}**，在职率：**{active_rate:.2%}**",
        f"- 平均薪资：**{avg_salary:,.2f}**，中位薪资：**{median_salary:,.2f}**",
        f"- 人员最多部门：**{top_dept}**（{top_dept_count} 人）",
        f"- 平均薪资最高部门：**{top_salary_dept}**",
        f"- 最近入职月份：**{latest_month}**，入职人数：**{latest_hires}**",
        "",
        "## 输出文件",
        f"- 总览看板 PNG：`{output_files['overview_png']}`",
        f"- 总览看板 SVG：`{output_files['overview_svg']}`",
        f"- 薪酬结构 PNG：`{output_files['comp_png']}`",
        f"- 薪酬结构 SVG：`{output_files['comp_svg']}`",
        f"- 聚合数据 Excel：`{output_files['xlsx']}`",
    ]
    summary_path = output_files["summary"]
    summary_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    out_dir.mkdir(parents=True, exist_ok=True)


def main() -> int:
    args = parse_args()
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)

    configure_plot_style()
    employees, departments = load_data(args)
    metrics = prepare_metrics(employees)

    output_files = {
        "overview_png": out_dir / "personnel_dashboard_overview.png",
        "overview_svg": out_dir / "personnel_dashboard_overview.svg",
        "comp_png": out_dir / "personnel_dashboard_compensation.png",
        "comp_svg": out_dir / "personnel_dashboard_compensation.svg",
        "xlsx": out_dir / "personnel_analysis_data.xlsx",
        "summary": out_dir / "personnel_analysis_summary.md",
    }

    plot_overview(metrics, output_files["overview_png"], output_files["overview_svg"])
    plot_compensation(metrics, output_files["comp_png"], output_files["comp_svg"])
    write_outputs(metrics, departments, out_dir, output_files)

    print("analysis_done")
    print(f"overview_png={output_files['overview_png']}")
    print(f"overview_svg={output_files['overview_svg']}")
    print(f"compensation_png={output_files['comp_png']}")
    print(f"compensation_svg={output_files['comp_svg']}")
    print(f"data_xlsx={output_files['xlsx']}")
    print(f"summary_md={output_files['summary']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
