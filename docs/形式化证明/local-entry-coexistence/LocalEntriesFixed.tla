---------------------------- MODULE LocalEntriesFixed ----------------------------
EXTENDS Naturals, FiniteSets, TLC
Fixed == TRUE
Processes == {"desktop", "cli-a", "cli-b"}
Threads == {"thread-a", "thread-b"}
VARIABLES opened, rejected, owners, drains, stopped
vars == <<opened, rejected, owners, drains, stopped>>
Init == /\ opened = {} /\ rejected = {}
        /\ owners = [thread \in Threads |-> "none"]
        /\ drains = {} /\ stopped = {}
Open(process) ==
    \* crates/wunder-runtime/src/storage/thread_store.rs:200; root lock acquisition
    /\ process \notin opened \cup rejected
    /\ opened' = IF Fixed \/ (opened \ stopped) = {} THEN opened \cup {process} ELSE opened
    /\ rejected' = IF Fixed \/ (opened \ stopped) = {} THEN rejected ELSE rejected \cup {process}
    /\ UNCHANGED <<owners, drains, stopped>>
Claim(process, thread) ==
    \* crates/wunder-runtime/src/storage/thread_store.rs:1391; kernel thread lock
    /\ process \in opened \ stopped
    \* crates/wunder-runtime/src/storage/thread_store.rs:1405; owner lock is exclusive
    /\ owners[thread] = "none"
    /\ owners' = [owners EXCEPT ![thread] = process]
    /\ UNCHANGED <<opened, rejected, drains, stopped>>
DropHandle(process, thread) ==
    \* crates/wunder-runtime/src/storage/rollout.rs:333; worker drains after handle drop
    /\ owners[thread] = process
    /\ <<process, thread>> \notin drains
    /\ drains' = drains \cup {<<process, thread>>}
    /\ UNCHANGED <<opened, rejected, owners, stopped>>
FinishDrain(process, thread) ==
    \* crates/wunder-runtime/src/storage/rollout.rs:333; owner retained through worker exit
    /\ <<process, thread>> \in drains
    /\ drains' = drains \ {<<process, thread>>}
    /\ owners' = [owners EXCEPT ![thread] = "none"]
    /\ UNCHANGED <<opened, rejected, stopped>>
Exit(process) ==
    \* crates/wunder-runtime/src/storage/thread_store.rs:1405; kernel releases on process exit
    /\ process \in opened \ stopped
    /\ stopped' = stopped \cup {process}
    /\ owners' = [thread \in Threads |-> IF owners[thread] = process THEN "none" ELSE owners[thread]]
    /\ drains' = {drain \in drains: drain[1] # process}
    /\ UNCHANGED <<opened, rejected>>
Next == (\E process \in Processes: Open(process) \/ Exit(process))
     \/ (\E process \in Processes, thread \in Threads: Claim(process, thread) \/ DropHandle(process, thread) \/ FinishDrain(process, thread))
Spec == Init /\ [][Next]_vars
\* Opening one local entry must not reject another solely because its root is in use.
IndependentEntries == rejected = {}
\* An asynchronous drain keeps the thread owner until all writes finish.
DrainKeepsOwnership == \A drain \in drains: owners[drain[2]] = drain[1]
TypeOK == /\ opened \subseteq Processes /\ rejected \subseteq Processes
          /\ stopped \subseteq opened
          /\ owners \in [Threads -> Processes \cup {"none"}]
          /\ drains \subseteq (Processes \X Threads)
=============================================================================
