---
id: task_email_triage
name: 邮件分诊与回复草拟
suite: office-workflow
category: workflow
grading_type: hybrid
timeout_seconds: 240
runs_recommended: 2
grading_weights:
  automated: 0.5
  llm_judge: 0.5
difficulty: medium
required_tools:
  - read_file
  - write_file
tags:
  - email
  - triage
  - office
languages:
  - zh-CN
workspace_files:
  - path: input/inbox.csv
    content: |
      email_id,from,subject,body
      E-001,client-a@example.com,Invoice mismatch,"The invoice total is 20% higher than the agreed quote. Please check today."
      E-002,ops@example.com,Weekly social event,"Please vote for Friday activity options before 6pm."
      E-003,boss@example.com,Customer escalation,"Please prepare a short status update for the delayed migration account before 3pm."
      E-004,security@example.com,Password reset policy,"Reminder: all team members must rotate passwords this week and confirm completion."
  - path: input/policy.md
    content: |
      # Handling Policy

      - Any customer billing or customer escalation issue is high priority.
      - Internal event or social notices are low priority.
      - Security compliance reminders are medium priority and should be assigned to operations.
      - Replies should be concise and action-oriented.
---

## Prompt

请根据 `{attempt_root}/input/inbox.csv` 和 `{attempt_root}/input/policy.md` 完成邮件分诊，并输出：

1. `{attempt_root}/output/triage.json`
2. `{attempt_root}/output/reply_drafts.md`

`triage.json` 必须是一个数组，每项结构如下：

```json
{
  "email_id": "",
  "priority": "high|medium|low",
  "category": "",
  "owner": "",
  "action": ""
}
```

要求：

- `E-001` 和 `E-003` 应视为高优先级
- `E-004` 应归给 operations 处理
- `reply_drafts.md` 至少为 `E-001` 和 `E-003` 写出简短回复草稿
- 回复内容要简洁、可执行

## Expected Behavior

智能体应能把半结构化信息转换为稳定的分诊结果，并基于规则生成简洁的业务回复。

## Grading Criteria

- [ ] 正确判断优先级
- [ ] 正确分配 owner 和 action
- [ ] 回复草稿覆盖关键邮件
- [ ] 表达简洁、专业、可执行

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "triage_json_exists": 0.0,
        "reply_drafts_exists": 0.0,
        "priority_correct": 0.0,
        "owner_correct": 0.0,
    }

    triage_path = os.path.join(workspace_path, "output", "triage.json")
    reply_path = os.path.join(workspace_path, "output", "reply_drafts.md")
    if os.path.exists(triage_path):
        scores["triage_json_exists"] = 1.0
    if os.path.exists(reply_path):
        scores["reply_drafts_exists"] = 1.0
    if not os.path.exists(triage_path):
        return scores

    with open(triage_path, "r", encoding="utf-8") as fp:
        items = json.load(fp)

    mapping = {item["email_id"]: item for item in items if isinstance(item, dict) and item.get("email_id")}
    if (
        mapping.get("E-001", {}).get("priority") == "high"
        and mapping.get("E-003", {}).get("priority") == "high"
        and mapping.get("E-002", {}).get("priority") == "low"
        and mapping.get("E-004", {}).get("priority") == "medium"
    ):
        scores["priority_correct"] = 1.0

    if mapping.get("E-004", {}).get("owner", "").lower() == "operations":
        scores["owner_correct"] = 1.0

    return scores
```

## LLM Judge Rubric

- 分诊结果是否与规则一致且有清晰执行动作
- 回复草稿是否简洁、专业、能直接发送
- 是否优先覆盖高优先级邮件
- 是否避免无关冗长表达
