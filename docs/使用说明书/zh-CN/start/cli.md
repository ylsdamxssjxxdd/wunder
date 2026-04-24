---
title: CLI 使用
summary: 开发者和自动化场景的入口。终端驱动、脚本化、JSONL 输出。
read_when:
  - 你需要在终端中使用 wunder
  - 你要把 wunder 集成到脚本或自动化流程
source_docs:
  - wunder-cli/src/main.rs
  - wunder-cli/src/tui/
updated_at: 2026-04-10
---

# CLI 使用

CLI 是开发者和自动化场景的入口。在终端里直接用，不需要图形界面。

## 什么时候选 CLI

| 场景 | 选 CLI |
|------|--------|
| 习惯终端操作 | ✅ |
| 需要脚本化、自动化 | ✅ |
| 要管道输出到其他工具 | ✅ |
| 不想装桌面端 | ✅ |
| 需要图形界面 | ❌ 选 Desktop |

## 安装

```bash
# 编译（需要 Rust 工具链）
cargo build --release

# 运行
./target/release/wunder-cli
```

## 第一次会话

```bash
wunder-cli
> 帮我写一个 Hello World 的 Python 脚本
```

你会看到模型思考 → 调用工具 → 展示结果 → 给出回复。

## 常用命令

| 命令 | 作用 |
|------|------|
| `/new` | 新建线程 |
| `/stop` | 停止当前执行 |
| `/compact` | 压缩当前对话 |
| `/fork` | 分叉当前线程 |
| `/resume` | 恢复之前的线程 |
| `/help` | 查看帮助 |
| `/quit` | 退出 |

## TUI 界面

CLI 内置 TUI（终端用户界面），类似 Codex 的交互体验：

- 上方：对话区
- 下方：输入区
- 侧边：线程列表

## JSONL 输出

CLI 支持 JSONL 格式输出，便于管道和自动化集成：

```bash
wunder-cli --format jsonl
```

每行一个 JSON 对象，包含事件类型和内容。

## 自动化场景

CLI 适合以下自动化场景：

- CI/CD 流水线中调用智能体
- 批量处理任务
- 与其他命令行工具组合
- 定时任务触发

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [核心概览](/docs/zh-CN/concepts/)
- [工具总览](/docs/zh-CN/tools/)
