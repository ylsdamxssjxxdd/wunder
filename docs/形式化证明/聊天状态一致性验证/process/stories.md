## 1. 气泡完成，工具转圈<br/>无读取，无刷新<br/>工具已落库

Rule broken: With no timer or request left, the displayed tool must reflect the durable result. Mutators: MessageTimelineTurnProcess.vue:363, :405, :439, :578.
Why: 终态刷新被在途读取吞掉，旧响应落地后没有后续刷新.
In the model, one step leads into this bug: "旧响应落地".

0. Start: 气泡运行，工具转圈<br/>无读取，无刷新<br/>工具未完成
1. 计数变化，安排刷新. Now: 气泡运行，工具转圈<br/>无读取，待刷新<br/>工具未完成
2. 延后 600 毫秒读取. Now: 气泡运行，工具转圈<br/>读取中，无刷新<br/>工具未完成
3. 工具结果落库. Now: 气泡运行，工具转圈<br/>读取中，无刷新<br/>工具已落库
4. 收到完成态，安排刷新. Now: 气泡完成，工具转圈<br/>读取中，待刷新<br/>工具已落库
5. 读取在途，丢弃刷新. Now: 气泡完成，工具转圈<br/>读取中，无刷新<br/>工具已落库
6. 旧响应落地. Now: 气泡完成，工具转圈<br/>无读取，无刷新<br/>工具已落库

With the fix, step 5 (读取在途，丢弃刷新) happens as "读取在途，保留刷新" and keeps you at: 气泡完成，工具转圈<br/>读取中，待刷新<br/>工具已落库.
