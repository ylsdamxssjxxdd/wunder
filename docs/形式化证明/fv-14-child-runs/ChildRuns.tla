---------------------------- MODULE ChildRuns ----------------------------
EXTENDS Naturals, TLC
Fixed == FALSE
VARIABLES ancestorCancelled, worker, escaped, generation, guard, lateRelease, reused, stopped, dispatched
vars == <<ancestorCancelled, worker, escaped, generation, guard, lateRelease, reused, stopped, dispatched>>
Init == /\ ancestorCancelled = FALSE /\ worker = "none" /\ escaped = FALSE
        /\ generation = 0 /\ guard = 0 /\ lateRelease = FALSE
        /\ reused = FALSE /\ stopped = FALSE /\ dispatched = FALSE
CancelAncestor ==
    \* ops/monitor.rs:1331; sessions lock then child_runs cancel_tree lock
    /\ ~stopped
    /\ stopped' = TRUE /\ ancestorCancelled' = TRUE
    /\ worker' = IF worker = "running" THEN "cancelled" ELSE worker
    /\ UNCHANGED <<escaped, generation, guard, lateRelease, reused, dispatched>>
DispatchFromSibling ==
    \* services/runtime/thread/child_runs.rs:43; sibling caller is not cancelled
    /\ ~dispatched
    \* services/runtime/thread/child_runs.rs:90; durable ancestor checked under registry lock
    /\ (~Fixed \/ ~ancestorCancelled)
    /\ dispatched' = TRUE /\ worker' = "running"
    /\ generation' = 1 /\ guard' = 1 /\ escaped' = ancestorCancelled
    /\ UNCHANGED <<ancestorCancelled, lateRelease, reused, stopped>>
FinishWorker ==
    \* services/runtime/thread/child_runs.rs:146; settle retains identity and ancestry
    /\ worker = "running"
    /\ worker' = "completed"
    /\ UNCHANGED <<ancestorCancelled, escaped, generation, guard, lateRelease, reused, stopped, dispatched>>
ReuseWorker ==
    \* services/runtime/thread/child_runs.rs:101; replacement cancels prior token
    /\ dispatched /\ ~reused /\ ~ancestorCancelled
    \* services/runtime/thread/child_runs.rs:45; active execution must settle first
    /\ worker = "completed"
    /\ reused' = TRUE /\ generation' = 2 /\ worker' = "running"
    /\ UNCHANGED <<ancestorCancelled, escaped, guard, lateRelease, stopped, dispatched>>
DropOldGuard ==
    \* services/runtime/thread/child_runs.rs:157; Arc identity check protects replacement
    /\ reused /\ ~lateRelease
    /\ lateRelease' = TRUE
    /\ worker' = IF guard = generation THEN "none" ELSE worker
    /\ UNCHANGED <<ancestorCancelled, escaped, generation, guard, reused, stopped, dispatched>>
Next == CancelAncestor \/ DispatchFromSibling \/ FinishWorker \/ ReuseWorker \/ DropOldGuard
Spec == Init /\ [][Next]_vars
\* A cancelled durable ancestor forbids sibling dispatch into its scope.
NoEscapedDescendant == ~escaped
\* A guard from the old generation cannot delete the replacement run.
LateGuardPreservesReplacement == (reused /\ lateRelease) => worker # "none"
TypeOK == /\ worker \in {"none", "running", "completed", "cancelled"}
          /\ generation \in 0..2 /\ guard \in 0..1
=============================================================================
