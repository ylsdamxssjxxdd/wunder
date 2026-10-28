---------------------------- MODULE NativeSettlement ----------------------------
EXTENDS Integers
VARIABLES phase, state, tool
vars == <<phase, state, tool>>
Init ==
    /\ phase = 0                 \* frontend-slint/src/native_chat.rs:75; one admitted turn
    /\ state = "running"         \* frontend-slint/src/native_chat.rs:79
    /\ tool = "none"             \* frontend-slint/src/native_chat.rs:85
ToolCall ==
    /\ phase = 0                 \* frontend-slint/src/native_chat.rs:3724; first tool event
    /\ phase' = 1
    /\ tool' = "running"         \* frontend-slint/src/timeline.rs:1154; upsert_tool
    /\ UNCHANGED state
CancelTerminal ==
    /\ phase = 1                 \* frontend-slint/src/native_chat.rs:3666; cancelled event
    /\ phase' = 2
    /\ state' = "cancelled"      \* frontend-slint/src/native_chat.rs:3680
    /\ UNCHANGED tool
FailedTerminal ==
    /\ phase = 1                 \* frontend-slint/src/native_chat.rs:3666; failed event
    /\ phase' = 3
    /\ state' = "failed"         \* frontend-slint/src/native_chat.rs:3681
    /\ UNCHANGED tool
LateFinal ==
    /\ phase = 3                 \* frontend-slint/src/native_chat.rs:3511; guard only tests stopped
    /\ phase' = 4
    /\ state' = "completed"      \* frontend-slint/src/native_chat.rs:3662
    /\ UNCHANGED tool
FinishCancelled ==
    /\ phase = 2                 \* frontend-slint/src/native_chat.rs:3423; stream Finished
    /\ phase' = 5
    /\ UNCHANGED <<state, tool>> \* frontend-slint/src/native_chat.rs:91; cancelled is not failed; timeline.rs:1258
FinishFailed ==
    /\ phase \in {3, 4}          \* frontend-slint/src/native_chat.rs:3423
    /\ phase' = 6
    /\ tool' = IF state = "failed" THEN "interrupted" ELSE tool \* frontend-slint/src/timeline.rs:1258; fail_running only on failure
    /\ UNCHANGED state
System == FinishCancelled \/ FinishFailed
Next == ToolCall \/ CancelTerminal \/ FailedTerminal \/ LateFinal \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..6 /\ state \in {"running", "cancelled", "failed", "completed"}
          /\ tool \in {"none", "running", "interrupted"}
\* Once the cancelled stream drains, no tool owned by that turn keeps spinning.
\* Mutators: native_chat.rs:3427; timeline.rs:1254.
CancelledToolSettles == phase = 5 => tool # "running"
\* A late final frame cannot rewrite the execution's failed terminal outcome.
\* Mutators: native_chat.rs:3511, :3662, :3681.
TerminalOutcomePreserved == phase = 4 => state = "failed"
Spec == Init /\ [][Next]_vars
=============================================================================
