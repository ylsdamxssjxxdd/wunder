---------------------------- MODULE ThreadHeadFixed ----------------------------
EXTENDS Integers

VARIABLES latestStatus, rowStatus, headSeq, seq
vars == <<latestStatus, rowStatus, headSeq, seq>>

Init ==
    /\ latestStatus = "idle"     \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ rowStatus = "none"        \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ headSeq = -1              \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ seq = 0                   \* frontend/src/realtime/chat/threadViewStore.ts:26

CommitThreadView ==
    /\ rowStatus = "none"        \* frontend/src/realtime/chat/threadViewStore.ts:357; bounded initial bootstrap
    /\ latestStatus' = "running" \* frontend/src/realtime/chat/threadViewStore.ts:388
    /\ rowStatus' = "running"    \* frontend/src/realtime/chat/threadViewStore.ts:385
    /\ headSeq' = 1              \* frontend/src/realtime/chat/threadViewStore.ts:320
    /\ seq' = 1                  \* frontend/src/realtime/chat/threadViewStore.ts:387

ApplyThreadViewHead ==
    /\ headSeq = 1               \* frontend/src/realtime/chat/threadViewStore.ts:353; incoming view_seq=2 > 1
    /\ latestStatus' = "completed" \* frontend/src/realtime/chat/threadViewStore.ts:311
    /\ headSeq' = 2              \* frontend/src/realtime/chat/threadViewStore.ts:320
    /\ rowStatus' = "completed" \* frontend/src/realtime/chat/threadViewStore.ts:340; matching status_turn_id projection
    /\ UNCHANGED seq \* frontend/src/realtime/chat/threadViewStore.ts:378; catalog never consumes replay

ApplyThreadViewChanges ==
    /\ headSeq = 2               \* frontend/src/realtime/chat/threadViewStore.ts:394; delayed view_seq=2 frame
    /\ seq = 1                   \* frontend/src/realtime/chat/threadViewStore.ts:403; transcript cursor not advanced by catalog
    /\ rowStatus' = "completed"  \* frontend/src/realtime/chat/threadViewStore.ts:398
    /\ seq' = 2                  \* frontend/src/realtime/chat/threadViewStore.ts:403
    /\ UNCHANGED <<latestStatus, headSeq>> \* frontend/src/realtime/chat/threadViewStore.ts:309; equal seq accepted

System == ApplyThreadViewChanges
Next == CommitThreadView \/ ApplyThreadViewHead \/ System
Settled == ~ENABLED System

\* The head, displayed row and their watermarks stay in the modeled domains.
TypeOK ==
    /\ latestStatus \in {"idle", "running", "completed"}
    /\ rowStatus \in {"none", "running", "completed"}
    /\ headSeq \in {-1, 1, 2}
    /\ seq \in {0, 1, 2}

\* A completed thread entry must not coexist with its still-running bubble.
\* Mutators: threadViewStore.ts:350, threadViewStore.ts:394.
ImmediateTerminalAgreement == latestStatus = "completed" => rowStatus = "completed"

\* At the same committed watermark the head and bubble must agree.
\* Mutators: threadViewStore.ts:308, threadViewStore.ts:394.
SameCutAgreement == seq = headSeq => latestStatus = rowStatus

\* Once the delayed display frame has landed, the terminal bubble must agree.
\* This is conditional convergence, not a bound or a proof of network delivery.
SettledTerminalAgreement == Settled => ImmediateTerminalAgreement

Spec == Init /\ [][Next]_vars
=============================================================================
