------------------------- MODULE ProcessDelivery -------------------------
EXTENDS Integers, Sequences
CONSTANT Fixed
VARIABLES scenario, phase, count, deadline, firstDeadline, clock, requestStarted, visibleTool, toolStatus, processItemsStatus, status, order
vars == <<scenario, phase, count, deadline, firstDeadline, clock, requestStarted, visibleTool, toolStatus, processItemsStatus, status, order>>
Init ==
    /\ scenario \in {"refresh", "tool", "order"} \* frontend/scripts/regression/status-consistency-replay.mjs:continuous events and online tool fixtures
    /\ phase = 0 /\ count = 1 /\ deadline = 0 /\ firstDeadline = 0 /\ clock = 0
    /\ requestStarted = FALSE /\ visibleTool = FALSE
    /\ toolStatus = "running" /\ processItemsStatus = "running" /\ status = "running"
    /\ order = <<>>
StructuralEvent ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:534; one non-resetting timer
    /\ scenario = "refresh" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:534; one non-resetting timer
    /\ count < 4 \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:534; one non-resetting timer
    /\ count' = count + 1 /\ clock' = clock + 1
    /\ deadline' = IF Fixed /\ deadline # 0 THEN deadline ELSE clock + 3
    /\ firstDeadline' = IF firstDeadline = 0 THEN clock + 3 ELSE firstDeadline
    /\ UNCHANGED <<scenario,phase,requestStarted,visibleTool,toolStatus,processItemsStatus,status,order>>
DeadlineFires ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:540; requestProcessRefresh
    /\ scenario = "refresh" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:540; requestProcessRefresh
    /\ count = 4 \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:540; requestProcessRefresh
    /\ ~requestStarted \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:540; requestProcessRefresh
    /\ clock' = deadline /\ requestStarted' = TRUE
    /\ UNCHANGED <<scenario,phase,count,deadline,firstDeadline,visibleTool,toolStatus,processItemsStatus,status,order>>
ToolStarts ==
    \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:604
    /\ scenario = "tool" \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:604
    /\ phase = 0 \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:604
    /\ phase' = 1 /\ visibleTool' = Fixed
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,toolStatus,processItemsStatus,status,order>>
ToolFinishes ==
    \* frontend/src/realtime/chat/threadViewStore.ts:604; completed upsert
    /\ scenario = "tool" \* frontend/src/realtime/chat/threadViewStore.ts:604; completed upsert
    /\ phase = 1 \* frontend/src/realtime/chat/threadViewStore.ts:604; completed upsert
    /\ phase' = 2 /\ toolStatus' = "completed" /\ processItemsStatus' = "completed" /\ visibleTool' = TRUE /\ status' = "completed"
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,order>>
LateToolStart ==
    \* frontend/src/realtime/chat/threadViewStore.ts:604; terminal item cannot regress
    /\ scenario = "tool" \* frontend/src/realtime/chat/threadViewStore.ts:604; terminal item cannot regress
    /\ phase = 2 \* frontend/src/realtime/chat/threadViewStore.ts:604; terminal item cannot regress
    /\ phase' = 3 /\ processItemsStatus' = processItemsStatus
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,visibleTool,toolStatus,status,order>>
ConcurrentToolStarts ==
    \* crates/wunder-runtime/src/orchestrator/event_stream.rs:296; emission lock serializes starts
    /\ scenario = "order" \* crates/wunder-runtime/src/orchestrator/event_stream.rs:296; emission lock serializes starts
    /\ phase = 0 \* crates/wunder-runtime/src/orchestrator/event_stream.rs:296; emission lock serializes starts
    /\ phase' = 1 /\ order' = IF Fixed THEN <<1,2>> ELSE <<>>
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,visibleTool,toolStatus,processItemsStatus,status>>
ReversedToolCompletion ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:225; durable/live process_seq sort
    /\ scenario = "order" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:225; durable/live process_seq sort
    /\ phase = 1 \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:225; durable/live process_seq sort
    /\ phase' = 2 /\ order' = IF Fixed THEN order ELSE <<2,1>>
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,visibleTool,toolStatus,processItemsStatus,status>>
Next == StructuralEvent \/ DeadlineFires \/ ToolStarts \/ ToolFinishes \/ LateToolStart \/ ConcurrentToolStarts \/ ReversedToolCompletion
System == DeadlineFires \/ ToolFinishes \/ LateToolStart \/ ReversedToolCompletion
Settled == ~ENABLED System
TypeOK == /\ scenario \in {"refresh","tool","order"} /\ phase \in 0..3 /\ count \in 1..4
          /\ deadline \in 0..5 /\ firstDeadline \in 0..3 /\ clock \in 0..5
          /\ requestStarted \in BOOLEAN /\ visibleTool \in BOOLEAN
          /\ toolStatus \in {"running","completed"} /\ processItemsStatus \in {"running","completed"} /\ status \in {"running","completed"}
          /\ order \in {<<>>,<<1,2>>,<<2,1>>}
FirstDeadlineNotPostponed == deadline # 0 => deadline = firstDeadline
LiveToolVisible == (scenario = "tool" /\ phase = 1) => visibleTool
ToolOrderStable == (scenario = "order" /\ phase = 2) => order = <<1,2>>
SettledToolAgreement == (status = "completed" /\ Settled) => processItemsStatus = toolStatus
NoFabricatedSuccess == processItemsStatus = "completed" => toolStatus = "completed"
Spec == Init /\ [][Next]_vars
=============================================================================
