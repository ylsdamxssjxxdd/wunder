## 1. 轮次已停止<br/>检查点已持久化<br/>存在持久检查点<br/>终态之后仍写入检查点

Rule broken: A checkpoint must not commit after the originating turn has settled.
Why: 检查点追加绕过轮次提交闸，停止后仍可写入.
In the model, one step leads into this bug: "持久提交检查点".

0. Start: 轮次执行中<br/>生成摘要<br/>没有持久检查点
1. 摘要生成完成. Now: 轮次执行中<br/>摘要已就绪<br/>没有持久检查点
2. 旧路径直接准备追加，未取得提交闸. Now: 轮次执行中<br/>检查提交权限<br/>没有持久检查点
3. 旧路径未核对轮次状态. Now: 轮次执行中<br/>等待持久化<br/>没有持久检查点
4. 停止并结清轮次. Now: 轮次已停止<br/>等待持久化<br/>没有持久检查点
5. 持久提交检查点. Now: 轮次已停止<br/>检查点已持久化<br/>存在持久检查点<br/>终态之后仍写入检查点

With the fix, step 2 (旧路径直接准备追加，未取得提交闸) happens as "取得线程提交闸" and takes you to: 轮次执行中<br/>检查提交权限<br/>没有持久检查点.
