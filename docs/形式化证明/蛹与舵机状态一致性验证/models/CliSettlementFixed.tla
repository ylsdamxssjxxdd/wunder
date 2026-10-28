---------------------------- MODULE CliSettlementFixed ----------------------------
EXTENDS Integers
VARIABLES phase, status, tool, terminal
vars == <<phase, status, tool, terminal>>
Init ==
    /\ phase = 0                 \* crates/wunder-cli/tui/app.rs:6967
    /\ status = "working"        \* crates/wunder-cli/tui/app.rs:6969
    /\ tool = "none"             \* crates/wunder-cli/tui/app.rs:6977
    /\ terminal = "none"         \* crates/wunder-cli/tui/thread_registry.rs:329
ToolCall ==
    /\ phase = 0                 \* crates/wunder-cli/tui/app.rs:8254
    /\ phase' = 1
    /\ tool' = "running"         \* crates/wunder-cli/tui/app.rs:8287
    /\ UNCHANGED <<status, terminal>>
FailedTerminal ==
    /\ phase = 1                 \* crates/wunder-cli/tui/app.rs:8135
    /\ phase' = 2
    /\ status' = "failed"        \* crates/wunder-cli/tui/thread_registry.rs:338; tui/app.rs:1967 takes precedence over a live subscription
    /\ terminal' = "failed"      \* crates/wunder-cli/tui/thread_registry.rs:337
    /\ UNCHANGED tool
StreamDone ==
    /\ phase = 2                 \* crates/wunder-cli/tui/app.rs:7972
    /\ phase' = 3
    /\ status' = terminal        \* crates/wunder-cli/tui/app.rs:7977
    /\ tool' = "interrupted"     \* crates/wunder-cli/tui/app.rs:7989; interrupt_pending_tools
    /\ UNCHANGED terminal
System == StreamDone
Next == ToolCall \/ FailedTerminal \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..3 /\ status \in {"working", "failed", "finished"}
          /\ tool \in {"none", "running", "interrupted"}
          /\ terminal \in {"none", "failed"}
\* Draining a failed execution's stream must not report a finished thread.
\* Mutators: tui/app.rs:1967, :7972, :8135; thread_registry.rs:335.
FailedOutcomePreserved == phase = 3 => status = "failed"
\* After stream completion a tool without a result is interrupted, never spinning or successful.
\* Mutators: tui/app.rs:7989, :8032.
SettledToolAgreement == phase = 3 => tool = "interrupted"
Spec == Init /\ [][Next]_vars
=============================================================================
