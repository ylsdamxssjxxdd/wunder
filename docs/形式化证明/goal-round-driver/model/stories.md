## 1. Active goal, 0 rounds done, nothing next

Rule broken: 目标一旦活跃且武装，只要会话空闲且未达轮次上限，就必须继续开轮。 crates/wunder-desktop/native_stream.rs:238; crates/wunder-runtime/src/services/goal/driver.rs:200
In the model, one step leads into this bug: "Human turn ends, session fence released".

0. Start: No goal, human turn owns the session
1. Model creates the goal mid-turn. Now: Goal waits for the human turn
2. Driver wake meets the busy session. Now: Goal waits for the human turn
3. Human turn ends, session fence released. Now: Active goal, 0 rounds done, nothing next

With the fix, step 2 (Driver wake meets the busy session) takes you to: Goal waits for the human turn, wake kept for the release.
