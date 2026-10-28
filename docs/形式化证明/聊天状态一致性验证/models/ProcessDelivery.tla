------------------------- MODULE ProcessDelivery -------------------------
EXTENDS Integers, Sequences
CONSTANT Fixed
VARIABLES scenario, phase, count, deadline, firstDeadline, clock, requestStarted, visibleTool, toolStatus, processItemsStatus, status, order
vars == <<scenario, phase, count, deadline, firstDeadline, clock, requestStarted, visibleTool, toolStatus, processItemsStatus, status, order>>
Init ==
    /\ scenario \in {"refresh", "tool", "order"} \* frontend/tests/e2e/timeline-process-stream.spec.ts:204; bounded four-event and two-tool fixture
    /\ phase = 0 /\ count = 1 /\ deadline = 0 /\ firstDeadline = 0 /\ clock = 0
    /\ requestStarted = FALSE /\ visibleTool = FALSE
    /\ toolStatus = "running" /\ processItemsStatus = "running" /\ status = "running"
    /\ order = <<>>
StructuralEvent ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:534; one non-resetting timer
    /\ scenario = "refresh" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:534; one non-resetting timer
    /\ count < 4 \* frontend/tests/e2e/timeline-process-stream.spec.ts:204; fixture bound, not a production guard
    /\ count' = count + 1 /\ clock' = clock + 1
    /\ deadline' = IF Fixed /\ deadline # 0 THEN deadline ELSE clock + 3
    /\ firstDeadline' = IF firstDeadline = 0 THEN clock + 3 ELSE firstDeadline
    /\ UNCHANGED <<scenario,phase,requestStarted,visibleTool,toolStatus,processItemsStatus,status,order>>
DeadlineFires ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:536; requestProcessRefresh
    /\ scenario = "refresh" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:536; requestProcessRefresh
    /\ count = 4 \* frontend/tests/e2e/timeline-process-stream.spec.ts:204; fixture stops producing after four events
    /\ ~requestStarted \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:536; requestProcessRefresh
    /\ clock' = deadline /\ requestStarted' = TRUE
    /\ UNCHANGED <<scenario,phase,count,deadline,firstDeadline,visibleTool,toolStatus,processItemsStatus,status,order>>
ToolStarts ==
    \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:611
    /\ scenario = "tool" \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:611
    /\ phase = 0 \* crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:611
    /\ phase' = 1 /\ visibleTool' = Fixed
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,toolStatus,processItemsStatus,status,order>>
ToolFinishes ==
    \* frontend/src/realtime/chat/threadViewStore.ts:611; completed upsert
    /\ scenario = "tool" \* frontend/src/realtime/chat/threadViewStore.ts:611; completed upsert
    /\ phase = 1 \* frontend/src/realtime/chat/threadViewStore.ts:611; completed upsert
    /\ phase' = 2 /\ toolStatus' = "completed" /\ processItemsStatus' = "completed" /\ visibleTool' = TRUE /\ status' = "completed"
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,order>>
LateToolStart ==
    \* frontend/src/realtime/chat/threadViewStore.ts:611; terminal item cannot regress
    /\ scenario = "tool" \* frontend/src/realtime/chat/threadViewStore.ts:611; terminal item cannot regress
    /\ phase = 2 \* frontend/src/realtime/chat/threadViewStore.ts:611; terminal item cannot regress
    /\ phase' = 3 /\ processItemsStatus' = processItemsStatus
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,visibleTool,toolStatus,status,order>>
ConcurrentToolStarts ==
    \* crates/wunder-runtime/src/orchestrator/event_stream.rs:315; emission lock serializes starts
    /\ scenario = "order" \* crates/wunder-runtime/src/orchestrator/event_stream.rs:315; emission lock serializes starts
    /\ phase = 0 \* crates/wunder-runtime/src/orchestrator/event_stream.rs:315; emission lock serializes starts
    /\ phase' = 1 /\ order' = IF Fixed THEN <<1,2>> ELSE <<>>
    /\ UNCHANGED <<scenario,count,deadline,firstDeadline,clock,requestStarted,visibleTool,toolStatus,processItemsStatus,status>>
ReversedToolCompletion ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:238; durable/live process_seq sort
    /\ scenario = "order" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:238; durable/live process_seq sort
    /\ phase = 1 \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:238; durable/live process_seq sort
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
\* Later structural events cannot postpone an armed refresh; mutator: frontend/src/components/chat/MessageTimelineTurnProcess.vue:533.
FirstDeadlineNotPostponed == deadline # 0 => deadline = firstDeadline
\* An admitted running tool is visible before its result; mutators: crates/wunder-runtime/src/orchestrator/event_stream.rs:240; frontend/src/realtime/chat/threadViewStore.ts:611.
LiveToolVisible == (scenario = "tool" /\ phase = 1) => visibleTool
\* Reverse completion preserves admission order; mutators: crates/wunder-runtime/src/orchestrator/event_stream.rs:315; frontend/src/components/chat/MessageTimelineTurnProcess.vue:238.
ToolOrderStable == (scenario = "order" /\ phase = 2) => order = <<1,2>>
\* Once settled, displayed tools reflect committed results; mutator: frontend/src/realtime/chat/threadViewStore.ts:611.
SettledToolAgreement == (status = "completed" /\ Settled) => processItemsStatus = toolStatus
\* Displayed success requires real tool success; mutator: frontend/src/realtime/chat/threadViewStore.ts:611.
NoFabricatedSuccess == processItemsStatus = "completed" => toolStatus = "completed"
Spec == Init /\ [][Next]_vars
=============================================================================
