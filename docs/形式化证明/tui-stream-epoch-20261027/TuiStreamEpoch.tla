-------------------------- MODULE TuiStreamEpoch --------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANT Fixed
Turns == 1..2
Ids == 1..3
VARIABLES epoch, applied, output, expected
vars == <<epoch, applied, output, expected>>
Init == /\ epoch = 1 /\ applied = {} /\ output = {} /\ expected = {}
StartNextTurn ==
    \* crates/wunder-cli/tui/thread_registry.rs:346
    /\ epoch = 1
    /\ epoch' = 2
    /\ applied' = IF Fixed THEN {} ELSE applied
    /\ UNCHANGED <<output, expected>>
Receive(id) ==
    \* crates/wunder-cli/tui/app.rs:8159; crates/wunder-cli/tui/thread_registry.rs:235
    /\ id \in Ids
    /\ expected' = expected \cup {<<epoch, id>>}
    /\ output' = IF id \in applied THEN output ELSE output \cup {<<epoch, id>>}
    /\ applied' = applied \cup {id}
    /\ UNCHANGED epoch
Next == StartNextTurn \/ (\E id \in Ids: Receive(id))
Spec == Init /\ [][Next]_vars
\* Every transport event that arrives in its stream is rendered once.
EveryArrivedEventRenders == output = expected
TypeOK == /\ epoch \in Turns /\ applied \subseteq Ids
          /\ output \subseteq (Turns \X Ids) /\ expected \subseteq (Turns \X Ids)
=============================================================================
