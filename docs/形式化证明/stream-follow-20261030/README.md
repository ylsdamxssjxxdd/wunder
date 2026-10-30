# 蛹聊天流式跟随验证（stream-follow-20261030）

验证日期：2026-10-30。对象：蛹（frontend-slint）聊天页面的两条流式跟随链——外层时间线（TimelineView）与工作过程列表（ToolProcess）。这是对指定事件交错的有界模型检查，不是整个渲染链路的形式化证明，也不是毫秒级端到端时延保证。

## 契约与不变量

跟随语义：读者停留在底部（`follow-output` / `follow-tail` 为真）时，新内容到达后视口必须自动跟到最新；读者上滚回看时不得抢夺滚动位置。

- `OuterTailFreshWhenFollowing`：跟随中且系统已安静（`Settled == ~ENABLED System`）时，外层时间线尾部必须就是最新内容。
- `InnerTailFreshWhenFollowing`：跟随中且系统已安静时，工作过程列表尾部必须就是最新内容。
- `TypeOK`：变量域与 `followed <= grown` 单调性。

`Fixed = FALSE` 建模修复前实现，`Fixed = TRUE` 建模当前实现，同一模块内切换。

## 代码检查点

- 外层发布帧重武装跟随：`frontend-slint/src/native_chat.rs:938`（stream_updates）→ `frontend-slint/ui/timeline.slint:883-888`（changed stream-revision）。
- 外层布局变高重武装跟随：`frontend-slint/ui/timeline.slint:889-899`（changed transcript-height）。
- 外层跟随定时器：`frontend-slint/ui/timeline.slint:865-875`、`840-842`（scroll-to-end）。
- 内层新增条目重武装：`frontend-slint/ui/tool_process.slint:231`（changed count）。
- 内层流式补丁只改高度：`frontend-slint/src/timeline.rs:498`（`sync_process_entry` 原地 `set_row_data`）；`process_revision` 只在分页时移动（`frontend-slint/src/chat_process.rs:315`）。
- 内层跟随定时器：`frontend-slint/ui/tool_process.slint:233-251`，条件在 `:244`。
- 修复：内层布局变高重武装，`frontend-slint/ui/tool_process.slint:210-216`（changed measured-height）。

## 结果

- 基础模型 1216 个状态（`summary.json` 的 `modelStates`），1 类复现反例：`InnerTailFreshWhenFollowing` 被打破。
- 反例路径（`bug-1.mmd` / `bug-1.png`，编号 1–7）：外层发布帧并跟随到底后，工作过程条目连续流式变高（无新增信号）三次，列表停在旧位置且仍显示"跟随中"——读者看到内容沉到可视区下方，必须手动滚动。
- 修复模型：0 个状态仍然违反（`chartStatus.wrong = 0`），15 个旧错误状态被消除或转为新增正确状态（`chartStatus.fixed = 15`）；同一反例路径的第 5 步在修复模型中进入"跟随待触发"状态。
- `OuterTailFreshWhenFollowing` 在两个模型中均成立：外层有 `changed transcript-height` 兜底，本来就不缺信号。
- `uncited` 为空：动作的每个守卫条件都有 file:line 代码锚点；无 `brokenProperties`。

## 真实回放

- 更新不及时（反例一）：用户在修复前的实际使用中观察到"工作过程区有些内容出来了但在下面看不到，需要手动滚动"（2026-10-30 会话记录），与 TLC 反例第 5–7 步同构；代码阅读逐行确认了机制（`sync_process_entry` 原地补丁不触 `count`/`revision`）。修复 `tool_process.slint:210-216` 已合入并通过编译。
- 缺漏类：模型内不存在内容丢失路径。真实代码中的有界截断属于设计而非缺陷：实时过程行滚动窗口 `MAX_LIVE_PROCESS_ROWS = 160`（`frontend-slint/src/timeline.rs:46`），被裁行可通过过程分页（`chat_process.rs`）向上翻回；思考详情截取末 32 KiB（`timeline.rs` flush_thinking）、条目 preview 上限 640 字符（`crates/wunder-desktop/native_workflow.rs:75`）。
- 未做自动化 UI 回放：内层跟随的端到端滚动断言目前没有 smoke 覆盖（`native_smoke.rs` 只断言外层滚动位置暂停/恢复），该路径由模型与代码锚点共同证明。

## 边界与未覆盖

- 布局延迟被抽象掉：模型中内容高度同步生效，真实的 3 次 16 ms 跟随 pass（`timeline.slint:865-875`）是其工程实现；token 级流式时延、帧合并节奏不在模型内。
- 上滚只建模"离开底部/回到底部"两态，未建模翻页加载更早历史（`load-older`）与锚定保持（`anchor-hold`）。
- 未覆盖轨迹视图（thread_trajectory）：它是按需打开的快报对话框，600 行封顶（`thread_trajectory.rs:43` `MAX_VIEW_ROWS`），预览有界，新旧页沿手动翻页；不在本模型范围。
- 云线程路径（`replace_external_timeline`）与本地路径共享同一视图跟随机制，模型未区分事件来源。

## 重跑

```
python scripts/check_stream_follow_formal.py --statechart C:\Users\sjxx\.codex\skills\statechart\scripts\statechart.mjs
```

脚本把 `models/` 复制到仓库外 scratch 运行 from-spec（含 `--fixed` 对照、`--expect-broken InnerTailFreshWhenFollowing`、`--png --cover`），断言无未引用条件、无残留错误状态、各不变量 TLC 日志完整。原始 TLC 输出与完整报告留在 scratch，不归档。
