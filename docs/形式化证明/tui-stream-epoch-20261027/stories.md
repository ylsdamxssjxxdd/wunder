## 1. 第 2 轮有事件未显示

Rule broken: Every transport event that arrives in its stream is rendered once.
Why: 传输事件编号在每轮重新开始，旧实现却把去重窗口保留到线程结束.
In the model, one step leads into this bug: "收到并投影一条流事件". The steps below are the shortest path that TLC found.

0. Start: 第 1 轮已显示全部到达事件
1. 收到并投影一条流事件. Now: 第 1 轮已显示全部到达事件
2. 开始下一轮并重置流去重窗口. Now: 第 2 轮已显示全部到达事件
3. 收到并投影一条流事件. Now: 第 2 轮有事件未显示

With the fix, step 3 (收到并投影一条流事件) keeps you at: 第 2 轮已显示全部到达事件.
