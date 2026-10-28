## 1. 服务暂不可用：未退避又请求一次

Rule broken: Transient HTTP failures cannot send another request before engine backoff (services/llm.rs:1454).
Why: transient.
In the model, one step leads into this bug: "绕过退避切换非流式".

0. Start: 服务暂不可用：准备请求
1. 通过取消闸后请求. Now: 服务暂不可用：请求在途
2. 收到故障或不完整流. Now: 服务暂不可用：收到暂时故障
3. 绕过退避切换非流式. Now: 服务暂不可用：未退避又请求一次

With the fix, step 3 (绕过退避切换非流式) cannot happen, so you stay at: 服务暂不可用：收到暂时故障.

## 2. 流中断：不完整流被当作成功

Rule broken: EOF without a terminal marker or finish reason cannot be accepted (services/llm.rs:1193).
Why: truncated.
In the model, one step leads into this bug: "分类故障并决定结清或重试".

0. Start: 流中断：准备请求
1. 通过取消闸后请求. Now: 流中断：请求在途
2. 收到故障或不完整流. Now: 流中断：缺少完成标记
3. 分类故障并决定结清或重试. Now: 流中断：不完整流被当作成功

With the fix, step 3 (分类故障并决定结清或重试) takes you to: 流中断：可取消的退避.

## 3. 鉴权失败：错误地重试同一鉴权请求

Rule broken: A permanent rejection never schedules another identical request (llm.rs:1302).
Why: auth.
In the model, one step leads into this bug: "分类故障并决定结清或重试".

0. Start: 鉴权失败：准备请求
1. 通过取消闸后请求. Now: 鉴权失败：请求在途
2. 收到故障或不完整流. Now: 鉴权失败：收到拒绝
3. 分类故障并决定结清或重试. Now: 鉴权失败：错误地重试同一鉴权请求

With the fix, step 3 (分类故障并决定结清或重试) takes you to: 鉴权失败：明确失败.

## 4. 工具失败后询问模型：未退避又请求一次

Rule broken: Transient HTTP failures cannot send another request before engine backoff (services/llm.rs:1454).
Why: tool.
In the model, one step leads into this bug: "绕过退避切换非流式".

0. Start: 工具失败后询问模型：准备执行工具
1. 执行一次工具. Now: 工具失败后询问模型：工具返回失败
2. 保留失败结果及调用身份. Now: 工具失败后询问模型：准备请求
3. 通过取消闸后请求. Now: 工具失败后询问模型：请求在途
4. 收到故障或不完整流. Now: 工具失败后询问模型：收到暂时故障
5. 绕过退避切换非流式. Now: 工具失败后询问模型：未退避又请求一次

With the fix, step 5 (绕过退避切换非流式) cannot happen, so you stay at: 工具失败后询问模型：收到暂时故障.
