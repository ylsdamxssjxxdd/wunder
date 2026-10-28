---------------------------- MODULE NativeSettlementFixed ----------------------------
EXTENDS Integers
VARIABLES phase, state, tool
vars == <<phase, state, tool>>
Init ==
    /\ phase = 0                 \* frontend-slint/src/native_chat.rs:77
    /\ state = "running"         \* frontend-slint/src/native_chat.rs:81
    /\ tool = "none"             \* frontend-slint/src/native_chat.rs:89
ToolCall ==
    /\ phase = 0                 \* frontend-slint/src/native_chat.rs:3768; first tool event
    /\ phase' = 1
    /\ tool' = "running"         \* frontend-slint/src/timeline.rs:1154
    /\ UNCHANGED state
CancelTerminal ==
    /\ phase = 1                 \* frontend-slint/src/native_chat.rs:3687
    /\ phase' = 2
    /\ state' = "cancelled"      \* frontend-slint/src/native_chat.rs:3705
    /\ UNCHANGED tool
FailedTerminal ==
    /\ phase = 1                 \* frontend-slint/src/native_chat.rs:3687
    /\ phase' = 3
    /\ state' = "failed"         \* frontend-slint/src/native_chat.rs:3706
    /\ UNCHANGED tool
LateFinal ==
    /\ phase = 3                 \* frontend-slint/src/native_chat.rs:3521; terminal guard rejects frame
    /\ phase' = 4
    /\ UNCHANGED <<state, tool>>
FinishCancelled ==
    /\ phase = 2                 \* frontend-slint/src/native_chat.rs:3439
    /\ phase' = 5
    /\ tool' = "interrupted"     \* frontend-slint/src/timeline.rs:1258; interrupt_running
    /\ UNCHANGED state
FinishFailed ==
    /\ phase \in {3, 4}          \* frontend-slint/src/native_chat.rs:3439
    /\ phase' = 6
    /\ tool' = "interrupted"     \* frontend-slint/src/timeline.rs:1258
    /\ UNCHANGED state
System == FinishCancelled \/ FinishFailed
Next == ToolCall \/ CancelTerminal \/ FailedTerminal \/ LateFinal \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..6 /\ state \in {"running", "cancelled", "failed", "completed"}
          /\ tool \in {"none", "running", "interrupted"}
\* Once the cancelled stream drains, no tool owned by that turn keeps spinning.
\* Mutators: native_chat.rs:3439; timeline.rs:1254.
CancelledToolSettles == phase = 5 => tool # "running"
\* A late final frame cannot rewrite the execution's failed terminal outcome.
\* Mutators: native_chat.rs:3521, :3681, :3687.
TerminalOutcomePreserved == phase = 4 => state = "failed"
Spec == Init /\ [][Next]_vars
=============================================================================
