---
title: ptc
summary: Python 临时脚本执行工具的当前行为与返回结构。
read_when:
  - 你要用一段完整 Python 脚本完成局部任务
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# ptc

`ptc` 是 programmatic tool call，定位是：

- 把一段完整 Python 脚本写到临时文件
- 在指定 `workdir` 下执行
- 返回 `stdout/stderr/returncode`

它不是随便执行 shell 的替代品。纯命令行任务还是优先用 [执行命令](/docs/zh-CN/tools/exec/)。

## 最小参数

```json
{
  "filename": "helper.py",
  "content": "print('hello')"
}
```

## 成功返回

```json
{
  "ok": true,
  "action": "ptc",
  "state": "completed",
  "summary": "Executed Python script C:/.../ptc_temp/helper.py.",
  "data": {
    "path": "ptc_temp/helper.py",
    "workdir": ".",
    "returncode": 0,
    "stdout": "hello\n",
    "stderr": ""
  }
}
```

## 失败返回

常见错误码：

- `TOOL_PTC_INVALID_FILENAME`
- `TOOL_PTC_CONTENT_REQUIRED`
- `TOOL_PTC_EXEC_ERROR`
- `TOOL_PTC_TIMEOUT`
- `TOOL_PTC_EXEC_FAILED`

失败时 `data` 里通常仍会保留：

- `path`
- `workdir`
- `returncode`
- `stdout`
- `stderr`

## 使用边界

适合：

- 临时数据处理
- 小型 Python 解析
- 需要几步 Python 逻辑但不值得单独落盘项目文件

不适合：

- 多命令 shell 流程
- 持续性脚本工程
- 复杂系统编译流程

这些更适合 [执行命令](/docs/zh-CN/tools/exec/)。
