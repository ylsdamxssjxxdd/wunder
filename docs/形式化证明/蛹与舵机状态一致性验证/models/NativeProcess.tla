---------------------------- MODULE NativeProcess ----------------------------
EXTENDS Integers
VARIABLES phase, status, entries, paged
vars == <<phase, status, entries, paged>>
Init ==
    /\ phase = 0                 \* frontend-slint/src/chat_process.rs:46; selected latest page
    /\ status = "running"        \* frontend-slint/src/native_chat_observer.rs:880
    /\ entries = "running"       \* frontend-slint/src/chat_process.rs:273; prior loaded tool
    /\ paged = TRUE              \* frontend-slint/src/chat_process.rs:306
TerminalHistory ==
    /\ phase = 0                 \* frontend-slint/src/timeline_text.rs:410; matching process id
    /\ phase' = 1
    /\ status' = "completed"     \* frontend-slint/src/native_chat_observer.rs:880
    /\ UNCHANGED <<entries, paged>> \* frontend-slint/src/timeline_text.rs:413; restores old page
RefreshTimer ==
    /\ phase = 1                 \* frontend-slint/ui/tool_process.slint:186
    /\ ~paged                    \* frontend-slint/ui/tool_process.slint:186; live refresh at :191 is off
    /\ phase' = 2
    /\ entries' = "completed"    \* frontend-slint/src/chat_process.rs:182; completed durable page
    /\ paged' = TRUE             \* frontend-slint/src/chat_process.rs:306
    /\ UNCHANGED status
System == RefreshTimer
Next == TerminalHistory \/ System
Settled == ~ENABLED System
TypeOK == /\ phase \in 0..2 /\ status \in {"running", "completed"}
          /\ entries \in {"running", "completed", "unloaded"} /\ paged \in BOOLEAN
\* A completed latest page cannot settle with a stale running tool and no scheduled reread.
\* Mutators: timeline_text.rs:413; tool_process.slint:186, :191; chat_process.rs:182.
SettledProcessAgreement == (status = "completed" /\ Settled) => entries = "completed"
Spec == Init /\ [][Next]_vars
=============================================================================
