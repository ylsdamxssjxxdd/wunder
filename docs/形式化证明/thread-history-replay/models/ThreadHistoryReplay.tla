--------------------------- MODULE ThreadHistoryReplay ---------------------------
\* Bounded model of reading a turn that left the projection's active window.
\* The projection keeps only the newest ACTIVE_WINDOW_ITEMS rows, so older turns
\* are replayed from the canonical rollout.  The pre-fix HEAD started at the
\* nearest sparse anchor; the fix starts at the turn's own first record and keeps
\* anchors only as a closer optimisation.  One read is modelled: the JSONL scan,
\* the record budget and the page the caller receives.
EXTENDS Naturals, Sequences
CONSTANTS Fixed, TurnStart, TurnEnd, TurnItems, AnchorStart, OldRecordBudget, NewRecordBudget
VARIABLES phase, pos, budget, collected, page, more

vars == <<phase, pos, budget, collected, page, more>>

RECURSIVE ReverseSeq(_)
ReverseSeq(seq) == IF seq = <<>> THEN <<>> ELSE Append(ReverseSeq(Tail(seq)), Head(seq))

\* The scan start of the fix is the turn boundary persisted by the projection;
\* the pre-fix start is the nearest anchor below the page edge.
StartAt == IF Fixed THEN TurnStart ELSE AnchorStart
RecordBudget == IF Fixed THEN NewRecordBudget ELSE OldRecordBudget

Init ==
    /\ phase = "idle"
    /\ pos = 0
    /\ budget = 0
    /\ collected = <<>>
    /\ page = <<>>
    /\ more = FALSE

CollectedItems == IF Len(collected) = 0 THEN {} ELSE { collected[i] : i \in 1..Len(collected) }

\* A chat row or process page requests a turn that is below the active window.
Start ==
    /\ phase = "idle"           \* crates/wunder-runtime/src/storage/thread_history.rs:1153
    /\ phase' = "scan"
    /\ pos' = StartAt           \* crates/wunder-runtime/src/storage/thread_history.rs:1201,1265
    /\ budget' = RecordBudget   \* crates/wunder-runtime/src/storage/thread_history.rs:1183
    /\ collected' = <<>>
    /\ page' = <<>>
    /\ more' = FALSE

\* Canonical JSONL is read forward one record at a time; only persisted items of
\* the requested turn are accepted, and every read record consumes the budget.
Advance ==
    /\ phase = "scan"           \* crates/wunder-runtime/src/storage/thread_history.rs:1383
    /\ pos <= TurnEnd           \* crates/wunder-runtime/src/storage/thread_history.rs:1420
    /\ budget > 0               \* crates/wunder-runtime/src/storage/thread_history.rs:1437
    /\ pos' = pos + 1
    /\ budget' = budget - 1
    /\ collected' = IF pos \in TurnItems THEN Append(collected, pos) ELSE collected
    /\ UNCHANGED <<phase, page, more>>

\* Older chunks are reversed before merging, so the page is newest-first and
\* keeps every accepted item of the turn.
Finish ==
    /\ phase = "scan"           \* crates/wunder-runtime/src/storage/canonical_thread_log.rs:658
    /\ pos > TurnEnd            \* crates/wunder-runtime/src/storage/thread_history.rs:1248
    /\ phase' = "done"
    /\ page' = ReverseSeq(collected)  \* crates/wunder-runtime/src/storage/thread_history.rs:1239
    /\ more' = FALSE
    /\ UNCHANGED <<pos, budget, collected>>

\* A chunk cut by the record budget is dropped instead of shown as the page top,
\* and the read reports that more data exists.
Exhaust ==
    /\ phase = "scan"           \* crates/wunder-runtime/src/storage/thread_history.rs:1229
    /\ budget = 0               \* crates/wunder-runtime/src/storage/thread_history.rs:1183
    /\ pos <= TurnEnd           \* crates/wunder-runtime/src/storage/thread_history.rs:1405
    /\ phase' = "truncated"
    /\ page' = <<>>             \* crates/wunder-runtime/src/storage/thread_history.rs:1229
    /\ more' = TRUE
    /\ UNCHANGED <<pos, budget, collected>>

Next == Start \/ Advance \/ Finish \/ Exhaust
Spec == Init /\ [][Next]_vars

\* A trimmed turn that still has canonical items never returns an empty page.
TrimmedTurnNeverEmpty == phase = "truncated" => collected /= <<>>
\* A finished replay returns every item the turn persisted.
TrimmedTurnComplete == phase = "done" => TurnItems \subseteq CollectedItems
\* Page contract: newest-first with strictly descending ordinals.
PageNewestFirst == Len(page) = 0 \/ \A i \in 1..(Len(page) - 1): page[i] > page[i + 1]
\* The scan never reads beyond its record budget.
BoundedScan == budget <= RecordBudget
=============================================================================
