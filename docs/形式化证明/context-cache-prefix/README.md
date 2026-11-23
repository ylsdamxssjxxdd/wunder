# 上下文缓存前缀冻结（提示词缓存前缀）

## 现象

长会话里会出现一种很突兀的计费形态：上一步的缓存命中率接近 99.9%，下一步突然掉到 0.16%，
单步有 72 万 token 未命中。上下文并没有变长多少，模型也没换，变的只是**请求消息序列的形状**。

- 提供方的提示词缓存（prompt cache / KV 前缀缓存）只在请求**逐字重复**上一条前缀时命中；
- 只要某条已发送消息的字节发生变化，从这条消息起往后的缓存全部作废，后面的内容即使没变也要重算；
- 改写发生在中间位置时，损失的不是那一条消息，而是它之后的一整段。

实测里有三条会改写已发送前缀的路径：

| 来源 | 行为 | 结果 |
| --- | --- | --- |
| 历史压缩 | 摘要替换历史头部（system 与最早几轮一起被重写） | 从第 0 条起分歧 |
| 图片预算 | 旧图片降级成文本占位符，消息就地变短 | 从该图片所在条起分歧 |
| 预算裁剪 / 工具结果裁剪 | 从头部丢消息或把旧工具结果换成占位符 | 从裁剪点起分歧 |

## 抽象

一条会话的请求消息序列抽象成两个对象：`SentPrefix`（模型里的 `sent`，上一轮已发送前缀）
与 `NextRequest`（模型里的 `surface`，本轮准备发送的序列）。

| 模型变量 | 含义 | 代码对应 |
| --- | --- | --- |
| `sent` | 上一轮真正发给提供方的消息序列（逐条哈希） | `crates/wunder-runtime/src/orchestrator/execute.rs` 的 `last_sent_message_hashes` / `save_session_sent_prefix` |
| `surface` | 本轮准备发送的消息序列，压缩/剪枝之后的历史视图 | 每轮重新组装的 `messages` |
| `nextMsg` | 下一条待追加消息的 id | 新的 user / assistant / tool 消息 |
| `mode` | `replay` = 逐字重放已发送前缀；`rewrite` = 重建后的视图 | `crates/wunder-runtime/src/orchestrator/context.rs::normalize_messages` 的执行路径 |
| `rewrote` | 自上次发送以来是否发生过就地改写（改写证据） | `crates/wunder-runtime/src/orchestrator/execute.rs` 的 `context_cache_prefix` 事件 |
| `reset` | 是否已声明缓存纪元重启（不再要求 `sent` 是前缀） | `context_cache_prefix { append_only: false }` |
| `compacted` / `compactions` | 本会话是否压缩过、压缩过几次 | `crates/wunder-runtime/src/orchestrator/memory.rs::maybe_compact_messages` |

消息本身只保留 `id` 与 `body` 两个字段：`IsPrefix` 逐条比较，等价于
`context.rs::prefix_divergence_index` 返回 `None`（`messages_extend_prefix` 返回 `true`）。

动作与代码的对应：

| 动作 | 代码位置 | 说明 |
| --- | --- | --- |
| `Step` | `crates/wunder-runtime/src/orchestrator/execute.rs`（`context_cache_prefix` 事件前的哈希比对） | 追加一条消息并发送；等价于 `Append` / `Retry` |
| `Compact` | `crates/wunder-runtime/src/orchestrator/memory.rs::maybe_compact_messages` | 摘要替换历史头部，头部被重写 |
| `Prune` | `crates/wunder-runtime/src/services/image_context.rs::enforce_image_budget` | 旧图片/工具结果就地换成占位符；等价于 `RewriteAt` 的具体分支 |
| `Restart` | `crates/wunder-runtime/src/orchestrator/execute.rs`（新用户轮次的前缀记账） | 新一轮/新会话从存储重建，开新缓存纪元 |

## 六条不变式

| 不变式 | 含义 | 违反后果 |
| --- | --- | --- |
| `TypeOK` | 所有变量落在声明的类型域内 | 模型本身不成立 |
| `AppendOnlyPrefix` | 未声明缓存纪元重启时，`sent` 必须是 `surface` 的逐字前缀 | 提供方从分歧点起整段缓存作废 |
| `NoSilentRewrite` | 前缀一旦分歧，必须留下 `rewrote` 证据 | 静默改写，监控看不到命中率下跌的原因 |
| `CompactionOnce` | 同一会话最多压缩一次（`compactions <= 1`） | 同一前缀反复付全价 |
| `ReplayKeepsSystem` | 逐字重放模式下，首条 system 消息保持原样 | system 一改，整段前缀从第 0 条失效 |
| `TailPreserved` | 压缩后至少留下 summary 加一条真实消息 | 历史被清空，模型失去上下文 |

`AppendOnlyPrefix` 的模型定义：

```tla
AppendOnlyPrefix == reset \/ IsPrefix(sent, surface)
```

`reset` 表示本轮已经公开声明「新缓存纪元」，此时不再承诺跨轮次复用前缀；
没有声明时，就必须逐字重复上一轮已发送的消息。

## 缺陷与反例

`models/PrefixStability.tla`（`Fixed = FALSE`）保留当前实现的真实分支：压缩与剪枝直接修改
`surface`，把 `rewrote` 记为 `TRUE`，但**不开新缓存纪元**（`reset` 仍为 `FALSE`）。模型中的
最短反例：

| 步 | 动作 | `sent` | `surface` | 说明 |
| --- | --- | --- | --- | --- |
| 0 | `Init` | `<<>>` | `<<system>>` | 会话开始，system 已就绪 |
| 1 | `Step` | `<<system>>` | `<<system, user>>` | 发出 system，追加 user |
| 2 | `Step` | `<<system, user>>` | `<<system, user, assistant>>` | 发出 user，追加 assistant |
| 3 | `Compact` | `<<system, user>>` | `<<summary, user, assistant>>` | 摘要替换头部 |

第 3 步之后 `reset = FALSE` 且 `IsPrefix(sent, surface)` 为假，`AppendOnlyPrefix` 反例成立。
按 `prefix_divergence_index` 的口径，分歧下标是 0：下一轮请求从第 0 条起全部未命中。

`Prune` 给出同样形态的反例：长度不变，只把 `sent` 范围内某条消息的 `body` 换成
`placeholder`，分歧下标是这条消息的下标。真实场景里这对应「旧图片降级成占位符」或
「历史工具结果被裁剪」。

## 修复与证明

修复模型由脚本自动生成：复制 `PrefixStability.tla`，把模块名改成 `PrefixStabilityFixed`，
把 `Fixed = FALSE` 改成 `Fixed = TRUE`。修复后的行为只有一处不同——**任何就地改写都先声明
新缓存纪元**：

```tla
IF Fixed THEN reset' = TRUE ELSE reset' = FALSE
```

这与运行时把 `context_cache_prefix { append_only: false, diverged_at, previous_len, current_len }`
上报给上层、并据此重置前缀记账是同一件事：改写不再伪装成「同一前缀的延续」，缓存命中率
的下跌因此可解释、可观测。声明新纪元等价于把 `sent` 置空，所以 `AppendOnlyPrefix` 在
`replay` 序列内恒成立，其余五条不变式不受影响。

## 运行时补强（2026-11-23）

- 图片预算在压缩摘要构造前完成，摘要与主请求共享同一图片视图；assistant/tool 尾部通过追加 user 指令重放完整前缀，不再因尾部角色或内联图片直接退回扁平化。
- 工具面快照只在定义与执行映射等价时复用；schema、权限或映射变化递增 `schema_epoch`，明确开启新缓存纪元。
- Anthropic 使用 tools/system/当前尾部断点，并在线程状态中保留上一请求尾部作为第四断点，覆盖 20 内容块回溯窗口；thinking/redacted_thinking 与签名无损进入历史重放。
- OpenAI Responses 的 `input_tokens_details.cached_tokens` 与包含式 `input_tokens` 采用独立归一化口径；OpenAI 官方请求使用线程级 `prompt_cache_key`。

## 复验

```bash
python3 scripts/check_context_cache_prefix_formal.py \
  --statechart skills/statechart/scripts/statechart.mjs \
  --out /tmp/prefix-stability-report
```

- 脚本把 `models/` 下的三个文件复制到仓库外的临时目录，生成修复模型与修复配置；
- 缺陷模型必须在 `AppendOnlyPrefix` 上报反例（`--expect-broken AppendOnlyPrefix`）；
- 修复模型必须在 `TypeOK` 与其余五条不变式上枚举完整个状态空间且无错；
- 任何偏差以非零码退出；TLC 日志与报告只写到 `--out` 指定的仓库外目录；
- 运行需要 `node` 与 TLC（`tla2tools.jar`，可用 `TLA2TOOLS` 指定）。

约束：`Messages = 3`，状态空间可枚举；`scripts/` 不保存 TLC 状态与原始日志。

## 结论

- 缺陷模型复现「压缩/剪枝就地改写已发送前缀」的反例，分歧下标为 0（压缩）或改写位置（剪枝）；
- 修复模型在声明新缓存纪元后通过全部六条不变式；
- 运行时对应关系明确：`prefix_divergence_index` / `messages_extend_prefix` 负责检测，
  `context_cache_prefix` 事件负责上报，压缩与图片预算负责触发。

## 文件

| 文件 | 作用 |
| --- | --- |
| `models/PrefixStability.tla` | 缺陷模型（`Fixed = FALSE`） |
| `models/PrefixStability.cfg` | 常量与六条不变式；脚本据此生成修复配置 |
| `models/PrefixStability.map.mjs` | 不变式/动作 ↔ 代码位置索引，并给 statechart 提供状态与步骤标签 |
| `模型清单.json` | 模型文件、不变式与预期结论的机器可读清单 |
