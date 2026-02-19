# wunder-cli 优化方案（对齐 codex 使用体验）

> 目标：在保持 wunder 现有“核心复用 + 单可执行 + WUNDER_TEMP 持久化 + tool_call/function_call 切换”原则不变的前提下，让 `wunder-cli` 获得更接近参考项目 `codex-main` 的终端使用体验（交互、效率、安全、可脚本化）。

> 注：按你的要求，本次落地不引入 `sandbox-mode`（文件/命令/网络隔离），仅对齐 codex 的审批体验、交互效率、diff/review 工作流与 TUI 性能细节。

## 1. 现状对比（wunder-cli vs codex）

### 1.1 wunder-cli 已具备（现状）

- **运行形态**：单二进制 `wunder-cli`；运行态落在启动目录 `WUNDER_TEMP/`（SQLite + override 配置 + session/extra_prompt）。代码入口：`wunder-cli/main.rs`、`wunder-cli/runtime.rs`。
- **交互**：TTY 默认进入 TUI（alternate screen），包含状态栏、会话区、输入区、快捷键弹窗、resume 会话选择弹窗。代码入口：`wunder-cli/tui/*`。
- **命令面**：`ask/chat/resume/tool/exec/mcp/skills/config/doctor`；交互态 slash：`/help /status /model /tool-call-mode(/mode) /session /system /resume /new /config /exit`。
- **渲染与事件**：支持流式事件渲染（text/JSONL），并对 tool_call/tool_result 做可见化输出。代码入口：`wunder-cli/render.rs`。
- **特色能力**：`tool_call`/`function_call` 可切换；对话统计展示的是“上下文占用 tokens”（context_used/peak），契合 wunder 的 token 统计口径。

### 1.2 codex 的“核心使用体验”要点（参考）

以 `codex-main/codex-rs` 的 Rust CLI/TUI 为主要对标对象：

- **安全默认值**：以“审批模式（+ 沙盒模式）”控制自治边界；默认更偏保守（写文件、执行命令需要用户确认）。参考：`codex-rs/tui/src/bottom_pane/approval_overlay.rs`、`codex-rs/cli/src/main.rs`。
- **强生产力输入框**：多 popup（slash/文件/skill）、跨会话输入历史、Windows 粘贴 burst 处理、消息队列/中断/退出提示等。参考：`codex-rs/tui/src/bottom_pane/chat_composer.rs`、`codex-main/docs/tui-chat-composer.md`、`codex-main/docs/exit-confirmation-prompt-design.md`。
- **Diff/Review 工作流**：在 TUI 内查看 diff/变更摘要、review、apply patch，并用审批 overlay 串起“提议 -> 审批 -> 落盘”。参考：`codex-rs/tui`、`codex-rs/apply-patch`、`codex-rs/execpolicy`。
- **性能细节**：对流式输出做 chunking/追帧策略，避免输出 backlog 导致 UI 滞后。参考：`codex-main/docs/tui-stream-chunking-review.md`。

## 2. 需要优化的方面（按优先级）

### 2.1 安全模型：审批（体验差距最大）

现状：wunder-cli 具备 `security.exec_policy_mode` 的基础机制（高风险命令识别/审批缓存），但 **CLI/TUI 侧缺少“可交互的审批体验”**，也缺少像 codex 那样“一眼可见的权限边界”（读/写/命令/网络）。

建议补齐：

- **审批模式（approval-mode）**：类似 codex 的 Suggest/Auto-Edit/Full-Auto，但结合 wunder 的工具体系做工具级别的 gating。

### 2.2 编排可控性：中断/退出/长任务可恢复

现状：TUI 中 Ctrl+C 直接退出；忙碌时不能中断当前回合；缺少类似 codex 的“二次 Ctrl+C 才退出”的保护与“Interrupt 优先”策略。

建议补齐：

- **Interrupt 优先**：忙碌时 Ctrl+C 先触发中断（或发出取消请求），空闲时才走退出确认。
- **退出保护**：空闲时第一次 Ctrl+C 显示提示，短时间内二次 Ctrl+C 才退出（对齐 codex 的退出设计）。

### 2.3 输入框与交互效率：文件/工具/技能“就地插入”

现状：wunder-cli TUI 输入区已有多行编辑与 slash 提示，但缺少 codex 的“文件搜索/mention/skill popup”、跨会话输入历史、粘贴 burst 处理等；用户要么手打路径，要么让模型自己去搜索，效率偏低。

建议补齐：

- **文件搜索/mention**：输入 `@` 或 `/mention` 弹窗选择文件路径，或直接把文件内容作为附件/上下文插入（对齐 codex 的提效路径）。
- **粘贴 burst**：Windows 终端对多行粘贴常表现为连续 KeyEvent；需要做 burst 识别，避免粘贴过程中误触快捷键/误提交。
- **跨会话输入历史**：将用户输入历史持久化到 `WUNDER_TEMP/`（或用户级目录），上/下方向键可跨会话复用。

### 2.4 变更可见性：Diff/Review/Apply 的闭环

现状：wunder-cli 通过工具直接写文件/编辑文件，缺少“先展示 diff -> 再落盘”的强审阅体验；对 codex 用户来说，这是最关键的“安全感/可控性”来源之一。

建议补齐：

- **/diff**：TUI 内直接展示当前 workspace 的 git diff（含 untracked），并支持复制/分页。
- **/review**：提供一键 review 当前变更（内部可复用 wunder 的 orchestrator + 评审提示词），输出“问题列表 + 风险 + 建议测试”。
- **写入工具的 Dry-Run**：在保守审批模式下，写文件/编辑文件先产出 patch/变更摘要（不直接落盘），由用户在审批 overlay 中确认后再 apply。

### 2.5 性能：长会话 + 大输出的 UI 流畅度

现状：TUI 每帧构建完整 transcript 的 `Paragraph`；日志条数上限虽有限制，但长输出（工具结果/代码块）仍可能导致绘制/换行计算成本上升；流式输出高频时缺少 chunking 策略。

建议补齐：

- **流式 chunking/backpressure**：当 backlog 累积时改为批量 drain，减少“输出追不上输入”的滞后感（对齐 codex 的 chunking 思路）。
- **Transcript 虚拟化**：按 viewport + scroll 只渲染可见范围（或做分段缓存），避免每帧全量重算。
- **工具输出折叠**：默认折叠长 stdout/stderr，提供展开查看（避免渲染巨量文本拖慢 UI）。

## 3. CLI 优化总体设计（建议形态）

### 3.1 核心概念对齐（wunder 语义 + codex 体验）

引入一个“对用户可见、可被切换/持久化”的概念：

1. **ApprovalMode（审批模式）**：控制工具调用的自治边界。

建议在 CLI 侧提供：

- 全局 flag：`--approval-mode`（并可 `/approvals` slash 调整）
- `config show` 中展示“当前生效值 + 来源层”（类似 codex 的 debug-config）

### 3.2 工具调用策略（Tool Policy Engine）

在 `src/orchestrator/execute.rs` 的工具执行前增加统一决策入口（基于现有 `exec_policy` 扩展覆盖更多工具类别）：

- 读取类：`read_file/search_content/...`（默认允许）
- 写入类：`write_file/edit_file/...`（按审批策略 gate）
- 执行类：`execute_command/ptc/...`（按审批策略 gate）

产出决策：

- `allowed`：允许直接执行
- `requires_approval`：允许但需要用户确认（TUI overlay / 非 TUI 提示）
- `dry_run_only`：只产出 patch/计划，不落盘（用于 Suggest/Auto-Edit 等模式）

### 3.3 TUI 交互组件补齐（审批 overlay + 文件 popup）

对齐 codex 的交互组件，但保持 wunder-cli 轻量实现：

- **ApprovalOverlay**：展示待确认的命令/patch，提供“同意一次/同意本会话/拒绝”等选项。
- **FileSearchPopup**：按文件名/路径/最近访问进行搜索选择，并支持插入到输入框。
- **Skill/MCP Popup（可选）**：在 TUI 内管理 skills/MCP（启用/禁用、查看配置、快速测试）。

## 4. 分阶段落地计划（可直接转任务）

### M0（快速收益，低风险）

- 新增退出保护与中断策略：空闲双击 Ctrl+C 退出、忙碌 Ctrl+C 优先中断（参考 codex 退出设计）。
- 为 `wunder-cli` 增加 shell completion 生成：`wunder-cli completion <bash|zsh|fish|powershell>`。
- 增强 `doctor`：输出 config 分层来源、当前 approval 生效值、关键路径与可用模型/工具摘要。

验收：

- TUI 忙碌时 Ctrl+C 不再直接退出；可中断当前回合并回到可输入状态。
- completion 可生成且基本可用（核心子命令 + 全局参数）。

### M1（对齐 codex 的“安全感”：审批模式）

- 引入 `--approval-mode`（建议三档）：`suggest | auto_edit | full_auto`。
- 在工具执行链路引入 `ToolPolicyEngine`（覆盖写文件/编辑文件/执行命令/ptc）。
- TUI 增加 ApprovalOverlay：展示待审批请求与变更摘要，用户选择后继续执行。

验收：

- `suggest` 下，写入/执行类工具默认不会直接落盘/执行；TUI 能弹出审批。
- 审批决策可缓存到 session（同一命令/同一 patch 重复触发可免二次确认）。

### M2（开发效率：文件 mention + diff/review/apply 闭环）

- 输入框增加文件/路径 popup：`@` 触发文件搜索；`/mention` 作为无冲突入口。
- 新增 `/diff`：展示 git diff（含 untracked）；无 git 时提供 fallback（按 workspace tree_version 对比或提示）。
- 新增 `/review`：调用 review 专用提示词/工具链，输出问题清单与建议测试。
- 写入工具增加 Dry-Run：在保守模式下先返回 patch；用户 approve 后再 apply（新增 apply 工具或复用 edit_file 执行）。

验收：

- 用户可以不离开 TUI 完成“看 diff -> review -> 决定是否 apply”的闭环。
- 在 suggest/auto_edit 模式下，变更可见性明显提升（不再“写完才看到”）。

### M3（性能与长期稳定）

- 流式 chunking/backpressure：高频 delta 时避免 UI backlog（参考 codex chunking）。
- transcript 虚拟化/分段缓存：避免每帧全量重绘造成卡顿。
- 工具结果折叠与分页查看：大输出默认折叠，按键展开/复制。
- 增加 CLI 集成测试：覆盖 ask/resume/config/approval/diff/review 等关键链路（可用 mock LLM + 固定工具输出）。

验收：

- 长会话（> 1000 行输出）仍能保持可接受的滚动与输入延迟。
- 关键交互在 Windows Terminal / PowerShell 下稳定（粘贴、多行编辑、鼠标滚轮、宽字符）。

## 5. 风险与取舍（提前明确）

- **审批/沙盒改造的入侵性**：要做出 codex 的“提议 -> 审批 -> 执行”体验，需要在 orchestrator 工具执行前引入可暂停/可恢复的状态机或 dry-run/apply 两段式写入；建议先 M1 做最小可用（先覆盖写/exec 两类），再逐步扩展。
- **Diff/Review 依赖 git**：codex 默认依赖 git；wunder-cli 需要兼容“无 git 的目录”，可采用提示 + 简化 fallback 策略，避免复杂实现拖慢落地。
- **性能优化优先级**：建议先把审批/可控性做出来，再做重型虚拟化；否则容易陷入“UI 很快但不够安全”的体验落差。

## 6. 与 codex-main 的差距复审（2026-02-19）

> 参考基线：`参考项目/codex-main/codex-rs` 当前实现（重点对比 `cli/src/main.rs`、`tui/src/slash_command.rs`、`tui/src/bottom_pane/chat_composer.rs`）。

### 6.1 已经明显对齐（本轮复盘结果）

- **审批体验主链路已具备**：CLI/TUI 均可触发审批（含一次批准/会话批准/拒绝），并支持 `/approvals` 切换策略。
- **高频工作流已覆盖**：`/diff`、`/review`、`/mention`、`/resume`、`/status`、`/session`、`/config` 已可在交互流中闭环使用。
- **基础交互可靠性提升**：忙时中断、双击退出保护、输入历史持久化、鼠标模式切换、会话恢复选择器等已落地。
- **双语体验显著提升**：CLI/TUI 操作指引、状态文本、日志与帮助说明基本实现中英双语输出（`--lang`/`--language`）。

### 6.2 仍与 codex 存在的主要差距（按优先级）

1. **命令与能力面差距（中）**
   - codex 内置更完整的会话/治理命令集（如 `fork`、`compact`、`plan`、`personality`、`debug-config`、`ps/clean`、`apps` 等）。
   - wunder-cli 目前命令集聚焦核心开发闭环，仍偏“精简版 codex”。

2. **输入框高级能力差距（高）**
   - codex 的 `ChatComposer` 具备更完整状态机：命令/File/Skill 多类 popup、Windows 粘贴 burst 检测、图像附件占位与恢复、外部编辑器回填一致性。
   - wunder-cli 当前已有 slash + mention 与基础粘贴队列，但尚未覆盖 burst 识别、技能弹窗、附件回填等高级细节。

3. **会话模型能力差距（中）**
   - codex 提供 resume + fork + 回溯编辑/草稿恢复的组合体验。
   - wunder-cli 目前具备 resume 与基础历史恢复，缺少 fork 语义和更细粒度回溯编辑链路。

4. **配置可观测性差距（中）**
   - codex 有更强的配置层可观测能力（debug-config、feature toggles 与来源解释）。
   - wunder-cli 当前 `doctor/config show` 可用，但“配置来源层级解释”和特性开关可视化仍不足。

5. **性能防抖与大输出治理差距（中）**
   - codex 对流式大输出有更系统的 chunking/backpressure 策略。
   - wunder-cli 仍以简单流式刷新为主，极端长输出下可能出现 UI backlog。

6. **沙盒与权限边界差距（已知不对齐项）**
   - codex 提供更完整 sandbox 模型与权限视图。
   - wunder 当前按项目要求不引入 sandbox-mode，此项是策略性差异，不作为当前阶段阻塞项。

### 6.3 建议下一阶段（R3）聚焦项

- **R3-1 输入框高级化**：补齐 paste burst 检测与 skill popup，减少 Windows 下粘贴误触与输入抖动。
- **R3-2 会话增强**：新增 `fork` 与会话分支可视化，强化“探索式改动”体验。
- **R3-3 可观测增强**：新增 `/debug-config`（或等价命令）输出“最终值 + 来源层 + 覆盖路径”。
- **R3-4 流式性能治理**：引入分批渲染与背压，确保长会话/大工具输出下 TUI 不卡顿。
