# 反例路径（有界模型检查）

固定配置：`ItemIds` 抽象为条目 1、2；`MaxItems = 2`；`MaxRequests = 2`。`Fixed = FALSE` 为修复前 HEAD，`Fixed = TRUE` 为修复后。

## bug-1 · AppendOnlyPrefix 违反（5 步）

编号路径对应 `bug-1.mmd`：

1. `Write1`：把 1 号条目写入线程模型上下文，回放投影为 `whole`（保留全部字段与内联图片）。
2. `Send`：请求 1 的消息前缀 = `<< "whole" >>`，并记录 `sentLen = 1`（该条目已被发送）。
3. `Shrink1`：在两次请求之间把 1 号条目重新投影为 `shrunk`。修复前 HEAD 允许回改任意已写入条目，包括已被请求携带过的条目。
4. `Write2`：追加 2 号条目（`whole`）。
5. `Send`：请求 2 的消息前缀 = `<< "shrunk", "whole" >>`。

此时请求 1 的前缀 `<< "whole" >>` 不再是请求 2 前缀的前缀：同一线程相邻两次模型请求不再共享逐字节一致的消息前缀，服务端 prompt cache 无法复用已发送部分，命中率跌向 0。

## 与代码的对应

- `Shrink1` / `Shrink2` 对应 `crates/wunder-runtime/src/orchestrator/execute.rs:1633` 的 `prune_previous_inline_image_followups`（在两轮之间就地回改更早的 followup 消息），以及写入侧与读取侧投影不一致时 `crates/wunder-runtime/src/storage/thread_store.rs:103` 的 `compact_replayed_model_message` 对同一持久条目产生另一份形态。
- `DriftSystem` 对应未冻结 system 时 `crates/wunder-runtime/src/orchestrator/prompt.rs:375` 的 `resolve_session_prompt` 在续轮重建 base prompt（冻结后走 `:397` 直接返回 `meta.initial_system_prompt`）。

## 修复后

`Fixed = TRUE` 时 `RewriteAllowed(id)` 变为 `id \notin SentIds`，`DriftSystem` 被禁用：已被某次请求携带过的条目不再被重新投影，system 块冻结。同一配置下 `AppendOnlyPrefix`、`SystemStable` 均无残余错误状态，第 5 步不再可达（`bug-1.mmd` 中绿色框表示修复后不可达）。