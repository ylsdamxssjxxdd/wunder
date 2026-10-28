---------------------------- MODULE CliSettlement ----------------------------
EXTENDS Integers
VARIABLES phase, status, tool
vars == <<phase, status, tool>>
Init ==
    /\ phase = 0                 \* crates/wunder-cli/tui/app.rs:6958; request admitted
    /\ status = "working"        \* crates/wunder-cli/tui/app.rs:6960
    /\ tool = "none"             \* crates/wunder-cli/tui/app.rs:6968
ToolCall ==
    /\ phase = 0                 \* crates/wunder-cli/tui/app.rs:8192; first call
    /\ phase' = 1
    /\ tool' = "running"         \* crates/wunder-cli/tui/app.rs:8227
    /\ UNCHANGED status
FailedTerminal ==
    /\ phase = 1                 \* crates/wunder-cli/tui/app.rs:8084; event switch
    /\ phase' = 2
    /\ UNCHANGED <<status, tool>> \* crates/wunder-cli/tui/app.rs:8337; terminal falls through
StreamDone ==
    /\ phase = 2                 \* crates/wunder-cli/tui/app.rs:7948; no pending replay
    /\ phase' = 3
    /\ status' = "finished"      \* crates/wunder-cli/tui/app.rs:7956
    /\ UNCHANGED tool            \* crates/wunder-cli/tui/app.rs:7976; cleanup leaves tool cards
System == StreamDone
Next == ToolCall \/ FailedTerminal \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..3 /\ status \in {"working", "failed", "finished"}
          /\ tool \in {"none", "running", "interrupted"}
\* Draining a failed execution's stream must not report a finished thread.
\* Mutators: tui/app.rs:7948, :8302, :8337.
FailedOutcomePreserved == phase = 3 => status = "failed"
\* After stream completion a tool without a result is interrupted, never spinning or successful.
\* Mutators: tui/app.rs:7948, :8192, :8254.
SettledToolAgreement == phase = 3 => tool = "interrupted"
Spec == Init /\ [][Next]_vars
=============================================================================
