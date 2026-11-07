# 桌面版 UI 线程消息帧重入验证（win32-reentry-20261104）

验证日期：2026-11-04。对象：桌面版（frontend-slint）Windows 后端 UI 线程的消息帧重入纪律——winit 窗口过程、Slint 同步事件分发、IMM32 输入法写调用与窗口过程互斥锁之间的嵌套关系。这是对 `docs/经验教训.md`「输入法死锁家族」三例（capture_mouse 持锁自死锁、触摸合成事件、IMM32 写调用重入）共性的有界模型检查，不是对闭源 Win32/IME DLL 内部行为的证明。

## 契约与不变量

模型核心事实（来自 vendored winit 源码阅读）：`runner.rs:160` 的 `should_buffer` 只检查事件处理器是否已被取走，与消息泵状态无关，因此 Slint 的事件处理同步运行在每个 `WM_*` 消息帧内；同线程 SendMessage 会在任意帧深处立即重入窗口过程。

模型状态：`frames` 是窗口过程里存活的消息帧栈（ime = 输入法消息、sys = 普通系统消息、exec = EXEC_MSG_ID 延迟闭包、wedge = 卡死吸收态）；`execPending` 是待执行的延迟写闭包数；`writeCtx` 记录上一步是否发生了会回到输入法状态机的写调用（`ImmSetCompositionWindow`/`ImmSetCandidateWindow`/`ImmAssociateContextEx`）及其所处上下文。

规则（读者语言，TLC 在每个状态上验证）：

- `NoImeWriteUnderImeFrame`：输入法消息帧存活期间，本线程绝不再发起会回到输入法状态机的写调用。
- `ImeWriteAtFrameBoundary`：会回到输入法的写调用只发生在消息泵轮的延迟闭包里，不占用任何系统消息帧。
- `FrameStackBounded`：帧栈深度 ≤ 2——同步重入链条不得超过「一条消息 + 一次输入法回弹」。
- `NoLockWedge`：线程不会进入持锁重入的卡死吸收态。
- `TypeOK`：变量域。

`Fixed = FALSE` 建模修复前实现（`execute_in_thread` 在事件循环线程内联执行、capture_mouse 持锁调用）；`Fixed = TRUE` 建模当前实现（`defer_to_thread` 永远走消息队列、capture_mouse 先放锁再 `SetCapture`）。legacy IMM32 输入法被抽象为「可能在任意写调用后同步回弹一条组合消息」的黑盒：动作内部用非确定分支表达回弹与否，这个分支正是实机上「偶发卡死」的形态来源。

## 代码检查点

- Slint 帧内同步分发：`frontend-slint/vendor/winit-0.30.2/src/platform_impl/windows/event_loop/runner.rs:160`（should_buffer 只看处理器取走与否）、`runner.rs:208`（send_event）。
- 泵轮收消息成帧：`event_loop.rs:1583`（WM_IME_STARTCOMPOSITION）、`event_loop.rs:1715`（WM_MOUSEMOVE 等普通消息）、`event_loop.rs:2838`（EXEC_MSG_ID 闭包执行）。
- 修复前帧内直接写：`event_loop.rs:836`（execute_in_thread 线程内内联分支）；修复后投递：`event_loop.rs:867`（defer_to_thread 永远 PostMessageW）、`window.rs:924`/`window.rs:937`（set_ime_cursor_area/set_ime_allowed 延迟）。
- Slint 侧触发链：`frontend-slint/vendor/i-slint-core-1.18/items/text.rs:1179`（UpdateComposition/Commit）、`text.rs:1773`（update_ime）、`frontend-slint/vendor/i-slint-backend-winit-1.18/winitwindowadapter.rs:2558`（input_method_request）。
- IMM32 写调用：`frontend-slint/vendor/winit-0.30.2/src/platform_impl/windows/ime.rs:145-146`（ImmSetCompositionWindow/ImmSetCandidateWindow）、`ime.rs:156-158`（ImmAssociateContextEx）。
- 锁家族：`event_loop.rs:1048-1057`（capture_mouse，修复后先 drop 守卫再 SetCapture）。

## 结果

- 基础模型 31 个状态（`summary.json` 的 `modelStates`），4 条规则全部按预期复现反例，共 11 类（`stories.md`、`bug-1..11.mmd`）：
  - `ImeWriteAtFrameBoundary` 4 类：输入法消息帧或普通消息帧内的直接写（含静默与回弹两种终止态）；
  - `NoImeWriteUnderImeFrame` 4 类：组合处理中的直接写、普通消息写引发的回弹帧内再写；
  - `FrameStackBounded` 2 类：从输入法消息或普通消息出发的「写 → 回弹 → 再写」递归到达 3 层帧——对应实机上 UI 线程冻死在 IME DLL 内；
  - `NoLockWedge` 1 类：持锁调用 SetCapture，重入窗口过程再次取锁（死锁家族第一例）。
- 修复模型：全部 5 条规则成立，0 个状态仍违反（`chartStatus.wrong = 0`），36 个旧错误状态被消除或转为新增正确状态；「写 → 回弹」受支持的重入路径（延迟闭包内回弹后只投递）被显式保留并通过验证。
- `uncited` 为空：两份模型的每个动作条件都有 file:line 锚点；`brokenProperties` 为空。

## 真实回放

- 递归类（反例 8-11）：2026-11-04 实机观察「输入过程中卡死，且卡住后其他软件输入法一并冻结」，与递归反例同构（win32k 需与挂死的焦点线程同步，故全系统输入法停摆）。修复 `defer_to_thread` 已合入 vendored winit，x86_64-pc-windows-gnu 编译通过；Win7 真机验收（组合选字、连续提交、组合进行中切换窗口）待完成。
- 持锁类（反例 7）：历史上以快速连点左右键文本框卡死出现，修复 capture_mouse 放锁已合入（`event_loop.rs:1055`）。
- 未做自动化 UI 回放：消息帧重入发生在 winit 窗口过程内部，现有 smoke 覆盖不达这一层；该纪律由模型、代码锚点与 `frontend-slint/vendor/patches/winit-0.30.2.patch` 归档共同约束。

## 边界与未覆盖

- 只建模单线程 UI 侧：Pet 窗口线程、后台渲染与轮询线程不在线程模型内。
- IME 是黑盒抽象：只建模「写调用后可能同步回弹一条组合消息」的可观察契约，不证明任何具体 IME 的内部行为；TSF/直接写 UI 的现代输入法不在模型内。
- `writeCtx` 只记录最近一次写，下一个动作即复位；「写之后帧弹出」等后续事实不追踪。
- 窗口创建时的内联 `ImeContext::set_ime_allowed(window, false)`（`window.rs:1173`）不建模：该点不在任何 Slint 分发帧内，且不可能存在活动组合。
- ImeCommitDeduplicator、WM_IME_SETCONTEXT 的 ISC_SHOWUICOMPOSITIONWINDOW 剥离、OLE 拖放、触控合成鼠标等路径不在模型内。
- 模型界限：`MaxDepth = 5`、`MaxExec = 2`；调大 `MaxDepth` 只会拉长递归反例路径，不改变规则结论。

## 重跑

```
python scripts/check_win32_reentry_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <外部scratch>/win32-reentry
```

脚本把 `models/` 复制到仓库外 scratch，派生 Fixed 变体后运行 from-spec（`--fixed` 对照、四条规则 `--expect-broken`、`--png --cover`），断言无未引用条件、无残留错误状态、反例类恰好等于四条家族规则、各不变量 TLC 日志完整。原始 TLC 输出与完整报告留在 scratch，不归档。本主题验证于 2026-11-04，TLC tla2tools 1.7.4（SHA256 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88）。
