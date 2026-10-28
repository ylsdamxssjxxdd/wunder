## 1. 无效编码或损坏事件：请求已结清，返回成功

Rule broken: Invalid UTF-8 or JSON is not concealed by a later DONE (services/llm.rs:1919; stream_decode.rs:20).
Why: 损坏内容被静默跳过.
In the model, one step leads into this bug: "返回正文与工具提议".

0. Start: 无效编码或损坏事件：收到响应
1. 按网络分块读取字节. Now: 无效编码或损坏事件：读取分块完成
2. 验证编码和错误封装. Now: 无效编码或损坏事件：准备返回结果
3. 返回正文与工具提议. Now: 无效编码或损坏事件：请求已结清，返回成功

With the fix, step 2 (验证编码和错误封装) takes you to: 无效编码或损坏事件：识别为故障.

## 2. 成功状态携带错误：请求已结清，返回成功

Rule broken: A failed provider envelope cannot be returned as a successful answer (services/llm.rs:1024).
Why: 错误封装被忽略.
In the model, one step leads into this bug: "返回正文与工具提议".

0. Start: 成功状态携带错误：收到响应
1. 按网络分块读取字节. Now: 成功状态携带错误：读取分块完成
2. 验证编码和错误封装. Now: 成功状态携带错误：准备返回结果
3. 返回正文与工具提议. Now: 成功状态携带错误：请求已结清，返回成功

With the fix, step 2 (验证编码和错误封装) takes you to: 成功状态携带错误：识别为故障.

## 3. 正常回复含空错误字段：请求已结清，错误拒绝正常回复

Rule broken: A completed response with a null error field remains successful (services/llm/failure.rs:102).
Why: 空字段被当作故障.
In the model, one step leads into this bug: "错误码优先分类".

0. Start: 正常回复含空错误字段：收到响应
1. 按网络分块读取字节. Now: 正常回复含空错误字段：读取分块完成
2. 验证编码和错误封装. Now: 正常回复含空错误字段：识别为故障
3. 错误码优先分类. Now: 正常回复含空错误字段：请求已结清，错误拒绝正常回复

With the fix, step 2 (验证编码和错误封装) takes you to: 正常回复含空错误字段：准备返回结果.

## 4. 永久拒绝含误导正文：请求已结清，错误触发上下文恢复

Rule broken: A permanent provider code takes priority over misleading message text (orchestrator/llm.rs:1335).
Why: 正文覆盖永久错误码.
In the model, one step leads into this bug: "错误码优先分类".

0. Start: 永久拒绝含误导正文：收到响应
1. 按网络分块读取字节. Now: 永久拒绝含误导正文：读取分块完成
2. 验证编码和错误封装. Now: 永久拒绝含误导正文：识别为故障
3. 错误码优先分类. Now: 永久拒绝含误导正文：请求已结清，错误触发上下文恢复

With the fix, step 3 (错误码优先分类) takes you to: 永久拒绝含误导正文：请求已结清，明确永久拒绝.

## 5. 中文跨网络分块：请求已结清，文字或参数已损坏

Rule broken: Split network chunks do not change the successful text or tool arguments (stream_decode.rs:9).
Why: 逐块替换了半个字符.
In the model, one step leads into this bug: "返回正文与工具提议".

0. Start: 中文跨网络分块：收到响应
1. 按网络分块读取字节. Now: 中文跨网络分块：读取分块完成
2. 验证编码和错误封装. Now: 中文跨网络分块：准备返回结果
3. 返回正文与工具提议. Now: 中文跨网络分块：请求已结清，文字或参数已损坏

With the fix, step 3 (返回正文与工具提议) takes you to: 中文跨网络分块：请求已结清，返回成功.
