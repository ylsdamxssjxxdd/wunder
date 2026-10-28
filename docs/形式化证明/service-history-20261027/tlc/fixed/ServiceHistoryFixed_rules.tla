---- MODULE ServiceHistoryFixed_rules ----
EXTENDS ServiceHistoryFixed
StatechartTLC == INSTANCE TLC
StatechartBroken == (IF BoundedDiagnostics THEN {} ELSE {"BoundedDiagnostics"}) \cup (IF MetadataNoBodyCopies THEN {} ELSE {"MetadataNoBodyCopies"}) \cup (IF ConstantCountRead THEN {} ELSE {"ConstantCountRead"}) \cup (IF ExactProjectionCount THEN {} ELSE {"ExactProjectionCount"})
StatechartRules == StatechartBroken = {} \/ StatechartTLC!PrintT(<<"statechart broken", [events |-> events, copiedBytes |-> copiedBytes, indexed |-> indexed, itemCount |-> itemCount, scannedRows |-> scannedRows], StatechartBroken>>)
====
