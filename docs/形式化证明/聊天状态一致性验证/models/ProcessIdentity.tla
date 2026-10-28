------------------------- MODULE ProcessIdentity -------------------------
EXTENDS Integers, Sequences
CONSTANT Fixed
VARIABLES scenario, phase, liveRound, visibleRound, oldText, incomingText, currentText
vars == <<scenario, phase, liveRound, visibleRound, oldText, incomingText, currentText>>

Init ==
    /\ scenario \in {"prefix", "tail", "snapshot"} \* frontend/scripts/regression/process-order-regression.test.ts:15; bounded real-entry fixtures
    /\ phase = 0
    /\ liveRound = 1
    /\ visibleRound = 1
    /\ oldText = "Fixture repeated completed"
    /\ incomingText = "Fixture repeated"
    /\ currentText = ""

NextRound ==
    /\ phase = 0 \* frontend/src/realtime/chat/threadViewStore.ts:649; next item admission
    /\ phase' = 1
    /\ liveRound' = 2
    /\ visibleRound' = 2
    /\ currentText' = incomingText
    /\ UNCHANGED <<scenario, oldText, incomingText>>

BindReasoning ==
    \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:178; exact canonical key
    /\ phase = 1 \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:178; exact canonical key
    /\ scenario = "prefix" \* frontend/src/components/chat/MessageTimelineTurnProcess.vue:178; exact canonical key
    /\ oldText' = IF Fixed THEN oldText ELSE incomingText
    /\ phase' = 2
    /\ UNCHANGED <<scenario, liveRound, visibleRound, incomingText, currentText>>

LateTail ==
    \* frontend/src/realtime/chat/threadViewStore.ts:672; false binding must return
    /\ phase = 1 \* frontend/src/realtime/chat/threadViewStore.ts:672; false binding must return
    /\ scenario = "tail" \* frontend/src/realtime/chat/threadViewStore.ts:672; false binding must return
    /\ currentText' = IF Fixed THEN currentText ELSE currentText \o " stale"
    /\ visibleRound' = IF Fixed THEN visibleRound ELSE 1
    /\ phase' = 2
    /\ UNCHANGED <<scenario, liveRound, oldText, incomingText>>

DurableRefresh ==
    \* frontend/src/realtime/chat/threadViewStore.ts:250; active newer model identity survives
    /\ phase = 1 \* frontend/src/realtime/chat/threadViewStore.ts:250; active newer model identity survives
    /\ scenario = "snapshot" \* frontend/src/realtime/chat/threadViewStore.ts:250; active newer model identity survives
    /\ currentText' = IF Fixed THEN currentText ELSE ""
    /\ visibleRound' = IF Fixed THEN visibleRound ELSE 1
    /\ phase' = 2
    /\ UNCHANGED <<scenario, liveRound, oldText, incomingText>>

Next == NextRound \/ BindReasoning \/ LateTail \/ DurableRefresh
TypeOK == /\ phase \in 0..2 /\ liveRound \in 1..2 /\ visibleRound \in 1..2
          /\ scenario \in {"prefix", "tail", "snapshot"}
          /\ oldText \in {"Fixture repeated completed", "Fixture repeated"}
          /\ incomingText = "Fixture repeated"
          /\ currentText \in {"", "Fixture repeated", "Fixture repeated stale"}
\* Completed canonical thoughts remain unchanged; mutators: frontend/src/components/chat/MessageTimelineTurnProcess.vue:178; frontend-slint/src/timeline_text.rs:251.
CompletedReasoningImmutable == oldText = "Fixture repeated completed"
\* Old model tails cannot move or append to the current thought; mutator: frontend/src/realtime/chat/threadViewStore.ts:673.
NoTailIdentityRollback == (scenario = "tail" /\ phase = 2) => (visibleRound = liveRound /\ currentText = incomingText)
\* A newer durable revision with an older model identity cannot replace live text; mutator: frontend/src/realtime/chat/threadViewStore.ts:250.
NoDurableIdentityRollback == (scenario = "snapshot" /\ phase = 2) => (visibleRound = liveRound /\ currentText = incomingText)
Spec == Init /\ [][Next]_vars
=============================================================================
