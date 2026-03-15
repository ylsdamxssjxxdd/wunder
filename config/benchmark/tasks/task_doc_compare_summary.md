---
id: task_doc_compare_summary
name: 对比版本文档并输出变更摘要
suite: knowledge-memory
category: synthesis
grading_type: hybrid
timeout_seconds: 240
runs_recommended: 2
grading_weights:
  automated: 0.6
  llm_judge: 0.4
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - docs
  - compare
  - synthesis
languages:
  - zh-CN
workspace_files:
  - path: input/product_v1.md
    content: |
      # Product V1

      ## Core Features
      - Email login
      - Project dashboard
      - CSV export

      ## Limitations
      - No audit log
      - No SSO
      - Single workspace only
  - path: input/product_v2.md
    content: |
      # Product V2

      ## Core Features
      - Email login
      - SSO login
      - Project dashboard
      - Audit log
      - Scheduled CSV export

      ## Changes
      - Single workspace mode removed, now every account must belong to an organization
      - Manual CSV export entry moved under Reports
---

## Prompt

请对比 `{attempt_root}/input/product_v1.md` 和 `{attempt_root}/input/product_v2.md`，输出：

1. `{attempt_root}/output/changes.json`
2. `{attempt_root}/output/brief.md`

`changes.json` 结构必须如下：

```json
{
  "added_features": [],
  "removed_features": [],
  "breaking_changes": [],
  "recommended_focus": []
}
```

要求：

- `added_features` 中列出新增能力
- `removed_features` 中列出 v1 中没有延续到 v2 的内容
- `breaking_changes` 中列出会影响旧用户或旧流程的变化
- `recommended_focus` 中列出 2~4 条建议，面向产品经理
- `brief.md` 用简洁中文总结重点，不要写成长文

## Expected Behavior

智能体应能完成多文档差异提炼，输出结构化差异清单，并把对业务有影响的变化单独识别出来。

## Grading Criteria

- [ ] 正确提取新增能力
- [ ] 正确识别影响旧用户的破坏性变化
- [ ] 结构化 JSON 完整且可读
- [ ] brief.md 简洁、重点明确

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "changes_json_exists": 0.0,
        "brief_exists": 0.0,
        "added_features_correct": 0.0,
        "breaking_changes_correct": 0.0,
    }

    changes_path = os.path.join(workspace_path, "output", "changes.json")
    brief_path = os.path.join(workspace_path, "output", "brief.md")
    if os.path.exists(changes_path):
        scores["changes_json_exists"] = 1.0
    if os.path.exists(brief_path):
        scores["brief_exists"] = 1.0
    if not os.path.exists(changes_path):
        return scores

    with open(changes_path, "r", encoding="utf-8") as fp:
        data = json.load(fp)

    added = " ".join(data.get("added_features") or []).lower()
    breaking = " ".join(data.get("breaking_changes") or []).lower()

    if "sso" in added and "audit" in added and "scheduled csv export" in added:
        scores["added_features_correct"] = 1.0
    if "organization" in breaking and "single workspace" in breaking:
        scores["breaking_changes_correct"] = 1.0

    return scores
```

## LLM Judge Rubric

- 是否完整覆盖新增、删除与破坏性变化
- 是否能区分“功能位置调整”和“真正的流程变更”
- `recommended_focus` 是否具体、能指导产品沟通
- `brief.md` 是否简洁且抓住重点
