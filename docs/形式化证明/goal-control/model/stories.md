## 1. Save returned, original objective retained

Rule broken: Saving the replacement objective through the goal banner updates the durable objective. frontend-slint/src/native_chat.rs:3124; crates/wunder-desktop/native_commands.rs:96
In the model, one step leads into this bug: "Banner save returns".

0. Start: Save changed objective: ready
1. User submits the command. Now: Save changed objective: control acquired
2. Banner save returns. Now: Save returned, original objective retained

With the fix, step 2 (Banner save returns) cannot happen, so you stay at: Save changed objective: control acquired.

## 2. Active goal, command missing after reload

Rule broken: A successful initial goal command remains visible after reloading its user turn. crates/wunder-desktop/native_commands.rs:107 (baseline); crates/wunder-runtime/src/api/chat_goal.rs:226
In the model, one step leads into this bug: "Goal creation returns".

0. Start: Create goal: ready
1. User submits the command. Now: Create goal: control acquired
2. Goal creation returns. Now: Active goal, command missing after reload

With the fix, step 2 (Goal creation returns) cannot happen, so you stay at: Create goal: control acquired.

## 3. Paused banner, executor still running

Rule broken: Returning from a human pause command has fenced the previous executor. crates/wunder-runtime/src/services/goal/command.rs:193; crates/wunder-runtime/src/api/chat_goal.rs:201
In the model, one step leads into this bug: "Pause command returns".

0. Start: Pause goal: ready
1. User submits the command. Now: Pause goal: control acquired
2. Pause command returns. Now: Paused banner, executor still running

With the fix, step 2 (Pause command returns) cannot happen, so you stay at: Pause goal: control acquired.
