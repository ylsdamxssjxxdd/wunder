------------------------- MODULE ProcessRefreshFixed -------------------------
EXTENDS Integers

VARIABLES status, count, toolStatus, processItemsStatus, loadAbort, reloadTimer, batchStatus, refreshPending
vars == <<status, count, toolStatus, processItemsStatus, loadAbort, reloadTimer, batchStatus, refreshPending>>

Init ==
    /\ status = "running"             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:111; active turn fixture
    /\ count = 1                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:578; bounded structural input
    /\ toolStatus = "running"         \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:385; initial endpoint snapshot
    /\ processItemsStatus = "running" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:401
    /\ loadAbort = FALSE               \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:439; initial load has settled
    /\ reloadTimer = FALSE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:150
    /\ refreshPending = FALSE \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:152; bounded dirty gate
    /\ batchStatus = "none"           \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:387; scalar response abstraction

WatchStructuralChange ==
    /\ count = 1                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:578; one count-change input
    /\ count' = 2                      \* frontend/src/views/messenger/sections/MessageTimelineAssistant.vue:10
    /\ refreshPending' = TRUE \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:590; every structural event retained
    /\ reloadTimer' = TRUE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ UNCHANGED <<status, toolStatus, processItemsStatus, loadAbort, batchStatus>>

ToolResultCommitted ==
    /\ toolStatus = "running"         \* crates/wunder-runtime/src/orchestrator/execute.rs:1707; bounded single successful tool result
    /\ toolStatus' = "completed"      \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:693; authoritative newer endpoint snapshot
    /\ UNCHANGED <<status, count, processItemsStatus, loadAbort, reloadTimer, batchStatus, refreshPending>>

WatchTerminalChange ==
    /\ status = "running"             \* frontend/src/views/messenger/sections/MessageTimelineAssistant.vue:12; one terminal input
    /\ toolStatus = "completed"       \* crates/wunder-runtime/src/orchestrator/execute.rs:1707; successful tool finishes before this terminal fixture
    /\ status' = "completed"          \* frontend/src/realtime/chat/threadViewStore.ts:61
    /\ refreshPending' = TRUE \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:590; every structural event retained
    /\ reloadTimer' = TRUE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ UNCHANGED <<count, toolStatus, processItemsStatus, loadAbort, batchStatus>>

ReloadTimerStartsRequest ==
    /\ reloadTimer                     \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582; callback fires after 600 ms
    /\ ~loadAbort                      \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:363
    /\ reloadTimer' = FALSE            \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:583
    /\ refreshPending' = FALSE \* frontend/src/components/chat/processRefreshGate.ts:11; consumed by new request
    /\ loadAbort' = TRUE               \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:371
    /\ batchStatus' = toolStatus       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:383; response may retain this earlier cut
    /\ UNCHANGED <<status, count, toolStatus, processItemsStatus>>

ReloadTimerDuringRequest ==
    /\ reloadTimer                     \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ loadAbort                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:595; dirty gate retained during in-flight read
    /\ reloadTimer' = FALSE            \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:583
    /\ UNCHANGED <<status, count, toolStatus, processItemsStatus, loadAbort, batchStatus, refreshPending>>

LoadPageSettles ==
    /\ loadAbort                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:438; matching request finishes
    /\ processItemsStatus' = batchStatus \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:405; older HTTP snapshot accepted
    /\ loadAbort' = FALSE              \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:439
    /\ batchStatus' = "none"          \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:444; release request before draining dirty gate
    /\ UNCHANGED <<status, count, toolStatus, reloadTimer, refreshPending>>

DrainPendingRefresh ==
    /\ refreshPending                \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:449; retained refresh is drained
    /\ ~loadAbort                    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:444; prior reader released
    /\ refreshPending' = FALSE       \* frontend/src/components/chat/processRefreshGate.ts:20; consume once
    /\ loadAbort' = TRUE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:373; issue follow-up
    /\ batchStatus' = toolStatus     \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:388; read latest durable snapshot
    /\ UNCHANGED <<status, count, toolStatus, processItemsStatus, reloadTimer>>

System == ReloadTimerStartsRequest \/ ReloadTimerDuringRequest \/ LoadPageSettles \/ DrainPendingRefresh
Next == WatchStructuralChange \/ ToolResultCommitted \/ WatchTerminalChange \/ System
Settled == ~ENABLED System

\* The abstraction keeps one visible tool and one HTTP request in bounded domains.
TypeOK ==
    /\ status \in {"running", "completed"}
    /\ count \in {1, 2}
    /\ toolStatus \in {"running", "completed"}
    /\ processItemsStatus \in {"running", "completed"}
    /\ loadAbort \in BOOLEAN
    /\ reloadTimer \in BOOLEAN
    /\ refreshPending \in BOOLEAN
    /\ batchStatus \in {"none", "running", "completed"}

\* With no timer or request left, the displayed tool must reflect the durable result.
\* Mutators: MessageTimelineTurnProcess.vue:363, :405, :439, :578.
SettledToolAgreement == (status = "completed" /\ Settled) => processItemsStatus = toolStatus

\* Never invent successful execution before the backend has completed the tool.
\* Mutators: MessageTimelineTurnProcess.vue:405, :693.
NoFabricatedSuccess == processItemsStatus = "completed" => toolStatus = "completed"

Spec == Init /\ [][Next]_vars
=============================================================================
