------------------------- MODULE ThreadHeadFixed -------------------------
EXTENDS Integers

VARIABLES latestStatus, rowStatus, headSeq, seq, transportClosed, notified, completedSeen
vars == <<latestStatus, rowStatus, headSeq, seq, transportClosed, notified, completedSeen>>

Init ==
    /\ latestStatus = "idle"       \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ rowStatus = "none"          \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ headSeq = -1                 \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ seq = 0                      \* frontend/src/realtime/chat/threadViewStore.ts:26
    /\ transportClosed = FALSE      \* frontend/src/stores/chatSendActions.ts:650
    /\ notified = FALSE              \* frontend/src/views/messenger/controller/messengerControllerInstall.ts:11309
    /\ completedSeen = FALSE         \* frontend/src/realtime/chat/threadViewStore.ts:465

CommitThreadView ==
    /\ rowStatus = "none"           \* frontend/src/realtime/chat/threadViewStore.ts:496
    /\ latestStatus' = "running"    \* frontend/src/realtime/chat/threadViewStore.ts:365
    /\ rowStatus' = "running"       \* frontend/src/realtime/chat/threadViewStore.ts:250
    /\ headSeq' = 1                  \* frontend/src/realtime/chat/threadViewStore.ts:376
    /\ seq' = 1                      \* frontend/src/realtime/chat/threadViewStore.ts:474
    /\ UNCHANGED <<transportClosed, notified, completedSeen>>

TransportFinal ==
    /\ latestStatus = "running" \* frontend/src/stores/chatSendActions.ts:679; authoritative head remains running
    /\ rowStatus = "running" \* frontend/src/realtime/chat/threadViewStore.ts:465; matching visible row remains running
    /\ transportClosed' = TRUE       \* frontend/src/utils/ws.ts:120; closes request only
    /\ UNCHANGED <<latestStatus, rowStatus, headSeq, seq, notified, completedSeen>>

ApplyThreadViewHead ==
    /\ headSeq = 1                   \* frontend/src/realtime/chat/threadViewStore.ts:357
    /\ latestStatus' = "completed"   \* frontend/src/realtime/chat/threadViewStore.ts:365
    /\ rowStatus' = "completed"      \* frontend/src/realtime/chat/threadViewStore.ts:229; matching status_turn_id projection
    /\ headSeq' = 2                  \* frontend/src/realtime/chat/threadViewStore.ts:376
    /\ UNCHANGED <<seq, transportClosed, notified, completedSeen>>

ApplyThreadViewChanges ==
    /\ headSeq = 2                   \* frontend/src/realtime/chat/threadViewStore.ts:465
    /\ seq = 1                       \* frontend/src/realtime/chat/threadViewStore.ts:474
    /\ rowStatus' = "completed"      \* frontend/src/realtime/chat/threadViewStore.ts:250
    /\ seq' = 2                      \* frontend/src/realtime/chat/threadViewStore.ts:474
    /\ completedSeen' = TRUE
    /\ UNCHANGED <<latestStatus, headSeq, transportClosed, notified>>

LateRunningReplay ==
    /\ completedSeen                 \* frontend/src/realtime/chat/threadViewStore.ts:361; retain terminal same turn
    /\ latestStatus = "completed" \* frontend/src/realtime/chat/threadViewStore.ts:361; terminal head cannot regress
    /\ rowStatus = "completed" \* frontend/src/realtime/chat/threadViewStore.ts:251; terminal row cannot reopen
    /\ headSeq' = 3 /\ seq' = 3
    /\ UNCHANGED <<latestStatus, rowStatus, transportClosed, notified, completedSeen>>

Next == CommitThreadView \/ TransportFinal \/ ApplyThreadViewHead \/ ApplyThreadViewChanges \/ LateRunningReplay

TypeOK ==
    /\ latestStatus \in {"idle", "running", "completed"}
    /\ rowStatus \in {"none", "running", "completed"}
    /\ headSeq \in {-1, 1, 2, 3}
    /\ seq \in {0, 1, 2, 3}
    /\ transportClosed \in BOOLEAN /\ notified \in BOOLEAN /\ completedSeen \in BOOLEAN

ImmediateTerminalAgreement == latestStatus = "completed" => rowStatus = "completed"
NoUnverifiedSuccess == notified => latestStatus = "completed" /\ rowStatus = "completed"
TerminalDoesNotReopen == completedSeen => latestStatus = "completed" /\ rowStatus = "completed"
SameCutAgreement == seq = headSeq => latestStatus = rowStatus

Spec == Init /\ [][Next]_vars
=============================================================================
