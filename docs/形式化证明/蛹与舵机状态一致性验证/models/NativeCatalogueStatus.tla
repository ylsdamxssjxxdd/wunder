---------------------------- MODULE NativeCatalogueStatus ----------------------------
EXTENDS Integers
VARIABLES phase, local, catalogue, shown
vars == <<phase, local, catalogue, shown>>
Init ==
    /\ phase = 0
    /\ local = "done"
    /\ catalogue = "done"
    /\ shown = FALSE
Send ==
    /\ phase = 0
    /\ phase' = 1
    /\ local' = "hot"
    /\ UNCHANGED <<catalogue, shown>>
OldCatalogueDone ==
    /\ phase = 1
    /\ phase' = 2
    /\ catalogue' = "done"
    /\ local' = "done"
    /\ shown' = TRUE
RealTerminal ==
    /\ phase = 1
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
    /\ phase = 4
    /\ phase' = 5
    /\ catalogue' = "done"
    /\ local' = "done"
    /\ shown' = TRUE
Next == Send \/ OldCatalogueDone \/ RealTerminal \/ BackgroundHot \/ BackgroundDone
TypeOK == /\ phase \in 0..5 /\ local \in {"hot", "done"} /\ catalogue \in {"hot", "done"} /\ shown \in BOOLEAN
\* A previous catalogue snapshot cannot finish a locally submitted turn or show its notice.
\* Old behaviour: e5a90a38 frontend-slint/src/native_chat.rs:1476 observed every returned row.
StaleCatalogueCannotComplete == phase = 2 => /\ local = "hot" /\ ~shown
Spec == Init /\ [][Next]_vars
=============================================================================
