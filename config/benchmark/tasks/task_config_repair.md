---
id: task_config_repair
name: 修复损坏配置并输出说明
suite: workspace-core
category: config
grading_type: automated
timeout_seconds: 180
runs_recommended: 2
difficulty: easy
required_tools:
  - read_file
  - write_file
  - edit_file
tags:
  - config
  - repair
  - json
languages:
  - zh-CN
workspace_files:
  - path: input/app_config.jsonc
    content: |
      {
        "server": {
          "host": "127.0.0.1",
          "port": "8088",
        },
        "llm": {
          "default_model": "qwen-plus"
          "temperature": 0.2,
        },
        "features": {
          "memory": true,
          "retry": {"enabled": true, "max_attempts": "5",},
        },
        "tools": ["read_file", "write_file", "search_content", "write_file", "execute_command",],
      }
---

## Prompt

`{attempt_root}/input/app_config.jsonc` 是一份损坏的配置草稿。请完成以下任务：

1. 生成合法 JSON 文件 `{attempt_root}/output/fixed_config.json`
2. 生成修复说明 `{attempt_root}/output/fix_notes.md`

修复要求：

- `server.port` 必须是数字 `8088`
- `llm.default_model` 必须保留为 `qwen-plus`
- `features.retry.max_attempts` 必须是数字 `5`
- `tools` 需要去重，但保留原有顺序
- `fix_notes.md` 至少写出 3 条修复说明

不要修改原始输入文件。

## Expected Behavior

智能体应能从损坏的 JSONC 草稿中恢复出合法 JSON，并输出简洁的修复说明，体现对语法修复、类型修复和去重要求的理解。

## Grading Criteria

- [ ] 输出合法 JSON
- [ ] 正确修复必需字段的类型和语法
- [ ] 正确去重 tools 列表
- [ ] 给出清晰的修复说明

## Automated Checks

```python
def grade(transcript, workspace_path):
    import json
    import os

    scores = {
        "fixed_json_exists": 0.0,
        "notes_exists": 0.0,
        "required_fields_correct": 0.0,
        "tools_deduplicated": 0.0,
        "notes_quality": 0.0,
    }

    config_path = os.path.join(workspace_path, "output", "fixed_config.json")
    notes_path = os.path.join(workspace_path, "output", "fix_notes.md")

    if os.path.exists(config_path):
        scores["fixed_json_exists"] = 1.0
    if os.path.exists(notes_path):
        scores["notes_exists"] = 1.0

    if not os.path.exists(config_path):
        return scores

    with open(config_path, "r", encoding="utf-8") as fp:
        data = json.load(fp)

    if (
        data.get("server", {}).get("port") == 8088
        and data.get("llm", {}).get("default_model") == "qwen-plus"
        and data.get("features", {}).get("retry", {}).get("max_attempts") == 5
    ):
        scores["required_fields_correct"] = 1.0

    if data.get("tools") == ["read_file", "write_file", "search_content", "execute_command"]:
        scores["tools_deduplicated"] = 1.0

    if os.path.exists(notes_path):
        with open(notes_path, "r", encoding="utf-8") as fp:
            notes = fp.read().strip()
        lines = [line for line in notes.splitlines() if line.strip()]
        if len(lines) >= 3 and ("port" in notes or "tools" in notes or "max_attempts" in notes):
            scores["notes_quality"] = 1.0

    return scores
```
