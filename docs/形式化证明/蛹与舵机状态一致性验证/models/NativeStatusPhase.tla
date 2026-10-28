--------------------------- MODULE NativeStatusPhase ---------------------------
\* How a live thread's status reaches the reader, before the fix.
\*
\* Every view used to re-read the status phrase with its own prefix table, so a
\* phrase that one table did not know fell through to "idle". This model keeps
\* the three values the code kept: the engine phase, the phrase, and what the
\* window title shows.
\*
\* Code behind each step, as it was before the fix:
\*   Admit      frontend-slint/src/native_chat.rs:2256  set_session_status("请求中")
\*   Promote    frontend-slint/src/native_chat.rs:3349  set_session_status("正在生成…")
\*   Reason     frontend-slint/src/native_chat.rs:3654  active.state = "正在思考…"
\*   Compact    frontend-slint/src/native_chat.rs:3804  active.state = "上下文压缩中…"
\*   Queue      frontend-slint/src/native_chat.rs:3818  active.state = "模型请求正在排队…"
\*   Request    frontend-slint/src/native_chat.rs:3712  active.state = "模型响应中"
\*   Settle     frontend-slint/src/native_chat.rs:3762  active.state = "任务完成"
\*   Fail       frontend-slint/src/native_chat.rs:4003  active.state = "执行失败"
\*   Project    frontend-slint/src/native_chat.rs:2963  runtime_status_kind(phrase)
\*   Title      frontend-slint/src/native_chat.rs:2931  update_active_thread_filter(rows)
EXTENDS Naturals, TLC

VARIABLES
    phase,   \* what the engine is doing: "idle", "queued", "running", "done", "error"
    label,   \* the phrase the reducer wrote, or "" before the first turn
    shown    \* what the reader sees: a phase code, not a phrase

Phases == {"idle", "queued", "running", "done", "error"}
Shown == {"idle", "pending", "running", "done", "error"}

\* The phrase table the title used. Phrase keys stand for the phrases the reducer
\* writes: streaming 正在生成, thinking 正在思考, queued 任务已排队,
\* queued_wait 模型请求正在排队 · 前方 3 个任务, admitted 请求中,
\* model_response 模型响应中, compacting 上下文压缩中, complete 任务完成,
\* failed 执行失败.
\* The table knew the 正在 family and the 排队 family and nothing else, so the
\* admission phrase, the model phrase, the compaction phrase and the failure
\* phrase fell through to idle.
\* frontend-slint/src/native_chat.rs:2963
Project(value) ==
    CASE value = "streaming" -> "running"
      [] value = "thinking" -> "running"
      [] value = "queued" -> "pending"
      [] value = "queued_wait" -> "idle"   \* the dock phrase had no rule here
      [] value = "admitted" -> "idle"     \* the admission phrase fell through
      [] value = "model_response" -> "idle"
      [] value = "compacting" -> "idle"
      [] value = "complete" -> "done"
      [] value = "failed" -> "idle"       \* no error family in this table
      [] OTHER -> "idle"

\* A thread that is still working must never look idle.
\* The reader's sentence: while the work process says a request is running, the
\* window must not say 空闲.
LiveNeverIdle ==
    phase \in {"queued", "running"} => shown \in {"running", "pending"}

\* A settled failure must read as a failure, not as an idle thread.
FailureShowsError == phase = "error" => shown = "error"

Init ==
    /\ phase = "idle"
    /\ label = ""
    /\ shown = "idle"

\* The user sends a message; the reducer admits it before the first token.
Admit ==
    /\ phase' = "running"
    /\ label' = "admitted"
    /\ shown' = Project(label')

\* The engine promotes the request and the first frame arrives.
Promote ==
    /\ phase' = "running"
    /\ label' = "streaming"
    /\ shown' = Project(label')

\* A reasoning delta.
Reason ==
    /\ phase' = "running"
    /\ label' = "thinking"
    /\ shown' = Project(label')

\* The turn is admitted to the model queue.
Queue ==
    /\ phase' = "queued"
    /\ label' = "queued_wait"
    /\ shown' = Project(label')

\* The model call is admitted.
Request ==
    /\ phase' = "running"
    /\ label' = "model_response"
    /\ shown' = Project(label')

\* Context compaction takes the foreground.
Compact ==
    /\ phase' = "running"
    /\ label' = "compacting"
    /\ shown' = Project(label')

\* The turn settles with an answer.
Settle ==
    /\ phase' = "done"
    /\ label' = "complete"
    /\ shown' = Project(label')

\* The turn settles with a failure or a stop.
Fail ==
    /\ phase' = "error"
    /\ label' = "failed"
    /\ shown' = Project(label')

Next ==
    \/ Admit
    \/ Promote
    \/ Reason
    \/ Queue
    \/ Request
    \/ Compact
    \/ Settle
    \/ Fail

Spec == Init /\ [][Next]_<<phase, label, shown>>

\* A thread that never wrote a phrase is idle and shown idle.
TypeOK ==
    /\ phase \in Phases
    /\ shown \in Shown

=============================================================================
