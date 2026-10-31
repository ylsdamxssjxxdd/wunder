# 线程历史窗口外回读验证

日期：2026-10-31。对象：本地线程历史的投影读取路径——活动窗口裁剪后，窗口外的用户消息、回答与工作过程如何从 canonical rollout 回读。范围是存储读取层，不覆盖前端渲染、滚动交互或网络传输。

## 契约与边界

投影只保留最新 `ACTIVE_WINDOW_ITEMS` 条完整行，更早的轮次必须从 canonical JSONL 按需回读，不能把索引重新膨胀回全量。旧实现以 `ordinal % 1024` 的稀疏锚点作为回读起点，并用「锚点间隔的两倍 + 页大小 + 余量」作为记录预算；真实长线程的锚点区间可跨 7168 个 ordinal / 约 13MB，远超预算，区间中间的轮次整段读不出来。修复后以 `thread_turns.start_rollout_byte_offset`（轮次自身首条记录的精确字节）作为回读起点，锚点只在更靠近页边界时作为优化，并把记录与字节预算变成硬上界。

模型把多个物理片段、压缩源和并发写抽象为单轮、单块、单向请求：`TurnStart = 6`、`TurnEnd = 11`，轮次条目分布在非展示记录之间（`TurnItems = {8, 9, 11}`）；预修复起点 `AnchorStart = 1`，旧记录预算 4，间距 5 条记录即可超出；修复起点等于 `TurnStart`，预算 8 覆盖整轮。真实分页的多 chunk 合并、字节截断、分叉血缘（多个物理片段）与压缩源由真实回归补强，不从小模型推出无限规模结论。

## 不变量与结果

`models/ThreadHistoryReplay.tla` 与 `ThreadHistoryReplay.cfg` 是权威源；复验脚本只在仓库外生成仅 `Fixed = TRUE` 不同的对照模型，不删除任何规则。

| 规则 | 修复前 | 修复后 | 含义 |
| --- | --- | --- | --- |
| `TrimmedTurnNeverEmpty` | 反例（6 步路径） | 通过 | 仍有 canonical 条目的窗口外轮次，不能返回空页 |
| `TrimmedTurnComplete` | 通过（无可达完成态） | 通过 | 回读完成的页必须包含该轮全部条目 |
| `PageNewestFirst` | 通过 | 通过 | 页内序号严格降序（新到旧） |
| `BoundedScan` | 通过 | 通过 | 单次扫描不超过记录预算 |

摘要：7 个模型状态、6 条迁移，修复后 0 个残留错误状态，`uncited` 为空。10 份 TLC 文本输出全部包含 `Model checking completed`，无异常。TLC 1.7.4 jar SHA256：`936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88`。

这是有界模型检查，不是参数化演绎证明。反例路径的语义是「起点在锚点、预算小于锚点到轮次条目的距离」；修复模型把起点改为轮次边界后，同一组规则下该路径不可达。

## 反例

`bug-1`：打开活动窗口外的轮次 → 预修复从最近稀疏锚点起步 → 连续读取记录直到预算耗尽 → 页面为空但仍报告还有更多。真实症状是较早轮次的用户消息与工作过程整段空白。蓝色边框为编号路径，绿色框表示修复后不可达。

证据：`chart.png` / `chart.mmd`、`bug-1.png` / `bug-1.mmd`、`stories.md`、`summary.json`；TLC 完整文本输出在 `tlc/` 与 `tlc/fixed/`，其中的机器绝对路径已替换为 `scratch/` 标识，未改动状态、反例与结论。

## 模型与代码的对应

| 状态/动作 | 实现与真实回归 |
| --- | --- |
| `Start` | `crates/wunder-runtime/src/storage/thread_history.rs:1153` 的 `scan_lineage` 入口；起点选择在 1201/1265 行（锚点与轮次起点取更靠近页边界者），预算在 1183 行；`canonical_thread_log.rs:417/716` 为快照与聊天行传入 `turn_scan_hint`。预修复 HEAD 只查 `segment_anchor`。 |
| `Advance` | `thread_history.rs:1383` 的 `scan_segment_records` 逐行读取；1405/1437 行按读取行累计字节并检查记录预算，非展示记录同样消耗预算。 |
| `Finish` | `thread_history.rs:1239` 对每个有界块反转为页序后合并；1248 行到达轮次下界后结束。 |
| `Exhaust` | `thread_history.rs:1229` 在预算截断时丢弃该块的半个结果，返回 `more` 而不在页顶留洞。 |

## 真实回归

`cargo test -p wunder-runtime --features sqlite-storage --lib storage::canonical_thread_log_tests::trimmed_active_window_replays_earliest_chat_and_process_from_rollout -- --test-threads=1`

该测试用生产写入器生成 60 个用户轮次 × 40 个模型轮次，确认窗口确实裁剪到 2000 行后，逐页回读最早、中段与全部聊天轮次；末尾删除全部 `thread_item_anchors` 后再做一次全量分页，证明锚点只是优化、正确性依赖轮次起点。真实长线程副本（100 个用户轮次、约 130MB rollout、窗口外 90 轮）另行做端到端逐页验证：全部轮次可读，中段轮次工作过程回放 300 项，轮次起点最大间隔约 1.42MB，锚点最大间隔约 13.4MB。

分叉血缘与压缩源由另外两条生产链路回归覆盖（同样先删光锚点）：

```powershell
cargo test -p wunder-runtime --features sqlite-storage --lib storage::canonical_thread_log_tests::forked_thread_replays_inherited_rounds_beyond_the_active_window -- --test-threads=1
cargo test -p wunder-runtime --features sqlite-storage --lib storage::canonical_thread_log_tests::compressed_rollout_replays_turns_beyond_the_active_window -- --test-threads=1
```

前者在 60 轮父线程的第 30 轮分叉，再给子线程写入自有轮次：继承前缀在父 rollout、自身记录在子 rollout，两个物理片段的 ordinal 区间相邻且互斥；删除全部锚点后逐页读出 31 轮，并完整回放最早继承轮次的工作过程，验证 `segment_hint` 只把轮次起点绑定到其所属片段。后者把 60 轮线程的冷 rollout 压缩成 `.zst` 后再做同样的全量分页，验证字节偏移契约在解码快照上仍然成立。

## 已知失败与未覆盖

- 没有未解决的规则违反；预修复反例是预期结果。
- 未建模多 segment 轮换、压缩源、多 chunk 合并的字节位置、并发投影与真实磁盘延迟。分叉血缘与压缩源已由上面两条真实回归覆盖，但仍不构成形式化证明；并发投影读取由快照先对齐投影再读取的既有竞态回归覆盖。
- 未覆盖前端渲染、滚动位置、事件时序与性能时延；显示链路的实时投影证据见 `../聊天状态一致性验证/` 与 `../stream-follow-20261030/`。
- 未证明任意长度、任意锚点分布或无限状态的定理；结论仅限本配置下 7 个状态、6 条迁移的穷举。

## 重跑

Java 与 TLC 准备好后，在仓库外输出；参数为占位符：

```powershell
python scripts/check_thread_history_replay_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/thread-history-replay
```

脚本复制 `models/` 的三份权威文件，生成仅 `Fixed = TRUE` 不同的修复模型，执行 `from-spec --fixed --expect-broken TrimmedTurnNeverEmpty --cover --png`，然后校验 10 份 TLC 输出、`uncited` 与残留错误状态。

```powershell
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
```
