# 验证记录

检查日期：2026-10-26。记录逻辑结果，不保存包含本机路径的原始测试输出。当前工作区有其他开发者同时修改，不能将所有全量失败归因于本补丁，也不能无证据宣称所有失败都在纯 HEAD 重现。

## TLC 与真实回归

四组旧模型均出现 README 所列规则反例；对应修复模型的 `TypeOK` 与全部原规则通过。`summary.json` 的未溯源条件均为空、修复对照图 `wrong=0`。其中 `NativeCatalogueStatus` 旧模型有 6 个状态、5 条迁移和 1 条预期反例；修复对照图仍错误状态为 0。模型、映射、编号 PNG/Mermaid、文字路径及统计均已归档；完整复现命令见 README。

真实旧行为对照采用仓库外独立副本，保留新增测试与必要字段适配，还原相关旧状态分支。实际断言结果：

| 生产入口 | 旧行为观测 | 修复版 |
| --- | --- | --- |
| 蛹取消终态与 Timeline.finish | 工具仍为运行态 | 工具非成功结清 |
| 蛹英文错误 | `failed()` 为 false | `failed()` 为 true |
| 蛹失败终态后 final | 状态被改成任务完成 | 保留失败且拒绝迟到正文 |
| 蛹 restore_folds | 终态仍保留 `process_paged=true` 与运行工具 | 最新页失效，新模型身份拒绝旧响应 |
| TUI 语义失败 / transport Error 后 Done | 实际 `Some(Finished)`，期望 `Some(Failed)` | 失败保持；活动订阅尚未关闭时条目已优先失败 |
| TUI 未完成普通工具卡 | 可见文本仍是调用中，未出现非成功结论 | 普通工具、读取组、命令和补丁显示失败或结果未知 |
| exec failed terminal | `saw_error()` 仍为 false | 语义失败被保留 |
| exec 普通工具缺失结果 | `open_items` 非空 | finish 关闭，且不伪造成功 |
| 蛹目录旧快照 | 用户提交后的旧 `done` 目录行立即替换本地热状态并触发完成提示 | 状态版本栅栏保留本地 `请求中`/运行状态；直接终态才提示，后台目录完成仍可提示 |

修复版定向回归共两组各 23 项通过：蛹 turn_tests 18 项（另显式跳过一个既有失败项）与 chat_process 5 项；舵机新 TUI 5 项、exec_events 9 项与 thread_registry 9 项。本次目录状态投影纯回归 6 项通过；它直接驱动生产 `StatusProjection`，覆盖本地提交、旧目录完成、直接终态、后台线程完成、下一次目录读取以及失败/停止终态保护。新增回归合计 21 项，其他为已有邻接测试。Slint 主窗口编译检查通过，静态组件截图已目视检查；修改 Rust 文件自身格式与 `git diff --check` 通过。

## 最终全量结果

```text
frontend-slint: 179 passed; 8 failed; 3 ignored
wunder-cli:    473 passed; 15 failed; 0 ignored
```

本轮新增终态回归在全量中也通过。全量失败不被隐藏，不通过删测、放松断言或修改无关实现强行变绿。

### 蛹失败项

这些相同失败在旧行为对照运行也出现，但对照不是纯 HEAD 全量验收：

- `file_icons::tests::resolves_like_the_web_panel`：默认文件图标预期过期。
- `help_browser::tests::art_sheet_images_typeset`：Slint 平台已在其他测试线程初始化。
- `help_browser::tests::typeset_smoke`：同上。
- `native_chat::turn_tests::one_tool_round_becomes_grouped_entries_and_frees_the_next_round`：原有过程折叠断言。
- `timeline::tests::first_action_uses_one_stable_process_fold_and_later_thought_keeps_its_row`：原有过程展开断言。
- `timeline::tests::history_prefix_move_rebases_process_handle_and_latest_body`：原有过程展开断言。
- `timeline::tests::process_children_patch_in_place_and_keep_folds_on_history_refresh`：原有过程展开断言。
- `workspace_files::tests::listing_projection_preserves_collapse_selection_and_rejects_old_results`：Slint 平台跨测试线程初始化。

### 舵机失败项

以下为当前工作区全量观测，不声明已经逐项在纯 HEAD 复验：

- `render::tests::unknown_results_keep_user_text_without_protocol_json`：未知结果字段过滤/文案预期。
- `tool_presentation::tests::english_aliases_still_classify_after_the_rename`：工具别名预期。
- `tui::app::exploring_group::tests::a_run_of_reads_shares_one_row_that_names_each_file_once`：合并读取行数预期。
- `tui::app::patch_log::tests::a_long_patch_folds_and_asks_for_enter`：补丁折叠文案预期。
- `tui::app::patch_log::tests::an_unknown_tools_summary_stays_in_the_sessions_language`：未知工具摘要预期。
- `tui::app::patch_log::tests::command_head_folds_a_multi_line_command_and_the_footer_states_the_outcome`：命令完成文案预期。
- `tui::app::patch_log::tests::two_files_each_keep_their_own_preview_rows`：补丁预览格式预期。
- `tui::app::snapshot_tests::a_slow_client_stops_the_feed_and_catches_up_by_replay`：fixture 在冻结线程提示词前写历史。
- `tui::app::snapshot_tests::an_open_selection_paints_over_a_hit_and_copies_the_same_text`：fixture 线程打开租约冲突。
- `tui::app::snapshot_tests::durable_replay_and_snapshot_recovery_land_the_same_text`：fixture 在冻结线程提示词前写历史。
- `tui::app::snapshot_tests::one_frame_scans_a_bounded_piece_and_the_frames_together_scan_the_document`：fixture 线程打开租约冲突。
- `tui::app::snapshot_tests::the_durable_feed_folds_commits_behind_the_start_baseline`：fixture 在冻结线程提示词前写历史。
- `tui::app::trajectory::tests::trajectory_reads_completed_content_and_exports_without_private_fields`：fixture 在冻结线程提示词前写历史。
- `tui::app::trajectory::tests::trajectory_refresh_catches_every_page_and_recovers_revisions`：同上。
- `tui::app::trajectory::tests::trajectory_storage_keyboard_search_detail_export_and_frames`：同上。

### 补充底层检查

扩大检查时也运行了桌面 native 单测组，观测 96 通过、6 失败，涉及未冻结提示词、单线程 Tokio 中的 `block_in_place`、Windows 路径与目标 follower 结清预期。本补丁没有修改这些 façade/目标领域实现，未将该组算作通过，也未据此宣称蛹完整链路已被形式化证明。

## 2026-10-27 增补：状态相位与过程分页游标

两组新模型的 TLC 结果（`--expect-broken` 未列出的规则打破会让命令失败，故全部规则都在下表里）：

| 模型 | TypeOK | 旧模型反例 | 修复模型 | 未溯源条件 |
| --- | --- | --- | --- | --- |
| `NativeStatusPhase` | 通过 | `LiveNeverIdle`、`FailureShowsError` | 两条均通过 | 空 |
| `NativeProcessPaging` | 通过 | `EarlierCanBeRead`、`LoadedPageKeepsItsEdge` | 两条均通过 | 空 |

计数取自各目录 `summary.json`：`NativeStatusPhase` 9 个模型状态、8 个图状态、37 条迁移；`NativeProcessPaging` 5 个模型状态、4 个图状态、5 条迁移。反例路径见 `native-status-phase/stories.md` 与 `native-process-paging/stories.md`；两者都是有限模型，不覆盖多线程计数、读取延迟、真实滚动位置与失败重试。

定向回归（本轮改动直接相关的测试组）：

| 测试组 | 结果 | 说明 |
| --- | --- | --- |
| `runtime_phase::tests` | 4 通过 | 穷举本端全部状态文案都能归入相位；`请求中` 归运行、排队短语归等待 |
| `completion_notice::tests` | 2 通过 | 完成通知改按相位判定，`请求中` 仍是 hot |
| `navigation_ui::tests` | 11 通过 | 侧栏行同时携带文案与相位，点更新写入相位 |
| `chat_process::tests` | 5 通过 | 分页投影、身份校验与历史页保留 |
| `native_chat::status::tests` | 6 通过 | 目录旧快照栅栏未受相位改造影响 |
| `native_chat::turn_tests` | 19 通过、1 失败 | 失败项是既有 `one_tool_round_becomes_grouped_entries_and_frees_the_next_round`，README 已列明需跳过 |
| `timeline::tests` | 22 通过、3 失败 | 三项断言 `open`/`process_open` 为真是既有「过程折叠预期」失败；本次未改动折叠逻辑，只新增 `process_first`/`process_last` 两个默认 `-1` 的字段 |

失败与本轮改动的关系说明：

- `timeline` 三项失败分别在 `first_action_uses_one_stable_process_fold_and_later_thought_keeps_its_row`、`history_prefix_move_rebases_process_handle_and_latest_body`、`process_children_patch_in_place_and_keep_folds_on_history_refresh`，断言的是过程折叠是否默认展开，不涉及分页边界字段。
- 全量测试没有在本机跑完：工作区 `target` 目录被其他开发者的 `cargo test -p wunder-runtime` 长时间占用，另建独立 `CARGO_TARGET_DIR` 时全量 codegen 在 58 分钟后仍未产出测试二进制，遂中止，不把该次运行算作证据。上述定向结果是复用 `frontend-slint/target` 缓存后实际执行的输出。
- Slint UI 通过 `cargo check`（build 脚本会编译 `.slint`，包含新字段与滚动加载逻辑）；本机 `slint-viewer` 版本与项目锁定的 Slint 1.18 不兼容，`--check` 报的是既有的 `index-of` 字段缺失，未作为验收证据。
- 滚动到顶自动加载、加载失败重试条、以及“加载更早后保持读者所在行”的视觉行为需要真实长过程人工验收，状态图与编译检查都不能替代。

## 重新运行全量

```powershell
cargo test --manifest-path frontend-slint/Cargo.toml --bin wunder-frontend-slint -- --test-threads=1
cargo test -p wunder-cli --bin wunder-cli -- --test-threads=1
```

不要用 `running 0 tests` 或只通过状态模型代替生产入口回归；不要将静态截图当作真实时延测量。未知工具结果仍需真实结果补水才能确定成功与否。
