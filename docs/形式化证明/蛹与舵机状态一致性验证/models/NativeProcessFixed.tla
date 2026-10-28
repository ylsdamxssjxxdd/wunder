---------------------------- MODULE NativeProcessFixed ----------------------------
EXTENDS Integers
VARIABLES phase, status, entries, paged
vars == <<phase, status, entries, paged>>
Init ==
    /\ phase = 0                 \* frontend-slint/src/chat_process.rs:46
    /\ status = "running"        \* frontend-slint/src/native_chat_observer.rs:880
    /\ entries = "running"       \* frontend-slint/src/chat_process.rs:270
    /\ paged = TRUE              \* frontend-slint/src/chat_process.rs:303
TerminalHistory ==
    /\ phase = 0                 \* frontend-slint/src/timeline_text.rs:413; running latest page becomes terminal
    /\ phase' = 1
    /\ status' = "completed"     \* frontend-slint/src/native_chat_observer.rs:880
    /\ entries' = "unloaded"     \* frontend-slint/src/timeline_text.rs:418; fresh bounded model identity
    /\ paged' = FALSE            \* frontend-slint/src/timeline_text.rs:419
RefreshTimer ==
    /\ phase = 1                 \* frontend-slint/ui/tool_process.slint:186
    /\ ~paged                    \* frontend-slint/ui/tool_process.slint:186
    /\ phase' = 2
    /\ entries' = "completed"    \* frontend-slint/src/chat_process.rs:179
    /\ paged' = TRUE             \* frontend-slint/src/chat_process.rs:303
    /\ UNCHANGED status
System == RefreshTimer
Next == TerminalHistory \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..2 /\ status \in {"running", "completed"}
          /\ entries \in {"running", "completed", "unloaded"} /\ paged \in BOOLEAN
\* A completed latest page cannot settle with a stale running tool and no scheduled reread.
\* Mutators: timeline_text.rs:413; tool_process.slint:186, :191; chat_process.rs:179.
SettledProcessAgreement == (status = "completed" /\ Settled) => entries = "completed"
Spec == Init /\ [][Next]_vars
=============================================================================
