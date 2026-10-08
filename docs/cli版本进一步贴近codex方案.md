# 舵机（cli）进一步贴近 codex 方案：单智能体 · 工作区中心 · 极简命令面

> 注：本文为历史方案记录；蜂群编排（swarm / beeroom / 编排态 / hive pack / 工蜂卡）已于本次重构移除，文中相关章节不再对应当前系统能力。

> 适用范围：`crates/wunder-cli/`（舵机）。
> 承接文档：`docs/本地易用重构方案.md`（蜂窝单智能体重构，已完成并验收）、`docs/cli版本实现方案.md`（舵机 N0–N7 与 §10 已落地）。
> 参考形态：OpenAI Codex CLI 的 Rust 实现（`codex-rs`）。
> 前提：原型阶段，**不考虑兼容性**，老旧/不合适的字段与架构直接移除，保持项目干净。

---

## 一、背景与目标

### 1.1 现状问题

蜂窝（desktop）已按「本地易用」重构为**单智能体 + 工作区为中心**，个人用户负担大幅下降。舵机（cli）虽然呈现层（N7）已经像素级对齐 codex，但产品形态仍是旧的多智能体治理思路，与蜂窝新范式脱节：

1. **多智能体残留**：`--agent` 参数、`/agent`、`/personality`、请求级 `agent_id` 覆盖、会话按智能体分组，个人用户不需要创建/切换/重复配置智能体。
2. **工作目录是私有沙箱**：`config.workspace.root` 默认 `~/.wunder/workspace`，启动目录只进 `allow_paths`；模型读写的是沙箱副本，与用户真实项目脱节——与桌面重构前的旧形态是同一问题。
3. **入口与交互双轨**：line-chat 与 TUI 两套对话循环并存（`chat` 子命令 + TTY 判定），心智不统一；`wunder-cli exec` 是「直跑 shell 命令」，不是 codex 语义的「非交互执行智能体任务」，与用户预期相反。
4. **斜杠命令 38 个**，其中约一半是多智能体/蜂群治理概念（agent/apps/branches/fork/ps/clean/backtrack/notify/personality…），远超个人用户需要。
5. **配置面过重**：首次启动把整份引擎 YAML（含 server/port、channels、gateway 等治理字段）复制到 `~/.wunder/config/wunder.yaml`，个人用户面对的是舰体治理配置。
6. **会话生命周期不 codex**：全局只有一个「当前会话」文件，没有「每次进入新线程 + 按工作区恢复」的心智；退出无摘要、无恢复提示。

### 1.2 目标

1. **单智能体**：整个 CLI 只有一个内置智能体 `__default__`，无创建/切换/覆盖入口（与蜂窝同一份固化逻辑）。
2. **cwd 即工作区**：启动目录就是工作区，工具直接读写真实项目文件夹；`-C/--cd` 切换；工作区在 `workspaces` 表自动建档，线程归属工作区。
3. **入口形态对齐 codex**：无子命令 = TUI（在 cwd 开新线程）；`exec` = 非交互一次执行；`resume` = 按工作区恢复；斜杠命令收敛为个人核心集。
4. **配置收敛为 codex 式 `~/.wunder/config.toml`**：精简投影，映射既有 ConfigStore，引擎 YAML 退为内部层。
5. **交互对齐 codex 键位与审批语义**：Esc 中断、Ctrl+T 转录、Ctrl+R 历史、审批面板 y/a/p/d/n/c、沙箱三档、退出摘要。
6. **呈现层保持既有 N7 对齐成果**，不改事件语义与 durable 回放底座。

### 1.3 设计原则

- 与蜂窝共用同一产品范式（单智能体、工作区中心、配置集中），三形态同源同引擎。
- 复用引擎资产（workspaces 表、`register_workspace_root`、`build_native_chat_request`、`__default__` 智能体、ThreadCatalogService），不另起执行链路。
- 对齐 codex 的「形态与交互心智」，不复制其 app-server daemon、OAuth 登录、guardian/auto-review 体系。
- 性能默认：无界队列/缓存禁止、有界分页、按帧合并、启动首帧不被后台初始化阻塞。
- 一切从源头（运行时/后端）解决，不在表层缝补。

### 1.4 非目标（本次不做）

- 不做 codex 的 daemon（app-server）、OAuth 登录/登出、`codex login` 流程（wunder 用 API key 体系）。
- 不做 worktree（`--worktree`、`/worktree`）：codex 中它由独立 crate + feature 门控且非默认（`codex-rs/exec/src/lib.rs:426`），原型阶段不做。
- 不做 guardian / auto-review（`--approve-for-me`）、hooks、插件市场、memories 斜杠命令面板（记忆碎片能力仍由引擎保留，不在 CLI 增加入口）。
- 不做多智能体/蜂群在 CLI 的任何入口（子智能体与蜂群是引擎能力，不在个人 CLI 暴露）。
- 不做 Markdown 富文本扩展、语音、桌宠（沿用既有约束）。

---

## 二、核心范式转变

| 维度 | 旧 CLI | 新 CLI（codex 形态） |
|---|---|---|
| 中心对象 | 智能体（agent_id 覆盖/切换） | **工作区（cwd）** |
| 智能体数量 | 多个可覆盖 | **固定内置 `__default__`**，无覆盖 |
| 工作目录 | `~/.wunder/workspace` 私有沙箱 | **启动目录真实直写**，`-C` 切换 |
| 线程归属 | `chat_sessions.agent_id` 维度 | **`chat_sessions.workspace_id`** |
| 入口 | ask/chat/resume/exec(shell)/… 多子命令 + line-chat | 无子命令=TUI；`exec`（agent 执行）；`resume` |
| 会话生命周期 | 全局「当前会话」文件自动续用 | **每次进入新线程**，`resume`/`Ctrl+R` 恢复 |
| 配置 | 整份引擎 YAML | **`~/.wunder/config.toml` 精简投影** + profile |
| 恢复 | `/resume` 数字列表 | **按 cwd 工作区过滤**的恢复选择器 + `--all` |
| 命令中心（←） | 智能体 + 线程目录 | **工作区 + 线程树**（对齐蜂窝左栏） |
| 斜杠命令 | 38 个（含治理） | **核心约 20 个** |
| 审批语义 | suggest/auto_edit/full_auto | **on-request/never + 沙箱三档**（内部映射兼容） |
| 退出 | 无摘要 | **退出摘要 + `wunder-cli resume <id>` 提示** |

---

## 三、目标形态（codex 对齐点）

> 本节只描述对齐后的用户可见形态；后端落点见 §五、§六。证据标注为 `codex-rs` 相对路径（仓库 `D:\proj\参考\codex-main`）。

### 3.1 入口与启动

| 场景 | 行为 |
|---|---|
| `wunder-cli`（TTY，无参数） | 进入 TUI，工作区 = 当前目录，**新线程**；空态显示品牌 + 工作区名 + 模型 + 键位提示，输入即消失 |
| `wunder-cli "提示词"`（TTY） | 进入 TUI 并直接发送首轮（现状已支持，保持） |
| `wunder-cli "提示词"`（非 TTY / 管道） | 一次执行：正文输出 stdout，过程输出 stderr（对齐 `codex exec` 文本契约） |
| `wunder-cli -C <dir>` | 在指定目录开工作台（工作区切换） |
| 首次运行 | 生成 `~/.wunder/config.toml` 模板；模型未配置时 composer 上方给出配置指引（`wunder-cli config` / `/config`），不阻塞进入 |

启动体验对齐 codex 的 **provisional composer** 理念（`codex-rs/tui/src/startup_draft.rs`）：首帧即可编辑输入，目录/统计/模型状态等后台初始化渐进填充，输入零丢失。wunder 落地：`TuiApp::new` 中除终端接管外的加载（历史、目录、模型状态、会话统计）延后到首帧之后，逐项异步填充（现状已部分是异步，收口为「首帧不等待」即可）。

codex 的 preflight 是**鉴权**预检（`startup_preflight.rs:22-74`）而非 git 预检；wunder 无登录体系，对应预检为「模型是否已配置」，缺失时不隐藏 composer、只显示提示条。

### 3.2 会话生命周期与退出摘要

- **每次进入默认新线程**（绑 cwd 工作区），不再自动续「上次会话」；`--session` 与 `resume` 是唯一恢复途径。
- **恢复列表按 cwd 工作区过滤**（codex 语义：`resume` 列表按 cwd 过滤，`--all` 关闭，`codex-rs/tui/src/lib.rs:958-980`）；`wunder-cli resume --all` 显示全部工作区线程。
  > 落地（C4）：`ThreadListQuery.workspace_id` 与 `ThreadSnapshot.workspace_id` 已加入目录契约；单工作区列表走 SQL 分页（`list_chat_sessions_by_workspace`），工作区+父线程组合过滤走有界分页扫描。`wunder-cli resume --last`、TUI `/resume [--all]`、恢复选择器均按当前工作区过滤，`--all` 是显式逃生口。
- 线程 ID 保持现有 uuid 简单形态（codex 为 UUIDv7，`protocol/src/thread_id.rs:13-32`；wunder 沿用现有 ID 即可，不引入迁移）。
- **退出摘要**（对齐 `codex-rs/tui/src/app/exit_summary.rs:82-154`）：
  - 中断轮次 → 打印 `current turn was stopped`（本地化）；
  - 正常退出 → 打印 `wunder-cli resume <session_id>` 恢复提示；
  - 仅致命错误返回非零退出码（1）。
  > 落地（C4）：TUI 退出时由 `TuiApp::exit_summary()` 输出上述两行；`exec`/一次性执行改为按「本轮是否真正走完」判定退出码（收到 `error` 事件、流结束却没有 `final`、或 stop_reason 为 `tool_failure_guard`/`tool_no_progress_guard`/`max_rounds` → 1），不再依赖引擎从不产出的 `error|failed|cancelled|interrupted` 字面量。
- Ctrl+C 两段语义保持（运行中先中断轮次，空闲时退出；Esc 仅中断轮次，N7-F 已落地）。

### 3.3 键位（对齐 codex 默认键位表）

> codex 权威键位表：`codex-rs/tui/src/keymap.rs:1642-1944`（`built_in_defaults`）。两处与旧印象不符的 fact：**Ctrl+O 是复制**（不是隐藏思考）、**Ctrl+E 无 shell 展开**（是行尾），shell 直发靠输入前缀 `!`。

| 键 | codex 语义 | wunder 现状 | 本方案 |
|---|---|---|---|
| `Esc` | 中断当前轮次 | ✅ N7-F 已实现 | 保持 |
| `Ctrl+C` | 两段：中断 / 退出 | ✅ 已实现 | 保持 |
| `Ctrl+T` | 打开/关闭全屏转录 | ✅ 已实现 | 保持 |
| `Ctrl+R` / `Ctrl+S` | composer 输入历史反向/正向搜索 | ❌ 无 | **新增**输入历史搜索（轻量，不挂恢复选择器） |
| 落地（C4） | | | 匹配到的历史条目直接写进输入框作为预览（Enter 采用、Esc 还原原草稿、Ctrl+R 继续向更早翻），footer 显示 `reverse-i-search` 状态与匹配位置；恢复选择器仍归 `/resume` |
| `Ctrl+G` | 打开外部编辑器 | 有 `/edit` 无绑定 | 绑定 `/edit` |
| `Ctrl+O` | 复制最后回复 | ❌ 无 | **新增**复制最后回复 |
| `Ctrl+L` | 清屏开新会话（同 `/clear`） | ❌ 无 | **新增** `/clear` + 绑定 |
| `F2` / `F3` | 警告面板 / 转录查找 | ✅ 已实现 | 保持 |
| `F4` | activity 聚焦 | wunder 为鼠标模式切换 | 保留现状，帮助文案标注差异 |
| `?` | 快捷键覆盖层 | ✅ 已实现 | 保持 |
| `Tab` | 任务运行中「排队」下一条消息 | 现状为编辑器行为 | 保留现状（原型不强制对齐） |
| `Alt+,` / `Alt+.` | 推理强度减/增 | ❌ 无 | **新增**（对齐蜂窝输入区模型/强度切换） |
| `@` | 文件提及弹窗 | 有 `/mention` 命令 | **新增** `@` 触发文件提及（复用 `/mention` 索引） |
| `!` | 前缀 = shell 命令直发 | ❌ 无 | **新增**（等价 `tool run execute_command`） |
| `/` | 斜杠命令弹窗 | ✅ 已实现 | 保持（命令集按 §4.4 收敛） |
| `↑/↓` | 输入历史 | ✅ 已实现 | 保持 |
| 审批面板 | `y/a/p/d/n/c`（接受一次/接受会话/修改策略/拒绝/取消） | 现有审批模态键位不同 | **改为 codex 键位**（`approval_events.rs:68-111`；wunder `ApprovalResponse` 三态可映射） |

### 3.4 审批与沙箱（两档词表对齐 codex，映射既有引擎）

codex 事实（`protocol/src/protocol.rs:961-984`、`protocol/src/config_types.rs:104-114`）：

- `sandbox_mode`：`read-only`（默认）/ `workspace-write` / `danger-full-access`；
- `approval_policy`：`on-request`（默认，模型决定何时问；`on-failure` 已是其别名）/ `never` / `granular` / `untrusted`（项目未信任时强制批准）。

wunder 映射（不改引擎判定，只改 CLI 词表与默认值）：

| codex 词表 | wunder 引擎落点 | 说明 |
|---|---|---|
| `sandbox_mode = workspace-write`（CLI 默认） | 工具根 = cwd + 允许根收敛到 cwd（丢弃继承自舰体模板的 `*`）+ 工作区外绝对路径直接拒绝 | 个人本地默认：工作区内自由读写执行，越界即拒 |
| `sandbox_mode = read-only` | 新增只读执行策略：写/执行类工具需确认或拒绝（复用 `exec_policy` 判定，前端确认不替代后端鉴权） | 与蜂窝「权限模式」三档同语义 |
| `sandbox_mode = danger-full-access` | `approval_mode = full_auto` + allow_paths 放开 | 高风险，需显式确认 |
| `approval_policy = on-request` | 现有 `auto_edit`（写免批、执行需批）；`suggest` 保留为「全部需批」 | 见下方实现说明 |
| `approval_policy = never`（CLI 默认） | 现有 `full_auto` | 失败直接回给模型 |
| `auto_edit` | 保留为兼容别名（写免批、执行需批） | 旧配置不破坏 |

> **实现说明（落地时的语义修正）**：wunder 引擎的审批粒度是「工具类别」（写 / 执行 / 控制），没有 codex 的「越界才请求升级」概念，因此把 `on-request` 实现为 `suggest` 会让本地 CLI 的每一次写文件都要批准，与「工作区内自由读写执行」的目标相反。落地取法为：**默认 `never`（`full_auto`）+ 由工作区边界兜底**（允许根收敛到 cwd，工作区外绝对路径返回「路径越界访问被禁止」）；`on-request` 映射到 `auto_edit`，需要更高摩擦时用 `--approval-mode suggest`。审批选择持久化在 `~/.wunder/cli/config/approval_mode.txt`（引擎配置每次启动重建，用户选择不能写在里面）。

- 命令入口：`-s/--sandbox`、`--approval-mode on-request|never`（现有 `--approval-mode` 词表收敛为 codex 式，旧值保留别名）；`/permissions` 斜杠命令显示并切换当前沙箱/审批策略（codex 同名命令）。
- TUI 批准面板改为 codex 底部 list-selection 形态（`bottom_pane/approval_overlay.rs:1-12`）：`y`=接受一次、`a`=本会话接受、`n`=拒绝、`c`=取消；`p/d`（策略修改）原型阶段不提供（无 execpolicy 体系），不伪造键位。
- 不引入 codex 的 `granular`/`untrusted` 信任等级体系；`--dangerously-bypass-approvals-and-sandbox`（yolo）以「`-s danger-full-access` + `--approval-mode never`」组合表达，不新增独立 flag（注：codex 并无 `--full-auto` 参数，`codex-rs` 全库 grep 无此 flag；完全自动化即上述组合）。

### 3.5 空态与 composer

- 空态：保留既有品牌横幅/齿轮动画（输入即消失，现状已如此），内容收敛为「工作区名 · 模型 · `/help` 与键位提示」，对齐 codex blossom 空态的克制风格（`codex-rs/tui/src/app/empty_state_policy.rs:10-48`）。
- composer 占位符：`Ask wunder to do anything`（本地化），N7 已对齐 codex 样式；footer 已对齐（模型 · 工作目录 · 任务标题 + `←` `?` 警告 `N% context left`）。

---

## 四、命令面设计

### 4.1 子命令面

| 子命令 | 旧语义 | 新语义（codex 对齐） |
|---|---|---|
| （无子命令） | TTY 进 TUI；非 TTY 一次执行 | 保持（**唯一交互入口**） |
| `ask` | 单轮问答 | **移除**（并入 `exec`） |
| `chat` | line-chat 交互循环 | **移除**（TUI 是唯一交互形态，codex 亦然） |
| `exec` | 直跑 shell 命令 | **改为 codex 语义**：非交互执行智能体任务（见 §4.3）；shell 直跑由 `tool run execute_command` 与 `!` 前缀覆盖 |
| `resume` | 恢复会话 | 保留；补 `--all`（关闭 cwd 过滤）、`--last`、`[SESSION_ID] [PROMPT]` 语义与 codex 同构（`codex-rs/exec/src/cli.rs:205-254`） |
| `tool` | run/list | 保留 |
| `mcp` | list/get/add/remove/enable/disable/login/logout/test | 保留（形态已近 `codex mcp`） |
| `skills` | list/enable/disable/upload/remove/root | 保留（wunder 特有，蜂窝同源） |
| `config` | show/set-tool-call-mode/set-approval-mode | **改为面向 config.toml**：`config show`（生效配置）、`config path`（文件路径）、`config edit`（打开编辑器，可选）、`config validate`（严格模式校验）；tool-call-mode/approval-mode 设置并入 toml 与 `/permissions` |
| `doctor` | 诊断 | 保留 |
| `completion` | 补全脚本 | 保留 |
| `review` | — | 可选（codex 有 `codex review`；wunder 已有 `/review`，原型阶段列可选，不阻塞主线） |

### 4.2 全局参数（对齐 codex `SharedCliOptions`，`codex-rs/utils/cli/src/shared_options.rs:10-77`）

| 参数 | 现状 | 方案 |
|---|---|---|
| `-m/--model` | ✅ | 保持 |
| `-C/--cd DIR` | ❌ | **新增**（工作区切换，codex 同名） |
| `-s/--sandbox read-only\|workspace-write\|danger-full-access` | ❌ | **新增** |
| `--approval-mode on-request\|never` | 有（suggest/auto_edit/full_auto） | 词表收敛（旧值保留别名） |
| `--json` | ✅ | 保持（exec 契约 §4.3） |
| `--attach` | ✅ | 保持（codex 为 `-i/--image`，不另加别名） |
| `-p/--profile NAME` | ❌ | **新增**（加载 `~/.wunder/profiles/<name>.toml` 叠加，codex v2 profile 形态） |
| `--strict-config` | ❌ | **新增**（config.toml 未知键报错，codex 同名） |
| `--session` | ✅ | 保持（指定恢复/续用线程） |
| `--no-stream`、`--lang`、`--config`、`--temp-root`、`--user` | ✅ | 保持 |
| `--worktree`、`--skip-git-repo-check`、`--add-dir` | ❌ | 不做（§1.4）；`--add-dir` 对应能力由工作区 `allow_paths` 扩展表达，原型不提供 |

### 4.3 exec 模式契约

```
wunder-cli exec [OPTIONS] [PROMPT]
```

- PROMPT 省略时从 stdin 读取；`-` 强制 stdin（与 codex 同构，`codex-rs/exec/src/cli.rs:81-85`）。
- **文本输出（默认）**：过程与工具轨迹输出 **stderr**（工具执行标签、完成/失败状态、推理摘要受 hide-reasoning 开关控制），最终回复正文输出 **stdout**（codex 契约：`event_processor_with_human_output.rs:68-160`）。
- **`--json`**：stdout 输出 JSONL 事件流。事件契约对齐 codex 骨架（`codex-rs/exec/src/exec_events.rs:11-37,107-133`），内部映射既有 `StreamEvent`：
  - `thread.started` / `turn.started` / `turn.completed`（含 usage）/ `turn.failed` / `item.started` / `item.updated` / `item.completed` / `error`；
  - item 类型：`agent_message` / `reasoning` / `command_execution` / `file_change` / `mcp_tool_call` / `web_search` / `error`（wunder 有对应物则映射，无则不做假事件）。
- **`-o/--output-last-message FILE`**：最后一条消息写入文件（codex 同名短选项）。
- **`--color always|never|auto`**（默认 auto）。
- **退出码**：成功 0；致命错误/轮次失败/中断 1（codex 契约：`codex-rs/exec/src/lib.rs:1296-1402`）。
- 可选扩展（原型不做，保留位）：`exec resume <SESSION_ID>`、`exec fork <SESSION_ID>`、`--output-schema FILE`、`--ephemeral`。

> **落地（C5）**：`exec_events::ExecEventWriter` 实现上述契约，`--no-stream --json` 输出同一形状（单次突发）。实现细节与刻意的取舍：
> - **条目身份**：普通工具与命令共用一个稳定键（`tool_call_id`）。命令先以 `tool_call` 出现、再以 `command_session_start` 出现，若不共用键，同一条命令会生成两张卡；现在第二条事件把已宣告的条目**升级**为 `command_execution`，终态关闭时同时记住该调用已结束，尾随的 `tool_result` 不会重开条目（有单测锁定）。无调用 ID 的结果仍如实生成一条已完成条目，不丢工作痕迹。
> - **item 类型**：`apply_patch`/`patch` → `file_change`，搜索/抓取类 → `web_search`，其余按 `tool_presentation` 分类器落 `command_execution`/`file_change`/`mcp_tool_call`/`tool`；未知工具落到 `tool`，不编造类型。
> - **推理（reasoning）不做假事件**：引擎 `stream()` 词汇里没有独立的推理事件（推理内容经 durable 条目下发），因此 JSONL 不产出 `reasoning` 条目。
> - **审批**：以 `item_type: "approval"` 的条目呈现 `requested` 与批准/拒绝结果（wunder 的真实对应物）。
> - **有界性**：单条目文本与输出各限 8 000 字符（超限保留头部并标注截断），已关闭调用键最多记 128 条。
> - **`--color` 只作用于过程流**：`always` 时 stderr 的进度行带语义色（dim 结构、红失败、绿成功），**stdout 永不带 ANSI**，管道里拿到的回复始终干净；`never`/`NO_COLOR`/非终端一律无色。
> - **退出码矩阵**：`error` 事件、流结束却没有 `final`（被中断或取消）、`stop_reason` 为 `tool_failure_guard`/`tool_no_progress_guard`（放弃）或 `max_rounds`（触顶）→ 打印原因并以 **1** 退出；其余 **0**。

### 4.4 斜杠命令收敛（38 → 核心约 20）

**保留**（个人核心集）：

| 命令 | 说明 |
|---|---|
| `/help` | 帮助（键位 + 命令） |
| `/new` | 新线程（当前工作区） |
| `/clear` | 清屏开新会话（**新增**，对齐 codex，绑定 Ctrl+L） |
| `/resume [--all]` | 恢复选择器（按工作区过滤，`--all` 关闭） |
| `/rename` | 线程改名 |
| `/compact` | 压缩上下文 |
| `/model` | 查看/切换模型与推理强度（codex 同名语义） |
| `/permissions` | 显示/切换沙箱与审批策略（**新增**，替代 `/approvals` 旧语义；`/approvals` 保留为别名） |
| `/mcp` | MCP 管理 |
| `/skills` | 技能管理 |
| `/status` | 会话配置与 token 用量 |
| `/diff` | git diff 摘要（含 untracked） |
| `/review` | 用模型审查当前改动 |
| `/init` | 生成 AGENTS.md 模板（codex 同名） |
| `/attach` | 附件队列 |
| `/edit` | 外部编辑器（绑定 Ctrl+G） |
| `/goal` | 长期目标（蜂窝目标条同源） |
| `/plan` | 让模型先出计划 |
| `/config` | 查看/编辑 config.toml |
| `/quit`、`/exit` | 退出 |

**移除**（多智能体/治理/重复，共 18 个）：`/agent`、`/personality`、`/apps`、`/branches`、`/fork`、`/backtrack`、`/ps`、`/clean`、`/notify`（迁移为配置项）、`/mention`（由 `@` 提及替代）、`/mouse`、`/statusline`、`/debug-config`、`/session`、`/system`、`/tool-call-mode`（并入 `/model` 与 config.toml）、`/threads`（由 `←` 命令中心替代）、`/config show` 独立命令（并入 `/config` 子命令形态）。

---

## 五、数据与后端改造

> 原则：工作区是已存在的一等实体（蜂窝重构落地），CLI 复用同一表与注入链路，不新建平行机制。

### 5.1 工作区自动建档与线程归属

1. 启动时按 `launch_dir`（规范化后）查 `workspaces` 表（`(user_id, root_path)` 唯一）：
   - 命中 → 直接使用该 `workspace_id`；
   - 未命中 → 自动建档：`name` = 文件夹名、`icon` = `folder`、`color` = `blue`、`root_path` = 启动目录（复用 `wunder-desktop/native_workspace.rs` 的建档/校验逻辑，下沉为 CLI 可用的共享 helper 或 CLI 侧小 façade；不复制实现）。
2. `-C DIR` 切换启动目录，同样查表/建档。
3. 新建线程（`ensure_cli_session_record`）写入 `workspace_id`（`ChatSessionRecord.workspace_id` 已存在）；`agent_id` 恒为 `None`→`__default__`。
4. 归档/删除工作区不在 CLI 提供入口（蜂窝设置页职责），CLI 只读使用。

### 5.2 工具根注入（cwd 直写）

1. 发送线程消息时按 `session.workspace_id` 读取 `root_path`。
2. 复用蜂窝同链路：`state.workspace.register_workspace_root(workspace_id, root_path)`（`wunder-runtime/src/services/workspace.rs:353`）+ 请求携带 `workspace_id`（`WunderRequest.workspace_id`，orchestrator `resolve_workspace_id` 消费，`wunder-runtime/src/orchestrator/request.rs:28-33`）。
   - TUI/line-chat 的请求构造收敛为一个共享 builder（与 `api/chat.rs::build_native_chat_request` 同规则），不再散落两处。
3. 路径安全：`is_within_root` 以 cwd 为界；`allow_paths` 收敛为 cwd（+ wunder home 技能等既有目录）；高风险操作仍以后端鉴权为准。
4. 工具返回路径相对化：展示用工作区相对路径，不暴露主机绝对路径（复用现有 `path_display`）。

### 5.3 单智能体固化

- 移除 `--agent` 参数、`/agent`、`/personality` 及其请求覆盖路径；`agent_id` 恒为 `None`，引擎侧按 `__default__` 解析（`default_agent_protocol`、`agent_management` 已由蜂窝落地，CLI 直接受益）。
- 智能体级配置（system prompt、默认模型、推理强度、工具启用集）走 §六的 config.toml 与既有 `__default__` 记录，CLI 不提供多智能体表单。
- 提示词冻结规则不变：线程首次确定后 system prompt 冻结；长期记忆仅线程初始化时注入一次。

### 5.4 会话迁移

- 复用蜂窝 `migrate_local_workspaces` 模式（`wunder-desktop/runtime.rs:1687-1750`）：启动时幂等迁移——无 `workspace_id` 的旧 CLI 会话批量挂到「默认工作区」（优先 cwd 工作区，其次首个已有工作区，否则按旧根新建），分页处理（页 200），迁移后不再保留多智能体字段语义。
- 旧的「当前会话」文件（`sessions/current_session.json`）语义删除：不再自动续用（§3.2），文件可移除。

### 5.5 AGENTS.md 注入（codex 核心能力引入）

> codex 事实：从 cwd 向上找 project root markers（默认 `.git`）确定项目根，收集「根 → cwd」沿途所有 `AGENTS.md` 拼接，不越过项目根；`AGENTS.override.md` 优先；全局 `~/.codex/AGENTS.md`；内容预算 `project_doc_max_bytes`（`codex-rs/core/src/agents_md.rs:1-18,42-72`、`config_toml.rs:335-337`）。

wunder 落点：

1. 发现：启动时扫描 `cwd → 项目根（默认 `.git`）` 沿途 `AGENTS.md`，外加 `~/.wunder/AGENTS.md` 全局指令；预算默认 32 KiB（可配置 `project_doc_max_bytes`），超出截断并提示。
2. 注入：**仅在线程初始化时注入一次**（作为系统提示词/首轮上下文的一部分），遵守「提示词冻结 + 记忆一次注入」规则；后续轮次不改写。
3. `--sandbox read-only` 或用户关闭 `project_doc` 时跳过项目文档，只保留全局指令（对齐 codex 的 untrusted 降级语义，`agents_md.rs:64-66`）。
4. `/init` 生成模板（已有实现，模板文案按新范式修订）。

### 5.6 线程中心（`←`）工作区化

- codex 的 agents command center 实际展示的是**线程（任务）**而非智能体（`codex-rs/tui/src/app/agents_overview_view.rs:1-3`，按 Needs input/Working/Ready/Inactive 分组）；wunder 现有命令中心数据与交互已对齐，只差产品形态收敛：
  - 删除 `ThreadGroupMode::Agent` 分组（`wunder-cli/tui/command_center.rs:20`），新增 `ThreadGroupMode::Workspace`；
  - 行显示工作区名（短名 + 颜色点，可复用蜂窝工作区颜色），详情栏显示工作区路径；
  - 状态页签、搜索、分页、详情、只读监视全部保留（复用 ThreadCatalogService 资产）。
- 命令中心标题/文案从「agent command center」改为「任务中心 / threads」（本地化）。

---

## 六、配置形态：config.toml 投影

### 6.1 用户配置

首次运行生成 `~/.wunder/config.toml`（带注释模板）。字段是 codex 式精简投影，**写回既有 ConfigStore**（`~/.wunder/config/wunder.yaml` 退为引擎内部层，用户不再直接编辑）：

| toml 字段 | 引擎落点 | 说明 |
|---|---|---|
| `model` | `llm.default` | 默认模型名 |
| `model_provider` | 模型 provider/base_url/api_key（models 表） | 无登录，API key 体系 |
| `model_reasoning_effort` | 会话推理强度默认值 | low/medium/high 档位映射现有 effort |
| `approval_policy = "on-request" \| "never"` | `security.approval_mode`（suggest/full_auto） | §3.4 映射 |
| `sandbox_mode = "workspace-write" \| "read-only" \| "danger-full-access"` | 工具根/只读策略/full_auto | §3.4 映射 |
| `project_doc_max_bytes` | AGENTS.md 注入预算 | 默认 32768 |
| `project_doc = true` | AGENTS.md 注入开关 | — |
| `mcp_servers` | user_tool_store MCP 列表 | 与 `wunder-cli mcp` 同源 |
| `skills`（enabled 集合） | `skills.enabled` | 与 `/skills` 同源 |
| `notify` | turn 通知（bell/osc9/command） | 现有 `/notify` 迁移为配置项（命令移除） |
| `language` | `i18n.default_language` | zh-CN / en-US |
| `send_key` | 发送键偏好 | Enter / Ctrl+Enter（蜂窝设置同源） |
| `ui`（hide_reasoning 等） | 呈现偏好 | 思考折叠开关 |

### 6.2 profile

- `-p/--profile NAME` 加载 `~/.wunder/profiles/<name>.toml` 叠加在用户配置之上（codex v2 profile 形态，`codex-rs/utils/cli/src/shared_options.rs:34-36`），可用于「工作/宽松/只读」等场景预设。

### 6.3 项目级配置

- `<cwd>/.wunder/config.toml`（**不用**项目根 `config.toml` 名，避免与用户项目自有的 config.toml 冲突——与 codex 直接读 `$PWD/config.toml` 的做法有意差异，wunder 更保守）。
- 优先级（低→高）：内置默认 < 用户 `~/.wunder/config.toml` < profile < 项目 `.wunder/config.toml` < CLI 参数（`-m`/`-s`/`--approval-mode` 等）。
- `--strict-config`：未知键报错（codex 同名参数）。

### 6.4 与蜂窝设置页的关系

- 同一 ConfigStore 的两个视图：蜂窝 = 12 分类设置页，舵机 = config.toml + `/config`；互不另起执行链路，两边改动即时互通。
- `wunder-cli config show` 输出「生效配置」合并视图（toml 层叠结果），`config validate` 供脚本校验。

### 6.5 落地状态与后续（C3 实现记录）

已实现：`~/.wunder/config.toml` 注释模板（首次运行生成、永不覆盖）、profile（`-p/--profile`）、项目级 `<工作区>/.wunder/config.toml`、`--strict-config`、`config show|path|validate`，以及投影到引擎配置的字段：`model`、`model_reasoning_effort`、`[provider]`（base_url/api_key/max_context）、`approval_policy`、`sandbox_mode`、`project_doc`、`project_doc_max_bytes`、`language`。`config show` 会给出每个键来自哪一层；`/approvals` 与 `config` 子命令不再写引擎 YAML，用户选择的审批策略写入 toml（原来放在 `approval_mode.txt` 的旁路文件已删除）。

**已知残留（下一轮 C3 收尾）**：引擎 YAML 是持久文件而非每次重建，因此「用户改过又删掉」的键可能残留在 YAML 里（已对 `llm.default` 做模板回退兜底，`[provider]` 尚无兜底）；彻底解法是启动时由「仓库模板 + CLI 默认 + 用户 toml」重建 YAML，同时把 `/config` 模型向导（`apply_cli_model_config`）的写入目标从 YAML 改为 toml，使「用户只编辑 toml、YAML 纯派生」这一条不变量真正成立。

---

## 七、性能与兼容门槛

| 项目 | 门槛（沿用 `docs/cli版本实现方案.md` §6 + 本方案新增） |
|---|---|
| 启动 | 首帧不被工作区建档/迁移/目录加载阻塞（全部 spawn_blocking + 幂等 + 渐进填充）；provisional composer 输入零丢失 |
| 目录/恢复列表 | 分页（单页 ≤100）；按工作区过滤在服务层执行；UI 只渲染可视行 |
| 流式 | 按帧合并（约 16–33ms）；只更新活动尾块；durable 游标与重放语义不变（§10 资产） |
| 内存 | 线程投影、工具预览、命令输出、事件队列均有界；AGENTS.md 注入有字节预算 |
| 配置 | toml 解析失败降级默认配置并提示，不阻塞启动；层叠合并只读一次缓存 |
| 锁 | 工作区建档/迁移不持长锁；运行、审批、取消仍按 session ID 隔离 |
| 兼容 | 本地 SQLite、离线启动；32 位 Windows 7 与 Ubuntu 18.04 目标构建；无 daemon、无本机 HTTP/WS bridge；无色终端可读 |

---

## 八、分阶段实施

> 既有 N0–N7/§10 资产（线程注册表、durable 回放、ToolCallKey、呈现对齐、快照测试 270 用例）全部保留为底座；本阶段只做形态收敛，先清理后重建，每节点可独立验收。

**落地状态**：C0–C7 全部实现并验证（各节点末尾的「落地」小节记录了实现细节、取舍与证据）。最终验证基线：`cargo test -p wunder-cli` **290 单测 + 3 集成**全绿、`cargo check --workspace --all-targets` 零错误零 `wunder-cli` 告警、`cargo test -p wunder-runtime thread_catalog` 11 用例通过，均在本仓库主树执行；另有真机探针覆盖沙箱三档的边界行为（工作区外写入被拒 / `danger-full-access` 下放行、`config show` 报告的沙箱与审批词）与「删除的配置不再残留」。**仅剩需环境的两项**：真终端人工验收（键位手感、批准面板、`!` 与 `Ctrl+O` 的终端兼容性），以及 Win7 x86 / Ubuntu 18.04 的 CI 目标构建。

### C0 — 基线清理（命令面）

- 改动：删除 line-chat 循环与 `ask`/`chat` 子命令；`exec` 语义改为 agent 执行（先做文本契约，JSONL 契约在 C5）；删除 `--agent`、`/agent`、`/personality` 及多智能体斜杠命令与 handler；删除「当前会话」自动续用文件逻辑；`/notify` 迁移为 config.toml 配置项后移除命令。
- 交付：`wunder-cli` 无子命令 = TUI/一次执行；`exec` 输出「过程 stderr、正文 stdout」。
- 验收：`cargo check --workspace` 通过；help 输出与 §4.1 一致；无多智能体残留入口。

### C1 — 工作区直连

- 改动：启动工作区查表/建档（§5.1）；请求 builder 收敛并注入 `workspace_id` + `register_workspace_root`（§5.2）；`-C/--cd`；`allow_paths`/路径安全收敛；会话迁移（§5.4）。
- 交付：在任意目录启动，模型工具直接读写该目录；工具路径展示相对化。
- 验收：单测覆盖建档幂等、`(user,root)` 唯一、线程归属、工具 cwd 实际落在启动目录、越界写被拦截；迁移用例（旧会话挂默认工作区）。

### C2 — 单智能体固化 + AGENTS.md

- 改动：请求/会话 `agent_id` 收敛 `__default__`（复用引擎）；AGENTS.md 发现与线程初始化注入（§5.5）+ 预算截断；`/init` 模板修订；`project_doc` 配置开关。
- 交付：单智能体全链路；项目指令自动生效。
- 验收：注入仅发生在线程初始化（断言轮次间 system prompt 不变）；预算截断用例；read-only 下跳过项目文档。

### C3 — 配置 toml 投影

- 改动：`~/.wunder/config.toml` 生成/解析/合并层（§6.1-6.3）；profile（`-p`）；`config` 子命令 show/path/validate（+可选 edit）；`--strict-config`；旧 `config` 子命令的 set-* 移除。
- 交付：toml 层叠生效；wunder.yaml 退为内部层。
- 验收：优先级用例（默认<用户<profile<项目<CLI）；未知键 strict 报错；解析失败降级启动；与蜂窝设置页互写可见。

### C4 — 会话生命周期与恢复

- 改动：新线程默认语义（§3.2）；退出摘要与 resume 提示；`resume --all`；恢复选择器按工作区过滤；`Ctrl+R/Ctrl+S` 输入历史搜索。
- 交付：每次进入新线程；退出打印 `wunder-cli resume <id>`；跨工作区恢复可发现。
- 验收：恢复列表过滤正确（当前工作区 + `--all`）；退出码约定；历史搜索快照测试。

### C5 — exec 契约收口

- 改动：`--json` JSONL 事件契约（thread/turn/item 骨架映射既有 StreamEvent）；`-o FILE`；`--color`；退出码 1（fatal/中断）。
- 交付：可脚本化的 `wunder-cli exec`。
- 验收：JSONL 事件类型/字段快照；stdout/stderr 分离断言；退出码矩阵（0/1）；管道场景。

### C6 — 键位、命令面与审批对齐

- 改动：§3.3 键位补齐（Ctrl+G/O/L、Alt+,/Alt+.、`@` 提及、`!` 前缀、审批面板 y/a/n/c）；`/clear`、`/permissions`；`-s/--sandbox` 三档与 `--approval-mode` 新词表（旧值别名）；只读执行策略。
- 交付：与 codex 键位表逐项对齐（有差异处帮助文案标注）。
- 验收：键位快照测试（含审批面板）；沙箱三档行为用例；旧配置词表兼容用例。

> **落地（C6）**
> - **词表**：`--approval-mode` 收敛为 `on-request | never | suggest`（`auto_edit`/`full_auto`/`auto`/`untrusted` 等旧词仍是别名，clap 与 `parse_approval_mode` 两处都接受）；新增 `-s/--sandbox read-only | workspace-write | danger-full-access`（别名 `read_only`/`workspace`/`danger` 等）。CLI 参数是最高层：`--approval-mode` 显式给出时压过沙箱隐含的默认值（`apply_cli_policy`）。
> - **沙箱落点**（`apply_sandbox_mode`，CLI 与 config.toml 共用一份判定，不再两处重复）：`workspace-write` = 现状（工具根收敛到工作区，越界拒绝）；`read-only` = 审批落到 `suggest`（写与执行都需批准；非交互下等于拒绝），用户同时显式写了 `approval_policy` 时不覆盖；`danger-full-access` = 允许根放开为 `*`（引擎 `build_allow_roots` 仍识别该 token）。`danger-full-access` 每次启动在 stderr 明确告警——显式 flag 即确认，交互式二次确认会在非交互入口挂死。
> - **`/permissions`**：显示沙箱 + 审批策略 + 工作区边界，参数接受审批词与沙箱词；写入 `~/.wunder/config.toml`（`sandbox_mode` / `approval_policy`），并对运行中的引擎配置即时生效；`/approvals` 保留为别名。
> - **审批面板**：`y` 仅本次、`a` 本会话、`n` 拒绝、`c`/`Esc` 取消（引擎只有三态，取消同样回 `Deny`，但日志明确写「已取消」而不是「已拒绝」）；面板行首显示键位字母，不再显示 1/2/3。
> - **键位**：`Ctrl+L` 清屏 + 开新线程（与 `/clear` 同义）；`Ctrl+G` 外部编辑器（与 `/edit` 同义）；`Ctrl+O` 复制最后一条回复（Windows 经临时文件 + `Set-Clipboard`，macOS `pbcopy`，Linux `wl-copy`/`xclip`/`xsel`，避免长文本撞命令行长度上限）；`Alt+,`/`Alt+.` 在 low/medium/high 阶梯上调整推理强度并持久化到 `model_reasoning_effort`；`!` 前缀直接执行 shell（复用引擎 `execute_command`，与工具调用同一套根目录与审批判定）；`@` 文件提及沿用既有 popup 索引（`/mention` 命令因此取消）。`F4` 仍是鼠标模式切换（与 codex 的 activity 聚焦不同），帮助覆盖层已标注该差异。
> - **命令面**：§4.4 的 21 条核心集（`/help` `/status` `/resume` `/new` `/clear` `/config` `/model` `/permissions` `/plan` `/goal` `/edit` `/init` `/attach` `/diff` `/review` `/skills` `/rename` `/compact` `/mcp` `/exit` `/quit`）。移除项的替代路径：`/notify` → `config.toml` 的 `notify` / `notify_when`；`/mention` → `@`；`/threads` → `←` 命令中心；`/system` 的额外提示词 → 全局 `$WUNDER_HOME/AGENTS.md`（后者随线程快照冻结，与项目 AGENTS.md 同一条链路）；`/config show` → `/config show` 子参数；`/model` 承接 `/tool-call-mode` 与强度切换。
> - **`/config`**：新增 `show`（当前生效配置）与 `edit`（用 `$EDITOR` 直接编辑 `~/.wunder/config.toml`，保存后立即投影到引擎配置）；模型向导（`/config` 或 `/config <base_url> <api_key> <model>`）除更新引擎配置外，同时写入 config.toml 的 `model` 与 `[provider]`，兑现 §6.5 的遗留项。
> - **验证证据**：`cargo test -p wunder-cli` 286 单测 + 3 集成用例全绿（隔离副本 = HEAD + 本次改动）；键位/词表用例覆盖 `approval_response_for_key`（y/a/n/c/Esc/Enter，且不伪造 `d`/数字键）、`-s` 三档与旧词表别名解析、沙箱与审批的优先级（`an_explicit_approval_policy_survives_a_read_only_sandbox`、`the_sandbox_flag_is_the_last_layer_and_the_approval_flag_beats_it`）、命令面收敛（`removed_commands_are_no_longer_recognized`）、`!` 直执的审批判定（`direct_shell_refuses_every_gated_policy`）。真机探针（用构建出的 `wunder-cli`，非交互）：默认沙箱下工作区外写入返回「路径越界访问被禁止」且文件不存在；`-s danger-full-access` 下同一写入成功落盘；`config show -s read-only` 报 `sandbox_mode: read-only` / `sandbox_policy: suggest` / `sandbox_mode_source: cli`。
> - **一处刻意的取舍**：`tool run`（用户显式调用工具）不经审批闸门，因为审批是"模型行为的监督"机制；而 TUI 的 `!` 直执发生在会话内，闸门为 `suggest`/`on-request` 时**拒绝执行并提示 `/permissions never`**，不静默绕过——`!` 自身没有批准入口，放行等于把 read-only 变成摆设。

### C7 — 线程中心工作区化、启动体验与发布门禁

- 改动：命令中心删 Agent 分组、增 Workspace 分组与工作区展示（§5.6）；启动首帧不等待后台初始化（§3.1）；空态收敛；帮助文案全量更新。
- 交付：`←` 打开的是「工作区 + 线程」任务中心；启动即输入。
- 验收：§9 全部标准；Win7 x86 与 Ubuntu 18.04 目标构建（CI 既有 job）；真终端人工验收；更新 `docs/功能迭代.md`。

> **落地（C7）**
> - **线程中心工作区化**：`ThreadGroupMode::Agent` 删除，改为 `Workspace`（分组键 = `workspace_id`，无归属的旧行排在最后而不是被隐藏）；「智能体」分组与详情里的 Agent 行一并移除。
> - **工作区展示**：`ThreadSnapshot` 增加 `workspace_name` / `workspace_color`，由目录服务在**每页一次**查询里解析（`storage.list_workspaces` 单次调用；页内没有任何工作区行时完全不查），前端不额外发请求。列表行「任务 · 工作区 · 状态 · 更新时间」四列对齐，工作区名缺失时回落为 id、i 无归属显示 `-`；窄终端先丢右侧列，标题与选中标记永远保留。详情面板显示工作区名与工作区 ID；搜索同时匹配标题、工作区名与工作区 ID。标题/帮助/窄屏提示统一为「任务中心 / Threads」，`/threads` 命令与 Agent 文案不再出现。
> - **启动首帧不等待**：`TuiApp::new` 只做首帧必需的事（状态装配、持久化历史、品牌横幅），文件索引（最多 5 万条的工作区遍历）、popup 目录、模型状态、会话统计四项移入后台任务，各自就绪即通过 `mpsc` 通道送进事件循环逐项填充（`StartupFill`；关闭的通道用 `pending()` 收口，不会把 `select!` 变成忙循环）。`sync_model_status` 拆为「读取（`compute_model_status`，只要能拿到 `&CliRuntime`）」+「应用（`apply_model_status`）」，同一份读取逻辑既服务首帧之后，也服务 `/model`、`/permissions`、`/config edit` 的即时刷新。测试通过 `complete_startup_fills()` 确定性地取齐首帧之后的状态。
> - **空态收敛**（§3.5）：品牌横幅第三行改为「工作目录 → `/help` · `@` 提及 · `!` 直执」；模型名不进空态（它在首帧之后才到达，footer 已经在报），缺失模型时在 composer 上方给一行指引（`/config` 或 `wunder-cli config`）而不阻塞输入。
> - **验收证据**：`cargo test -p wunder-cli` 在**主树** 289 单测 + 3 集成用例全绿，`cargo check --workspace --all-targets` 零错误零告警，`cargo test -p wunder-runtime thread_catalog` 11 用例通过（隔离副本同结果）；新增用例 `workspace_grouping_replaces_the_agent_grouping`（三档分组轮转、同工作区聚拢、名称回落 id、无归属为 `-`）、`the_workspace_column_lines_up_and_falls_back_to_a_dash`、`narrow_rows_never_exceed_terminal_columns`（32/40/60/90 列均不越界）、`the_first_frame_does_not_wait_for_background_fills`（构造返回时索引为空且 composer 有焦点，填充后才出现工作区文件）、`header_carries_the_mark_version_path_and_hints`；快照矩阵的宽/窄/空态断言随标题与空态文案更新。
> - **仍未完成（需要环境，不在本机可判定范围）**：真终端人工验收（`←`/`g`/`Tab`/`/` 交互、Ctrl+G/O/L 与 `!` 的实机手感、批准面板 y/a/n/c）、Win7 x86 与 Ubuntu 18.04 的 CI 目标构建。

---

## 九、验收标准

### 9.1 功能

- 全程单智能体 `__default__`，无任何智能体创建/切换/覆盖入口。
- 任意目录启动，工具直写该目录；`-C` 切换工作区；线程归属工作区，越界写被拦截。
- 无子命令 = TUI（新线程）；`exec` 非交互执行（stdout 正文/stderr 过程）；`resume` 按工作区过滤 + `--all`。
- 斜杠命令为 §4.4 收敛集；键位与 §3.3 表一致；审批面板 y/a/n/c 生效。
- AGENTS.md 在线程初始化时注入且仅注入一次；预算截断有效。
- 退出摘要与 `wunder-cli resume <id>` 提示正确；退出码符合 §4.3。

### 9.2 性能

- 启动首帧不被建档/迁移/目录/统计加载阻塞；provisional composer 输入零丢失。
- 恢复列表/命令中心分页有界；流式只更新活动尾块；事件缓冲有界不丢字（沿用 durable 重放验收真相）。
- 无无界队列/缓存；AGENTS.md 与 diff 预览有字节/行数上限。

### 9.3 兼容

- Windows 7 x86 与 Ubuntu 18.04 目标构建通过；SQLite 离线启动；无色终端降级可读。
- 旧配置词表（suggest/auto_edit/full_auto、旧 approval-mode flag 值）保留别名可用；旧会话迁移后可按工作区恢复。

### 9.4 质量

- 新键位/命令面/审批/exec 契约均有 TestBackend 或单测快照（40/80/120 列）。
- 测试、代码、日志、文档中不出现真实业务名称、身份、密钥或本机路径示例。
- `cargo check --workspace`、`cargo test -p wunder-cli` 全绿。

---

## 十、移除清单

- 移除：line-chat 交互循环与 `chat`/`ask` 子命令；`--agent` 参数与 `/agent`、`/personality`。
- 移除：`exec` 的「直跑 shell」语义（shell 直跑由 `tool run execute_command` / `!` 前缀覆盖）。
- 移除：`/apps`、`/branches`、`/fork`、`/backtrack`、`/ps`、`/clean`、`/notify`（迁移为配置项）、`/mention`（并入 `@`）、`/mouse`、`/statusline`、`/debug-config`、`/session`、`/system`、`/tool-call-mode`、`/threads`。
- 移除：全局「当前会话」自动续用文件（`sessions/current_session.json`）语义；`config` 子命令的 set-tool-call-mode/set-approval-mode（并入 toml）。
- 移除：命令中心 `ThreadGroupMode::Agent` 分组；智能体维度的会话分组展示。
- 不移除：引擎级多智能体/蜂群/子智能体能力（舰体与引擎仍需），仅从个人 CLI 入口移除。

> 凡移除的字段/组件，同步清理存储投影、状态与文档残留；反复出现的坑记入 `docs/经验教训.md`。

---

## 十一、风险与注意事项

1. **cwd 直写风险**：同蜂窝重构 §14——必须保留根边界校验与权限模式；禁止任何「删除工作区磁盘内容」能力；`danger-full-access` 需显式确认。
2. **exec 语义变更**：从「shell 直跑」改为「agent 执行」会破坏既有脚本习惯；原型阶段直接变更，在 help 与发布说明中明示，不提供两套语义并存。
3. **配置双轨过渡**：config.toml 投影与引擎 wunder.yaml 并存期间，写回路径必须单一（toml → ConfigStore 单向投影），防止两处手改互相覆盖。
4. **AGENTS.md 与提示词冻结**：注入只允许在线程初始化时发生，违反缓存规则的回退不可接受；注入超预算必须截断并提示，不得静默丢内容。
5. **恢复语义变化**：不再自动续「上次会话」会让老用户困惑；退出摘要的 resume 提示是主要缓解手段，帮助文案需同步更新。
6. **多任务并行**：调度、锁、实时状态仍按线程隔离；工作区聚合展示不得污染线程间状态（沿用既有隔离资产）。
7. **Win7 终端键位**：legacy console 下 Esc/Ctrl+O 等键位行为需在真机验收（快照证明帧内容，不证明终端兼容性）。

---

## 十二、与既有方案文档的关系

- `docs/cli版本实现方案.md` 的 **N0–N7 与 §10 全部保留**：线程注册表与有界事件泵（N2）、TranscriptCell 与 ToolCallKey（N3/N4）、durable 游标与共享 feeder（§10）、呈现规范化（N7）是本方案不可动摇的底座；本方案仅改变其上的产品形态（单智能体、工作区、命令面、配置）。
- `docs/本地易用重构方案.md` 是产品范式的来源：蜂窝的 workspaces 表、`__default__` 固化、工作目录直连链路被 CLI 直接复用，两形态从此同构。
- 冲突处以本方案为准：命令中心标题/分组、斜杠命令集、`exec` 语义、审批词表按本方案执行；既有文档相应章节标注「已被《cli版本进一步贴近codex方案》取代」。

---

## 附：参考索引

**codex 侧（提取原则，不复制实现）**：

- `codex-rs/utils/cli/src/shared_options.rs`：全局 flag 面（-m/-C/-s/-p/--add-dir 等）；
- `codex-rs/tui/src/cli.rs`、`codex-rs/exec/src/cli.rs`：TUI 入口与 exec 完整参数/子命令；
- `codex-rs/exec/src/event_processor_with_human_output.rs`、`event_processor_with_jsonl_output.rs`、`exec_events.rs`：exec 输出契约（stdout/stderr 分工、JSONL 事件、退出码）；
- `codex-rs/tui/src/startup_draft.rs`、`startup_preflight.rs`、`app/empty_state_policy.rs`：provisional composer、鉴权预检、空态；
- `codex-rs/tui/src/keymap.rs:1642-1944`、`slash_command.rs`：键位默认表与斜杠命令全集；
- `codex-rs/tui/src/app/agents_overview_view.rs`、`bottom_pane/approval_overlay.rs`、`approval_events.rs`：线程中心与审批面板形态；
- `codex-rs/protocol/src/config_types.rs:99-114`、`protocol/src/protocol.rs:961-984`：sandbox_mode 与 approval_policy 词表；
- `codex-rs/core/src/agents_md.rs`、`config/src/loader/mod.rs:123-139`：AGENTS.md 发现/预算与配置层叠顺序；
- `codex-rs/tui/src/app/exit_summary.rs`：退出摘要与 resume 提示。

**wunder 侧（复用/改造）**：

- `crates/wunder-cli/main.rs`、`args.rs`、`slash_command.rs`：入口、参数、斜杠命令（本方案改造对象）；
- `crates/wunder-cli/runtime.rs`：启动/配置/会话文件（工作区建档与 toml 投影落点）；
- `crates/wunder-cli/tui/command_center.rs`、`tui/app.rs`：线程中心与键位路由；
- `crates/wunder-desktop/native_workspace.rs`、`runtime.rs:1687-1750`：工作区建档/校验与迁移（下沉复用）；
- `crates/wunder-runtime/src/services/workspace.rs:353`、`api/chat.rs:440-470`、`orchestrator/request.rs:28-33`：工作区根注册与请求注入链路；
- `docs/本地易用重构方案.md`、`docs/cli版本实现方案.md`（§6 门槛、§10 durable 基线）。
