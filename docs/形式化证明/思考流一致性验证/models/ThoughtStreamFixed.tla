---- MODULE ThoughtStreamFixed ----
\* 同一抽象的修复版：
\*   1) 工具帧到达时一律先结算思考行（不论该 id 是否已存在）；
\*   2) 迟到的思考帧不再丢弃，而是补进它自己那一轮的行（不重开思考）。
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

Reason(r) ==
  /\ r \in Rounds
  /\ seen < MaxSeen
  /\ LET id == ReasonId(r)
         exists == \E i \in 1..Len(rows) : rows[i].id = id
         closed == IF slot # 0 /\ rows[slot].id # id
                   THEN [rows EXCEPT ![slot].status = "done"]
                   ELSE rows
         late == r < clock
     IN /\ seen' = seen + 1
        /\ shown' = shown + 1
        /\ UNCHANGED <<clock, called>>
        /\ IF exists
           THEN /\ rows' = closed
                /\ slot' = slot
           ELSE IF late
                THEN /\ rows' = Append(closed, Row(id, "reason", "done"))
                     /\ slot' = slot
                ELSE /\ rows' = Append(closed, Row(id, "reason", "running"))
                     /\ slot' = Len(closed) + 1

ToolResult(t) ==
  /\ t \in Tools
  /\ UNCHANGED <<clock, seen, shown, called>>
  /\ LET id == ToolId(t)
         exists == \E i \in 1..Len(rows) : rows[i].id = id
         closed == IF slot # 0 THEN [rows EXCEPT ![slot].status = "done"] ELSE rows
     IN IF exists
        THEN /\ rows' = closed
             /\ slot' = 0
        ELSE /\ rows' = Append(closed, Row(id, "tool", "done"))
             /\ slot' = 0

ToolCall(t) ==
  /\ t \in Tools
  /\ UNCHANGED <<clock, seen, shown>>
  /\ called' = called \cup {ToolId(t)}
  /\ LET id == ToolId(t)
         exists == \E i \in 1..Len(rows) : rows[i].id = id
         closed == IF slot # 0 THEN [rows EXCEPT ![slot].status = "done"] ELSE rows
     IN IF exists
        THEN /\ rows' = closed
             /\ slot' = 0
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

\* 运行中的思考行之后不允许再出现工具行：这正是"后面条目出来后仍转圈"。
NoRunningBehindAction ==
  \A i \in 1..Len(rows) :
    rows[i].kind = "reason" /\ rows[i].status = "running" =>
      ~\E j \in (i+1)..Len(rows) : rows[j].kind = "tool"

\* 已产生的思考文本必须都已显示。
TextKept == shown = seen

\* 已处理过调用帧的工具行之后，不允许还有思考行停在运行中。
NoReasonAfterCalled ==
  \A i \in 1..Len(rows) :
    rows[i].kind = "reason" /\ rows[i].status = "running" =>
      ~\E j \in (i+1)..Len(rows) :
        rows[j].kind = "tool" /\ rows[j].id \in called

====
