--------------------------- MODULE NativeStatusPhaseFixed ---------------------------
\* How a live thread's status reaches the reader, with the fix.
\*
\* The reducer still writes free-form phrases, but every write now carries the
\* phase, and every view reads that phase:
\*   - frontend-slint/src/runtime_phase.rs      Phase, the one phrase table
\*   - frontend-slint/src/native_chat.rs        set_session_status(.., phase)
\*   - frontend-slint/src/native_chat.rs        update_active_thread_filter(rows)
\*   - frontend-slint/ui/sidebar.slint          StatusDot(phase:)
\*
\* Only foreign text — the durable catalogue, a cloud row — still goes through
\* the phrase table, and that table now covers the phrases the reducer writes.
EXTENDS Naturals, TLC

VARIABLES
    phase,
    label,
    shown

Phases == {"idle", "queued", "running", "done", "error"}
Shown == {"idle", "pending", "running", "done", "error"}

\* What a view shows is a function of the phase. The phrase no longer decides.
\* frontend-slint/src/runtime_phase.rs:Phase::live + ui/runtime_status.slint:from-phase
Show(value) ==
    CASE value = "queued" -> "pending"
      [] value = "running" -> "running"
      [] value = "done" -> "done"
      [] value = "error" -> "error"
      [] OTHER -> "idle"

\* A thread that is still working must never look idle.
LiveNeverIdle ==
    phase \in {"queued", "running"} => shown \in {"running", "pending"}

\* A settled failure must read as a failure, not as an idle thread.
FailureShowsError == phase = "error" => shown = "error"

Init ==
    /\ phase = "idle"
    /\ label = ""
    /\ shown = Show(phase)

Admit ==
    /\ phase' = "running"
    /\ label' = "admitted"
    /\ shown' = Show(phase')

Promote ==
    /\ phase' = "running"
    /\ label' = "streaming"
    /\ shown' = Show(phase')

Reason ==
    /\ phase' = "running"
    /\ label' = "thinking"
    /\ shown' = Show(phase')

Queue ==
    /\ phase' = "queued"
    /\ label' = "queued_wait"
    /\ shown' = Show(phase')

Request ==
    /\ phase' = "running"
    /\ label' = "model_response"
    /\ shown' = Show(phase')

Compact ==
    /\ phase' = "running"
    /\ label' = "compacting"
    /\ shown' = Show(phase')

Settle ==
    /\ phase' = "done"
    /\ label' = "complete"
    /\ shown' = Show(phase')

Fail ==
    /\ phase' = "error"
    /\ label' = "failed"
    /\ shown' = Show(phase')

Next ==
    \/ Admit
    \/ Promote
    \/ Reason
    \/ Queue
    \/ Request
    \/ Compact
    \/ Settle
    \/ Fail

Spec == Init /\ [][Next]_<<phase, label, shown>>

TypeOK ==
    /\ phase \in Phases
    /\ shown \in Shown

=============================================================================
