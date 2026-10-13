---
title: 舵机（cli）使用
summary: 手工本地操作入口。TTY 下进 ratatui TUI，非 TTY 降级为一次性 exec；22 条 slash 命令、exec --json JSONL 契约、cloud 子命令全套；舵机侧关闭定时任务与引擎排队。
read_when:
  - 需要在终端里用 wunder
  - 要把 wunder 集成到脚本、CI/CD 或管道
  - 需要 cloud 子命令做设备互通运维
source_docs:
  - crates/wunder-cli/main.rs
  - crates/wunder-cli/args.rs
  - crates/wunder-cli/runtime.rs
  - crates/wunder-cli/slash_command.rs
  - crates/wunder-cli/tui/app.rs
  - crates/wunder-cli/exec_events.rs
  - crates/wunder-cli/cloud_command.rs
updated_at: 2026-10-13
---

# 舵机（cli）使用

舵机（cli，`crates/wunder-cli/`）是 wunder 的**手工本地操作入口**，属于可选形态：日常使用装蜂窝（desktop）即可；终端驱动、脚本化与自动化场景才用舵机。它同进程调引擎（`CliRuntime.state: Arc<AppState>` → `runtime.state.kernel.orchestrator.stream`），复用 AppState、配置、工具与存储语义，不另起一套执行链路。

## 适用场景

| 场景 | 选舵机 |
| --- | --- |
| 习惯终端操作 | ✅ |
| 需要脚本化、自动化、CI/CD | ✅ |
| 需要管道输出（JSONL） | ✅ |
| 需要 `cloud` 子命令运维设备互通 | ✅ |
| 不想装蜂窝 | ✅ |
| 需要图形界面 | ❌ 选蜂窝 |
| 需要浏览器访问 | ❌ 选蜂巢（随舰体） |

## 装与启动

```bash
# 编译（需要 Rust 工具链）
cargo build --release -p wunder-cli

# 运行（无必需参数）
./target/release/wunder-cli
```

- **TTY**（stdin/stdout/stderr 全部是 TTY）：进入 **ratatui TUI**，`Viewport::Inline`（`crates/wunder-cli/tui/mod.rs:191`）
- **非 TTY**：自动降级为**一次性 exec**（`main.rs:216-233`）；管道、脚本走这条
- **数据目录**：`WUNDER_HOME` → `~/.wunder`（`runtime.rs:1018-1029`）
- **引擎 YAML**：生成于 `<temp>/config/wunder.yaml`（`runtime.rs:610-632`）
- **用户配置**：`~/.wunder/config.toml`（`runtime.rs:104-108`）

## 子命令（`args.rs:228-259`）

| 子命令 | 别名 | 作用 |
| --- | --- | --- |
| `exec` | `e` | 非交互执行一次智能体任务；`--json` 走 JSONL 事件契约 |
| `resume` | — | 恢复历史会话 |
| `tool` | — | 直接运行 builtin / MCP / skill 工具 |
| `mcp` | — | 管理本地 MCP 服务器 |
| `skills` | — | 管理当前用户本地技能 |
| `config` | — | 查看与修改运行配置 |
| `stress-thread` | — | 生成本地合成线程做渲染压测 |
| `doctor` | — | 诊断本地运行环境 |
| `cloud` | — | 管理本地云端连接与设备互通 |
| `completion` | — | 生成 Shell 补全脚本 |

### `cloud` 子命令（`args.rs:268-299`）

设备互通与云工作区运维全套：

```
cloud login | logout | status | models | refresh | logs
cloud devices                    # 本账号互通节点
cloud ws ls / cat / pull / push  # 云端工作区
cloud threads / thread           # 云端线程列表与详情
cloud send / watch               # 经互通台账驱动线程
cloud audit / approve            # 命令台账与审批
```

### `exec --json` JSONL 契约

`crates/wunder-cli/exec_events.rs` 定义：每行一个 JSON 对象，事件名 `thread` / `turn` / `item.*`。用于管道与自动化：

```bash
wunder-cli exec --json "整理当前目录的文件" | jq -c 'select(.type=="item.text")'
```

## Slash 命令（22 条，`slash_command.rs:183-208`）

| 命令 | 别名 | 作用 |
| --- | --- | --- |
| `/help` | `/h` | 帮助 |
| `/status` | — | 当前状态 |
| `/resume` | `/r` | 恢复历史会话 |
| `/new` | — | 新建线程 |
| `/clear` | `/cls` | 清屏 |
| `/model` | — | 切换模型 |
| `/permissions` | `/perm` / `/approvals` / `/approval` | 权限与审批 |
| `/plan` | — | 计划面板 |
| `/goal` | — | 目标态管理 |
| `/edit` | — | 编辑 |
| `/init` | — | 初始化 |
| `/attach` | — | 附件 |
| `/diff` | — | 补丁差异 |
| `/review` | — | 审阅 |
| `/skills` | — | 技能列表 |
| `/rename` | — | 重命名 |
| `/compact` | — | 压缩对话 |
| `/mcp` | — | MCP 管理 |
| `/config` | `/config show` 是子形式 | 配置查看与修改 |
| `/exit` | — | 退出 |
| `/quit` | `/q` | 退出 |

**没有** `cloud`、`cron`、`channel`、`memory`、`knowledge` 斜杠命令：`cloud` 是子命令形式；后四者在舵机上无管理界面。

## 键位（权威表 `tui/app.rs:2241-2280`）

| 键 | 作用 |
| --- | --- |
| `/` | 打开命令面板 |
| `Enter` | 发送 |
| `!` | 直接执行 shell |
| `Shift+Enter` / `Ctrl+J` | 换行 |
| `Tab` | 补全 |
| `@` | 文件提及 |
| `#` | 技能提及 |
| `$` | 工具提及 |
| `Ctrl+V` | 粘贴图片 |
| 拖入图片或文件 | 附加 |
| `Alt+,` / `Alt+.` | 降低 / 提高推理强度 |
| `Ctrl+R` / `Ctrl+S` | 搜索输入历史 |
| `Ctrl+G` | 外部编辑器 |
| `Ctrl+O` | 复制最后回复 |
| `Ctrl+T` | 完整转录 |
| `Ctrl+L` | 清屏开新会话 |
| `Ctrl+C` | 退出 |
| `Esc` | 中断当前轮次；空闲时清空草稿；关闭快捷键面板 |
| `F3` | 查看输出并选中卡片 |
| `Enter`（F3 面板内） | 展开 / 收起 |
| `F2` | 运行提醒 |
| `F4` | 切换鼠标模式（与 codex 的差异） |
| `?` | 快捷键面板 |
| `←` | 任务中心 |

## 第一次会话

```bash
wunder-cli
> 帮我写一个 Hello World 的 Python 脚本
```

看到模型思考 → 工具批次 → 结果 → 回复即跑通。

## 舵机独有能力（其他界面没有）

- `exec --json` JSONL 脚本契约
- `cloud` 子命令全套（`args.rs:268-299`）
- `doctor` 本地运行环境诊断
- `stress-thread` 本地合成线程
- `completion` Shell 补全
- `tool` / `mcp` / `skills` 直接命令行运行（不走智能体主循环）

## 舵机**没有**的东西

- **定时任务被显式关闭**：`runtime.rs:683 config.cron.enabled = false`。`cron.enabled=false` 时**只拒绝 add/update/enable/run**，status、列表、删除与禁用仍可用（`services/cron.rs:116-121`）——所以你能看到旧任务列表，但舵机不会再排新任务。
- **引擎排队也被显式关闭**：`runtime.rs:682 config.agent_queue.enabled = false`。后果是**运行中再发一条消息不会进排队条**，而是直接撞会话锁被判「该用户已有会话正在执行」，等满忙窗口才返回。这与蜂窝相反——蜂窝在 native 模式会强制打开队列。要在运行中插话、排队或改写下一条，请用蜂窝或蜂巢。
- **渠道 / 知识库 / 长期记忆 / 形象** 只有工具卡文案映射（`tool_presentation.rs:117,141`），**没有管理界面**
- **无 ASR**：舵机没有语音输入；贴图靠 `Ctrl+V` 与拖拽
- **无交互式终端 UI**：宿主本身就是终端；shell 走 `!` 前缀
- **无截图采集 / 托盘 / 全局热键**

## 运行时文件

舵机默认把运行时文件放在 `~/.wunder` 下：

- 运行时配置与 SQLite 状态：`~/.wunder/cli/WUNDER_TEMP/`
- 用户工具与技能：`~/.wunder/user_tools/` 与 `~/.wunder/skills/`
- 临时文件 API：`WUNDER_TEMP_DIR_ROOT` 指向同一个 `~/.wunder/cli/WUNDER_TEMP/`

脚本需要隔离运行目录时，用 `--temp-root <path>` 覆盖。

## 自动化场景

- CI/CD 流水线里调用 `exec --json` 消费 JSONL
- 批量任务：外部调度循环调 `exec`
- 管道：`cat input.txt | wunder-cli` 触发非 TTY 降级
- 与 `tool` / `mcp` / `skills` 组合做纯工具运行
- **定时任务不在舵机里做**：舵机关闭了 cron；请放到蜂窝（`ui/cron_page.slint`）或蜂巢（`SettingsCronCategory`）

## 常见问题

**能进图形界面吗？** 不能。需要 GUI 用蜂窝。

**能建定时任务吗？** 不能，舵机显式关闭 cron。

**能在管道里用吗？** 可以。非 TTY 自动降级为一次性 exec；`--json` 出 JSONL。

**如何做本地环境体检？** `wunder-cli doctor`。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [界面概览](/docs/zh-CN/surfaces/index/)
- [核心概览](/docs/zh-CN/concepts/)
- [工具总览](/docs/zh-CN/tools/)
- [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)
