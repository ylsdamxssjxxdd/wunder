--------------------------- MODULE NativeProcessPagingFixed ---------------------------
\* Showing the earlier steps of a long work process, with the fix.
\*
\* The page edges now live on the row itself, so a rebuilt view continues the
\* page it is showing, and the reader asks for the older page by scrolling past
\* the top. The button is gone because it was a second way to say the same
\* thing, and the one that could silently do nothing.
\*
\* Code behind each step, after the fix:
\*   Open        frontend-slint/src/timeline.slint      ToolProcess opens on toggle
\*   ReadLatest  frontend-slint/src/chat_process.rs     direction 0, both cursors absent
\*   Rebuild     frontend-slint/src/chat_process.rs     row.process_first/last survive
\*   ScrollTop   frontend-slint/ui/tool_process.slint   scrolled at the top asks for -1
EXTENDS Naturals, TLC

VARIABLES
    panel,
    page,
    older_left,
    edge,
    entry

EarlierCanBeRead ==
    (panel = "open" /\ older_left /\ page = "latest") =>
        (entry = "scroll" \/ edge = "set")

LoadedPageKeepsItsEdge ==
    page /= "none" /\ older_left => edge = "set"

Init ==
    /\ panel = "folded"
    /\ page = "none"
    /\ older_left = TRUE
    /\ edge = "unknown"
    /\ entry = "scroll"

Open ==
    /\ panel' = "open"
    /\ UNCHANGED <<page, older_left, edge, entry>>

ReadLatest ==
    /\ panel = "open"  \* frontend-slint/ui/timeline.slint:608
    /\ page' = "latest"
    /\ edge' = "set"
    /\ UNCHANGED <<panel, older_left, entry>>

\* The view is rebuilt, but the page and its edges belong to the row, so
\* nothing about the page is lost.
Rebuild ==
    /\ page = "latest"  \* frontend-slint/src/chat_process.rs:50
    /\ UNCHANGED <<panel, page, older_left, edge, entry>>

\* Scrolling past the top is the request for the older page. It only happens
\* when the loaded page carries the cursor that continues it.
ScrollTop ==
    /\ panel = "open"  \* frontend-slint/ui/timeline.slint:608
    /\ older_left  \* frontend-slint/src/chat_process.rs:96
    /\ edge = "set"  \* frontend-slint/ui/tool_process.slint:252
    /\ page' = "older"
    /\ UNCHANGED <<panel, older_left, edge, entry>>

MarkOlder ==
    /\ page = "latest"  \* frontend-slint/src/timeline.rs:832
    /\ older_left' = TRUE
    /\ UNCHANGED <<panel, page, edge, entry>>

Next ==
    \/ Open
    \/ ReadLatest
    \/ Rebuild
    \/ ScrollTop
    \/ MarkOlder

Spec == Init /\ [][Next]_<<panel, page, older_left, edge, entry>>

TypeOK ==
    /\ panel \in {"folded", "open"}
    /\ page \in {"none", "latest", "older"}
    /\ entry \in {"button", "scroll"}

=============================================================================
