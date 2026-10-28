## 1. 已启动：蛹<br/>启动被拦：舵机甲<br/>任务甲：无<br/>任务乙：无<br/>排空中的写入：0

Rule broken: Opening one local entry must not reject another solely because its root is in use.
Why: 线程已经隔离，但数据根的进程级独占仍拒绝其他本地入口.
In the model, one step leads into this bug: "打开本地入口". The steps below are the shortest path that TLC found.

0. Start: 已启动：无<br/>启动被拦：无<br/>任务甲：无<br/>任务乙：无<br/>排空中的写入：0
1. 打开本地入口. Now: 已启动：蛹<br/>启动被拦：无<br/>任务甲：无<br/>任务乙：无<br/>排空中的写入：0
2. 打开本地入口. Now: 已启动：蛹<br/>启动被拦：舵机甲<br/>任务甲：无<br/>任务乙：无<br/>排空中的写入：0

With the fix, step 2 (打开本地入口) takes you to: 已启动：蛹、舵机甲<br/>启动被拦：无<br/>任务甲：无<br/>任务乙：无<br/>排空中的写入：0.
