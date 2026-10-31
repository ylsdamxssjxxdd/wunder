# 上下文缓存前缀冻结验证

日期：2026-10-31。对象：单线程模型请求的消息前缀——同一次会话的相邻两次（同一轮内多模型轮次之间、以及跨用户轮次）发送给大模型的请求，是否逐字节重复上一次请求的前缀，以及冻结的 system 块是否保持不变。范围是请求组装与回放投影，不覆盖服务端缓存策略、网络传输或模型服务端 chat template。

## 契约与边界

大模型 API 的提示词前缀缓存只有在「新请求逐字节重复旧请求的消息前缀」且「system 块不变」时才复用已缓存前缀。仓库已把线程 system prompt 在首轮确定后按 `crates/wunder-runtime/src/orchestrator/prompt.rs:375` 的 `resolve_session_prompt` 冻结（`:397` 直接返回 `meta.initial_system_prompt`），并把持久条目回放按 `crates/wunder-runtime/src/storage/thread_store.rs:103` 的 `compact_replayed_model_message` 投影成只含模型协议字段的形态。本主题验证的是这条不变量本身，而不是缓存命中率数值；命中率另作测量（见 `../形式化验证必做清单.md` FV-12 注）。

模型把持久存储抽象为一条只增条目序列 `written`，把每个条目抽象为「运行时回放给模型的投影」：`whole` 保留全部诊断字段与内联图片，`shrunk` 是读取侧重新推导出的精简投影。一次请求的正文 `RequestBody` 等于「已写入全部条目的回放投影」。因此相邻两次请求共享前缀，当且仅当：没有已发送过的条目被重新投影，且 system 块不变。

预修复 HEAD 允许在两次请求之间对任意已写入条目重新投影（最现实的触发点是 `crates/wunder-runtime/src/orchestrator/execute.rs:1633` 的 `prune_previous_inline_image_followups` 在两轮之间就地改写更早的 followup 消息；以及写入侧形态与读取侧投影不一致时同一持久条目被回放成另一份形态）。修复为：条目在写入时一次性选定投影、已被某次请求携带过的条目不再回改、system 块冻结。

## 不变量与结果

`models/PrefixStability.tla` 与 `models/PrefixStability.cfg` 是权威源；复验脚本只在仓库外生成仅 `Fixed = TRUE` 不同的对照模型，不删除任何规则。

| 规则 | 修复前 | 修复后 | 含义 |
| --- | --- | --- | --- |
| `AppendOnlyPrefix` | 反例（5 步路径） | 通过 | 每个新请求逐字节重复上一次请求的消息前缀 |
| `SystemStable` | 反例（`DriftSystem`） | 通过 | 线程创建后冻结的 system 块不再变化 |

摘要：条目 1、2 与至多 2 次请求的有界状态空间；修复后同组两条规则无残余错误状态，`uncited` 为空。TLC 文本输出与 `summary.json` 由复验脚本在具备 Java/TLC 的机器上生成后归档到本目录的 `tlc/`；本机（无 Java、无 `TLA2TOOLS`）暂未归档，避免伪造运行结果。

这是有界模型检查，不是参数化演绎证明。反例路径的语义是「已发送过的条目在两次请求之间被重新投影」；修复模型把回改限制到未被请求携带过的条目后，同一组规则下该路径不可达。

## 反例

`bug-1`：写 1 号条目 → 发送请求 1（前缀 `<< whole >>`）→ 回改 1 号条目投影为 `shrunk` → 写 2 号条目 → 发送请求 2（前缀 `<< shrunk, whole >>`）。真实症状是同一线程相邻两次模型请求不再共享逐字节一致前言，服务端 prompt cache 无法复用，命中率跌向 0。蓝色边框为编号路径，绿色框表示修复后不可达。

证据：`bug-1.mmd`、`chart.mmd`、`stories.md`；PNG 与 TLC 文本输出由复验脚本生成后归档。

## 模型与代码的对应

| 状态/动作 | 实现与真实回归 |
| --- | --- |
| `Init` / `Write*` | `crates/wunder-runtime/src/orchestrator/execute.rs:503` 从持久历史取 `model_messages` 回放，`:509`/`:514` 依次追加历史与当前用户消息；`:1051`/`:1121` 追加助手消息，`:1572` 追加工具观察。 |
| `Send` / `RequestBody` | `crates/wunder-runtime/src/orchestrator/llm.rs:434` 的 `build_chat_messages` 逐条构造请求消息，`:666` 的 `build_context_cache_probe` 计算全套前缀哈希。 |
| `Shrink*` | `crates/wunder-runtime/src/services/chat_payload_sanitizer.rs` 的 `prune_previous_inline_image_followups`（由 `execute.rs:1633` 调用）与 `crates/wunder-runtime/src/storage/thread_store.rs:103` 的 `compact_replayed_model_message` 读取侧投影。 |
| `DriftSystem` | `crates/wunder-runtime/src/orchestrator/prompt.rs:375` 的 `resolve_session_prompt`（冻结分支 `:397` 返回 `initial_system_prompt`）。 |
| `AppendOnlyPrefix` 运行时校验 | 新增 `crates/wunder-runtime/src/orchestrator/context.rs` 的 `stable_messages_hash` / `prefix_divergence_index`，在 `execute.rs` 每轮请求前比较本轮与上一轮已发送前缀。 |

## 真实回归

`crates/wunder-runtime/src/orchestrator/context.rs` 内的单元测试覆盖前缀连续性判定与首个分歧下标；`crates/wunder-runtime/src/storage/thread_store.rs` 内新增测试覆盖「持久条目读取侧投影幂等且与写入侧形态一致」。

```powershell
cargo test -p wunder-runtime --lib orchestrator::context::tests -- --test-threads=1
cargo test -p wunder-runtime --lib storage::thread_store::tests::compact_replayed_model_message -- --test-threads=1
```

## 已知失败与未覆盖

- 本机无 Java/TLC，TLC 穷举结果、PNG 与 `summary.json` 待具备工具链后由复验脚本生成；模型与脚本已就绪。
- 未建模内联图片 base64 的具体字节、多条目一次性回改、请求分页与传输层；未证明任意长度、任意条目分布或无限状态。
- 只讨论 Wunder 侧「是否逐字节重复前缀」。服务端缓存逐出（如两次请求间隔过久）、native tools 由服务端 chat template 注入到最新用户消息附近等**服务端**因素不在本模型范围内，只能通过真实链路测量（见 `../形式化验证必做清单.md` FV-12 注与 `../../经验教训.md`）。

## 重跑

Java 与 TLC 准备好后，在仓库外输出；参数为占位符：

```powershell
python scripts/check_context_cache_prefix_formal.py --statechart <statechart-skill>/scripts/statechart.mjs --out <external-scratch>/context-cache-prefix
```

脚本复制 `models/` 的三份权威文件，生成仅 `Fixed = TRUE` 不同的修复模型，执行 `from-spec --fixed --expect-broken AppendOnlyPrefix --cover --png`，然后校验 6 份 TLC 输出、`uncited` 与残留错误状态。

```powershell
python -m unittest discover -s tests -p test_formal_proof_layout.py -v
```