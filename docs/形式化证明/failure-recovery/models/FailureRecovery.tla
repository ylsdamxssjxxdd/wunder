-------------------------- MODULE FailureRecovery --------------------------
EXTENDS Naturals, TLC
CONSTANT Fixed
VARIABLES scenario, phase, attempt, requests, cancelled, lockHeld,
          permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult
vars == <<scenario, phase, attempt, requests, cancelled, lockHeld,
          permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
Init == /\ scenario \in {"auth", "transient", "truncated", "tool"}
        /\ phase = IF scenario = "tool" THEN "tool_ready" ELSE "ready"
        /\ attempt = 0 /\ requests = 0 /\ cancelled = FALSE /\ lockHeld = TRUE
        /\ permanentRetried = FALSE /\ bypassedBackoff = FALSE
        /\ incompleteAccepted = FALSE /\ toolRuns = 0 /\ toolResult = FALSE
DispatchTool ==
    \* crates/wunder-runtime/src/orchestrator/execute_tools.rs:690
    /\ phase = "tool_ready"
    /\ phase' = "tool_finished" /\ toolRuns' = toolRuns + 1
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolResult>>
ObserveToolFailure ==
    \* crates/wunder-runtime/src/orchestrator/execute.rs:1570
    /\ phase = "tool_finished"
    /\ phase' = "ready" /\ toolResult' = TRUE
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns>>
BeginCall ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:1055
    /\ phase = "ready" /\ ~cancelled
    /\ phase' = "calling" /\ attempt' = attempt + 1 /\ requests' = requests + 1
    /\ UNCHANGED <<scenario, cancelled, lockHeld, permanentRetried,
                   bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
ProviderFailure ==
    \* crates/wunder-runtime/src/services/llm.rs:1108
    /\ phase = "calling" /\ ~cancelled
    /\ phase' = CASE scenario = "auth" -> "rejected"
                    [] scenario = "truncated" -> "eof"
                    [] OTHER -> "unavailable"
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
ProviderSuccess ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:1122
    /\ phase = "calling" /\ ~cancelled /\ scenario # "auth"
    /\ phase' = "completed"
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
LegacyFallback ==
    \* crates/wunder-runtime/src/services/llm.rs:1454 (pre-fix transient HTTP fallback)
    /\ phase = "unavailable" /\ ~cancelled /\ ~Fixed
    /\ phase' = "fallback_failed" /\ requests' = requests + 1 /\ bypassedBackoff' = TRUE
    /\ UNCHANGED <<scenario, attempt, cancelled, lockHeld,
                   permanentRetried, incompleteAccepted, toolRuns, toolResult>>
ClassifyFailure ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:1241; crates/wunder-runtime/src/services/llm.rs:1193
    /\ phase \in {"rejected", "eof", "fallback_failed", "unavailable"} /\ ~cancelled
    /\ (Fixed \/ phase # "unavailable")
    /\ phase' = IF phase = "eof" /\ ~Fixed THEN "completed"
                 ELSE IF phase = "rejected" /\ Fixed THEN "failed"
                 ELSE IF attempt < (IF scenario = "auth" THEN 2 ELSE 6)
                      THEN "backoff" ELSE "failed"
    /\ permanentRetried' = (permanentRetried \/ (phase = "rejected" /\ ~Fixed /\ attempt < 2))
    /\ incompleteAccepted' = (incompleteAccepted \/ (phase = "eof" /\ ~Fixed))
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   bypassedBackoff, toolRuns, toolResult>>
BackoffElapsed ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:397; crates/wunder-runtime/src/orchestrator/llm.rs:1055
    /\ phase = "backoff" /\ ~cancelled
    /\ phase' = "ready"
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
Cancel ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:404; crates/wunder-runtime/src/orchestrator/llm.rs:426
    /\ phase \in {"calling", "backoff"} /\ ~cancelled
    /\ cancelled' = TRUE
    /\ UNCHANGED <<scenario, phase, attempt, requests, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
CancelObserved ==
    \* crates/wunder-runtime/src/orchestrator/llm.rs:404; crates/wunder-runtime/src/orchestrator/execute_tools.rs:62
    /\ cancelled /\ phase \notin {"cancelled", "released"}
    /\ phase' = "cancelled"
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, lockHeld,
                   permanentRetried, bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
ReleaseResources ==
    \* crates/wunder-runtime/src/orchestrator/execute.rs:2286; crates/wunder-runtime/src/orchestrator/execute_tools.rs:26
    /\ phase \in {"completed", "failed", "cancelled"}
    /\ phase' = "released" /\ lockHeld' = FALSE
    /\ UNCHANGED <<scenario, attempt, requests, cancelled, permanentRetried,
                   bypassedBackoff, incompleteAccepted, toolRuns, toolResult>>
Next == DispatchTool \/ ObserveToolFailure \/ BeginCall \/ ProviderFailure \/ ProviderSuccess
        \/ LegacyFallback \/ ClassifyFailure \/ BackoffElapsed \/ Cancel \/ CancelObserved \/ ReleaseResources
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)
EventuallyReleased == <>(phase = "released")
TypeOK == /\ scenario \in {"auth", "transient", "truncated", "tool"}
          /\ phase \in {"tool_ready", "tool_finished", "ready", "calling", "rejected", "eof",
                        "unavailable", "fallback_failed", "backoff", "completed", "failed", "cancelled", "released"}
          /\ attempt \in 0..6 /\ requests \in 0..12 /\ toolRuns \in 0..1
          /\ cancelled \in BOOLEAN /\ lockHeld \in BOOLEAN /\ toolResult \in BOOLEAN
          /\ permanentRetried \in BOOLEAN /\ bypassedBackoff \in BOOLEAN /\ incompleteAccepted \in BOOLEAN
\* A permanent rejection never schedules another identical request (llm.rs:1302).
NoPermanentRetry == ~permanentRetried
\* Transient HTTP failures cannot send another request before engine backoff (services/llm.rs:1454).
NoHiddenTransientFallback == ~bypassedBackoff
\* EOF without a terminal marker or finish reason cannot be accepted (services/llm.rs:1193).
NoIncompleteSuccess == ~incompleteAccepted
\* Retrying the followup model request does not repeat the observed tool (execute.rs:1570).
ToolNotReexecuted == /\ toolRuns <= 1 /\ (toolResult => toolRuns = 1)
\* A settled execution has released its own lock (execute_tools.rs:26).
SettledResourcesReleased == phase = "released" => ~lockHeld
\* An observed cancellation cannot restart a model call (llm.rs:404,426,1055).
CancelledCannotRestart == cancelled => phase \notin {"ready", "completed"}
=============================================================================
