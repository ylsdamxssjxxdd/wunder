--------------------------- MODULE ServiceHistoryFixed ---------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANT Fixed
VARIABLES events, copiedBytes, indexed, itemCount, scannedRows
vars == <<events, copiedBytes, indexed, itemCount, scannedRows>>
Init == /\ events = <<>> /\ copiedBytes = 0
        /\ indexed = {} /\ itemCount = 0 /\ scannedRows = 0
Min(left, right) == IF left < right THEN left ELSE right
Emit(size) ==
    \* crates/wunder-runtime/src/ops/monitor.rs:2777; crates/wunder-runtime/src/ops/monitor/event_projection.rs:5
    /\ LET projected == IF Fixed THEN Min(size, 2) ELSE size
           appended == Append(events, projected)
       IN events' = IF Len(appended) > 2 THEN Tail(appended) ELSE appended
    /\ UNCHANGED <<copiedBytes, indexed, itemCount, scannedRows>>
Bytes(sequence) == IF Len(sequence) = 0 THEN 0 ELSE
    IF Len(sequence) = 1 THEN sequence[1] ELSE sequence[1] + sequence[2]
MetadataRead ==
    \* crates/wunder-runtime/src/ops/monitor.rs:322; pre-fix HEAD monitor.rs:330
    /\ copiedBytes' = IF Fixed THEN 0 ELSE Bytes(events)
    /\ UNCHANGED <<events, indexed, itemCount, scannedRows>>
Commit(item) ==
    \* crates/wunder-runtime/src/storage/postgres/thread_display.rs:161; crates/wunder-runtime/src/storage/postgres/thread_store.rs:755
    /\ indexed' = indexed \cup {item}
    /\ itemCount' = itemCount + IF item \in indexed THEN 0 ELSE 1
    /\ UNCHANGED <<events, copiedBytes, scannedRows>>
ReadCount ==
    \* crates/wunder-runtime/src/storage/postgres/thread_display.rs:99; pre-fix HEAD thread_log_facade.rs:788
    /\ scannedRows' = IF Fixed THEN 0 ELSE Cardinality(indexed)
    /\ UNCHANGED <<events, copiedBytes, indexed, itemCount>>
Next == (\E size \in 1..3: Emit(size)) \/ MetadataRead
        \/ (\E item \in 1..3: Commit(item)) \/ ReadCount
Spec == Init /\ [][Next]_vars
\* Per-event diagnostics have a byte budget, not merely a slot limit.
BoundedDiagnostics == \A position \in 1..Len(events): events[position] <= 2
\* Metadata reads copy no diagnostic body under the global monitor mutex.
MetadataNoBodyCopies == copiedBytes = 0
\* Hot count reads inspect no historical item rows after initialization.
ConstantCountRead == scannedRows = 0
\* Canonical projection, successful insert and count delta commit atomically.
ExactProjectionCount == itemCount = Cardinality(indexed)
=============================================================================

