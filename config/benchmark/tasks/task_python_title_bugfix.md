---
id: task_python_title_bugfix
name: 修复标题去重逻辑缺陷
suite: coding-agent
category: code_fix
grading_type: automated
timeout_seconds: 240
runs_recommended: 3
difficulty: medium
required_tools:
  - read_file
  - edit_file
  - write_file
  - execute_command
tags:
  - python
  - bugfix
  - code
languages:
  - zh-CN
workspace_files:
  - path: src/text_utils.py
    content: |
      def summarize_titles(records):
          seen = []
          titles = []
          for record in records:
              title = record.get("title", "").strip()
              if not title:
                  continue
              normalized = title.lower()
              if normalized in seen:
                  continue
              seen.append(title)
              titles.append(title)
          return ", ".join(sorted(titles))


      def pick_non_empty(values):
          output = []
          for value in values:
              if value and value.strip():
                  output.append(value.strip())
          return output
  - path: input/sample_records.json
    content: |
      [
        {"title": "Alpha"},
        {"title": "beta"},
        {"title": " alpha "},
        {"title": "Gamma"},
        {"title": "BETA"},
        {"title": ""}
      ]
---

## Prompt

请修复 `{attempt_root}/src/text_utils.py` 中的缺陷，但不要改变 `summarize_titles(records)` 的函数签名。

任务目标：

- `summarize_titles(records)` 需要对标题做 **大小写不敏感去重**
- 空标题需要忽略
- 最终输出仍然使用逗号拼接，并按字母顺序排序

此外，请输出一份 `{attempt_root}/output/fix_report.md`，说明：

1. 根因是什么
2. 你做了什么修复
3. 你如何验证

## Expected Behavior

智能体应识别 Python 代码中的逻辑错误，完成最小必要修复，并给出清晰的修复说明。

## Grading Criteria

- [ ] 正确修复大小写不敏感去重问题
- [ ] 保持函数签名不变
- [ ] 不破坏无关函数
- [ ] 输出有效修复说明

## Automated Checks

```python
def grade(transcript, workspace_path):
    import importlib.util
    import json
    import os

    scores = {
        "code_fixed": 0.0,
        "signature_preserved": 0.0,
        "edge_cases_correct": 0.0,
        "report_present": 0.0,
    }

    source_path = os.path.join(workspace_path, "src", "text_utils.py")
    report_path = os.path.join(workspace_path, "output", "fix_report.md")

    if os.path.exists(report_path):
        scores["report_present"] = 1.0

    if not os.path.exists(source_path):
        return scores

    with open(source_path, "r", encoding="utf-8") as fp:
        code = fp.read()
    if "def summarize_titles(records):" in code:
        scores["signature_preserved"] = 1.0

    spec = importlib.util.spec_from_file_location("text_utils", source_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    result = module.summarize_titles([
        {"title": "Alpha"},
        {"title": "beta"},
        {"title": " alpha "},
        {"title": "Gamma"},
        {"title": "BETA"},
        {"title": ""},
    ])
    if result == "Alpha, Gamma, beta":
        scores["code_fixed"] = 1.0

    result_2 = module.summarize_titles([
        {"title": "Zoo"},
        {"title": "zoo"},
        {"title": "Apple"},
        {"title": " apple "},
    ])
    if result_2 == "Apple, Zoo":
        scores["edge_cases_correct"] = 1.0

    return scores
```
