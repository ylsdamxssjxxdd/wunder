## 1. 订阅已结束<br/>线程已完成，工具转圈

Rule broken: Draining a failed execution's stream must not report a finished thread. Mutators: tui/app.rs:7948, :8302, :8337.
Why: 执行失败终态被忽略，订阅结束又被当成成功，工具未结清.
In the model, one step leads into this bug: "订阅结束，结算展示".

0. Start: 请求已受理<br/>线程工作中，尚无工具
1. 开始工具调用. Now: 工具已启动<br/>线程工作中，工具转圈
2. 收到执行失败终态. Now: 执行已失败，订阅待结束<br/>线程工作中，工具转圈
3. 订阅结束，结算展示. Now: 订阅已结束<br/>线程已完成，工具转圈

With the fix, step 2 (收到执行失败终态) takes you to: 执行已失败，订阅待结束<br/>线程失败，工具转圈.

## 2. 订阅已结束<br/>线程已完成，工具转圈

Rule broken: After stream completion a tool without a result is interrupted, never spinning or successful. Mutators: tui/app.rs:7948, :8192, :8254.
Why: 执行失败终态被忽略，订阅结束又被当成成功，工具未结清.
In the model, one step leads into this bug: "订阅结束，结算展示".

0. Start: 请求已受理<br/>线程工作中，尚无工具
1. 开始工具调用. Now: 工具已启动<br/>线程工作中，工具转圈
2. 收到执行失败终态. Now: 执行已失败，订阅待结束<br/>线程工作中，工具转圈
3. 订阅结束，结算展示. Now: 订阅已结束<br/>线程已完成，工具转圈

With the fix, step 2 (收到执行失败终态) takes you to: 执行已失败，订阅待结束<br/>线程失败，工具转圈.
