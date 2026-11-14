---- MODULE ThoughtStream ----
\* 桌面版实时投影：思考行与工具行的结算模型（缺陷版，对应当前代码）。
\*
\* 抽象自 crates/wunder-desktop/frontend/src：
\*   Reason(r)      append_reasoning_item + apply_event 的 round 守卫
\*   ToolCall(t)    upsert_tool 的 pending 分支（含"已存在 id 早退"）
\*   ToolResult(t)  upsert_tool 的非 pending 分支
\*   Advance        llm_request / llm_output 推进 active.round
\*   Finish         finish() 的 interrupt_running
\*
\* rows 按创建顺序 append；slot 是 reducer 的 thinking 槽（0 表示空）。
EXTENDS Naturals, Sequences, TLC

CONSTANTS Rounds, Tools, MaxSeen

VARIABLES rows, slot, clock, seen, shown, called

vars == <<rows, slot, clock, seen, shown, called>>

ReasonId(r) == "r" \o ToString(r)
ToolId(t) == "t" \o ToString(t)

Row(id, kind, status) == [id |-> id, kind |-> kind, status |-> status]

Init ==
  /\ rows = <<>>
  /\ slot = 0
  /\ clock = 1
  /\ seen = 0
  /\ shown = 0
  /\ called = {}

\* 一条思考增量帧。r < clock 时，apply_event 直接丢弃（文本永久缺失）。
Reason(r) ==
  /\ r \in Rounds
  /\ seen < MaxSeen
  /\ IF r < clock
     THEN /\ UNCHANGED <<rows, slot, clock, shown, called>>
          /\ seen' = seen + 1
     ELSE LET id == ReasonId(r)
              exists == \E i \in 1..Len(rows) : rows[i].id = id
              closed == IF slot # 0 /\ rows[slot].id # id
                        THEN [rows EXCEPT ![slot].status = "done"]
                        ELSE rows
          IN /\ seen' = seen + 1
             /\ shown' = shown + 1
             /\ UNCHANGED <<clock, called>>
             /\ IF exists
                THEN /\ rows' = closed
                     /\ slot' = slot
                ELSE /\ rows' = Append(closed, Row(id, "reason", "running"))
                     /\ slot' = Len(closed) + 1

\* 工具结果帧：先到时会自己建一行（已结算）。
ToolResult(t) ==
  /\ t \in Tools
  /\ UNCHANGED <<slot, clock, seen, shown, called>>
  /\ LET id == ToolId(t)
         exists == \E i \in 1..Len(rows) : rows[i].id = id
     IN IF exists
        THEN rows' = rows
        ELSE rows' = Append(rows, Row(id, "tool", "done"))

\* 工具调用帧。缺陷：id 已存在时提前 return，思考行因此从不结算。
ToolCall(t) ==
  /\ t \in Tools
  /\ UNCHANGED <<clock, seen, shown>>
  /\ called' = called \cup {ToolId(t)}
  /\ LET id == ToolId(t)
         exists == \E i \in 1..Len(rows) : rows[i].id = id
         closed == IF slot # 0
                   THEN [rows EXCEPT ![slot].status = "done"]
                   ELSE rows
     IN IF exists
        THEN /\ rows' = rows
             /\ slot' = slot
        ELSE /\ rows' = Append(closed, Row(id, "tool", "running"))
             /\ slot' = 0

Advance ==
  /\ \E c \in Rounds : c > clock
  /\ clock' = clock + 1
  /\ UNCHANGED <<rows, slot, seen, shown, called>>

Finish ==
  /\ rows' = [i \in 1..Len(rows) |-> [rows[i] EXCEPT !.status = "done"]]
  /\ slot' = 0
  /\ UNCHANGED <<clock, seen, shown, called>>

Next ==
  \/ \E r \in Rounds : Reason(r)
  \/ \E t \in Tools : ToolCall(t)
  \/ \E t \in Tools : ToolResult(t)
  \/ Advance
  \/ Finish

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ rows \in Seq([id : {"r1", "r2", "t1", "t2"}, kind : {"reason", "tool"},
                   status : {"running", "done"}])
  /\ slot \in 0..Len(rows)
  /\ clock \in Rounds
  /\ seen \in 0..MaxSeen
  /\ shown \in 0..MaxSeen
  /\ called \subseteq {"t1", "t2"}

\* 只有最后一条行可以是"运行中"。旧条目转圈 = 该不变式被破坏。
NoStaleRunning ==
  \A i \in 1..Len(rows) : rows[i].status = "running" => i = Len(rows)

\* 已产生的思考文本必须都已显示：丢帧会让它永久缺字。
TextKept == shown = seen

\* 一条已经处理过调用帧的工具行之后，不允许还有思考行停在"运行中"。
\* 这正是截图里的形态：思考行夹在两个工具行之间，一直转圈。
NoReasonAfterCalled ==
  \A i \in 1..Len(rows) :
    rows[i].kind = "reason" /\ rows[i].status = "running" =>
      ~\E j \in (i+1)..Len(rows) :
        rows[j].kind = "tool" /\ rows[j].id \in called

\* 截图形态：一条思考行后面已经出现了工具行，它就不能还在转圈。
NoRunningBehindAction ==
  \A i \in 1..Len(rows) :
    rows[i].kind = "reason" /\ rows[i].status = "running" =>
      ~\E j \in (i+1)..Len(rows) : rows[j].kind = "tool"

====