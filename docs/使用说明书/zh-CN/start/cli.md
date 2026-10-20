---
title: 舵机（cli）使用
summary: 手工本地操作入口。TTY 下进交互界面、非 TTY 下次性执行；11 个子命令、22 条斜杠命令、exec --json 事件流、cloud 子命令全套；定时任务与引擎排队在舵机侧关闭。
updated_at: 2026-10-16
---

# 舵机（cli）使用

手工本地操作入口，属于可选形态。日常使用装蛹（desktop）；终端驱动、脚本化与自动化场景用舵机。它同进程调用引擎，复用同一套配置、工具与存储，不另起一套执行链路。

## 适用条件

| 场景 | 选舵机 |
| --- | --- |
| 习惯终端操作 | ✅ |
| 需要脚本化、自动化、CI/CD | ✅ |
| 需要管道输出（JSONL） | ✅ |
| 需要 `cloud` 子命令运维设备互通 | ✅ |
| 不想装蛹 | ✅ |
| 需要图形界面 | ❌ 选蛹 |
| 需要浏览器访问 | ❌ 选蜂巢（随舰体） |

## 装与启动

```bash
# 编译（需要 Rust 工具链）
cargo build --release -p wunder-cli

# 运行（无必需参数）
./target/release/wunder-cli
```

- **TTY**（stdin/stdout/stderr 全部是 TTY）：进入**交互式界面**
- **非 TTY**：自动降级为**一次性执行**；管道、脚本走这条
- **数据目录**：`WUNDER_HOME` → `~/.wunder`
- **工作区**：启动命令时所在的目录；`-C/--cd` 可指定其他目录
- **用户配置**：`~/.wunder/config.toml`

第一次会话：

```bash
wunder-cli
> 帮我写一个 Hello World 的 Python 脚本
```

看到模型思考 → 工具批次 → 结果 → 回复即跑通。`wunder-cli doctor` 检查本地运行环境是否就绪。

## 子命令

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

### cloud 子命令

设备互通与云工作区运维全套：

```
cloud login | logout | status | models | refresh | logs
cloud devices                    # 本账号互通节点
cloud ws ls / cat / pull / push  # 云端工作区
cloud threads / thread           # 云端线程列表与详情
cloud send / watch               # 经互通台账驱动线程
cloud audit / approve            # 命令台账与审批
```

### exec --json 事件流

每行一个 JSON 对象，事件名 `thread` / `turn` / `item.*`。用于管道与自动化：

```bash
wunder-cli exec --json "整理当前目录的文件" | jq -c 'select(.type=="item.text")'
```

## 斜杠命令

共 22 条：

| 命令 | 别名 | 作用 |
| --- | --- | --- |
| `/help` | `/h` | 帮助 |
| `/status` | — | 当前状态 |
| `/resume` | `/r` | 恢复历史会话 |
| `/trajectory` | `/trace` | 查看当前或指定线程轨迹 |
| `/new` | — | 新建线程 |
| `/clear` | `/cls` | 清屏 |
| `/model` | — | 切换模型；`/model add` 登记新端点 |
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
| `/config` | `/config show` 是子形式 | 查看生效配置；`/config edit` 编辑 config.toml |
| `/exit` | — | 退出 |
| `/quit` | `/q` | 退出 |

**没有** `cloud`、`cron`、`channel`、`memory`、`knowledge` 斜杠命令：`cloud` 是子命令形式；后四者在舵机上没有管理界面。

## 轨迹日志

交互界面输入 `/trajectory`（别名 `/trace`）查看当前线程，或 `/trajectory <session_id>` 查看其他线程。任务中心按 `t` 打开选中线程的轨迹，无需切换执行线程。

顶部提供顺序 / 耗时、轮次折叠、工具调用折叠和缩放；中间是输入、模型、工具三条泳道；下方为按用户轮次 / 模型轮次排列的台账，选中后查看详情。宽终端并排显示台账与详情，窄终端按 `i` 切换详情页。

| 键 | 轨迹查看操作 |
| --- | --- |
| `↑↓` / `j k`、`PageUp PageDown` | 选择记录；到窗口边缘自动读取较早 / 较新记录 |
| `Enter` | 进入选中用户轮次；轮次内切换详情焦点 |
| `i` / `→` | 切换详情焦点；焦点在详情时方向键和翻页键滚动正文 |
| `Tab` / `Shift+Tab` | 切换概览、正文、原始数据、参数、结果、契约、耗时等适用页签 |
| `h` / `l`（详情内） | 左右平移长行 |
| `d`、`+ -`、`0` | 顺序 / 耗时、围绕当前条目缩放、复位缩放 |
| `t`、空格、`c` | 折叠全部轮次、当前轮次、同模型轮次的工具调用 |
| `/` | 输入关键词后按 Enter 全文搜索；单次最多 20 个命中，提示更多时缩小查询范围 |
| `f`、`r` | 切换实时跟随、刷新 / 重试；手动浏览暂停跟随 |
| `y`、`e` | 复制当前详情页签、导出完整 JSONL 到启动目录（显示保存位置） |
| `Esc` / `q` | 返回上一层；`?` 查看完整帮助 |

轨迹与其他端共用引擎投影和正文接口，后台查询不阻塞终端操作。列表窗口最多保留 600 条记录，双向游标允许重读已移出的记录；单条详情预览上限 1 MiB，各页签最多 16,384 行，超出时明确提示并可导出完整日志。导出保留用户可见记录，系统提示词内容保持隐藏。

脚本和管道使用独立子命令（默认读取当前工作区最近线程，可显式传入线程 ID）：

```bash
wunder-cli trajectory --raw
wunder-cli trajectory <session_id> --turn <root_turn_id> --limit 80 --raw
wunder-cli trajectory <session_id> --before <cursor> --raw
wunder-cli trajectory <session_id> --after <cursor> --raw
wunder-cli trajectory <session_id> --search <query> --raw
wunder-cli trajectory <session_id> --item <item_id>
wunder-cli trajectory <session_id> --export trajectory.jsonl
wunder-cli trajectory <session_id> --export -
```

`--before` / `--after` 互斥，使用响应里的 `next_before` / `next_after`。`--raw` 或全局 `--json` 输出结构化数据。导出文件必须尚不存在，防止覆盖；最后一行 `export_complete` 表示导出完成，缺少时视为不完整文件。

## 键位

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
| 鼠标点击 | 选中卡片；点在「Show details / Show less」行即展开或收起；点在输入框定位光标 |
| 鼠标拖拽 / 双击 / 三击 | 拖拽选中多行并复制、双击取词并复制、三击取整行并复制；转录上的标记是反显，随时可右击再复制 |
| `Ctrl` + 点击链接 | 用系统默认程序打开该 http/https/file 链接 |
| `F4` | 切换鼠标模式：capture（应用接管点击与滚轮）↔ native（交回终端做选择、复制与滚轮） |
| `?` | 快捷键面板 |
| `←` | 任务中心 |

## 与其他界面的能力差

### 舵机独有

- `exec --json` 事件流脚本契约
- `cloud` 子命令全套
- `doctor` 本地运行环境诊断
- `stress-thread` 本地合成线程
- `completion` Shell 补全
- `tool` / `mcp` / `skills` 直接命令行运行（不走智能体主循环）

### 舵机没有

- **定时任务被显式关闭**：不能新增、修改、启用或立即运行定时任务；但查看状态、列表、删除与禁用仍可用——所以你能看到旧任务列表，舵机却不会再排新任务。
- **引擎排队也被显式关闭**：运行中再发一条消息不会进排队条，而是直接撞会话锁，被判「该用户已有会话正在执行」，要等满忙窗口才返回。这和蛹相反——蛹会强制打开队列。想在运行中插话、排队或改写下一条，请用蛹或蜂巢。
- **渠道 / 知识库 / 长期记忆 / 形象**在舵机上**没有管理界面**
- **无语音输入**：舵机没有 ASR；贴图靠 `Ctrl+V` 与拖拽
- **无交互式终端 UI**：宿主本身就是终端；shell 走 `!` 前缀
- **无截图采集 / 托盘 / 全局热键**

## 运行时文件

舵机默认把运行时文件放在 `~/.wunder` 下：

- 运行时配置与 SQLite 状态：`~/.wunder/cli/WUNDER_TEMP/`
- 引擎配置 YAML：`~/.wunder/config/wunder.yaml`，与蛹共用同一份；舵机读回它、按层叠投影后回写，模型清单因此两边可见
- 用户工具与技能：`~/.wunder/user_tools/` 与 `~/.wunder/skills/`
- 临时文件 API：`WUNDER_TEMP_DIR_ROOT` 指向同一个 `~/.wunder/cli/WUNDER_TEMP/`

脚本需要隔离运行目录时，用 `--temp-root <path>` 覆盖。

## 自动化场景

- CI/CD 流水线里调用 `exec --json` 消费 JSONL
- 批量任务：外部调度循环调 `exec`
- 管道：`cat input.txt | wunder-cli` 触发非 TTY 降级
- 与 `tool` / `mcp` / `skills` 组合做纯工具运行
- **定时任务不在舵机里做**：舵机关闭了 cron；请放到蛹或蜂巢的定时任务页面
