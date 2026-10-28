------------------------------ MODULE ToolTimeout ------------------------------
EXTENDS Naturals, TLC
CONSTANT Fixed
VARIABLES phase, sideEffects, retryable, unknown, verified
vars == <<phase, sideEffects, retryable, unknown, verified>>
Init == /\ phase = "running" /\ sideEffects = 0 /\ retryable = FALSE
        /\ unknown = FALSE /\ verified = FALSE
SideEffect ==
    \* crates/wunder-runtime/src/services/tools/command_tool.rs:974 (external child runs before wait resolves)
    /\ phase = "running" /\ sideEffects = 0
    /\ sideEffects' = 1
    /\ UNCHANGED <<phase, retryable, unknown, verified>>
Timeout ==
    \* crates/wunder-runtime/src/services/tools/command_tool.rs:2172; crates/wunder-runtime/src/services/tools/tool_error.rs:52
    /\ phase = "running"
    /\ phase' = "observation" /\ unknown' = TRUE /\ retryable' = ~Fixed
    /\ UNCHANGED <<sideEffects, verified>>
ConsumeRetryAdvice ==
    \* crates/wunder-runtime/src/orchestrator/execute.rs:1570 (model consumes the observation; following advice is environmental)
    /\ phase = "observation" /\ retryable /\ ~verified
    /\ phase' = "repeated" /\ sideEffects' = sideEffects + 1
    /\ UNCHANGED <<retryable, unknown, verified>>
VerifyOutcome ==
    \* crates/wunder-runtime/src/services/tools/tool_error.rs:52 (hint requires external outcome verification)
    /\ phase = "observation" /\ ~retryable
    /\ phase' = "verified" /\ verified' = TRUE
    /\ UNCHANGED <<sideEffects, retryable, unknown>>
Next == SideEffect \/ Timeout \/ ConsumeRetryAdvice \/ VerifyOutcome
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in {"running", "observation", "repeated", "verified"}
          /\ sideEffects \in 0..2 /\ retryable \in BOOLEAN /\ unknown \in BOOLEAN /\ verified \in BOOLEAN
\* A timed out, unverified side effect must not be advertised as safe to retry.
UnknownNeverAdvertisedRetryable == unknown /\ ~verified => ~retryable
\* A client following the advice must not repeat a completed side effect without verification.
NoUnverifiedDuplicate == ~verified => sideEffects <= 1
=============================================================================
