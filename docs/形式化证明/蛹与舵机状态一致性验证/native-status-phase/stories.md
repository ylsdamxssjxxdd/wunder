## 1. 任务已失败，标题显示空闲

Rule broken: A settled failure must read as a failure, not as an idle thread.
Why: 这条状态的文字没有被标题的判定认识，正在跑的任务被读成了空闲.
In the model, one step leads into this bug: "任务失败". The steps below are the shortest path that TLC found.

0. Start: 没有任务，标题显示空闲
1. 任务失败. Now: 任务已失败，标题显示空闲

With the fix, step 1 (任务失败) takes you to: 任务已失败，标题显示错误.

## 2. 任务正在跑，标题显示空闲

Rule broken: A thread that is still working must never look idle. The reader's sentence: while the work process says a request is running, the window must not say 空闲.
Why: 这条状态的文字没有被标题的判定认识，正在跑的任务被读成了空闲.
In the model, 4 different steps lead into this bug: "发送消息，请求被受理", "开始压缩上下文", "模型响应开始", and "请求进入模型队列". The steps below are the shortest path that TLC found.

0. Start: 没有任务，标题显示空闲
1. 发送消息，请求被受理. Now: 任务正在跑，标题显示空闲

With the fix, step 1 (发送消息，请求被受理) takes you to: 任务正在跑，标题显示运行中.
