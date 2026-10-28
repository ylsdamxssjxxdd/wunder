--------------------------- MODULE GoalControlFixed ---------------------------
EXTENDS Naturals
CONSTANT Unified
VARIABLES command, stage, phase, objective, durableCommand, running, armed
vars == <<command, stage, phase, objective, durableCommand, running, armed>>

Init ==
    /\ command \in {"create", "save", "pause"}
    /\ stage = "ready"
    /\ phase = IF command = "create" THEN "none" ELSE "active"
    /\ objective = IF command = "create" THEN "none" ELSE "original"
    /\ durableCommand = FALSE
    /\ running = (command = "pause")
    /\ armed = (command # "create")

EnterCommand ==
    /\ stage = "ready" \* crates/wunder-desktop/native_commands.rs:73
    /\ stage' = "controlled"
    /\ UNCHANGED <<command, phase, objective, durableCommand, running, armed>>

DisarmAdmission ==
    /\ Unified \* crates/wunder-desktop/native_commands.rs:103
    /\ stage = "controlled" \* crates/wunder-runtime/src/api/chat_goal.rs:192
    /\ stage' = "fenced"
    /\ armed' = FALSE
    /\ UNCHANGED <<command, phase, objective, durableCommand, running>>

CancelExecutor ==
    /\ Unified \* crates/wunder-desktop/native_commands.rs:103
    /\ stage = "fenced" \* crates/wunder-runtime/src/api/chat_goal.rs:201
    /\ stage' = "cancelled"
    /\ running' = FALSE
    /\ UNCHANGED <<command, phase, objective, durableCommand, armed>>

PersistCommand ==
    /\ Unified \* crates/wunder-desktop/native_commands.rs:103
    /\ stage = "cancelled" \* crates/wunder-runtime/src/api/chat_goal.rs:226
    /\ stage' = "persisted"
    /\ durableCommand' = TRUE
    /\ UNCHANGED <<command, phase, objective, running, armed>>

CompleteCommand ==
    /\ Unified \* crates/wunder-desktop/native_commands.rs:103
    /\ stage = "persisted" \* crates/wunder-runtime/src/api/chat_goal.rs:270
    /\ stage' = "recorded"
    /\ UNCHANGED <<command, phase, objective, durableCommand, running, armed>>

CreateGoal ==
    /\ stage = IF Unified THEN "recorded" ELSE "controlled" \* crates/wunder-runtime/src/api/chat_goal.rs:281; crates/wunder-desktop/native_commands.rs:107 (baseline)
    /\ command = "create" \* crates/wunder-desktop/native_commands.rs:95
    /\ phase = "none" \* crates/wunder-runtime/src/services/goal/command.rs:142
    /\ stage' = "done"
    /\ phase' = "active"
    /\ objective' = "original"
    /\ armed' = TRUE
    /\ UNCHANGED <<command, durableCommand, running>>

SaveGoal ==
    /\ stage = IF Unified THEN "recorded" ELSE "controlled" \* crates/wunder-runtime/src/api/chat_goal.rs:281; crates/wunder-desktop/native_commands.rs:107 (baseline)
    /\ command = "save" \* frontend-slint/src/native_chat.rs:3124
    /\ phase = "active" \* crates/wunder-runtime/src/services/goal/command.rs:142
    /\ stage' = "done"
    /\ objective' = IF Unified THEN "revised" ELSE objective
    /\ armed' = TRUE
    /\ UNCHANGED <<command, phase, durableCommand, running>>

PauseGoal ==
    /\ stage = IF Unified THEN "recorded" ELSE "controlled" \* crates/wunder-runtime/src/api/chat_goal.rs:281; crates/wunder-desktop/native_commands.rs:107 (baseline)
    /\ command = "pause" \* crates/wunder-desktop/native_commands.rs:99
    /\ phase = "active" \* crates/wunder-runtime/src/services/goal/service.rs:397
    /\ stage' = "done"
    /\ phase' = "paused"
    /\ armed' = FALSE
    /\ UNCHANGED <<command, objective, durableCommand, running>>

Next == EnterCommand \/ DisarmAdmission \/ CancelExecutor \/ PersistCommand \/ CompleteCommand \/ CreateGoal \/ SaveGoal \/ PauseGoal
Spec == Init /\ [][Next]_vars

\* A successful initial goal command remains visible after reloading its user turn.
\* crates/wunder-desktop/native_commands.rs:107 (baseline); crates/wunder-runtime/src/api/chat_goal.rs:226
CreateHasDurableCommand == (stage = "done" /\ command = "create") => durableCommand

\* Saving the replacement objective through the goal banner updates the durable objective.
\* frontend-slint/src/native_chat.rs:3124; crates/wunder-desktop/native_commands.rs:96
BannerSaveUpdatesGoal == (stage = "done" /\ command = "save") => objective = "revised"

\* Returning from a human pause command has fenced the previous executor.
\* crates/wunder-runtime/src/services/goal/command.rs:193; crates/wunder-runtime/src/api/chat_goal.rs:201
PauseFencesExecutor == (stage = "done" /\ command = "pause") => ~running
=============================================================================
