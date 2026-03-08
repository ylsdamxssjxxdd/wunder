# wunder-cli 终端体验对齐 codex-main 对比分析方案

> 日期：2026-03-06  
> 参考项目：`C:\Users\32138\Desktop\参考项目\codex-main`

## 1. 结论

- `codex-main` 的终端界面明确是基于 `ratatui` 构建的。
- `wunder-cli` 与 `codex-main` 的差距不在于是否使用 `ratatui`，而在于围绕终端交互做了多少工程化打磨。
- 想把体验对齐到 codex 风格，关键不只是“更好看”，而是同时做到：
  - 更少空转重绘
  - 更稳定的流式输出节奏
  - 更清晰的输入/输出/弹窗语义分层
  - 更克制的终端原生视觉风格
  - 更可靠的渲染回归测试

## 2. 参考项目观察

从 `codex-main` 可归纳出几类关键能力：

### 2.1 事件驱动重绘

- 不是固定 tick 死循环刷新。
- 只有状态变化、输入事件、流式输出推进时才请求新帧。
- 对高频 redraw 做限流，避免刷帧过猛导致终端抖动和 CPU 空转。

### 2.2 更语义化的渲染单元

- 会话区、状态栏、输入区、弹窗、diff、代码块都各自是清晰的渲染单元。
- UI 不是单纯“日志文本堆叠”，而是有明确的语义层次。

### 2.3 更终端原生的视觉策略

- 颜色使用偏克制，避免强行覆盖终端主题。
- 重点依赖层次、边框、间距、焦点状态和高亮，而不是大面积背景色。

### 2.4 更细的流式输出治理

- 对 backlog、输出合并、帧率和可见区域刷新有明确控制。
- 在“快速但不抖”这件事上做了工程化取舍。

### 2.5 更强的回归手段

- 终端体验长期稳定，离不开 snapshot/vt100 级别的渲染测试。
- 否则每次改文案、改布局、改滚动逻辑都容易回退。

## 3. wunder-cli 之前的主要问题

- 重绘模型偏粗，终端体验更像“持续刷新中的应用”，不像“响应式终端 UI”。
- `app.rs` 承担了过多职责，交互状态、渲染细节、文案、流式处理耦合太重。
- 渲染模块拆分不足，导致后续继续打磨体验时成本很高。
- 中文文案存在乱码与占位字符串，直接拉低完成度。
- 缺少覆盖弹窗、popup、布局层的回归测试。

## 4. 本轮优化目标

本轮将“对齐 codex 体验”收敛为以下目标：

1. 将 TUI 从固定频率刷新切换到事件驱动 redraw。
2. 拆分会话区、输入区、状态栏、弹窗、popup 等渲染模块。
3. 引入统一主题与代码高亮基础设施。
4. 清理中文乱码与占位文案，统一 UTF-8。
5. 补一批轻量但稳定的 TUI 回归测试。

## 5. 本轮落地方案

### 5.1 调度层

- 新增 `wunder-cli/tui/frame_scheduler.rs`
- 将绘制从“固定轮询”改为“显式请求下一帧”
- 流式事件、键鼠输入、审批/问询状态变化统一触发 redraw

### 5.2 渲染层拆分

- `wunder-cli/tui/ui/layout.rs`
- `wunder-cli/tui/ui/status_line.rs`
- `wunder-cli/tui/ui/transcript.rs`
- `wunder-cli/tui/ui/composer.rs`
- `wunder-cli/tui/ui/modals.rs`
- `wunder-cli/tui/ui/popup.rs`

### 5.3 视觉与文本层

- 新增 `wunder-cli/tui/theme.rs`
- 新增 `wunder-cli/tui/highlight.rs`
- `markdown_render.rs` 接入代码块语法高亮
- 清理 TUI 相关中文乱码和 `???` 占位内容

### 5.4 回归测试层

- 为 `layout` 补布局尺寸回归测试
- 为 `popup` 补标题与候选渲染测试
- 为 `modals` 补中文标题与提示文案渲染测试

## 6. 当前阶段性结果

- `wunder-cli` 的 redraw 模型已经更接近 codex 风格。
- UI 模块边界已经清晰，后续继续做 diff、terminal backend、snapshot 不再需要继续把逻辑堆进单文件。
- 中文文案与 UTF-8 状态已经收口。
- `cargo check`、`cargo clippy -- -D warnings`、`cargo test --bin wunder-cli` 已可作为本轮交付基线。

## 7. 下一阶段建议

如果继续向 codex 体验逼近，建议按顺序推进：

1. **补 vt100/snapshot 测试**：把 transcript、approval、inquiry、popup 收进快照体系。
2. **做 diff 专用渲染器**：让补丁、文件变更、工具输出更像 codex 的结构化终端视图。
3. **引入 custom terminal 层**：处理 OSC 链接、宽字符宽度、终端兼容差异。
4. **继续拆小 `app.rs`**：把审批、问询、会话恢复、popup 状态机继续拆到 `wunder-cli/tui/app/`。

## 8. 判断标准

只有同时满足下面几条，才能认为真正“对齐 codex 终端体验”：

- 空闲时几乎不空转刷新
- 流式输出顺滑但不过刷
- 输入区、会话区、弹窗焦点稳定
- 代码块和工具结果有稳定视觉层次
- 小改文案或布局后，测试可以快速兜住回归
