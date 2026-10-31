---- MODULE ThreadHistoryReplay_rules ----
EXTENDS ThreadHistoryReplay
StatechartTLC == INSTANCE TLC
StatechartBroken == (IF TrimmedTurnNeverEmpty THEN {} ELSE {"TrimmedTurnNeverEmpty"}) \cup (IF TrimmedTurnComplete THEN {} ELSE {"TrimmedTurnComplete"}) \cup (IF PageNewestFirst THEN {} ELSE {"PageNewestFirst"}) \cup (IF BoundedScan THEN {} ELSE {"BoundedScan"})
StatechartRules == StatechartBroken = {} \/ StatechartTLC!PrintT(<<"statechart broken", [phase |-> phase, pos |-> pos, budget |-> budget, collected |-> collected, page |-> page, more |-> more], StatechartBroken>>)
====
