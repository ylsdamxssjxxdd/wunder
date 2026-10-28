## 1. 工具超时，却建议可直接重试

Rule broken: A timed out, unverified side effect must not be advertised as safe to retry.
In the model, one step leads into this bug: "等待超时并返回错误建议". The steps below are the shortest path that TLC found.

0. Start: 命令执行中
1. 等待超时并返回错误建议. Now: 工具超时，却建议可直接重试

With the fix, step 1 (等待超时并返回错误建议) takes you to: 工具超时，明确结果未知并要求核对.

## 2. 未核对结果，副作用重复发生

Rule broken: A client following the advice must not repeat a completed side effect without verification.
In the model, one step leads into this bug: "调用者按建议重试".

0. Start: 命令执行中
1. 外部命令先完成一次写入. Now: 命令已写入，尚未返回
2. 等待超时并返回错误建议. Now: 工具超时，却建议可直接重试
3. 调用者按建议重试. Now: 未核对结果，副作用重复发生

With the fix, step 2 (等待超时并返回错误建议) takes you to: 工具超时，明确结果未知并要求核对.
