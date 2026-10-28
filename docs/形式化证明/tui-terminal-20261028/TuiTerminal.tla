----------------------------- MODULE TuiTerminal -----------------------------
EXTENDS Naturals, TLC
CONSTANT Fixed
VARIABLES epoch, phase, terminal_status, reported_error, empty_output_error
vars == <<epoch, phase, terminal_status, reported_error, empty_output_error>>
Init == /\ epoch = 1 /\ phase = "streaming" /\ terminal_status = "none"
        /\ reported_error = FALSE /\ empty_output_error = FALSE
ReceiveError ==
    \* crates/wunder-cli/tui/app.rs:8448
    /\ phase = "streaming"
    /\ phase' = "error"
    /\ terminal_status' = "failed"
    /\ reported_error' = TRUE
    /\ UNCHANGED <<epoch, empty_output_error>>
RecordCancelled ==
    \* crates/wunder-cli/tui/app.rs:8221; crates/wunder-cli/tui/thread_registry.rs:336
    /\ phase = "error"
    /\ phase' = "terminal"
    /\ terminal_status' = IF Fixed /\ terminal_status = "failed" THEN "failed" ELSE "finished"
    /\ UNCHANGED <<epoch, reported_error, empty_output_error>>
RecordEmptyFinished ==
    \* crates/wunder-cli/tui/app.rs:8221; crates/wunder-cli/tui/thread_registry.rs:334
    /\ phase = "streaming"
    /\ phase' = "terminal"
    /\ terminal_status' = "finished"
    /\ UNCHANGED <<epoch, reported_error, empty_output_error>>
Done ==
    \* crates/wunder-cli/tui/app.rs:8074
    /\ phase = "terminal"
    /\ phase' = "done"
    /\ empty_output_error' = (terminal_status # "failed")
    /\ UNCHANGED <<epoch, terminal_status, reported_error>>
InsertStream ==
    \* crates/wunder-cli/tui/thread_registry.rs:350
    /\ phase = "done" /\ epoch = 1
    /\ epoch' = 2 /\ phase' = "streaming" /\ terminal_status' = "none"
    /\ reported_error' = FALSE /\ empty_output_error' = FALSE
Next == ReceiveError \/ RecordCancelled \/ RecordEmptyFinished \/ Done \/ InsertStream
Spec == Init /\ [][Next]_vars
\* A reported request error must not be followed by a synthetic empty-output error.
NoDuplicateEmptyError == ~(reported_error /\ empty_output_error)
\* The next stream must not inherit the previous stream's error status.
FreshStreamReset == phase = "streaming" =>
    /\ terminal_status = "none" /\ ~reported_error /\ ~empty_output_error
TypeOK == /\ epoch \in 1..2
          /\ phase \in {"streaming", "error", "terminal", "done"}
          /\ terminal_status \in {"none", "failed", "finished"}
          /\ reported_error \in BOOLEAN /\ empty_output_error \in BOOLEAN
=============================================================================
