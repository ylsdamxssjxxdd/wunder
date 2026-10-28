------------------------------ MODULE Inquiry ------------------------------
EXTENDS Naturals, FiniteSets, TLC
Fixed == FALSE
Clients == {"a", "b"}
VARIABLES status, answers, claim, pending, attempted, replayed, online, crashed, late
vars == <<status, answers, claim, pending, attempted, replayed, online, crashed, late>>
Init == /\ status = "waiting" /\ answers = {} /\ claim = "none"
        /\ pending = {} /\ attempted = {} /\ replayed = {}
        /\ online = TRUE /\ crashed = FALSE /\ late = FALSE
Submit(client) ==
    \* services/runtime/thread/runtime.rs:232; orchestrator/request.rs:121
    /\ online
    \* storage/thread_admission.rs:625; two distinct client_message_id values
    /\ client \notin attempted
    \* storage/thread_admission.rs:625-651; claim lookup precedes waiting check
    /\ (~Fixed \/ claim # "none" \/ status = "waiting")
    /\ answers' = IF Fixed /\ claim # "none" THEN answers ELSE answers \cup {client}
    /\ claim' = IF Fixed /\ claim = "none" THEN client ELSE claim
    /\ pending' = pending \cup {IF Fixed /\ claim # "none" THEN claim ELSE client}
    /\ attempted' = attempted \cup {client}
    /\ late' = (late \/ (status = "cancelled" /\ ~(Fixed /\ claim # "none")))
    /\ UNCHANGED <<status, replayed, online, crashed>>
Replay(client) ==
    \* storage/thread_admission.rs:614-642; committed identities survive restart
    /\ online /\ client \in answers /\ client \notin replayed
    /\ pending' = IF Fixed THEN pending \cup {client} ELSE pending
    /\ replayed' = replayed \cup {client}
    /\ UNCHANGED <<status, answers, claim, attempted, online, crashed, late>>
Settle(client) ==
    \* orchestrator/thread_log_committer.rs:218; gate covers read through control update
    /\ online /\ client \in pending
    \* storage/thread_admission.rs:81; old terminal transition rejects cancelled
    /\ status = "waiting"
    \* storage/thread_admission.rs:81-87; persistent answer identity must match
    /\ (~Fixed \/ claim = client)
    /\ status' = "completed" /\ pending' = pending \ {client}
    /\ UNCHANGED <<answers, claim, attempted, replayed, online, crashed, late>>
Stop ==
    \* orchestrator/thread_log_committer.rs:318; cancel_active_turns waiting_input
    /\ online /\ status = "waiting"
    /\ status' = "cancelled"
    /\ UNCHANGED <<answers, claim, pending, attempted, replayed, online, crashed, late>>
Crash ==
    \* orchestrator/thread_log_committer.rs:91; durable admission precedes settlement
    /\ online /\ ~crashed
    /\ online' = FALSE /\ crashed' = TRUE /\ pending' = {}
    /\ UNCHANGED <<status, answers, claim, attempted, replayed, late>>
Restart ==
    \* storage/thread_admission.rs:625; fresh connection reads durable claim
    /\ ~online
    /\ online' = TRUE
    /\ UNCHANGED <<status, answers, claim, pending, attempted, replayed, crashed, late>>
Next == (\E client \in Clients: Submit(client) \/ Replay(client) \/ Settle(client))
        \/ Stop \/ Crash \/ Restart
Spec == Init /\ [][Next]_vars
\* One inquiry must allocate at most one answer, even with different client keys.
AtMostOneAnswer == Cardinality(answers) <= 1
\* Stopping before any answer was accepted must reject a later answer.
StopWins == ~late
TypeOK == /\ status \in {"waiting", "completed", "cancelled"}
          /\ answers \subseteq Clients /\ pending \subseteq Clients
          /\ claim \in Clients \cup {"none"}
=============================================================================
