---------------------------- MODULE NativeCatalogueStatusFixed ----------------------------
EXTENDS Integers
VARIABLES phase, local, catalogue, shown
vars == <<phase, local, catalogue, shown>>
Init ==
    /\ phase = 0
    /\ local = "done"
    /\ catalogue = "done"
    /\ shown = FALSE
Send ==
    /\ phase = 0                         \* frontend-slint/src/native_chat.rs:2234; record the local status before worker I/O
    /\ phase' = 1
    /\ local' = "hot"
    /\ UNCHANGED <<catalogue, shown>>
OldCatalogueDone ==
    /\ phase = 1                         \* frontend-slint/src/native_chat_status.rs:37, :41, :46; local ownership or newer revision fences the read
    /\ phase' = 2
    /\ UNCHANGED <<local, catalogue, shown>>
RealTerminal ==
    /\ phase = 1                         \* frontend-slint/src/native_chat.rs:2906; direct execution status reports completion
    /\ phase' = 3
    /\ local' = "done"
    /\ shown' = TRUE
    /\ UNCHANGED catalogue
BackgroundHot ==
    /\ phase = 0
    /\ phase' = 4
    /\ catalogue' = "hot"
    /\ local' = "hot"
    /\ UNCHANGED shown
BackgroundDone ==
    /\ phase = 4                         \* frontend-slint/src/native_chat_status.rs:52; an accepted catalogue transition may notify
    /\ phase' = 5
    /\ catalogue' = "done"
    /\ local' = "done"
    /\ shown' = TRUE
Next == Send \/ OldCatalogueDone \/ RealTerminal \/ BackgroundHot \/ BackgroundDone
TypeOK == /\ phase \in 0..5 /\ local \in {"hot", "done"} /\ catalogue \in {"hot", "done"} /\ shown \in BOOLEAN
\* A previous catalogue snapshot cannot finish a locally submitted turn or show its notice.
\* Mutators: frontend-slint/src/native_chat_status.rs:37, :41; frontend-slint/src/native_chat.rs:1533.
StaleCatalogueCannotComplete == phase = 2 => /\ local = "hot" /\ ~shown
Spec == Init /\ [][Next]_vars
=============================================================================
