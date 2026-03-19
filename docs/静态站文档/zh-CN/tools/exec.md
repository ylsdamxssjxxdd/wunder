---
title: 执行命令
summary: `执行命令` 负责真实命令执行、编译测试和预算化运行，是 Wunder 运行态工具链里最直接的执行入口。
read_when:
  - 你要理解 Wunder 的命令执行边界
  - 你要区分 `执行命令` 和 `ptc`
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - config/wunder-example.yaml
  - src/services/tools/catalog.rs
---

# 执行命令

如果文件工具解决的是“看和改”，那么 `执行命令` 解决的就是“在环境里跑起来”。

## `执行命令` 解决什么

它用于在当前工作区里执行系统命令。

常见场景：

- 编译
- 测试
- 运行脚本
- 做产物检查

常用参数：

- `content`
- `workdir`
- `timeout_s`
- `dry_run`
- `time_budget_ms`
- `output_budget_bytes`
- `max_commands`

## 为什么它不是完全自由执行

因为 Wunder 会同时考虑：

- `allow_commands`
- `allow_paths`
- `deny_globs`
- 本机执行或沙盒执行

所以它不是无边界 shell，而是受系统治理约束的命令执行工具。

## 和 `ptc` 怎么选

可以这样记：

- `执行命令`：运行已有命令、脚本和构建链路
- [`ptc`](/docs/zh-CN/tools/ptc/)：把内容组织成脚本产物再执行

如果你的目标是“在环境里执行一个已有命令”，优先用 `执行命令`。

如果你的目标是“先形成一个程序化文件或脚本产物，再继续走链路”，优先看 `ptc`。

## 最容易犯的错

### 把它当成唯一编辑方式

不对。

文件编辑优先仍然是：

- `写入文件`
- `应用补丁`

### 忽视预算

长命令和海量输出都可能把上下文拖爆，所以预算参数不是摆设。

## 你最需要记住的点

- `执行命令` 用于真实命令执行，并受安全与沙盒策略约束。
- 文件改动和命令执行应分层处理，不要混在一个工具里强行完成。
- `ptc` 是并列工具，但它解决的是“脚本产物生成”，不是同一件事。

## 相关文档

- [ptc](/docs/zh-CN/tools/ptc/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [文件与工作区工具](/docs/zh-CN/tools/workspace-files/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
