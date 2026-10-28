## 1. 取消流已结束<br/>气泡已停止，工具转圈

Rule broken: Once the cancelled stream drains, no tool owned by that turn keeps spinning. Mutators: native_chat.rs:3427; timeline.rs:1254.
Why: 取消未被识别为非成功结算，工具仍转圈.
In the model, one step leads into this bug: "取消订阅结束".

0. Start: 轮次开始<br/>气泡运行，尚无工具
1. 开始工具调用. Now: 工具运行<br/>气泡运行，工具转圈
2. 收到取消终态. Now: 收到取消终态<br/>气泡已停止，工具转圈
3. 取消订阅结束. Now: 取消流已结束<br/>气泡已停止，工具转圈

With the fix, step 3 (取消订阅结束) takes you to: 取消流已结束<br/>气泡已停止，工具已中断.

## 2. 失败后收到迟到回复<br/>气泡完成，工具转圈

Rule broken: A late final frame cannot rewrite the execution's failed terminal outcome. Mutators: native_chat.rs:3511, :3662, :3681.
Why: 失败终态没有保护，迟到回复覆盖为完成.
In the model, one step leads into this bug: "迟到最终回复".

0. Start: 轮次开始<br/>气泡运行，尚无工具
1. 开始工具调用. Now: 工具运行<br/>气泡运行，工具转圈
2. 收到失败终态. Now: 收到失败终态<br/>气泡失败，工具转圈
3. 迟到最终回复. Now: 失败后收到迟到回复<br/>气泡完成，工具转圈

With the fix, step 3 (迟到最终回复) takes you to: 失败后收到迟到回复<br/>气泡失败，工具转圈.
