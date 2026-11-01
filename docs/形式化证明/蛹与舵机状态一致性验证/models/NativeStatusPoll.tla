--------------------------- MODULE NativeStatusPoll ---------------------------
\* Bounded model of the sidebar's thread-status projection across a switch.  The
\* reader watches one thread at a time; a background thread may finish while
\* another is selected.  A row glyph and its completion notice are correct only
\* when the durable phase of the threads the reader is NOT looking at reaches
\* their rows.
\*
\* This model now also carries the second defect.  The bounded pump alone is not
\* enough: the projection keeps a per-thread fence around any status this process
\* authored, and a stale local "running" mark for a thread the reader switched
\* away from keeps fencing the catalogue's terminal.  So the shipped pump-only
\* build (Pump = TRUE, Scoped = FALSE) still shows the bug, and only scoping the
\* fence (Scoped = TRUE) lets a background settle land.
\*
\* Pump = FALSE was the original build (no independent poll); Pump = TRUE is the
\* bounded pump (start_status_poll, frontend-slint/src/native_chat_observer.rs).
\* Scoped = FALSE is the sticky fence (native_chat_status.rs); Scoped = TRUE
\* fences only a locally authored terminal, or a catalogue read that is not
\* terminal.  The configurations checked here are the shipped pump-only state
\* (Pump = TRUE, Scoped = FALSE) against the scoped-fence fix (Scoped = TRUE).
\*
\* "A" is selected first; "B" runs in the background.
EXTENDS Naturals
CONSTANTS
    Pump,        \* TRUE = the bounded catalogue pump exists
    Scoped       \* TRUE = the projection-fence fix is applied

Threads == {"A", "B"}

VARIABLES
    dA, dB,          \* durable[t] \in {"running","terminal"} -- runtime truth
    pA, pB,          \* projection in the row/notice \in {"live","settled"}
    active,          \* the selected thread
    observable,      \* window owns the sidebar: visible AND local, cloud inactive
    fA, fB           \* a locally authored hot mark still fences this thread's row

vars == <<dA, dB, pA, pB, active, observable, fA, fB>>

Mapped(r) == IF r = "running" THEN "live" ELSE "settled"

Init ==
    /\ dA = "terminal"      \* A idle at start
    /\ dB = "running"       \* B already running in the background
    /\ pA = "settled"
    /\ pB = "live"          \* B's row was last seen running
    /\ active = "A"
    /\ observable = TRUE
    /\ fA = FALSE
    /\ fB = TRUE            \* B was submitted locally, so a hot mark fences it

\* ----- the reader -----

SendA ==   \* reader posts to A; A becomes running at once
    \* frontend-slint/src/native_chat.rs:3015 -- set_session_status writes the local phase on submit
    /\ dA' = "running"
    /\ pA' = "live"
    /\ fA' = TRUE
    /\ UNCHANGED <<dB, pB, active, observable, fB>>

SendB ==
    \* frontend-slint/src/native_chat.rs:3015 -- set_session_status writes the local phase on submit
    /\ dB' = "running"
    /\ pB' = "live"
    /\ fB' = TRUE
    /\ UNCHANGED <<dA, pA, active, observable, fA>>

SwitchToA ==   \* selecting a thread attaches its watcher, which writes its row and clears its fence
    \* frontend-slint/src/native_chat_observer.rs:357 -- a watch attaches to the selected thread
    /\ active' = "A"
    /\ pA' = Mapped(dA)
    /\ fA' = FALSE
    /\ UNCHANGED <<dA, dB, pB, observable, fB>>

SwitchToB ==
    \* frontend-slint/src/native_chat_observer.rs:357 -- a watch attaches to the selected thread
    /\ active' = "B"
    /\ pB' = Mapped(dB)
    /\ fB' = FALSE
    /\ UNCHANGED <<dA, dB, pA, observable, fA>>

Hide ==    \* window hidden, or the cloud surface takes the sidebar
    \* frontend-slint/src/native_chat_observer.rs:236 -- start_status_poll yields while !is_visible() || cloud_chat_active
    /\ observable' = FALSE
    /\ UNCHANGED <<dA, dB, pA, pB, active, fA, fB>>

Restore ==
    /\ observable' = TRUE
    /\ UNCHANGED <<dA, dB, pA, pB, active, fA, fB>>

\* ----- the runtime, without the reader -----

SettleA == \* A's run finishes on its own
    /\ dA = "running"
    /\ dA' = "terminal"
    /\ UNCHANGED <<dB, pA, pB, active, observable, fA, fB>>

SettleB == \* B's run finishes on its own, while A is selected
    /\ dB = "running"
    /\ dB' = "terminal"
    /\ UNCHANGED <<dA, pA, pB, active, observable, fA, fB>>

ObserveA == \* A's own watcher copies A's durable phase into A's row
    \* frontend-slint/src/native_chat_observer.rs:357 -- a watch attaches to the selected thread
    /\ active = "A"
    \* frontend-slint/src/native_chat_observer.rs:511 -- only a changed durable status is written back
    /\ pA /= Mapped(dA)
    /\ pA' = Mapped(dA)
    /\ UNCHANGED <<dA, dB, pB, active, observable, fA, fB>>

ObserveB == \* B's watcher only fires while B is the selected thread
    \* frontend-slint/src/native_chat_observer.rs:357 -- the watch follows the selection, not B
    /\ active = "B"
    \* frontend-slint/src/native_chat_observer.rs:511 -- a background thread raises no such write
    /\ pB /= Mapped(dB)
    /\ pB' = Mapped(dB)
    /\ UNCHANGED <<dA, dB, pA, active, observable, fA, fB>>

Poll == \* the bounded pump raises the catalogue poll for any thread
    \* frontend-slint/src/native_chat_observer.rs:229 -- start_status_poll raises the poll on its own cadence
    /\ Pump
    \* frontend-slint/src/native_chat_observer.rs:236 -- the pump yields while the window is hidden or the cloud owns the sidebar
    /\ observable
    /\ \/ (pA /= Mapped(dA) /\ ~(fA /\ ~Scoped) /\ pA' = Mapped(dA)
           /\ UNCHANGED <<dA, dB, pB, active, observable, fA, fB>>)
       \/ (pB /= Mapped(dB) /\ ~(fB /\ ~Scoped) /\ pB' = Mapped(dB)
           /\ UNCHANGED <<dA, dB, pA, active, observable, fA, fB>>)
    \* frontend-slint/src/native_chat_status.rs:94 -- a stale local "hot" mark fences the catalogue read
    \*   unless the fence is scoped (Scoped = TRUE) to a locally authored terminal.

Next ==
    \/ SendA \/ SendB
    \/ SwitchToA \/ SwitchToB
    \/ Hide \/ Restore
    \/ SettleA \/ SettleB
    \/ ObserveA \/ ObserveB
    \/ Poll

Spec == Init /\ [][Next]_vars

\* ----- what the reader relies on -----

\* Actions that happen without the reader; Quiescent is where nothing else can act.
System == SettleA \/ SettleB \/ ObserveA \/ ObserveB \/ Poll
Quiescent == ~ENABLED System

\* Once nothing else can act and the sidebar is owned, a thread that has settled
\* must not still read as running.
BackgroundSettledIsShown ==
    (Quiescent /\ observable) =>
        /\ (dA = "terminal" => pA = "settled")
        /\ (dB = "terminal" => pB = "settled")

TypeOK ==
    /\ dA \in {"running", "terminal"}
    /\ dB \in {"running", "terminal"}
    /\ pA \in {"live", "settled"}
    /\ pB \in {"live", "settled"}
    /\ active \in Threads
    /\ observable \in BOOLEAN
    /\ fA \in BOOLEAN
    /\ fB \in BOOLEAN
=============================================================================