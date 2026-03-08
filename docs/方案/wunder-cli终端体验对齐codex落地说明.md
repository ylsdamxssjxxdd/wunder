# wunder-cli 终端体验对齐 codex 落地说明

> 日期：2026-03-06  
> 范围：`wunder-cli` TUI 重构与终端体验收口

## 1. 本次已落地内容

- 继续收口输入区与底栏布局：输入框标题行不再堆叠大段操作提示，仅保留 `输入/Input` 与附件摘要；底部新增独立 footer 快捷提示行，按可用宽度依次展示 `@ 文件路径 / Ctrl+V 图片 / Tab 补全 / ↑ 历史 / Shift+Enter 换行`，更接近 codex 的 composer + footer 分层结构。
- 将输入区高度从 6 行提升到 7 行，为 footer 单独预留 1 行，避免提示信息挤占输入正文区域，同时保持光标、自动换行与附件标题渲染稳定。
- 将 `tool_call` / `tool_result` 的文本样式继续向 codex 对齐：普通工具改为 `• Called/调用` + 树形 `└`/缩进行，执行命令单独展示命令本体，减少 `[tool_call]` / `[tool_result]` 这类调试味较重的标签。
- 将 `apply_patch` 的调用预览和结果摘要升级为近似 codex patch cell 的块状结构：调用阶段展示 `files=N, lines=M` 与 `A/M/D/R path` 文件级预览；结果阶段展示 `• 已修改 N 个文件 / • Edited N files`、`+/~/-/↦` 统计、错误码、hint 与受影响文件列表。
- 进一步将 TUI 中的 `apply_patch` 从“两条普通工具日志”升级成“单个可更新 patch block”：收到 `tool_call` 时先插入待应用补丁块，收到 `tool_result` 后原地更新为成功/失败块，而不是再追加一条结果日志，使补丁编辑历史更接近 codex 的独立 patch cell 心智。
- 继续将 `执行命令 / execute_command` 升级成独立 command block：收到 `tool_call` 时展示 `Running/正在运行` 命令块，收到结果后原地更新为 `Ran/已运行` 块，并带 `exit=...`、耗时、stdout/stderr 摘要与空输出占位，减少普通文本日志的割裂感，向 codex 的 exec cell 进一步靠拢。
- 将 command block 进一步细化为更接近 codex exec cell 的布局：标题行内联命令首行，续行与 metrics 统一走 `│` gutter，stdout/stderr 预览采用“头尾保留 + 中间折叠”的方式，长输出不会把 transcript 拉得过长。
- 将其余普通工具也纳入统一的单块 tool cell：默认工具不再以“调用一条 + 结果若干条”追加到 transcript，而是先插入 `Calling/调用` 块，结果到达后原地更新为 `Called/已完成` 或失败块，并保留参数摘要、错误信息与紧凑结果 JSON，整体交互更接近 codex 的 MCP tool cell。
- 将 patch / command / generic tool 三类特殊块接入**宽度感知渲染**：长命令、长参数、长错误文本、长文件路径在窄终端中换行时，会继续保留 `└` / `│` / 缩进 gutter，而不是交给段落组件做生硬折行，显著改善 transcript 的视觉秩序。
- 继续向 codex 的 tool cell 收口 header 行为：普通工具与补丁块现在会优先尝试把参数摘要 / patch summary 内联到标题行中；只有在终端宽度不足时才自动下沉为 `└` 子行，使宽终端下的信息密度更接近 codex。
- 继续补齐 patch block 的大补丁场景：当预览文件数超过摘要上限时，不再静默截断，而是明确追加 `… +N more files / 还有 N 个文件` 提示，降低用户误判补丁范围的概率。
- 将 pending patch 的摘要和文件行进一步向 codex diff summary 靠拢：摘要优先显示 `files=N, +A, -B`，文件行优先显示 `(+/-)`，后端返回结果时则补充 `hunks` 信息，让补丁范围判断更直接。
- 将 patch block 的头部进一步做成 codex 的单/多文件差异化：单文件补丁会直接在 header 中内联路径（如 `Applying patch src/main.rs` / `Edited src/main.rs`），多文件补丁才保留 `Edited N files` 形式，减少冗余文件行。
- 继续对齐 patch 细节文案：补丁预览与结果中的重命名路径统一改为 `→`，视觉上更贴近 codex；同时 approval / inquiry 浮层新增底部确认提示（`Enter ... · Esc ...`），让选择动作更接近 codex 的 overlay 心智。
- 将非 TUI 的 `wunder-cli/render.rs` 与 TUI 侧 helper 统一到同一套工具渲染语义，避免普通 CLI 输出与 TUI 输出在 patch / command / generic tool 上出现体验漂移。
- 为审批弹窗补齐 `apply_patch` 专项预览：当工具为应用补丁时，不再直接把整段 patch JSON 挤进 modal，而是展示补丁摘要与文件级 `A/M/D/R` 预览，降低审批判断成本，更接近 codex 的补丁审批心智。
- 继续精简审批弹窗信息层级：补丁审批时优先展示 `工具 + 摘要 + patch preview`，再展示操作项，去掉无助于决策的冗余元信息，减少弹窗噪声并提升与 codex 审批浮层的一致性。
- 将 approval / inquiry modal 的文本行改为分层着色：标签、操作项、分节标题、补丁预览标记（`A/M/D/R`）分别使用不同样式，避免弹窗内所有内容都是同一种纯文本灰度，提升信息扫描效率。
- 进一步强化 inquiry modal 的可读性：候选项中的 `（推荐） / (recommended)` 标签会单独高亮，审批预览中的补丁摘要与 transcript 中的 patch block 保持同一套 `+/-/hunks` 信息结构，减少不同区域之间的认知切换。
- 将 composer footer 的提示进一步做成宽度感知折叠：宽度足够时显示 `快捷键 + 标签`，宽度不足时优先保留快捷键本身并缩短标签文案，使窄终端下仍能保留更多操作提示，整体更接近 codex footer 的保真策略。
- 将 approval / inquiry 弹窗的结构进一步向 codex 靠拢：顶部保留上下文信息，中段只展示简洁的选项标题与列表，底部专门给出 `Enter / Esc / 数字快捷键` 提示，减少把交互说明塞进标题行导致的噪声。
- 继续对齐 codex 的 transcript 层级节奏：`patch` / generic tool 的子项改为使用 `├ / │ / └` 连接符渲染，补丁多文件场景下能清晰看出文件块的起止关系，不再出现“第一行有树枝、后续文件平铺断开”的割裂感。
- 继续细化 `apply_patch` 的文件级心智：单文件成功补丁现在会区分 `Added / Deleted / Renamed / Edited` 头部文案，重命名路径在 TUI 与普通 CLI 中统一使用 `→`，让补丁审批、transcript 结果块和纯文本输出保持一致。
- 继续优化底部与浮层的细节回退：footer 在窄宽度下会优先尝试截断标签再退化为仅显示快捷键；approval / inquiry modal 的选中项保留数字、推荐标记与高亮层级，避免选中后退化成整行纯反白文本。
- 继续向 codex 的底部区收口：composer footer 现在支持左侧快捷提示 + 右侧上下文摘要（context 剩余、附件数、滚动偏移）的分布式布局，窄终端下会优先压缩左侧标签，再在必要时回退到仅显示快捷键。
- 将状态/审批提示做得更接近 codex 的 overlay 语言：审批浮层顶部改为自然语言问题句，选项改为 `Yes, ... / No, ...` 风格；底部提示同步强调 `Y/A/N` 与 `Enter`，活动行也统一改成同一套快捷键心智。
- 顶栏状态行改为“左侧帮助、右侧状态”布局，避免帮助项与状态串在一侧造成拥挤，让整体信息层级更接近 codex 的分区式终端体验。

- 将 TUI 主循环改为**事件驱动重绘**，新增 `wunder-cli/tui/frame_scheduler.rs`，由输入事件、流式事件和状态变化主动请求 redraw，替代固定频率空转刷新。
- 将原先集中在单文件内的 UI 渲染逻辑拆成模块化结构：
  - `wunder-cli/tui/ui/layout.rs`
  - `wunder-cli/tui/ui/status_line.rs`
  - `wunder-cli/tui/ui/transcript.rs`
  - `wunder-cli/tui/ui/composer.rs`
  - `wunder-cli/tui/ui/modals.rs`
  - `wunder-cli/tui/ui/popup.rs`
- 为 TUI 增加统一主题与高亮基础设施：
  - `wunder-cli/tui/theme.rs`
  - `wunder-cli/tui/highlight.rs`
- 将 Markdown 代码块渲染接入 `syntect`，提升代码块在终端中的可读性，减少与 codex 风格的落差。
- 为输入区、会话区补齐 viewport 状态同步接口，保证拆分后的渲染模块仍可正确计算换行、滚动和光标位置。
- 补齐审批弹窗、问询面板、快捷键面板、会话恢复面板的中文文案，清理 `app.rs` 与 `ui/` 模块中的乱码与占位文本，统一为 UTF-8 正常中文。
- 修复流式请求触发 redraw 的接入链路，确保工具调用、模型输出、完成事件都能及时驱动界面刷新。
- 修复 `wunder-cli/tui/app.rs` 中活动行的中文乱码，并将 spinner / 分隔符 / 忙碌态提示统一为更接近 codex 的轻量文案风格。
- 新增“拖入本地图片/文件即自动入附件队列”的输入能力：终端收到拖入的本地路径后，不再把路径文本塞进输入框，而是异步准备附件并在下一轮自动随请求发送。
- 输入历史改为**过滤 slash 命令**：历史持久化加载、运行时写入、提交前路径三处同时收口，避免按上键回看历史时被 `/attach`、`/config` 等命令阻断。
- 更新输入区 placeholder 与快捷键文案，补充 `@` 文件路径提示以及“拖入图片/文件自动附加”的能力说明，使 composer 提示更接近 codex 的输入心智。
- 为 `Ctrl+V / Shift+Insert` 补充“优先粘贴剪贴板图片，回退到文本粘贴”的行为：在 Windows 下会优先尝试通过 PowerShell 读取系统剪贴板图片并落为临时 PNG，再自动加入附件队列。
- 在 composer 标题行增加附件可见性提示：当存在待发送附件时，输入框顶部会直接展示 `已附加` / `attached` 摘要，减少自动拖入或粘贴图片后的确认成本，同时避免像普通日志那样污染会话流。

## 2. 本次结构调整

### 2.1 状态与调度

- `TuiApp` 新增 `frame_requester`，在流式事件、键鼠事件和忙碌态轮询提示下统一调度下一帧。
- 保留 transcript / input 的状态在 `app.rs` 中集中管理，避免把交互状态分散到多个 widget 文件中。

### 2.2 渲染层

- `ui.rs` 只负责协调布局与调用各渲染子模块。
- `app.rs` 继续负责：
  - 输入编辑与光标移动
  - transcript 数据组织
  - slash 命令与流式事件处理
  - 审批 / 问询 / 会话恢复等交互状态

### 2.3 体验对齐方向

- 更接近 codex 的点：
  - redraw on demand
  - 更轻的状态栏
  - 更明确的输入 / 输出分区
  - 更稳定的流式刷新节奏
  - 更接近终端原生的文本化视觉层
- 仍待继续补齐的点：
  - 自定义 terminal backend
  - diff 专用渲染器
  - vt100 / snapshot 级别的视觉回归测试

## 3. 验证结果

- `cargo check --bin wunder-cli`
- `cargo clippy --bin wunder-cli -- -D warnings`
- `cargo test --bin wunder-cli`
- `cargo build --release --bin wunder-cli`

以上命令均已通过。

## 4. 后续建议

- 增加 TUI 快照测试，覆盖 transcript、modal、popup、markdown block 四类核心视图。
- 继续压缩 `wunder-cli/tui/app.rs`，把问询面板、审批流程、会话恢复等状态机继续拆入 `wunder-cli/tui/app/` 子模块。
- 参考 codex 的 `custom_terminal` 思路，为 OSC 链接、宽字符宽度和 diff 背景色做更深的终端兼容优化。

## 5. 2026-03-08 真 diff 补齐

- `apply_patch` 的 transcript patch block 已改为真实 diff 预览，不再只展示文件级摘要；当前会直接渲染 `diff` 文件头、`@@` hunk、`+/-` 变更行、context 行以及截断提示。
- patch 完成态会继承调用态的 diff 预览，因此工具返回结果后仍能保留完整补丁上下文，体验更接近 codex 的单块 patch cell。
- 审批弹窗中的 `Patch preview` 已改为真实 diff 预览，确认前即可直接查看实际修改内容，而不是只看 `A/M/D/R` 文件列表。
- modal 预览样式已补齐 `diff / @@ / + / - / …` 的高亮规则，与 transcript 中的补丁视觉语义保持一致。
- patch diff 解析已抽成 `wunder-cli/patch_diff.rs` 共享模块，TUI transcript、审批弹窗和普通 CLI/line-chat 输出现在复用同一套 diff 预览逻辑，避免后续继续分叉。
- 普通 CLI/line-chat 的 `apply_patch` 工具调用现在也会显示真实 diff 预览，并统一使用 `files=...`, `+...`, `-...` 的摘要格式，整体心智与 TUI 更一致。
- TUI 中的 `+/-/@@` diff 行已增加更接近 codex 的整行色带效果：新增行使用低饱和绿色底，删除行使用低饱和红色底，hunk 行使用冷色底，补丁块的层次感更清晰。
- 普通 CLI/line-chat 的 patch 预览缩进也进一步调整为“diff 头 + 内层 hunk/变更行”两级结构，阅读路径更接近 codex 的 patch 展示习惯。
## 6. 2026-03-08 目录上下文与底部区继续对齐

- footer 右侧上下文新增工作目录感知：优先显示 `repo_root` 相对路径，并在长度受限时按 `首段/…/尾段` 方式压缩，整体更接近 codex 对 workspace/cwd 的轻量提示风格。
- status line 默认项从 `session` 调整为 `cwd`，空白态优先展示 `cwd | elapsed | speed | tools | context`，减少内部会话 id 对主阅读路径的干扰。
- 新增共享目录显示模块 `wunder-cli/path_display.rs`，统一处理：
  - repo root 相对显示
  - home 目录 `~` 缩写
  - 中间省略的路径截断
- `/statusline set ...` 现支持 `cwd/dir/workspace/目录/工作目录` 等别名，便于和 codex / 常见 shell 心智保持一致。
- 输入区与底部区可见中文文案已统一修复为 UTF-8 正常文本，重点覆盖：
  - composer 标题与空态提示
  - composer footer 快捷键标签
  - status line 帮助与命令入口
  - app 内与状态栏、目录、附件相关的中文提示
- 本轮验证已重新通过：`cargo check --bin wunder-cli`、`cargo test --bin wunder-cli`、`cargo clippy --bin wunder-cli -- -D warnings`、`cargo build --release --bin wunder-cli`。

## 7. 2026-03-08 project / branch 工作区上下文补齐

- 新增共享模块 `wunder-cli/workspace_context.rs`：
  - 读取 `repo_root` basename 作为 project 名称；
  - 直接解析 `.git/HEAD` 与 worktree `.git` 指针文件，低开销获取当前分支；
  - 对超长 branch 名做中间截断，避免底部区被长分支名挤爆。
- TUI 底部右侧上下文继续向 codex 靠拢：当前会按 `目录 · 分支 · ctx/附件/滚动` 的顺序组织信息，其中分支名会做紧凑截断。
- status line 现支持新增条目：`project`、`branch`；默认空配置方案升级为 `cwd | branch | elapsed | speed | tools | context`，更接近 codex 的“目录 + 分支 + 执行状态”阅读路径。
- `/statusline set ...` 现支持 `project/repo/root/项目/项目根目录` 与 `branch/git/git_branch/分支/git分支` 等别名。
- 工作区上下文会在启动时初始化，并在每轮流式任务结束、异常结束或流断开后自动刷新，确保命令或工具修改 git 状态后底部区能及时更新。
- 本轮验证已重新通过：`cargo check --bin wunder-cli`、`cargo test --bin wunder-cli`、`cargo clippy --bin wunder-cli -- -D warnings`、`cargo build --release --bin wunder-cli`。
## 8. 2026-03-08 真实 CLI 长任务实测与自动化回归补齐

- 新增真实可用性回归脚本 `scripts/wunder_cli_e2e_smoke.py`，支持：独立 `temp_root`、复制现有模型配置、probe、长任务、产物复核、日志归档与 `summary.json` 输出。
- 新增长任务提示词 `scripts/prompts/wunder_cli_long_task_diff_lens.txt`，默认要求模型在隔离工作区内构建并验证一个小型 Rust CLI，用于稳定复现多步工具链路。
- 补充专项方案文档 `docs/方案/wunder-cli真实可用性与自动化测试方案.md`，沉淀本轮真实实测结果、已暴露问题与后续回归分层。
- 2026-03-08 的实测观察表明：当前 CLI 已能稳定完成多步编码任务并产出可验证项目，但仍存在 `PowerShell &&`、`2>&1` 伪失败、generic tool JSON 直出和缺少最终收尾答复等体验缺口。

## 9. 2026-03-08 第二轮 Codex 体验对齐

- Windows `execute_command` 壳层继续向 codex 靠拢：对未加引号的 `&&`、`||` 与 `2>&1` 自动切到 `cmd.exe` 执行，避免 PowerShell 5 语法不兼容与 stderr 合流误判。
- generic tool transcript 不再只回落到 `compact_json`：`list_files`、`search_content`、`read_files`、`write_file`、`read_image`、`skill_call`、`lsp_query`、`ptc` 现统一产出“标题 + 紧凑摘要 + 少量 preview”样式，普通 CLI 与 TUI 共用同一套摘要逻辑模块 `wunder-cli/tool_display.rs`。
- TUI 与 line-chat 均补上“工具执行完毕但 final.answer 为空”的自然语言收尾兜底，避免 transcript 最后一条停在工具输出上。
- 工具层默认加入 codex 风格噪音目录过滤：`list_files/search_content` 会跳过 `.git/target/node_modules/.next/.nuxt/.turbo/.cache`，减少模型二次检索污染。
- 本轮本地验证已通过：`cargo check --bin wunder-cli`、`cargo test --bin wunder-cli`、`cargo clippy --bin wunder-cli -- -D warnings`、`cargo build --release --bin wunder-cli`。
- 本轮线上 E2E 重试被上游模型账户 `Arrearage` 阻断，日志见 `temp_dir/cli-e2e/runs/20260308-201846/logs/probe.log`；账户恢复后可直接复跑 `python scripts/wunder_cli_e2e_smoke.py --model qwen3.5-122b`。

## 10. 2026-03-08 鼠标滚动与原生选区复制对齐

- 根因排查结果：`wunder-cli` 之前在 TUI 生命周期内始终开启 `EnableMouseCapture`，因此终端的鼠标拖选、原生复制与部分滚轮行为都会被应用截获；这与 codex 的处理策略不同。
- 本地 `codex-main` 对照结果表明：
  - codex 的事件流默认直接忽略 mouse events；
  - codex 不依赖 mouse capture 做主交互；
  - codex 在 alt-screen 下额外开启 `alternate scroll`（ANSI `?1007h`），让终端在不启用 mouse capture 时仍可把滚轮转换为滚动行为。
- 据此，`wunder-cli` 本轮改为：
  - `auto`：默认不捕获鼠标，优先交还终端原生选区/复制；
  - `select`：显式保持原生选择复制模式；
  - `scroll`：仅在用户明确切换后才启用 `EnableMouseCapture`，用于精确滚动 transcript；
  - 在关闭 mouse capture 时启用 `alternate scroll`，尽量保留接近 codex 的滚轮体验。
- 当前对齐后的心智模型：
  - 想像 codex 一样直接拖选输出文本：保持 `auto` 或切到 `/mouse select`；
  - 想让滚轮严格由 TUI 接管滚动输出区：切到 `/mouse scroll`；
  - `F2` 继续作为鼠标模式快速切换键。
- 当前限制也明确保留：在 `auto/select` 下，右键粘贴与基于鼠标坐标的局部交互不会再由应用接管，这是为了换取与 codex 更一致的“原生选区复制优先级”。

## 11. 2026-03-08 顶栏移除与滚轮优先修正

- 顶部提示行已全部移除，不再渲染 ? shortcuts、/ commands 或其它顶栏状态文案，把纵向空间全部还给 transcript。
- 输入区底部 footer 不再展示 @ 文件 / Ctrl+V / Tab / ↑↓ / Shift+Enter 等操作提示，避免持续占据注意力。
- 底部状态改为只保留本轮核心统计：耗时 | 速度 | 工具，与 codex 的轻量收口方向保持一致。
- 鼠标模式同步时机前移到事件循环开始前，默认 uto 会在等待输入前就开启鼠标捕获，因此滚轮优先滚动输出区，不再先落到输入框历史。
- select 模式保留为原生拖选/复制入口；scroll 模式继续作为显式的输出区滚动模式；当前默认体验是 uto=输出优先。
- 这一轮已重新通过 cargo check --bin wunder-cli、cargo test --bin wunder-cli、cargo clippy --bin wunder-cli -- -D warnings 与 cargo build --release --bin wunder-cli。
