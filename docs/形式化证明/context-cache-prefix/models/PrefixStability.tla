---- MODULE PrefixStability ----
\* 上下文缓存前缀冻结：一条会话连续两次请求之间的前缀关系（缺陷版）。
\*
\* 抽象自 crates/wunder-runtime/src：
\*   Step     orchestrator/execute.rs 每轮组装 messages 并发送（只追加尾部）
\*   Compact  orchestrator/memory.rs::maybe_compact_messages（摘要替换历史头部）
\*   Prune    services/image_context.rs::enforce_image_budget（旧图片/工具结果就地换占位符）
\*   Restart  新一轮用户轮次/新会话的上下文重建（开新缓存纪元）
\*
\* Fixed = FALSE 时保留当前实现的真实分支：压缩与剪枝就地改写已经发出的
\* 前缀，却仍按旧缓存前缀记账；于是提供方的 KV 前缀从分歧点起整段失效。
\* 脚本 scripts/check_context_cache_prefix_formal.py 把本文件复制一份，只改
\* 模块名与 Fixed 常量，得到修复模型 PrefixStabilityFixed.tla。
EXTENDS Naturals, Sequences, TLC

\* Fixed: FALSE = 当前实现（缺陷），TRUE = 改写时声明新缓存纪元（修复）。
\* Messages: 抽象消息条数上限，够覆盖一次压缩加一次尾部追加。
CONSTANTS Fixed, Messages

VARIABLES
    sent,          \* 上一轮真正发给提供方的消息序列（哈希口径见 stable_json_message_hash）
    surface,       \* 本轮准备发送的消息序列（压缩/剪枝之后的历史视图）
    nextMsg,       \* 下一条待追加消息的 id
    mode,          \* "replay" = 逐字重放已发送前缀，"rewrite" = 重建后的视图
    rewrote,       \* 自上次发送以来是否发生过就地改写（改写证据）
    reset,         \* 是否已声明缓存纪元重启（前缀比对不再要求 sent 是前缀）
    compacted,     \* 本会话是否已经压缩过
    compactions    \* 本会话的压缩次数

vars == <<sent, surface, nextMsg, mode, rewrote, reset, compacted, compactions>>

Bodies == {"system", "user", "assistant", "placeholder", "summary"}

Body(n) == IF n = 1 THEN "system" ELSE IF n = 2 THEN "user" ELSE "assistant"

Msg(n, b) == [id |-> n, body |-> b]

\* 逐条比较消息本体，等价于 context.rs::prefix_divergence_index 返回 None。
IsPrefix(p, s) ==
  /\ Len(p) <= Len(s)
  /\ \A i \in 1..Len(p) : p[i] = s[i]

Init ==
  /\ sent = <<>>
  /\ surface = <<Msg(1, "system")>>
  /\ nextMsg = 2
  /\ mode = "replay"
  /\ rewrote = FALSE
  /\ reset = FALSE
  /\ compacted = FALSE
  /\ compactions = 0

\* 追加并发送：本轮请求 = 上一轮请求 + 一条新消息，前缀逐字不变。
Step ==
  /\ nextMsg <= Messages                          \* execute.rs:992 发送前比对前缀哈希
  /\ sent' = surface
  /\ surface' = Append(surface, Msg(nextMsg, Body(nextMsg)))
  /\ nextMsg' = nextMsg + 1
  /\ rewrote' = FALSE
  /\ reset' = FALSE
  /\ UNCHANGED <<mode, compacted, compactions>>

\* 压缩：摘要替换历史头部（第 1 条 system 也被改写），前缀从 0 开始分歧。
Compact ==
  /\ ~compacted                                   \* memory.rs:155 只压缩一次
  /\ Len(sent) >= 2                               \* memory.rs:155 历史够长才触发压缩
  /\ compacted' = TRUE
  /\ compactions' = compactions + 1
  /\ surface' = <<Msg(1, "summary")>> \o Tail(surface)
  /\ mode' = "rewrite"
  /\ rewrote' = TRUE
  /\ IF Fixed THEN reset' = TRUE ELSE reset' = FALSE
  /\ UNCHANGED <<sent, nextMsg>>

\* 剪枝 / 图片预算：把一条已发送的旧消息就地换成占位符，长度不变、字节改变。
Prune ==
  /\ (\E i \in 1..Len(sent) :                     \* image_context.rs:181 只处理历史消息
        /\ i <= Len(surface)
        /\ surface[i].body \in {"user", "assistant"}
        /\ surface' = [surface EXCEPT ![i].body = "placeholder"])
  /\ mode' = "rewrite"
  /\ rewrote' = TRUE
  /\ IF Fixed THEN reset' = TRUE ELSE reset' = FALSE
  /\ UNCHANGED <<sent, nextMsg, compacted, compactions>>

\* 新一轮用户轮次/新会话：从存储重建，开一个新缓存纪元（前缀从头开始）。
Restart ==
  /\ Len(sent) > 0                                \* execute.rs:992 已有前缀才谈重建
  /\ nextMsg <= Messages
  /\ sent' = <<>>
  /\ surface' = <<Msg(1, "system")>>
  /\ nextMsg' = 2
  /\ mode' = "replay"
  /\ rewrote' = FALSE
  /\ reset' = FALSE
  /\ compacted' = FALSE
  /\ compactions' = 0

Next ==
  \/ Step
  \/ Compact
  \/ Prune
  \/ Restart

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ sent \in Seq([id : 1..Messages, body : Bodies])
  /\ surface \in Seq([id : 1..Messages, body : Bodies])
  /\ nextMsg \in 1..(Messages + 1)
  /\ mode \in {"replay", "rewrite"}
  /\ rewrote \in BOOLEAN
  /\ reset \in BOOLEAN
  /\ compacted \in BOOLEAN
  /\ compactions \in 0..1

\* 1. 未声明缓存纪元重启时，上一轮请求必须是本轮请求的逐字前缀。
\*    违反它就意味着提供方的 KV 前缀从分歧点起全部作废。
AppendOnlyPrefix ==
  reset \/ IsPrefix(sent, surface)

\* 2. 前缀一旦发生分歧，就必须留下改写证据，不允许“悄悄改写”。
NoSilentRewrite ==
  IsPrefix(sent, surface) \/ rewrote

\* 3. 同一会话最多压缩一次，避免同一前缀反复付全价。
CompactionOnce ==
  compactions <= 1

\* 4. 逐字重放模式下，首条 system 消息必须保持原样。
ReplayKeepsSystem ==
  mode = "replay" =>
    (Len(surface) = 0 \/ (surface[1].id = 1 /\ surface[1].body = "system"))

\* 5. 压缩后尾部窗口仍然非空：至少留下 summary 加一条真实消息。
TailPreserved ==
  compacted => Len(surface) >= 2

====
