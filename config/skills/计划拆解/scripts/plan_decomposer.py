#!/usr/bin/env python3
"""
计划拆解 — 工具脚本
将计划目标拆解为层级任务、责任矩阵、资源匹配、风险预案

目标用户: 单位管理者、计划制定者、项目管理者
输出产物: 计划文档、任务分解表、责任矩阵、资源分析、风险预案
"""

import sys
import json
import os
import argparse
from datetime import datetime, timedelta
from typing import Dict, List, Any, Optional

try:
    import pandas as pd
    HAS_PANDAS = True
except ImportError:
    HAS_PANDAS = False

DATA_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")


def ensure_dirs():
    os.makedirs(DATA_DIR, exist_ok=True)


def generate_quarter_milestones(year: int, goals: List[Dict]) -> Dict[str, List[Dict]]:
    """Generate quarterly milestones from annual goals."""
    quarters = {
        "Q1": {"start": f"{year}-01-01", "end": f"{year}-03-31", "milestones": []},
        "Q2": {"start": f"{year}-04-01", "end": f"{year}-06-30", "milestones": []},
        "Q3": {"start": f"{year}-07-01", "end": f"{year}-09-30", "milestones": []},
        "Q4": {"start": f"{year}-10-01", "end": f"{year}-12-31", "milestones": []},
    }

    for i, goal in enumerate(goals):
        quarter_idx = i % 4
        quarter_key = f"Q{quarter_idx + 1}"
        quarters[quarter_key]["milestones"].append({
            "name": f"{goal.get('name', f'目标{i+1}')} - 阶段性成果",
            "deliverable": goal.get("deliverable", "待明确"),
            "owner": goal.get("owner", "待分配"),
        })

    return quarters


def generate_project_phases(goals: List[Dict], start_date: str, end_date: str) -> List[Dict]:
    """Generate project phases from goals."""
    phases = [
        {"name": "启动阶段", "tasks": [], "deliverables": []},
        {"name": "实施阶段", "tasks": [], "deliverables": []},
        {"name": "收尾阶段", "tasks": [], "deliverables": []},
    ]

    for i, goal in enumerate(goals):
        phase_idx = min(i % 3, 2)
        phases[phase_idx]["tasks"].append({
            "name": goal.get("name", f"任务{i+1}"),
            "owner": goal.get("owner", "待分配"),
            "deliverable": goal.get("deliverable", "待明确"),
        })

    return phases


def generate_monthly_tasks(year: int, goals: List[Dict]) -> List[Dict]:
    """Generate monthly task breakdown."""
    tasks = []
    months = ["01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12"]

    for goal in goals:
        goal_name = goal.get("name", "未命名目标")
        for i, month in enumerate(months):
            tasks.append({
                "month": f"{year}-{month}",
                "goal": goal_name,
                "task": f"{goal_name} - 月度任务{i+1}",
                "owner": goal.get("owner", "待分配"),
                "status": "待启动",
                "priority": "P1" if i < 6 else "P2",
            })

    return tasks


def generate_raci_matrix(tasks: List[Dict]) -> List[Dict]:
    """Generate RACI responsibility matrix."""
    raci = []
    for task in tasks:
        task_name = task.get("task", "") or task.get("name", "")
        raci.append({
            "task": task_name,
            "responsible": task.get("owner", "待分配"),
            "accountable": "待指定",
            "consulted": "待指定",
            "informed": "待指定",
        })
    return raci


def generate_resource_analysis(goals: List[Dict]) -> Dict[str, Any]:
    """Analyze resource requirements."""
    return {
        "human_resources": {
            "total_person_days": sum(g.get("person_days", 0) for g in goals),
            "by_department": {},
            "gap_analysis": "需根据实际情况补充",
        },
        "budget": {
            "total": sum(g.get("budget", 0) for g in goals),
            "allocated": 0,
            "gap": sum(g.get("budget", 0) for g in goals),
        },
        "dependencies": [g.get("dependencies", []) for g in goals],
        "risks": [
            {"risk": "资源不足", "impact": "高", "probability": "中", "mitigation": "提前储备资源"},
            {"risk": "进度延误", "impact": "中", "probability": "中", "mitigation": "设置缓冲时间"},
        ],
    }


def cmd_run(args):
    """Main workflow for plan decomposition."""
    ensure_dirs()

    plan_type = args.type or "annual"
    input_data = args.input or ""
    output_dir = args.output or DATA_DIR

    os.makedirs(output_dir, exist_ok=True)

    # Parse input goals
    goals = [
        {"name": f"目标{i+1}", "owner": "待分配", "budget": 0, "person_days": 0}
        for i in range(3)
    ]

    if input_data:
        try:
            parsed = json.loads(input_data)
            if isinstance(parsed, list):
                goals = parsed
            elif isinstance(parsed, dict) and "goals" in parsed:
                goals = parsed["goals"]
        except json.JSONDecodeError:
            pass

    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    year = args.year or datetime.now().year

    # Generate outputs based on plan type
    if plan_type == "annual":
        quarters = generate_quarter_milestones(year, goals)
        monthly_tasks = generate_monthly_tasks(year, goals)
        raci = generate_raci_matrix(monthly_tasks)
        resources = generate_resource_analysis(goals)

        # Save quarterly milestones
        quarters_file = os.path.join(output_dir, f"季度里程碑_{year}_{timestamp}.json")
        with open(quarters_file, "w", encoding="utf-8") as f:
            json.dump(quarters, f, ensure_ascii=False, indent=2)

        # Save monthly tasks
        if HAS_PANDAS:
            tasks_file = os.path.join(output_dir, f"月度任务分解表_{year}_{timestamp}.xlsx")
            df = pd.DataFrame(monthly_tasks)
            df.to_excel(tasks_file, index=False, engine="openpyxl")
        else:
            tasks_file = os.path.join(output_dir, f"月度任务分解表_{year}_{timestamp}.json")
            with open(tasks_file, "w", encoding="utf-8") as f:
                json.dump(monthly_tasks, f, ensure_ascii=False, indent=2)

        tasks_count = len(monthly_tasks)

    elif plan_type == "project":
        start_date = args.start or f"{year}-01-01"
        end_date = args.end or f"{year}-12-31"
        phases = generate_project_phases(goals, start_date, end_date)
        raci = generate_raci_matrix([t for p in phases for t in p.get("tasks", [])])
        resources = generate_resource_analysis(goals)

        # Save project phases
        phases_file = os.path.join(output_dir, f"项目阶段_{timestamp}.json")
        with open(phases_file, "w", encoding="utf-8") as f:
            json.dump(phases, f, ensure_ascii=False, indent=2)

        tasks_count = sum(len(p.get("tasks", [])) for p in phases)

    else:  # special
        raci = generate_raci_matrix(goals)
        resources = generate_resource_analysis(goals)
        tasks_count = len(goals)

    # Save RACI matrix
    raci_file = os.path.join(output_dir, f"责任矩阵_{timestamp}.json")
    with open(raci_file, "w", encoding="utf-8") as f:
        json.dump(raci, f, ensure_ascii=False, indent=2)

    # Save resource analysis
    resources_file = os.path.join(output_dir, f"资源分析_{timestamp}.json")
    with open(resources_file, "w", encoding="utf-8") as f:
        json.dump(resources, f, ensure_ascii=False, indent=2)

    result = {
        "status": "success",
        "plan_type": plan_type,
        "year": year if plan_type in ["annual", "project"] else None,
        "goals_count": len(goals),
        "tasks_count": tasks_count,
        "output_dir": output_dir,
        "files": {
            "raci": raci_file,
            "resources": resources_file,
        },
        "message": f"计划拆解完成，类型：{plan_type}，共拆解 {len(goals)} 个目标，{tasks_count} 项任务",
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


def cmd_status(args):
    """Check current status."""
    data_files = []
    if os.path.exists(DATA_DIR):
        data_files = [f for f in os.listdir(DATA_DIR) if not f.startswith(".")]
    result = {
        "skill": "计划拆解",
        "data_dir": DATA_DIR,
        "data_files": data_files,
        "file_count": len(data_files),
        "pandas_available": HAS_PANDAS,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


def cmd_export(args):
    """Export results."""
    fmt = getattr(args, "format", "json") or "json"
    data_files = []
    if os.path.exists(DATA_DIR):
        data_files = [os.path.join(DATA_DIR, f) for f in os.listdir(DATA_DIR) if not f.startswith(".")]

    if fmt == "json":
        output = json.dumps({"files": data_files, "count": len(data_files)}, ensure_ascii=False, indent=2)
    else:
        output = "\n".join(data_files)

    print(output)
    return 0


def cmd_template(args):
    """Generate template files."""
    template_type = args.type or "all"
    output_dir = args.output or DATA_DIR
    os.makedirs(output_dir, exist_ok=True)

    templates = {
        "goal": {
            "name": "目标名称",
            "description": "目标描述",
            "metrics": [{"name": "指标名称", "target": "目标值", "unit": "单位"}],
            "owner": "责任部门",
            "budget": 0,
            "person_days": 0,
            "dependencies": [],
            "risks": [],
        },
        "task": {
            "name": "任务名称",
            "goal": "所属目标",
            "start_date": "YYYY-MM-DD",
            "end_date": "YYYY-MM-DD",
            "owner": "责任人",
            "status": "待启动",
            "priority": "P1",
            "deliverables": [],
        },
    }

    if template_type == "all":
        for name, template in templates.items():
            file_path = os.path.join(output_dir, f"template_{name}.json")
            with open(file_path, "w", encoding="utf-8") as f:
                json.dump(template, f, ensure_ascii=False, indent=2)
        print(json.dumps({"status": "success", "templates": list(templates.keys())}, ensure_ascii=False, indent=2))
    elif template_type in templates:
        file_path = os.path.join(output_dir, f"template_{template_type}.json")
        with open(file_path, "w", encoding="utf-8") as f:
            json.dump(templates[template_type], f, ensure_ascii=False, indent=2)
        print(json.dumps({"status": "success", "template": template_type}, ensure_ascii=False, indent=2))
    else:
        print(json.dumps({"status": "error", "message": f"Unknown template type: {template_type}"}, ensure_ascii=False, indent=2))
        return 1

    return 0


def main():
    parser = argparse.ArgumentParser(description="计划拆解工具")
    subparsers = parser.add_subparsers(dest="command", help="可用命令")

    run_p = subparsers.add_parser("run", help="执行计划拆解")
    run_p.add_argument("--input", "-i", help="输入数据（JSON格式或文件路径）")
    run_p.add_argument("--type", "-t", choices=["annual", "project", "special"],
                       default="annual", help="计划类型：annual(年度)/project(项目)/special(专项)")
    run_p.add_argument("--year", "-y", type=int, help="计划年度")
    run_p.add_argument("--start", "-s", help="开始日期（项目计划）")
    run_p.add_argument("--end", "-e", help="结束日期（项目计划）")
    run_p.add_argument("--output", "-o", help="输出目录")

    subparsers.add_parser("status", help="查看当前状态")

    export_p = subparsers.add_parser("export", help="导出结果")
    export_p.add_argument("format", nargs="?", default="json", help="导出格式")

    template_p = subparsers.add_parser("template", help="生成模板文件")
    template_p.add_argument("--type", "-t", help="模板类型（goal/task/all）")
    template_p.add_argument("--output", "-o", help="输出目录")

    args = parser.parse_args()

    if args.command == "run":
        return cmd_run(args)
    elif args.command == "status":
        return cmd_status(args)
    elif args.command == "export":
        return cmd_export(args)
    elif args.command == "template":
        return cmd_template(args)
    else:
        parser.print_help()
        return 1


if __name__ == "__main__":
    sys.exit(main())
