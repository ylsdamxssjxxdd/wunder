---------------------------- MODULE CheckpointFixed ----------------------------
EXTENDS Naturals, TLC
Fixed == TRUE
VARIABLES status, phase, gate, durable, applied, late, restarted
vars == <<status, phase, gate, durable, applied, late, restarted>>
Init == /\ status = "running" /\ phase = "generating" /\ gate = "none"
        /\ durable = FALSE /\ applied = FALSE /\ late = FALSE /\ restarted = FALSE
SummaryReady ==
    \* orchestrator/memory.rs:438; summary generation finishes before commit
    /\ phase = "generating"
    /\ phase' = "ready"
    /\ UNCHANGED <<status, gate, durable, applied, late, restarted>>
AcquireCheckpoint ==
    \* orchestrator/thread_log_committer.rs:478; per-thread committer gate
    /\ phase = "ready" /\ gate = "none"
    /\ gate' = IF Fixed THEN "checkpoint" ELSE "none"
    /\ phase' = "checking"
    /\ UNCHANGED <<status, durable, applied, late, restarted>>
CheckRunning ==
    \* orchestrator/thread_log_committer.rs:489; running check under gate
    /\ phase = "checking"
    \* orchestrator/thread_log_committer.rs:482-490; reject a settled origin
    /\ (~Fixed \/ status = "running")
    /\ phase' = "writing"
    /\ UNCHANGED <<status, gate, durable, applied, late, restarted>>
RejectCheckpoint ==
    \* orchestrator/thread_log_committer.rs:496; rejected commit never applies replacement
    /\ phase = "checking" /\ Fixed /\ status # "running"
    /\ phase' = "failed" /\ gate' = "none"
    /\ UNCHANGED <<status, durable, applied, late, restarted>>
PersistCheckpoint ==
    \* storage/thread_store.rs:2286; durable append precedes replacement
    /\ phase = "writing"
    /\ durable' = TRUE /\ late' = (status # "running") /\ phase' = "committed"
    /\ UNCHANGED <<status, gate, applied, restarted>>
WriteFails ==
    \* orchestrator/microcompaction.rs:238; failed commit returns None
    /\ phase = "writing"
    /\ phase' = "failed" /\ gate' = "none"
    /\ UNCHANGED <<status, durable, applied, late, restarted>>
ReleaseCheckpoint ==
    \* orchestrator/thread_log_committer.rs:502; function exits after append
    /\ phase = "committed"
    /\ phase' = "returned" /\ gate' = "none"
    /\ UNCHANGED <<status, durable, applied, late, restarted>>
ApplyReplacement ==
    \* orchestrator/microcompaction.rs:255; replacement returned only after commit
    /\ phase = "returned"
    /\ applied' = TRUE /\ phase' = "applied"
    /\ UNCHANGED <<status, gate, durable, late, restarted>>
Stop ==
    \* orchestrator/thread_log_committer.rs:221; terminal update uses the same gate
    /\ status = "running"
    \* orchestrator/thread_log_committer.rs:225; terminal writes acquire the gate
    /\ (~Fixed \/ gate = "none")
    /\ status' = "cancelled"
    /\ UNCHANGED <<phase, gate, durable, applied, late, restarted>>
CrashAndReopen ==
    \* storage/thread_store.rs:916; replay derives model context from durable checkpoint
    /\ ~restarted
    /\ restarted' = TRUE /\ applied' = durable /\ gate' = "none" /\ phase' = "reopened"
    /\ UNCHANGED <<status, durable, late>>
Next == SummaryReady \/ AcquireCheckpoint \/ CheckRunning \/ RejectCheckpoint
        \/ PersistCheckpoint \/ WriteFails \/ ReleaseCheckpoint \/ ApplyReplacement
        \/ Stop \/ CrashAndReopen
Spec == Init /\ [][Next]_vars
\* A checkpoint must not commit after the originating turn has settled.
NoLateCheckpoint == ~late
\* Neither live replacement nor recovery may use an uncommitted snapshot.
DurableBeforeApply == applied => durable
TypeOK == /\ status \in {"running", "cancelled"}
          /\ gate \in {"none", "checkpoint"}
=============================================================================
