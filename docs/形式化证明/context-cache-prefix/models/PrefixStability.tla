--------------------------- MODULE PrefixStability ---------------------------
\* Bounded model of the message prefix the runtime sends to the model across the
\* rounds of one turn and across turns of one thread.  A provider prompt cache is
\* reused only while every new request repeats the previous request's message
\* prefix byte for byte and the frozen system block does not change.
\*
\* The durable store is abstracted to an append-only sequence of items; each item
\* is abstracted to the projection the runtime replays for it ("whole" keeps every
\* diagnostic field and inline image; "shrunk" is the compacted projection the
\* read path re-derives).  A request body is the replayed projection of every item
\* written so far, so two consecutive requests share a prefix iff no already-sent
\* item was re-projected and the system block did not change.
\*
\* Pre-fix HEAD re-projected an item a sent request already carried (inline-image
\* pruning, or metadata stripped on write while the read path re-derives another
\* form) and rebuilt the system block.  The fix chooses one projection when the
\* item is written, never rewrites an item a request already carried, and freezes
\* the system block.
EXTENDS Naturals, Sequences
CONSTANTS Fixed, MaxItems, MaxRequests

VARIABLES written, form1, form2, sys, sentLen, requests

vars == <<written, form1, form2, sys, sentLen, requests>>

WrittenIds == { written[i] : i \in 1..Len(written) }
SentIds == { written[i] : i \in 1..sentLen }

FormOf(id) == IF id = 1 THEN form1 ELSE form2

RequestBody == << FormOf(written[i]) : i \in 1..Len(written) >>

IsPrefix(a, b) ==
    /\ Len(a) <= Len(b)
    /\ \A i \in 1..Len(a): a[i] = b[i]

\* The fix never re-projects an item a sent request already carried; the pre-fix
\* HEAD allowed rewriting any written item.
RewriteAllowed(id) ==
    IF Fixed THEN id \notin SentIds ELSE TRUE

Init ==
    /\ written = <<>>
    /\ form1 = "whole"
    /\ form2 = "whole"
    /\ sys = "S0"
    /\ sentLen = 0
    /\ requests = <<>>

Write1 ==
    /\ Len(written) < MaxItems
    /\ 1 \notin WrittenIds
    /\ written' = Append(written, 1)
    /\ UNCHANGED <<form1, form2, sys, sentLen, requests>>

Write2 ==
    /\ Len(written) < MaxItems
    /\ 2 \notin WrittenIds
    /\ written' = Append(written, 2)
    /\ UNCHANGED <<form1, form2, sys, sentLen, requests>>

\* A model request is sent: its body is the replayed projection of everything
\* written so far, and it records how much of the store it carried.
Send ==
    /\ Len(requests) < MaxRequests
    /\ requests' = Append(requests, RequestBody)
    /\ sentLen' = Len(written)
    /\ UNCHANGED <<written, form1, form2, sys>>

Shrink1 ==
    /\ form1 = "whole"
    /\ RewriteAllowed(1)
    /\ form1' = "shrunk"
    /\ UNCHANGED <<written, form2, sys, sentLen, requests>>

Shrink2 ==
    /\ form2 = "whole"
    /\ RewriteAllowed(2)
    /\ form2' = "shrunk"
    /\ UNCHANGED <<written, form1, sys, sentLen, requests>>

\* Re-derive the frozen system block from volatile inputs (modelled defect).
DriftSystem ==
    /\ ~Fixed
    /\ sys = "S0"
    /\ sys' = "S1"
    /\ UNCHANGED <<written, form1, form2, sentLen, requests>>

Next == Write1 \/ Write2 \/ Send \/ Shrink1 \/ Shrink2 \/ DriftSystem

Spec == Init /\ [][Next]_vars

\* Every new request repeats the previous request's message prefix byte for byte.
AppendOnlyPrefix ==
    \A k \in 1..(Len(requests) - 1): IsPrefix(requests[k], requests[k + 1])
\* The frozen system block never changes after the thread is created.
SystemStable == sys = "S0"
=============================================================================