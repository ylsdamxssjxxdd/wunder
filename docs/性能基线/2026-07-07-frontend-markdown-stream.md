# 2026-07-07 frontend markdown-stream 性能对比

## 目标

- 优化链路：用户侧聊天气泡普通文本流式输出。
- 风险点：每个文本增量触发 Markdown 重渲染、父页面布局刷新和 DOM 资源扫描，抢占主线程后影响头像动画、工作目录交互和流式输出顺滑度。

## 环境

- 代码状态：本地工作区含用户侧前端实时渲染与工具工作流相关未提交改动。
- 运行方式：Node 微基准 + 前端回归脚本。
- 浏览器/客户端：本次未采集浏览器 Performance trace。
- 关键配置：普通文本流式阶段走轻量文本渲染，最终状态再渲染 Markdown；投影版本更新加入 24ms 合并窗口。

## 采样方法

- 预热：同一进程内重复 5 组，取中位数。
- 数据规模：420 段普通文本增量。
- 入口：`renderMarkdown` CPU 路径微基准，对比“每段都渲染 Markdown”和“流式文本直出、最终渲染一次 Markdown”。

## 对比结果

| 指标 | 基线 | 优化后 | 变化 | 结论 |
| --- | ---: | ---: | ---: | --- |
| 普通文本 420 段流式渲染 CPU 路径中位数 | 40.14ms | 0.22ms | -39.92ms | 通过 |

## 细分瓶颈

| 子步骤 | 基线 | 优化后 | 变化 | 说明 |
| --- | ---: | ---: | ---: | --- |
| 每段增量 Markdown 渲染 | 420 次 | 0 次 | 大幅减少 | 普通文本流式阶段直接文本渲染 |
| 最终 Markdown 渲染 | 已包含在每段重渲染中 | 1 次 | 收敛 | 输出结束或出现 Markdown 结构后再走 Markdown |
| 最新消息资源扫描 | 跟随内容长度变化 | 仅资源/工作流变化或正文组件报告需要补水 | 减少无效 DOM 查询 | 避免普通文本触发资源卡片扫描 |

## 回归验证

- `npm run typecheck`
- `npm run test:messenger-renderable-source`
- `npm run test:chat-runtime-projection-version`
- `npm run test:message-render`
- `npm run test:message-viewport-runtime`
- `npm run test:chat-runtime-reducer`
- `npm run test:chat-runtime-render-adapter`
- `node ../node_modules/esbuild/bin/esbuild scripts/regression/tool-workflow-command-regression.test.ts --bundle --platform=node --format=cjs --alias:@=./src --outfile=../temp_dir/frontend-tests/tool-workflow-command-regression.cjs; node ../temp_dir/frontend-tests/tool-workflow-command-regression.cjs`
- `node ../node_modules/esbuild/bin/esbuild scripts/regression/tool-workflow-regression.test.ts --bundle --platform=node --format=cjs --alias:@=./src --outfile=../temp_dir/frontend-tests/tool-workflow-regression.cjs; node ../temp_dir/frontend-tests/tool-workflow-regression.cjs`

## 结论

- 判定：通过微基准和回归验证。
- 理由：普通文本流式阶段减少重复 Markdown 解析和父级资源扫描，投影刷新节奏从逐帧改为 24ms 合并窗口。
- 后续观察：仍需在真实浏览器 Performance trace 中确认头像动画帧耗时、工作目录交互延迟和长消息自动滚动表现。

## 2026-07-07 补充：父级投影刷新隔离

- 问题：仅跳过 Markdown 重渲染后，普通文本 delta 仍会递增 `runtimeProjectionVersion`，导致 Messenger 父级消息列表、头像状态、工作流面板、工作目录和浮动形象一起重新计算。
- 优化：稳态 `assistant_delta/assistant_reasoning_delta` 只递增 `runtimeProjectionContentVersionByMessage[messageId]`；`MessageMarkdownBody` 按 message id 直接读取 runtime projection 正文；物化消息缓存不再把 `content/reasoning/updatedSeq` 作为外层对象重建条件。
- 额外处理：普通文本 DOM 写入合并到 `requestAnimationFrame`；浮动形象完成提示改读 runtime projection，避免扫描 legacy `chatStore.messages`。
- 回归：新增断言确认普通文本续写不 bump `runtimeProjectionVersion`，首 token 和结构变化仍刷新父级。
- 验证：`npm run typecheck`、`npm run test:chat-runtime-projection-version`、`npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-render-adapter`、`npm run test:messenger-renderable-source`、`npm run test:message-render`、`npm run test:message-viewport-runtime` 均通过。

## 2026-07-07 补充：本地模型卡顿实测诊断

- 样本：`sess_21428bf2fef24493829c801d05781e29` 的导出 JSONL 与 debug 日志。
- 后端时间线：第一轮 `llm_output` 的 `ttft_ms=10412`、`prefill_duration_s=10.4119`；第二轮最终回复 `ttft_ms=3731`、`prefill_duration_s=3.7310`。这说明本地模型首 token 慢是可见等待的主要来源之一。
- 前端时间线：debug 中出现 12 次 `content-clock-slow-flush`，延迟约 48-63ms；2 次 `plain-text-slow-flush`，约 50-52ms。慢点集中在本地模型流式阶段，不是 Markdown 慢渲染。
- 优化：`chat.stream.perf` 与大 payload 调试日志保留到 debug history，但不再直接输出完整对象到 console；流式纯文本只更新正文显示，布局测量降频为轻量事件，最终或 Markdown 分支再完整测量。
- 诊断增强：后端 `llm_output.stream_timing` 新增 `chunk_count`、`content_delta_chars`、`reasoning_delta_chars`、`prefill_ms`、`decode_ms`、`max_chunk_gap_ms`，用于判断本地模型是否将多个 delta 攒成突发 chunk。
- 回归验证：`npm run typecheck`、`npm run test:messenger-renderable-source`、`npm run test:chat-runtime-projection-version`、`npm run test:message-render`、`npm run test:message-viewport-runtime`、`npm run test:chat-runtime-render-adapter`、`cargo check -p wunder-runtime -j 8` 均通过。
- 回退结论：若本地模型服务本身 `prefill_ms` 或 `max_chunk_gap_ms` 很大，前端只能降低主线程放大效应，不能把上游没有吐出的 token 变成真实流式；应进一步从模型服务线程/GPU/CPU 资源隔离与上游 SSE chunk 策略排查。

## 2026-07-07 补充：发送流序号缺口诊断

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，会话 `sess_3eeaeff5f73c404eb962bcd61e3a7c0e`。
- 现象：整体流式明显改善，但最后一轮从 `05:01:12` 开始连续出现 `send_event_seq_gap` / `send_pending_event_seq_gap`，前端进入 `skip_interactive_stream`；到 `05:01:22` 发送结束时本地 assistant 占位仍是 `contentLength=0`，所以用户看到最终内容在终态/补水后一股脑出现。
- 原因：在线发送流把后端持久 `event_id/event_seq` 当成可渲染文本事件的严格连续序号。后端的状态、工具、诊断或被投影过滤的事件也会占用 id，文本 `llm_output_delta` 正常跳号时被误判为缺序。
- 追加样本：新的 debug 仍出现 10 次 `send_event_seq_gap`、26 次 `send_pending_event_seq_gap`，并且最后 `stream-finish` 时本地 assistant 仍是 `contentLength=0`；这说明仅放宽文本 delta 不够，发送阶段的状态、工具或终态事件也可能先触发 `syncRequired`，随后让可见文本进入补水保护。
- 优化：`phase=send` 的在线 WS 事件全部不再使用 `event_seq` 做严格连续校验；仍保留 `event_id` 去重、`lastAppliedEventId` replay cursor 和 watch/snapshot 的严格顺序保护。发送通道本身是同一条交互 WS 顺序流，缺失补偿应交给结束后的 watch/replay。
- 回归验证：新增 `send stream text deltas keep flowing across persisted event id gaps`，覆盖发送阶段 `thread_status/tool_call/llm_output_delta` 均正常跳号但不触发补水保护；并验证 `npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-projection-version`、`npm run test:chat-runtime-render-adapter`、`npm run test:messenger-renderable-source`、`npm run typecheck` 通过。

## 2026-07-07 补充：交互流期间后台脉冲让路

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，会话 `sess_8dfb95490133473a8d11cf2fd7a12dbc`。
- 现象：最新样本已经没有 `send_event_seq_gap` / `send_pending_event_seq_gap`，投影里第一轮最终 `contentLength=614`、第二轮最终 `contentLength=170`，说明发送流和投影推进已恢复；但输出期间仍每 2.5 秒左右触发 `realtime-pulse`，其中 `loadSessions` 多次耗时约 100-330ms，并伴随 `active-realtime-recovery-plan=skip_interactive_stream`。
- 原因：本地模型输出时 CPU/GPU 和浏览器主线程竞争更明显，后台会话列表刷新、运行中智能体刷新和元数据刷新虽不改变正文投影，却会触发请求、列表合并、排序和派生态更新，放大头像动画、浮动形象和工作目录的掉帧。
- 优化：`createMessengerRealtimePulse` 增加 `shouldDefer`，当前会话存在 send/resume controller 时整轮延后后台 pulse；`loadSessions` 与 `refreshRealtimeChatSessions` 也增加交互流保护，避免直接调用路径在发送中刷新全量会话列表。
- 诊断增强：发送结束 debug 同时记录本地占位 assistant 和 runtime projection 中的 `projectedLatestAssistant`；终态 `llm_output.stream_timing` 被提取为 `chat.stream.perf:llm-stream-timing`，用于区分上游 prefill/chunk 间隔和前端 flush 延迟。
- 回归验证：`npm run test:realtime-pulse`、`npm run test:messenger-renderable-source`、`npm run test:chat-runtime-render-adapter`、`npm run typecheck` 通过。
- 回退结论：如果后续 debug 仍显示最后一段一股脑出现，需要优先看新增的 `llm-stream-timing.maxChunkGapMs/prefillMs`；若这些值大，瓶颈在本地模型服务或上游 chunk 策略，而不是前端渲染。

## 2026-07-07 补充：终态快照尾部平滑

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，第二轮中间纯文本已自然渲染到 `contentLength=21`，随后约 25 秒没有新的内容刷新日志，最终 `stream-finish.projectedLatestAssistant.contentLength=235`，说明最后一段来自终态快照补齐。
- 优化：发送流在收到终态 `llm_output` 时记录 `llm-terminal`、`send-content-event` 和 `terminal-tail-smoothing-plan`，包含 delta 长度、终态内容长度、投影长度和内容事件间隔；当终态纯文本以当前投影内容为前缀且缺失尾部超过阈值时，先按帧合成 `llm_output_delta` 补齐尾部，再应用原始终态事件。
- 渲染补充：纯文本终态继续走轻量文本路径，避免最后从 streaming text 切换到 Markdown 全量渲染造成额外主线程抖动；含代码块、表格、链接、标题、列表、强调符号或公式的内容仍走原 Markdown 终态逻辑。
- 回归验证：`npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-render-adapter`、`npm run test:messenger-renderable-source`、`npm run test:realtime-pulse`、`npm run test:message-render`、`npm run typecheck` 通过。
- 回退结论：若后续 debug 中 `send-content-event` 显示终态前仍长时间没有真实 `llm_output_delta`，前端只能平滑显示终态尾部；真实流式连续性仍需继续检查本地模型服务的 chunk 输出和上游转发策略。
