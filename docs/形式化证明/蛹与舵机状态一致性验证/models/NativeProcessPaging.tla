--------------------------- MODULE NativeProcessPaging ---------------------------
\* Showing the earlier steps of a long work process, before the fix.
\*
\* The panel kept one durable page whose edges (the exclusive item indexes
\* before/after the page) were held in a per-view cursor map. The map was reset
\* whenever the row's entry model changed identity, so after a rebuild the
\* panel still said "there are earlier steps" but no longer had the cursor that
\* asks for them. Pressing the button then re-read the newest page, which is
\* why the control looked dead.
\*
\* Code behind each step, as it was before the fix:
\*   Open        frontend-slint/src/timeline.slint:607    ToolProcess opens on toggle
\*   ReadLatest  frontend-slint/src/chat_process.rs:104   direction 0, both cursors absent
\*   Rebuild     frontend-slint/src/chat_process.rs:87    cursor reset on model identity
\*   PressButton frontend-slint/src/chat_process.rs:104   direction -1 with cursor.first
\*   MarkOlder   frontend-slint/src/timeline.rs:829       process_older = true
\*   ShowOlder   frontend-slint/src/chat_process.rs:208   apply_page prepends the older page
EXTENDS Naturals, TLC

VARIABLES
    panel,       \* "folded" or "open"
    page,        \* "none", "latest" (newest page only), or "older" (an earlier page is shown)
    older_left,  \* the storage still has steps older than the loaded page
    edge,        \* the loaded page's own cursor: "unknown" or "set"
    entry        \* how the reader can ask for the older page: "button" or "scroll"

\* With older steps left and the newest page on screen, the reader must still be
\* able to reach them.
\* The reader's sentence: 还有更早的过程时，向上滑或点击都要能真的看到它们.
EarlierCanBeRead ==
    (panel = "open" /\ older_left /\ page = "latest") =>
        (entry = "scroll" \/ edge = "set")

\* A loaded page keeps the cursor that continues it, whatever the view does.
LoadedPageKeepsItsEdge ==
    page /= "none" /\ older_left => edge = "set"

Init ==
    /\ panel = "folded"
    /\ page = "none"
    /\ older_left = TRUE
    /\ edge = "unknown"
    /\ entry = "button"

\* The reader opens the work process.
Open ==
    /\ panel' = "open"
    /\ UNCHANGED <<page, older_left, edge, entry>>

\* The first read returns the newest page and its edges.
ReadLatest ==
    /\ panel = "open"  \* frontend-slint/ui/timeline.slint:608
    /\ page' = "latest"
    /\ edge' = "set"
    /\ UNCHANGED <<panel, older_left, entry>>

\* The row model is rebuilt (thread switch, page reload, live tail replaced),
\* which resets the cursor map this view owned.
Rebuild ==
    /\ page = "latest"  \* frontend-slint/src/chat_process.rs:50
    /\ edge' = "unknown"
    /\ UNCHANGED <<panel, page, older_left, entry>>

\* The reader asks for the older page through the button. Without a cursor the
\* read is the newest page again: nothing on screen changes.
PressButton ==
    /\ panel = "open"  \* frontend-slint/ui/timeline.slint:608
    /\ older_left  \* frontend-slint/src/chat_process.rs:96
    /\ page' = IF edge = "set" THEN "older" ELSE "latest"
    /\ UNCHANGED <<panel, older_left, edge, entry>>

\* Storage still has steps older than the page that is loaded.
MarkOlder ==
    /\ page = "latest"  \* frontend-slint/src/timeline.rs:832
    /\ older_left' = TRUE
    /\ UNCHANGED <<panel, page, edge, entry>>

Next ==
    \/ Open
    \/ ReadLatest
    \/ Rebuild
    \/ PressButton
    \/ MarkOlder

Spec == Init /\ [][Next]_<<panel, page, older_left, edge, entry>>

TypeOK ==
    /\ panel \in {"folded", "open"}
    /\ page \in {"none", "latest", "older"}
    /\ entry \in {"button", "scroll"}

=============================================================================
