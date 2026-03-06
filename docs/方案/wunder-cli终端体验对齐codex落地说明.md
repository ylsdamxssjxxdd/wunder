# wunder-cli 终端体验对齐 codex 落地说明

> 日期：2026-03-06  
> 范围：`wunder-cli` TUI 重构与终端体验收口

## 1. 本次已落地内容

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

以上命令均已通过。

## 4. 后续建议

- 增加 TUI 快照测试，覆盖 transcript、modal、popup、markdown block 四类核心视图。
- 继续压缩 `wunder-cli/tui/app.rs`，把问询面板、审批流程、会话恢复等状态机继续拆入 `wunder-cli/tui/app/` 子模块。
- 参考 codex 的 `custom_terminal` 思路，为 OSC 链接、宽字符宽度和 diff 背景色做更深的终端兼容优化。
