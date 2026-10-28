------------------------- MODULE ProcessRefresh -------------------------
EXTENDS Integers

VARIABLES status, count, toolStatus, processItemsStatus, loadAbort, reloadTimer, batchStatus
vars == <<status, count, toolStatus, processItemsStatus, loadAbort, reloadTimer, batchStatus>>

Init ==
    /\ status = "running"             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:111; active turn fixture
    /\ count = 1                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:578; bounded structural input
    /\ toolStatus = "running"         \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:385; initial endpoint snapshot
    /\ processItemsStatus = "running" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:401
    /\ loadAbort = FALSE               \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:439; initial load has settled
    /\ reloadTimer = FALSE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:150
    /\ batchStatus = "none"           \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:387; scalar response abstraction

WatchStructuralChange ==
    /\ count = 1                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:578; one count-change input
    /\ count' = 2                      \* frontend/src/views/messenger/sections/MessageTimelineAssistant.vue:10
    /\ reloadTimer' = TRUE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ UNCHANGED <<status, toolStatus, processItemsStatus, loadAbort, batchStatus>>

ToolResultCommitted ==
    /\ toolStatus = "running"         \* crates/wunder-runtime/src/orchestrator/execute.rs:1707; bounded single successful tool result
    /\ toolStatus' = "completed"      \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:693; authoritative newer endpoint snapshot
    /\ UNCHANGED <<status, count, processItemsStatus, loadAbort, reloadTimer, batchStatus>>

WatchTerminalChange ==
    /\ status = "running"             \* frontend/src/views/messenger/sections/MessageTimelineAssistant.vue:12; one terminal input
    /\ toolStatus = "completed"       \* crates/wunder-runtime/src/orchestrator/execute.rs:1707; successful tool finishes before this terminal fixture
    /\ status' = "completed"          \* frontend/src/realtime/chat/threadViewStore.ts:61
    /\ reloadTimer' = TRUE             \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ UNCHANGED <<count, toolStatus, processItemsStatus, loadAbort, batchStatus>>

ReloadTimerStartsRequest ==
    /\ reloadTimer                     \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582; callback fires after 600 ms
    /\ ~loadAbort                      \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:363
    /\ reloadTimer' = FALSE            \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:583
    /\ loadAbort' = TRUE               \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:371
    /\ batchStatus' = toolStatus       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:383; response may retain this earlier cut
    /\ UNCHANGED <<status, count, toolStatus, processItemsStatus>>

ReloadTimerDuringRequest ==
    /\ reloadTimer                     \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:582
    /\ loadAbort                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:363; returns without retaining refresh demand
    /\ reloadTimer' = FALSE            \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:583
    /\ UNCHANGED <<status, count, toolStatus, processItemsStatus, loadAbort, batchStatus>>

LoadPageSettles ==
    /\ loadAbort                       \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:438; matching request finishes
    /\ processItemsStatus' = batchStatus \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:405; older HTTP snapshot accepted
    /\ loadAbort' = FALSE              \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:439
    /\ batchStatus' = "none"          \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:443; request scope ends, no dirty retry
    /\ UNCHANGED <<status, count, toolStatus, reloadTimer>>

System == ReloadTimerStartsRequest \/ ReloadTimerDuringRequest \/ LoadPageSettles
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
    /\ batchStatus \in {"none", "running", "completed"}

\* With no timer or request left, the displayed tool must reflect the durable result.
\* Mutators: MessageTimelineTurnProcess.vue:363, :405, :439, :578.
SettledToolAgreement == (status = "completed" /\ Settled) => processItemsStatus = toolStatus

\* Never invent successful execution before the backend has completed the tool.
\* Mutators: MessageTimelineTurnProcess.vue:405, :693.
NoFabricatedSuccess == processItemsStatus = "completed" => toolStatus = "completed"

Spec == Init /\ [][Next]_vars
=============================================================================
