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

## 2026-07-08 补充：工具轮次终态快照回滚

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，有工具调用的轮次在每次模型动作结束时都会出现终态 `llm_output` 快照；这些快照不是整轮累计正文，而是单次模型调用片段。
- 现象：第 2 轮最后一个终态快照从已流式累计的 `contentLength=165` 回写为 `finalContentChars=144`；第 3 轮多次出现 `86 -> 64`、`106 -> 42`、`69 -> 27`、`356 -> 329`。这会让页面表现为工具后正文突然回退、最后再跳变。
- 原因：前端 reducer 之前把 `llm_output/final` 终态快照直接当成 authoritative message snapshot 覆盖 `message.content`；工具轮次里这个语义不成立，`answer/final_response` 或模型动作终态常常只代表当前片段。
- 优化：`llm_output/final` 快照只在确实扩展当前正文时替换；如果快照比当前正文短，或只是当前正文的尾部/子串，则保留已流式累计正文。发送流和继续/恢复流都纳入 `final` 终态诊断与尾部平滑，恢复流额外记录 `resume-terminal` 和 `resume-terminal-tail-smoothing-plan`。
- 回归验证：`npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-render-adapter`、`npm run test:message-render`、`npm run test:realtime-pulse`、`npm run typecheck` 通过。
- 回退结论：如果后续仍有最后一段卡顿，优先看 `llm-terminal/resume-terminal` 中 `finalContentChars` 与 `projectedBefore.contentLength` 的关系；若终态快照短于投影但页面仍跳变，说明还有其他覆盖路径绕过了 runtime reducer。

## 2026-07-08 补充：正文显示 flush 延迟

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，终态快照已不再回滚正文；第 2 轮终态 `finalContentChars=123`，投影已是 `contentLength=138`，第 3 轮终态 `finalContentChars=391`，投影已是 `contentLength=439`，均被正确保留。
- 仍可见卡顿的原因拆分：第 3 轮存在 `gapMs=56212` 和 `gapMs=9358`，表示工具/本地模型下一次动作期间上游长时间没有正文 delta；这部分前端无法变成真实流式。与此同时，前端仍有 `plain-text-slow-flush=51-130ms`、`content-clock-slow-flush=68-144ms`，说明收到 delta 后还有显示延迟。
- 优化：纯文本流式正文不再等待 `requestAnimationFrame` 后写入 `visiblePlainText`，收到 projection content tick 后同步更新文本节点；runtime projection content clock 改为 8ms timer，避免本地推理占用主线程时 rAF 延迟；`MessageMarkdownBody` 按 messageId 订阅 content clock 并直接读取 runtime projection 正文，外层 renderable 消息列表不订阅 token 级 content clock，让头像、统计、滚动、工作目录和调试不再随每个 token 重建。
- 回归验证：`npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-render-adapter`、`npm run test:chat-runtime-projection-version`、`npm run test:messenger-renderable-source`、`npm run test:realtime-pulse`、`npm run test:message-render`、`npm run typecheck` 通过。
- 回退结论：后续如果 `send-content-event.gapMs` 仍是多秒级，瓶颈在工具执行、本地模型 prefill 或上游 chunk 转发；如果 `gapMs` 正常但仍卡，应优先看 `plain-text-slow-flush`、`content-clock-slow-flush` 是否继续出现。

## 2026-07-08 补充：final_response 工具参数可见流桥接

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，会话 `sess_20711044aad444c7bbd0d7393ea5f069`。第三轮在工具调用后出现 `send-content-event` 空窗：`eventId=211 -> 212` 间隔约 56213ms，随后 `eventId=212 -> 239` 仍间隔约 9358ms；后续 218 个 `llm_output_delta` 基本能以 15-35ms 推进，说明中后段前端渲染已自然，卡顿主要来自上游/后端在工具参数阶段没有产生可见文本增量。
- 优化：后端 LLM SSE 解析层新增 `final_response`/`最终回复` 预览桥接，支持 OpenAI Chat Completions、Responses API 与 Anthropic `input_json_delta` 三类工具参数流；从 `content`、`answer`、`message` 字段提取增长文本并通过现有 `on_delta` 发给前端。该预览只用于可见流，不追加到 LLM `combined` 内容，避免改变最终工具调用、历史和统计语义。
- 前端配合：外层 renderable 消息列表不订阅 `runtimeProjectionContentVersion`，token 级更新只推动对应 `MessageMarkdownBody`；纯文本流式正文同步写入轻量文本节点，content clock 使用短 timer，减少头像、外部形象和工作目录被 token 级更新拖慢。
- 回归验证：新增后端单测覆盖 OpenAI 兼容流、Responses API 和 Anthropic 输入 JSON 的最终回复工具参数分片；前端回归继续断言 renderable 控制器不订阅内容时钟。
- 回退结论：若后续 debug 的 `send-content-event.gapMs` 仍达到数秒以上，需要继续看上游本地模型 prefill、工具执行耗时或模型服务 chunk 策略；若 gap 正常但 `plain-text-slow-flush/content-clock-slow-flush` 继续出现，则优先检查前端文本节点 flush 和父级订阅是否回归。

## 2026-07-08 补充：空白气泡与终态覆盖

- 样本：`C:\Users\sjxx\Desktop\debug.txt`，第二轮同一模型轮次中先出现两个中间 `llm_output` 片段，最终 `finalContentChars=135`，但修复前投影 `contentLength=183`，说明中间预览没有被最终答案清掉。
- 问题：为了让工具后的 `final_response` 参数也能可见流式输出，前端会先把这些预览 delta 写入当前 assistant；如果 `final` 事件继续沿用保守合并策略，最终答案会被当成“短快照”忽略，导致中间内容残留。同时空 `streaming` assistant 被挂载为正文气泡，用户发送后会先看到白框。
- 优化：正文气泡只在存在真实可显示正文或失败提示时挂载，空 waiting/streaming/tooling 不再生成白框；`source_event_type=final` 的 `assistant_final` 作为整轮权威结果直接覆盖正文，普通 `llm_output` 仍保持片段快照保护，避免工具轮次回滚。
- 回归验证：新增 `final stream snapshot replaces accumulated final-response previews`，覆盖中间预览加最终 `final` 的投影结果；并验证 `npm run test:messenger-renderable-source`、`npm run test:chat-runtime-reducer`、`npm run test:chat-runtime-projection-version`、`npm run test:message-render`、`npm run typecheck`、`cargo test -p wunder-runtime --lib final_response -j 8`、`cargo check -p wunder-runtime -j 8` 通过。
- 回退结论：如果后续仍看到最终气泡拼入中间内容，优先检查最终事件是否以 `eventType=final` 进入 canonical reducer；如果只是生成过程中短暂显示预览但最终正确覆盖，说明是可见预览链路本身在工作，不应再用 `llm_output` 片段快照覆盖整轮正文。
