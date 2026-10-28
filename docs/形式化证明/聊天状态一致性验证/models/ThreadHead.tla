---------------------------- MODULE ThreadHead ----------------------------
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
    /\ transportClosed' = TRUE       \* frontend/src/utils/ws.ts:120; request transport closes
    /\ notified' = TRUE              \* old frontend/src/realtime/chat/chatCanonicalEvents.ts:532
    /\ UNCHANGED <<latestStatus, rowStatus, headSeq, seq, completedSeen>>

ApplyThreadViewHead ==
    /\ headSeq = 1                   \* frontend/src/realtime/chat/threadViewStore.ts:357
    /\ latestStatus' = "completed"   \* frontend/src/realtime/chat/threadViewStore.ts:365
    /\ headSeq' = 2                  \* frontend/src/realtime/chat/threadViewStore.ts:376
    /\ UNCHANGED <<rowStatus, seq, transportClosed, notified, completedSeen>>

ApplyThreadViewChanges ==
    /\ headSeq = 2                   \* frontend/src/realtime/chat/threadViewStore.ts:465
    /\ seq = 1                       \* frontend/src/realtime/chat/threadViewStore.ts:474
    /\ rowStatus' = "completed"      \* frontend/src/realtime/chat/threadViewStore.ts:250
    /\ seq' = 2                      \* frontend/src/realtime/chat/threadViewStore.ts:474
    /\ completedSeen' = TRUE
    /\ UNCHANGED <<latestStatus, headSeq, transportClosed, notified>>

LateRunningReplay ==
    /\ completedSeen                 \* frontend/src/realtime/chat/threadViewStore.ts:471; same status_turn_id has already settled
    /\ latestStatus = "completed" /\ rowStatus = "completed"
    /\ latestStatus' = "running"     \* old frontend/src/realtime/chat/threadViewStore.ts:365
    /\ rowStatus' = "running"        \* old frontend/src/realtime/chat/threadViewStore.ts:250
    /\ headSeq' = 3 /\ seq' = 3
    /\ UNCHANGED <<transportClosed, notified, completedSeen>>

Next == CommitThreadView \/ TransportFinal \/ ApplyThreadViewHead \/ ApplyThreadViewChanges \/ LateRunningReplay
Settled == ~ENABLED Next

TypeOK ==
    /\ latestStatus \in {"idle", "running", "completed"}
    /\ rowStatus \in {"none", "running", "completed"}
    /\ headSeq \in {-1, 1, 2, 3}
    /\ seq \in {0, 1, 2, 3}
    /\ transportClosed \in BOOLEAN /\ notified \in BOOLEAN /\ completedSeen \in BOOLEAN

\* A completed thread entry must not coexist with its still-running bubble.
\* Mutators: frontend/src/realtime/chat/threadViewStore.ts:229, :465.
ImmediateTerminalAgreement == latestStatus = "completed" => rowStatus = "completed"

\* A request transport close has no execution identity and cannot announce success.
\* Mutators: frontend/src/stores/chatSendActions.ts:679, messengerControllerInstall.ts:11313.
NoUnverifiedSuccess == notified => latestStatus = "completed" /\ rowStatus = "completed"

\* A terminal row for the same execution cannot return to running on a late snapshot.
\* Mutators: frontend/src/realtime/chat/threadViewStore.ts:229, :243, :465.
TerminalDoesNotReopen == completedSeen => latestStatus = "completed" /\ rowStatus = "completed"

\* At the same committed watermark the head and bubble must agree.
SameCutAgreement == seq = headSeq => latestStatus = rowStatus

Spec == Init /\ [][Next]_vars
=============================================================================
