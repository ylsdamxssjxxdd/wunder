--------------------------- MODULE NativeStatusPoll ---------------------------
\* Bounded model of the sidebar's thread-status projection across a switch.  The
\* reader watches one thread at a time; a background thread may finish while
\* another is selected.  The row glyph and the completion notice are only correct
\* when something observes the durable phase of the threads the reader is NOT
\* looking at.
\*
\* Pre-fix (Fixed = FALSE): the catalogue poll (native_thread_update_ready ->
\* refresh_chat, frontend-slint/src/native_chat.rs) is raised only by the selected
\* thread's own watcher, so a background thread that settles keeps its last row
\* value until the reader selects it again (attaching the watcher fires
\* set_session_status(latest.stats_status), native_chat_observer.rs).
\*
\* Fixed (Fixed = TRUE): an independent bounded pump (start_status_poll,
\* frontend-slint/src/native_chat_observer.rs) raises the catalogue poll on its own
\* 500 ms / 3 s cadence while the window owns the sidebar, so every thread's durable
\* phase reaches its row without a user action.
\*
\* "A" is selected first; "B" runs in the background.
EXTENDS Naturals
CONSTANT Fixed          \* FALSE = pre-fix (no independent poll); TRUE = bounded pump

Threads == {"A", "B"}

VARIABLES
    dA, dB,          \* durable[t] \in {"running","terminal"} -- runtime truth
    pA, pB,          \* projection in the row/notice \in {"live","settled"}
    active,          \* the selected thread
    observable       \* window owns the sidebar: visible AND local, cloud inactive

vars == <<dA, dB, pA, pB, active, observable>>

Mapped(r) == IF r = "running" THEN "live" ELSE "settled"

Init ==
    /\ dA = "terminal"      \* A idle at start
    /\ dB = "running"       \* B already running in the background
    /\ pA = "settled"
    /\ pB = "live"          \* B's row was last seen running
    /\ active = "A"
    /\ observable = TRUE

\* ----- the reader -----

SendA ==   \* reader posts to A; A becomes running at once
    \* native_chat.rs set_session_status writes the local phase on submit
    /\ dA' = "running"
    /\ pA' = "live"
    /\ UNCHANGED <<dB, pB, active, observable>>

SendB ==
    /\ dB' = "running"
    /\ pB' = "live"
    /\ UNCHANGED <<dA, pA, active, observable>>

SwitchToA ==   \* selecting a thread is what attaches its watcher
    /\ active' = "A"
    /\ UNCHANGED <<dA, dB, pA, pB, observable>>

SwitchToB ==
    /\ active' = "B"
    /\ UNCHANGED <<dA, dB, pA, pB, observable>>

Hide ==    \* window hidden, or the cloud surface takes the sidebar
    \* start_status_poll yields while !is_visible() || cloud_chat_active
    /\ observable' = FALSE
    /\ UNCHANGED <<dA, dB, pA, pB, active>>

Restore ==
    /\ observable' = TRUE
    /\ UNCHANGED <<dA, dB, pA, pB, active>>

\* ----- the runtime, without the reader -----

SettleA == \* A's run finishes on its own
    /\ dA = "running"
    /\ dA' = "terminal"
    /\ UNCHANGED <<dB, pA, pB, active, observable>>

SettleB == \* B's run finishes on its own, while A is selected
    /\ dB = "running"
    /\ dB' = "terminal"
    /\ UNCHANGED <<dA, pA, pB, active, observable>>

ObserveA == \* A's own watcher copies A's durable phase into A's row
    \* frontend-slint/src/native_chat_observer.rs:357 -- a watch attaches to the selected thread
    /\ active = "A"
    \* frontend-slint/src/native_chat_observer.rs:511 -- only a changed durable status is written back
    /\ pA /= Mapped(dA)
    /\ pA' = Mapped(dA)
    /\ UNCHANGED <<dA, dB, pB, active, observable>>

ObserveB == \* B's watcher only fires while B is the selected thread
    \* frontend-slint/src/native_chat_observer.rs:357 -- the watch follows the selection, not B
    /\ active = "B"
    \* frontend-slint/src/native_chat_observer.rs:511 -- a background thread raises no such write
    /\ pB /= Mapped(dB)
    /\ pB' = Mapped(dB)
    /\ UNCHANGED <<dA, dB, pA, active, observable>>

Poll == \* FIX: the bounded pump raises the catalogue poll for any thread
    \* frontend-slint/src/native_chat_observer.rs:229 -- start_status_poll raises the poll on its own cadence
    /\ Fixed
    \* frontend-slint/src/native_chat_observer.rs:236 -- the pump yields while the window is hidden or the cloud owns the sidebar
    /\ observable
    /\ \/ (pA /= Mapped(dA) /\ pA' = Mapped(dA)
           /\ UNCHANGED <<dA, dB, pB, active, observable>>)
       \/ (pB /= Mapped(dB) /\ pB' = Mapped(dB)
           /\ UNCHANGED <<dA, dB, pA, active, observable>>)

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
=============================================================================