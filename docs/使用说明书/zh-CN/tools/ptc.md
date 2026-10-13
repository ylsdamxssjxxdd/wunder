---
title: ptc
summary: ptc 是 Python 临时脚本执行工具，已从 wunder 中移除，本页仅作历史参考。
updated_at: 2026-09-21
---

# ptc

> **已移除**：ptc 已从 wunder 中移除，对智能体与用户都不可见，本页仅作历史参考。现在要执行命令请用 [执行命令](/docs/zh-CN/tools/exec/)。

ptc 是 programmatic tool call，原本用来把一段完整的 Python 脚本写成临时文件、在指定目录下跑，再把 `stdout`、`stderr` 和执行返回码交回来。

它不是随便执行 shell 的替代品：纯命令行任务一直优先用 [执行命令](/docs/zh-CN/tools/exec/)。

## 它是怎么用的

参数是脚本文件名和脚本内容：

```json
{
  "filename": "helper.py",
  "content": "print('hello')"
}
```

每次调用都会把脚本单独存到 `ptc_temp/<invocation_id>/` 下，避免多人共用工作区时同名脚本互相覆盖。返回里的 `path` 指向这份脚本，你指定的 `workdir` 仍是程序实际运行的目录。

跑完会返回脚本路径、运行目录、返回码、`stdout` 和 `stderr`；即使失败，这几项通常也会保留，方便定位是哪一步出的问题。

## 当时适合做什么

- 临时的数据处理。
- 小段的 Python 解析。
- 需要几步 Python 逻辑，但不值得单独落一个项目文件的事。

## 当时不适合做什么

- 多条命令串起来的 shell 流程。
- 需要长期维护的脚本工程。
- 复杂的系统编译流程。

这些交给 [执行命令](/docs/zh-CN/tools/exec/) 更合适。
