## 1. 等待回答<br/>已受理回答：2<br/>消费身份：none<br/>进程在线

Rule broken: One inquiry must allocate at most one answer, even with different client keys.
Why: 只按客户端消息键去重，未绑定问询消费身份.
In the model, one step leads into this bug: "客户端提交回答（受理事务）". The steps below are the shortest path that TLC found.

0. Start: 等待回答<br/>已受理回答：0<br/>消费身份：none<br/>进程在线
1. 客户端提交回答（受理事务）. Now: 等待回答<br/>已受理回答：1<br/>消费身份：none<br/>进程在线
2. 客户端提交回答（受理事务）. Now: 等待回答<br/>已受理回答：2<br/>消费身份：none<br/>进程在线

With the fix, step 1 (客户端提交回答（受理事务）) takes you to: 等待回答<br/>已受理回答：1<br/>消费身份：a<br/>进程在线.

## 2. 问询已停止<br/>已受理回答：1<br/>消费身份：none<br/>进程在线<br/>停止后仍受理新回答

Rule broken: Stopping before any answer was accepted must reject a later answer.
Why: 受理前未检查原轮次是否仍等待输入.
In the model, one step leads into this bug: "客户端提交回答（受理事务）". The steps below are the shortest path that TLC found.

0. Start: 等待回答<br/>已受理回答：0<br/>消费身份：none<br/>进程在线
1. 停止问询. Now: 问询已停止<br/>已受理回答：0<br/>消费身份：none<br/>进程在线
2. 客户端提交回答（受理事务）. Now: 问询已停止<br/>已受理回答：1<br/>消费身份：none<br/>进程在线<br/>停止后仍受理新回答

With the fix, step 2 (客户端提交回答（受理事务）) cannot happen, so you stay at: 问询已停止<br/>已受理回答：0<br/>消费身份：none<br/>进程在线.
