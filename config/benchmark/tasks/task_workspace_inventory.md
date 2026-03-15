---
id: task_workspace_inventory
name: 工作区项目盘点与摘要
suite: workspace-core
category: filesystem
grading_type: automated
timeout_seconds: 180
runs_recommended: 2
difficulty: easy
required_tools:
  - list_files
  - read_file
  - write_file
tags:
  - filesystem
  - report
  - summary
languages:
  - zh-CN
workspace_files:
  - path: input/projects.csv
    content: |
      project_id,name,owner,status,priority
      P-101,alpha-rewrite,Alice,in_progress,high
      P-205,billing-cleanup,Bob,blocked,medium
      P-330,migration-dashboard,Chen,planning,high
  - path: input/notes/alpha-rewrite.txt
    content: |
      The API schema must be frozen before Friday.
      Alice is waiting for one frontend field confirmation.
  - path: input/notes/billing-cleanup.txt
    content: |
      The cleanup is currently blocked by the finance export bug.
      Bob suggests escalating to the platform team.
  - path: input/notes/migration-dashboard.txt
    content: |
      Migration dashboard should include a launch checklist and owner handoff section.
      Chen plans to start implementation next Monday.
---

## Prompt

请阅读 `{attempt_root}/input` 下的材料，并输出两份文件：

1. `{attempt_root}/output/report.md`
2. `{attempt_root}/output/inventory.json`

要求：

- `report.md` 必须包含以下二级标题：`## 项目清单`、`## 高优先级`、`## 阻塞项`
- `report.md` 中每个项目都要有一句简短摘要，并带出 owner、状态和下一步建议
- `inventory.json` 必须是合法 JSON，结构如下：

```json
{
  "total_projects": 0,
  "high_priority": [],
  "blocked_projects": [],
  "owners": {}
}
```

- `high_priority` 中写项目名称
- `blocked_projects` 中写项目名称
- `owners` 是对象，key 为 owner，value 为该 owner 负责的项目数量

请只在 `{attempt_root}/output` 下写结果，不要修改输入文件。

## Expected Behavior

智能体应正确盘点 CSV 和说明文档，提炼出高优先级项目与阻塞项，并生成结构化 JSON 以及适合人阅读的 Markdown 摘要。

## Grading Criteria

- [ ] 正确统计项目总数与 owner 数量
- [ ] 正确识别高优先级项目
- [ ] 正确识别阻塞项目
- [ ] Markdown 摘要包含关键风险与下一步建议

## Automated Checks

```python
def grade(transcript, workspace_path):
    import csv
    import json
    import os

    scores = {
        "inventory_json_exists": 0.0,
        "report_md_exists": 0.0,
        "counts_correct": 0.0,
        "priority_and_blocked_correct": 0.0,
        "report_sections_present": 0.0,
    }

    inventory_path = os.path.join(workspace_path, "output", "inventory.json")
    report_path = os.path.join(workspace_path, "output", "report.md")

    if os.path.exists(inventory_path):
        scores["inventory_json_exists"] = 1.0
    if os.path.exists(report_path):
        scores["report_md_exists"] = 1.0

    if not os.path.exists(inventory_path):
        return scores

    with open(inventory_path, "r", encoding="utf-8") as fp:
        inventory = json.load(fp)

    if inventory.get("total_projects") == 3 and inventory.get("owners") == {"Alice": 1, "Bob": 1, "Chen": 1}:
        scores["counts_correct"] = 1.0

    high_priority = set(inventory.get("high_priority") or [])
    blocked_projects = set(inventory.get("blocked_projects") or [])
    if high_priority == {"alpha-rewrite", "migration-dashboard"} and blocked_projects == {"billing-cleanup"}:
        scores["priority_and_blocked_correct"] = 1.0

    if os.path.exists(report_path):
        with open(report_path, "r", encoding="utf-8") as fp:
            report = fp.read()
        if "## 项目清单" in report and "## 高优先级" in report and "## 阻塞项" in report:
            if "billing-cleanup" in report and "platform team" in report.lower():
                scores["report_sections_present"] = 1.0

    return scores
```
