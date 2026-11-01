---- MODULE SelfUpdateFixed_rules ----
EXTENDS SelfUpdateFixed
StatechartTLC == INSTANCE TLC
StatechartBroken == (IF StagedAlwaysVerified THEN {} ELSE {"StagedAlwaysVerified"}) \cup (IF ExeRestorable THEN {} ELSE {"ExeRestorable"}) \cup (IF RotationRequiresVerified THEN {} ELSE {"RotationRequiresVerified"}) \cup (IF OnlyDesktopRotationTouchesExe THEN {} ELSE {"OnlyDesktopRotationTouchesExe"}) \cup (IF RestoreNoSameVersion THEN {} ELSE {"RestoreNoSameVersion"})
StatechartRules == StatechartBroken = {} \/ StatechartTLC!PrintT(<<"statechart broken", [phase |-> phase, target |-> target, staged |-> staged, staged_target |-> staged_target, staged_version |-> staged_version, staged_verified |-> staged_verified, applying |-> applying, exe_state |-> exe_state, exe_version |-> exe_version, exe_old |-> exe_old, exe_new |-> exe_new], StatechartBroken>>)
====
