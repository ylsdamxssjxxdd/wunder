----------------------------- MODULE PostgresIo -----------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fixed
VARIABLES started, workerWaiters, blockingWaiters, ready, completed, ioIndependent
vars == <<started, workerWaiters, blockingWaiters, ready, completed, ioIndependent>>
Requests == 1..4
Init == /\ started = {} /\ workerWaiters = {} /\ blockingWaiters = {}
        /\ ready = {} /\ completed = {} /\ ioIndependent = Fixed
Begin(request) ==
    \* crates/wunder-runtime/src/storage/postgres.rs:491
    /\ request \notin started
    \* crates/wunder-server/src/main.rs:36; finite abstraction: two caller workers.
    /\ Cardinality(workerWaiters) < 2
    /\ started' = started \cup {request}
    /\ workerWaiters' = workerWaiters \cup {request}
    /\ UNCHANGED <<blockingWaiters, ready, completed, ioIndependent>>
Handoff(request) ==
    \* crates/wunder-runtime/src/storage/postgres.rs:288
    /\ request \in workerWaiters
    \* crates/wunder-server/src/main.rs:37; finite abstraction: two replacement threads.
    /\ Cardinality(blockingWaiters) < 2
    /\ workerWaiters' = workerWaiters \ {request}
    /\ blockingWaiters' = blockingWaiters \cup {request}
    /\ UNCHANGED <<started, ready, completed, ioIndependent>>
Poll(request) ==
    \* crates/wunder-runtime/src/storage/postgres.rs:491
    /\ request \in (workerWaiters \cup blockingWaiters) \ ready
    \* crates/wunder-runtime/src/storage/postgres.rs:108; crates/wunder-runtime/src/storage/postgres.rs:286
    /\ ioIndependent \/ Cardinality(workerWaiters) < 2
    /\ ready' = ready \cup {request}
    /\ UNCHANGED <<started, workerWaiters, blockingWaiters, completed, ioIndependent>>
FinishWorker(request) ==
    \* crates/wunder-runtime/src/storage/postgres.rs:288; crates/wunder-runtime/src/storage/postgres.rs:491
    /\ request \in ready \cap workerWaiters
    /\ workerWaiters' = workerWaiters \ {request}
    /\ ready' = ready \ {request}
    /\ completed' = completed \cup {request}
    /\ UNCHANGED <<started, blockingWaiters, ioIndependent>>
FinishBlocking(request) ==
    \* crates/wunder-runtime/src/storage/postgres.rs:288; crates/wunder-runtime/src/storage/postgres.rs:491
    /\ request \in ready \cap blockingWaiters
    /\ blockingWaiters' = blockingWaiters \ {request}
    /\ ready' = ready \ {request}
    /\ completed' = completed \cup {request}
    /\ UNCHANGED <<started, workerWaiters, ioIndependent>>
System == \E request \in Requests: Begin(request) \/ Handoff(request)
          \/ Poll(request) \/ FinishWorker(request) \/ FinishBlocking(request)
Spec == Init /\ [][System]_vars
\* An unfinished database request must retain an enabled internal progress step.
NoIoStarvation == completed = Requests \/ ENABLED System
\* A response cannot complete a request twice or complete a request never started.
ExactCompletion == /\ completed \subseteq started
                   /\ completed \cap (workerWaiters \cup blockingWaiters) = {}
                   /\ started = completed \cup workerWaiters \cup blockingWaiters
=============================================================================
