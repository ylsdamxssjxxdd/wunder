------------------------- MODULE StreamFollow -------------------------
EXTENDS Naturals
CONSTANT MaxGrowth, Fixed
VARIABLES oGrown, oFollowed, oFollowTail, oPending, iGrown, iFollowed, iFollowTail, iPending
vars == <<oGrown, oFollowed, oFollowTail, oPending, iGrown, iFollowed, iFollowTail, iPending>>
\* Two independent follow lanes of the pupa chat page: the outer transcript
\* (TimelineView) and the inner work-process list (ToolProcess). Each lane
\* tracks how many abstract content units the stream produced (grown) and how
\* many the viewport tail has caught up to (followed).
Init ==
    /\ oGrown = 0 /\ oFollowed = 0 /\ oFollowTail = TRUE /\ oPending = FALSE
    /\ iGrown = 0 /\ iFollowed = 0 /\ iFollowTail = TRUE /\ iPending = FALSE
OuterPublishesFrame ==
    \* frontend-slint/src/native_chat.rs:938; one stream_updates bump per coalesced frame
    /\ oGrown < MaxGrowth \* model bound; three growths cover the interleaving
    /\ oGrown' = oGrown + 1
    /\ oPending' = TRUE \* frontend-slint/ui/timeline.slint:883-888; changed stream-revision re-arms the follow
    /\ UNCHANGED <<oFollowed, oFollowTail, iGrown, iFollowed, iFollowTail, iPending>>
OuterContentSettles ==
    \* frontend-slint/ui/timeline.slint:889-899; changed transcript-height re-arms the follow
    /\ oGrown < MaxGrowth \* model bound; layout settling only changes height
    /\ oGrown' = oGrown + 1
    /\ oPending' = TRUE \* frontend-slint/ui/timeline.slint:895-898
    /\ UNCHANGED <<oFollowed, oFollowTail, iGrown, iFollowed, iFollowTail, iPending>>
OuterFollowPass ==
    \* frontend-slint/ui/timeline.slint:865-875; follow-timer scrolls to the end
    /\ oPending \* frontend-slint/ui/timeline.slint:866; running: follow-output && follow-pending
    /\ oPending' = FALSE
    /\ oFollowed' = IF oFollowTail THEN oGrown ELSE oFollowed \* frontend-slint/ui/timeline.slint:840-842; scroll-to-end
    /\ UNCHANGED <<oGrown, oFollowTail, iGrown, iFollowed, iFollowTail, iPending>>
InnerNewEntry ==
    \* frontend-slint/ui/tool_process.slint:231; changed count re-arms scroll-pending
    /\ iGrown < MaxGrowth \* model bound; a new row both grows and signals
    /\ iGrown' = iGrown + 1
    /\ iPending' = TRUE \* frontend-slint/ui/tool_process.slint:231
    /\ UNCHANGED <<iFollowed, iFollowTail, oGrown, oFollowed, oFollowTail, oPending>>
InnerEntryGrows ==
    \* frontend-slint/src/timeline.rs:498; sync_process_entry patches the child in place,
    \* so the row only grows taller: count (tool_process.slint:231) does not move and
    \* process_revision (frontend-slint/src/chat_process.rs:315) moves only when paging.
    /\ iGrown < MaxGrowth \* model bound; streamed text keeps growing one row
    /\ iGrown' = iGrown + 1
    \* Fixed: frontend-slint/ui/tool_process.slint:210-216 re-arms on changed measured-height.
    \* Before the fix no signal existed for silent growth, so pending stayed as it was.
    /\ iPending' = IF Fixed THEN TRUE ELSE iPending
    /\ UNCHANGED <<iFollowed, iFollowTail, oGrown, oFollowed, oFollowTail, oPending>>
InnerFollowPass ==
    \* frontend-slint/ui/tool_process.slint:233-251; armed pass pulls the list to its tail
    /\ iPending \* frontend-slint/ui/tool_process.slint:234; running: root.scroll-pending
    /\ iPending' = FALSE
    /\ iFollowed' = IF iFollowTail THEN iGrown ELSE iFollowed \* frontend-slint/ui/tool_process.slint:244; follow-tail && !anchor-hold
    /\ UNCHANGED <<iGrown, iFollowTail, oGrown, oFollowed, oFollowTail, oPending>>
ReaderScrollsUpOuter ==
    \* frontend-slint/ui/timeline.slint:959-968; scrolled recomputes follow-output
    /\ oFollowTail \* reading back only matters while still at the tail
    /\ oFollowTail' = FALSE
    /\ UNCHANGED <<oGrown, oFollowed, oPending, iGrown, iFollowed, iFollowTail, iPending>>
ReaderBackToTailOuter ==
    \* frontend-slint/ui/timeline.slint:1031-1034; back-to-latest resumes the follow
    /\ ~oFollowTail \* only a reader who left the tail can return to it
    /\ oFollowTail' = TRUE /\ oFollowed' = oGrown
    /\ UNCHANGED <<oGrown, oPending, iGrown, iFollowed, iFollowTail, iPending>>
ReaderScrollsUpInner ==
    \* frontend-slint/ui/tool_process.slint:265-276; scrolled recomputes follow-tail
    /\ iFollowTail \* reading back only matters while still at the tail
    /\ iFollowTail' = FALSE
    /\ UNCHANGED <<iGrown, iFollowed, iPending, oGrown, oFollowed, oFollowTail, oPending>>
ReaderBackToTailInner ==
    \* frontend-slint/ui/tool_process.slint:265-276; reaching the tail resumes the follow
    /\ ~iFollowTail \* only a reader who left the tail can return to it
    /\ iFollowTail' = TRUE /\ iFollowed' = iGrown
    /\ UNCHANGED <<iGrown, iPending, oGrown, oFollowed, oFollowTail, oPending>>
Next == OuterPublishesFrame \/ OuterContentSettles \/ OuterFollowPass
       \/ InnerNewEntry \/ InnerEntryGrows \/ InnerFollowPass
       \/ ReaderScrollsUpOuter \/ ReaderBackToTailOuter
       \/ ReaderScrollsUpInner \/ ReaderBackToTailInner
System == OuterPublishesFrame \/ OuterContentSettles \/ OuterFollowPass
         \/ InnerNewEntry \/ InnerEntryGrows \/ InnerFollowPass
Settled == ~ENABLED System
TypeOK == /\ oGrown \in 0..MaxGrowth /\ oFollowed \in 0..MaxGrowth
          /\ iGrown \in 0..MaxGrowth /\ iFollowed \in 0..MaxGrowth
          /\ oFollowTail \in BOOLEAN /\ oPending \in BOOLEAN
          /\ iFollowTail \in BOOLEAN /\ iPending \in BOOLEAN
          /\ oFollowed <= oGrown /\ iFollowed <= iGrown
\* 跟随中且系统已安静时，外层时间线尾部必须就是最新内容；mutator: frontend-slint/ui/timeline.slint:866.
OuterTailFreshWhenFollowing == Settled => (oFollowTail => oFollowed = oGrown)
\* 跟随中且系统已安静时，工作过程列表尾部必须就是最新内容；mutator: frontend-slint/ui/tool_process.slint:244.
InnerTailFreshWhenFollowing == Settled => (iFollowTail => iFollowed = iGrown)
Spec == Init /\ [][Next]_vars
=============================================================================
