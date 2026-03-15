---
id: task_order_parser_fix
name: 修复订单解析器
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
  - parser
  - bugfix
languages:
  - zh-CN
workspace_files:
  - path: src/order_parser.py
    content: |
      def parse_orders(text):
          orders = []
          for raw_line in text.split("\n"):
              if not raw_line:
                  continue
              order_id, customer, amount = raw_line.split("|")
              amount = float(amount.replace("$", ""))
              orders.append(
                  {
                      "order_id": order_id,
                      "customer": customer,
                      "amount": amount,
                  }
              )
          return orders
  - path: input/orders.txt
    content: |
      ORD-001 | Alice | $12.50

      ORD-002| Bob | 7.00
      ORD-003 |Chen| $19.99
      # comment line should be ignored
      ORD-004 | Dana | $0.99
---

## Prompt

请修复 `{attempt_root}/src/order_parser.py` 中的 `parse_orders(text)`，要求：

- 忽略空行
- 忽略以 `#` 开头的注释行
- 兼容字段两侧的额外空格
- `amount` 必须解析为数字
- 返回结果中的 `order_id` 和 `customer` 不能保留多余空格

另外输出 `{attempt_root}/output/fix_report.md`，用简短文字说明修复点。

## Expected Behavior

智能体应能对输入格式边界情况进行健壮处理，并在不改动接口的前提下修复解析器。

## Grading Criteria

- [ ] 正确处理空行和注释行
- [ ] 正确清理字段空格
- [ ] 正确解析金额为数字
- [ ] 输出修复说明

## Automated Checks

```python
def grade(transcript, workspace_path):
    import importlib.util
    import os

    scores = {
        "parser_fixed": 0.0,
        "ignores_noise_lines": 0.0,
        "trims_fields": 0.0,
        "report_present": 0.0,
    }

    source_path = os.path.join(workspace_path, "src", "order_parser.py")
    report_path = os.path.join(workspace_path, "output", "fix_report.md")
    if os.path.exists(report_path):
        scores["report_present"] = 1.0

    spec = importlib.util.spec_from_file_location("order_parser", source_path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)

    text = """ORD-001 | Alice | $12.50\n\nORD-002| Bob | 7.00\n# ignore me\nORD-003 |Chen| $19.99\nORD-004 | Dana | $0.99"""
    result = module.parse_orders(text)
    if len(result) == 4 and abs(result[0]["amount"] - 12.50) < 1e-9 and abs(result[-1]["amount"] - 0.99) < 1e-9:
        scores["parser_fixed"] = 1.0
    if [item["order_id"] for item in result] == ["ORD-001", "ORD-002", "ORD-003", "ORD-004"]:
        scores["ignores_noise_lines"] = 1.0
    if [item["customer"] for item in result] == ["Alice", "Bob", "Chen", "Dana"]:
        scores["trims_fields"] = 1.0

    return scores
```
