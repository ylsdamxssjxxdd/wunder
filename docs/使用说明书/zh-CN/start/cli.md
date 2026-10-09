---
title: 舵机（cli）使用
summary: 可选形态。终端与自动化场景使用：终端驱动、脚本化、JSONL 输出。
read_when:
  - 需要在终端中使用 wunder
  - 要把 wunder 集成到脚本或自动化流程
source_docs:
  - crates/wunder-cli/main.rs
  - crates/wunder-cli/tui/
updated_at: 2026-04-10
---

# 舵机（cli）使用

舵机是 wunder 的命令版，定位是**手工本地操作入口**，属于可选形态：日常使用装蜂窝（desktop）即可，在终端里手工操作、写脚本或做自动化时才用舵机。

## 适用场景

| 场景 | 选舵机 |
|------|--------|
| 习惯终端操作 | ✅ |
| 需要脚本化、自动化 | ✅ |
| 要管道输出到其他工具 | ✅ |
| 不想装蜂窝 | ✅ |
| 需要图形界面 | ❌ 选蜂窝 |

## 安装

```bash
# 编译（需要 Rust 工具链）
cargo build --release -p wunder-cli

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

舵机内置 TUI（终端用户界面），类似 Codex 的交互体验：

- 上方：对话区
- 下方：输入区
- 侧边：线程列表

## 运行时文件

舵机默认把运行时文件放在用户目录的 `.wunder` 下：

- 运行时配置和 SQLite 状态：`~/.wunder/cli/WUNDER_TEMP/`
- 用户工具和技能：`~/.wunder/user_tools/` 与 `~/.wunder/skills/`
- 临时文件 API：通过 `WUNDER_TEMP_DIR_ROOT` 指向同一个 `~/.wunder/cli/WUNDER_TEMP/`

只有脚本需要隔离运行目录时，才使用 `--temp-root <path>` 覆盖默认位置。

## JSONL 输出

舵机支持 JSONL 格式输出，便于管道和自动化集成：

```bash
wunder-cli --format jsonl
```

每行一个 JSON 对象，包含事件类型和内容。

## 自动化场景

舵机适合以下自动化场景：

- CI/CD 流水线中调用智能体
- 批量处理任务
- 与其他命令行工具组合
- 定时任务触发

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [核心概览](/docs/zh-CN/concepts/)
- [工具总览](/docs/zh-CN/tools/)
