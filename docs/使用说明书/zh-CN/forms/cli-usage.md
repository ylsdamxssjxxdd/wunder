---
title: 命令行使用
nav_title: 使用
summary: 命令行使用：子命令、斜杠命令与键位，以及命令行独有与缺失的能力。
updated_at: 2026-11-14
---

# 命令行使用

## 使用

第一次会话：

```bash
wunder-cli
> 帮我写一个 Hello World 的 Python 脚本
```

看到模型思考 → 工具批次 → 结果 → 回复就说明跑通。

### 子命令

| 子命令 | 别名 | 作用 |
| --- | --- | --- |
| `exec` | `e` | 非交互执行一次智能体任务；`--json` 走 JSONL 事件流 |
| `resume` | — | 恢复历史会话 |
| `trajectory` | — | 查看、搜索与导出线程轨迹；支持 JSON 与双向游标 |
| `tool` | — | 直接运行内置 / MCP / 技能工具 |
| `mcp` | — | 管理本地 MCP 服务器 |
| `skills` | — | 管理当前用户本地技能 |
| `config` | — | 查看与修改运行配置 |
| `stress-thread` | — | 生成本地合成线程做渲染压测 |
| `doctor` | — | 诊断本地运行环境 |
| `cloud` | — | 管理本地云端连接与设备互通 |
| `completion` | — | 生成 Shell 补全脚本 |

### 斜杠命令

`/help`（`/h`）、`/status`、`/resume`（`/r`）、`/trajectory`（`/trace`）、`/new`、`/clear`（`/cls`）、`/model`（`/model add` 登记新端点）、`/permissions`（`/perm`）、`/plan`、`/goal`、`/edit`、`/init`、`/attach`、`/diff`、`/review`、`/skills`、`/rename`、`/compact`、`/mcp`、`/config`（`/config edit` 编辑配置）、`/exit`、`/quit`（`/q`）。

没有 `cloud`、`cron`、`channel`、`memory`、`knowledge` 斜杠命令：`cloud` 是子命令形式，后四者在命令行上没有管理界面。

### 键位

| 键 | 作用 |
| --- | --- |
| `/` | 打开命令面板 |
| `!` | 直接执行 shell |
| `Tab` | 补全 |
| `@` / `#` / `$` | 文件 / 技能 / 工具提及 |
| `Ctrl+V` | 粘贴图片；也可把图片或文件拖进来 |
| `Alt+,` / `Alt+.` | 降低 / 提高推理强度 |
| `Ctrl+R` / `Ctrl+S` | 搜索输入历史 |
| `Ctrl+G` | 外部编辑器 |
| `Ctrl+O` | 复制最后回复 |
| `Ctrl+T` | 完整转录 |
| `Ctrl+C` | 退出 |
| `Esc` | 中断当前轮次；空闲时清空草稿 |
| `F4` | 切换鼠标模式（应用接管 ↔ 交回终端选择） |
| `←` | 任务中心 |
| `?` | 快捷键面板 |

## 独有与缺失

- **只有命令行能做**：`exec --json` 事件流脚本契约、`cloud` 子命令全套、`doctor` 环境诊断、`stress-thread` 压测、`completion` 补全，以及不经智能体主循环直接跑 `tool` / `mcp` / `skills`。
- **命令行没有**：定时任务被显式关闭（能看旧任务，但不再排新任务）；渠道、知识库、长期记忆与形象没有管理界面；无语音输入、无截图采集、无托盘与全局热键；宿主本身就是终端，所以没有交互式终端面板。
