# golutra 多智能体协同借鉴分析

## 1. 结论先行

`golutra` 做得好的地方，不是“多智能体更多”，而是把 **多智能体协同的控制面、执行面、可视化面** 串成了一条闭环：

- **控制面**：用户通过聊天/mention/成员选择发起协同，系统自动把消息分发给目标智能体。
- **执行面**：每个智能体背后绑定一个终端执行会话，支持并行、串行、缓冲、重试与语义 flush。
- **可视化面**：前端实时看到“谁在执行、谁在流式输出、谁已完成、点谁可以继续干预”。

而 `wunder` 当前的问题不是没有能力，恰恰相反，`subagent_control`、`agent_swarm`、`a2a观察/a2a等待`、`team_runs`、`agent_runtime`、流式事件持久化这些基础能力已经比较全；真正缺的是：

1. **协同触发策略不够上层化**：模型要自己判断何时用子智能体、何时用蜂群、何时用 A2A，心智负担大。
2. **前端展示仍偏“事件日志视角”**：已经有 team/subagent 事件，但没有形成“协同工作台”。
3. **会话层级关系没有真正进入交互层**：后端已经记录 `parent_session_id / spawn_label / spawned_by`，但前端没有把这些关系用起来。

所以，最值得借鉴 `golutra` 的，不是照搬终端形态，而是借鉴它的 **“协同即产品主路径”** 的设计方式。

---

## 2. golutra 为什么看起来“协同感很强”

## 2.1 协同入口非常自然

从代码和 README 看，`golutra` 的协同入口是“聊天消息 -> 目标成员 -> 终端分发”：

- 在描述层，它强调的是“parallel execution + orchestration + real-time tracking”。
- 在实现层，聊天消息可根据 `dm` 或 `mention` 自动解析目标成员，再下发到对应终端。
- 这意味着用户不需要先理解复杂工具树，只要“对谁说话”或“@谁”即可触发协同。

对应代码：

- `C:\Users\32138\Desktop\参考项目\golutra-master\README.md:67`
- `C:\Users\32138\Desktop\参考项目\golutra-master\README.md:69`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\orchestration\dispatch.rs:76`

### 对 wunder 的启发

`wunder` 当前更像“模型可调用的多智能体工具集合”，而不是“用户可直接操控的协同界面”。

这会导致两个问题：

- 用户不清楚系统到底有没有真正调度多个智能体。
- 即便调度发生了，用户也难以形成稳定心智模型。

`wunder` 需要补一个 **协同入口层**：

- 用户直接选择“单智能体 / 子智能体 / 蜂群 / 外部 A2A”。
- 或者在智能体配置中声明“该智能体默认使用协同模式”。
- 对模型端则保留现有工具，但前端应把协同触发从“纯工具细节”提升为“显式交互动作”。

---

## 2.2 执行面与展示面是同一条链路

`golutra` 的一个关键设计是：**流式输出与最终落库都围绕同一份 terminal chat payload**。

- `semantic_worker` 负责从终端语义快照中提取 stream/final。
- `message_pipeline` 同时承担“流式发前端”和“最终写回聊天记录”。
- 前端 `chatStore` 会先把流式内容作为临时消息显示，再在 final 时替换成正式消息。

对应代码：

- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\terminal_engine\session\semantic_worker.rs:43`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\terminal_engine\session\semantic_worker.rs:201`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\ui_gateway\message_pipeline.rs:26`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\ui_gateway\message_pipeline.rs:43`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src\features\chat\chatStore.ts:348`

### 对 wunder 的启发

`wunder` 现在也有很强的流式事件系统：

- `team_start / team_task_dispatch / team_task_update / team_task_result / team_merge / team_finish / team_error` 已经持久化。
- 会话还保留了 `parent_session_id / spawn_label / spawned_by`。

对应代码：

- `src/orchestrator/event_stream.rs:149`
- `src/orchestrator/event_stream.rs:151`
- `src/orchestrator/event_stream.rs:152`
- `src/orchestrator/event_stream.rs:153`
- `src/api/chat.rs:1744`
- `src/api/chat.rs:1760`

但问题在于，前端没有把这条数据链路转译成“可理解的协同界面”，而只是转成通用 workflow item。

目前 `team_*` 事件在前端只是这样处理：

- 按事件名生成一个通用工作流条目。
- 状态只分成 `loading/completed/failed`。
- 没有任务泳道、没有 agent 维度、没有子会话跳转、没有合并摘要结构化展示。

对应代码：

- `frontend/src/stores/chat.ts:3993`
- `frontend/src/stores/chat.ts:4000`

这就是为什么后端“已经协同了”，用户却仍然“感受不到协同”。

---

## 2.3 它把“目标成员”做成了稳定对象，而不是临时事件

在 `golutra` 里，成员/终端是长期对象：

- 有 member id。
- 有 terminal session。
- 有状态。
- 可从头像点击进入具体终端。

对应代码：

- `C:\Users\32138\Desktop\参考项目\golutra-master\src\features\chat\ChatInterface.vue:806`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src\stores\terminalOrchestratorStore.ts:215`

这种设计非常重要，因为它让“协同对象”不只是一次 tool call，而是前端能长期呈现、复用、观察、二次干预的实体。

### 对 wunder 的启发

`wunder` 当前已有这些实体基础：

- `agent_threads`
- `agent_tasks`
- `team_runs`
- `team_tasks`
- `chat_sessions`

但在前端交互上，它们还没有被提升为稳定对象。尤其是：

- 子会话创建后，前端没有把它当成“可展开节点”。
- 蜂群任务创建后，前端没有形成“run -> tasks -> spawned session”的树。
- A2A 任务也没有跟本地 run/task 进入同一视图。

因此用户感知到的依然是“一个聊天窗口里的若干事件块”，而不是“一个正在协同工作的团队”。

---

## 2.4 它非常重视调度节流与执行边界

`golutra` 的 `chat_dispatch_batcher` 很值得借鉴。它不是简单地“有消息就发”，而是：

- 按 terminal 建立队列。
- 同上下文消息合并。
- 等待 semantic flush 完成后再放行下一批。
- 避免重复 message id 冲突。

对应代码：

- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\orchestration\chat_dispatch_batcher.rs:68`
- `C:\Users\32138\Desktop\参考项目\golutra-master\src-tauri\src\orchestration\chat_dispatch_batcher.rs:148`

### 对 wunder 的启发

`wunder` 在资源治理上其实已经有更好的底座：

- `agent_runtime` 的主线程/排队。
- `team_run_runner` 的并发限制与取消。
- swarm policy 的 `max_active_team_runs/max_parallel_tasks_per_team/max_retry`。

对应代码：

- `src/api/team_runs.rs:34`
- `src/services/swarm/runner.rs:260`
- `src/services/swarm/runner.rs:698`

但当前的不足在于：

- **调度策略对模型可见，但对用户不可见。**
- **调度状态有事件，但没有前端的“调度面板”。**

也就是说，`wunder` 已经有“调度引擎”，但缺“调度驾驶舱”。

---

## 3. wunder 现状：能力已具备，但呈现断层明显

## 3.1 后端能力其实已经很强

从代码看，`wunder` 的多智能体后端基础并不弱，甚至比 `golutra` 更通用：

### 已有能力

1. **子会话/子智能体**
- `subagent_control` 支持 `list/history/send/spawn`。
- `spawn` 会创建子会话，并在完成后向父会话追加 `subagent_announce`。

对应代码：

- `src/services/tools.rs:1309`
- `src/services/tools.rs:3206`

2. **智能体蜂群 / TeamRun**
- `agent_swarm` 支持 `list/status/send/history/spawn/batch_send/wait`。
- 文案也明确写了典型流程是 `batch_send -> wait`。
- `team_run_runner` 支持真正的并行任务执行、任务重试、汇总合并。

对应代码：

- `src/services/tools/catalog.rs:427`
- `src/services/swarm/runner.rs:260`
- `src/services/swarm/runner.rs:927`
- `src/services/swarm/runner.rs:1226`

3. **A2A 协同**
- `a2a观察 / a2a等待` 已经存在，适合外部智能体联动。

对应代码：

- `src/services/tools/catalog.rs:212`
- `src/services/tools/catalog.rs:227`

4. **事件持久化与回放**
- team 事件已经进入通用流式事件体系。
- SSE/WS 重连后仍可回放，适合前端做协同 timeline。

对应代码：

- `src/orchestrator/event_stream.rs:149`
- `src/api/chat.rs:2176`

### 现状判断

所以 `wunder` 当前不是“后端不能协同”，而是“后端协同没有形成清晰产品路径”。

---

## 3.2 前端的主要问题不在渲染能力，而在视图抽象层级

### 问题一：蜂群事件被降级成普通 workflow item

当前 `chat.ts` 里，`team_*` 事件仅转成普通条目，没有结构化建模。

对应代码：

- `frontend/src/stores/chat.ts:3993`

结果是：

- 看不出 run 与 task 的关系。
- 看不出 task 属于哪个 agent。
- 看不出哪个 task 派生了哪个 session。
- 看不出 merge 是在汇总什么。

### 问题二：已有 `SwarmPanel`，但实际上没有接入主界面

前端已经有一个专门的蜂群面板组件，可以拉取 run 和 tasks：

- `frontend/src/components/chat/SwarmPanel.vue:2`
- `frontend/src/components/chat/SwarmPanel.vue:72`

但我没有在 `frontend/src` 里找到它被实际引入到主链路中。这意味着团队任务详情能力实际上处于“写了组件，但不在主路径上”的状态。

### 问题三：子会话关系存在于后端，但没有进入前端心智模型

后端 session payload 已带：

- `parent_session_id`
- `parent_message_id`
- `spawn_label`
- `spawned_by`

对应代码：

- `src/api/chat.rs:1744`

但是前端 session 侧的主要组织方式，仍是按 agent 选“主会话”聚合，而不是按 parent-child 形成树。

对应代码：

- `frontend/src/views/MessengerView.vue:3211`

这会直接导致：

- 子会话被埋没在普通 session 列表中。
- 用户无法从父消息跳到子会话。
- “协同路径”无法被观察和回放。

### 问题四：`subagent_announce` 只有后端写入，没有前端专门解析

后端明确写入了 `meta.type = subagent_announce`。但我没有在 `frontend/src` 里找到对应的专用消费逻辑。

这意味着：

- 子智能体完成后虽然会回传结果，
- 但前端并没有把它提升成“子任务完成卡片”或“可点击跳转的回执卡”。

### 问题五：A2A、本地子智能体、蜂群各自为政

现在三类协同能力在后端是存在的，但前端没有统一视角：

- 子智能体像“子会话工具”。
- 蜂群像“team run API + 若干 workflow 事件”。
- A2A 像“远端工具调用结果”。

用户看到的是三套机制，而不是一套“协同系统”。

---

## 4. 真正值得借鉴 golutra 的五个方向

## 4.1 借鉴一：引入“协同控制面”而不是继续堆工具

### 建议

在 `wunder` 现有 orchestrator 之上，加一层轻量的 **Coordination Layer（协同控制层）**，不要让模型直接裸用三套机制。

这层不一定一开始就需要独立 LLM，可以先做规则驱动：

- **子智能体模式**：适合单一深挖任务、需要保留上下文连续性的情况。
- **蜂群模式**：适合多角色并行拆解、统一回收结果。
- **A2A 模式**：适合调用远端专业能力或跨系统代理。

### 推荐做法

新增一个统一的“协同计划”抽象，例如：

- `coordination_run`
- `coordination_nodes`
- `coordination_edges`

先不替换现有 `team_runs` / `subagent_control` / `a2a_*`，而是做一个上层映射：

- `subagent` = 一种 node
- `swarm task` = 一种 node
- `a2a task` = 一种 node
- `merge` = 一种 node

这样前端才有可能用统一模型展示本地与远端协同。

---

## 4.2 借鉴二：让“协同对象”成为可点击实体

`golutra` 的核心体验不是日志，而是“点头像看执行”。

`wunder` 可以把协同对象定义为三类实体：

1. **智能体节点**：agent card
2. **任务节点**：task card
3. **会话节点**：child session / spawned session

### 前端应支持的动作

- 点击任务卡：展开详情、耗时、状态、摘要、错误。
- 点击 agent：查看该 agent 最近主会话与历史任务。
- 点击 child session：跳转子会话详情。
- 点击 A2A task：查看 endpoint/service_name 与状态。

这类交互会明显提升“团队在工作”的感知强度。

---

## 4.3 借鉴三：把流式协同做成“泳道视图”，不要只做列表

当前 team 事件已经足够支持前端构造基础泳道。

### 最低可行展示（建议优先做）

在当前助手消息下方新增 **协同卡片**：

- 顶部：`本轮协同中` / `已完成` / `失败`
- 左侧：run 级摘要（策略、并行数、耗时、merge policy）
- 中间：按 agent 分 lane 展示 task
- 右侧：结果摘要 / 失败原因 / 跳转按钮

### 数据来源

- `team_start`：创建协同卡片
- `team_task_dispatch`：创建 lane item
- `team_task_update`：更新状态
- `team_task_result`：补全摘要与 child session
- `team_merge`：渲染汇总
- `team_finish`：收口状态
- `team_error`：标红错误

这部分完全可以基于现有事件实现，不需要等后端大改。

---

## 4.4 借鉴四：把“会话树”真正做出来

`wunder` 的会话已经有 parent-child 关系，所以应该在前端明确做两层结构：

### 左侧会话列表建议改造

当前：

- 以 agent 的主会话为主。

建议：

- 主会话下可展开子会话。
- 子会话旁显示：来源标签（`spawn_label`）、创建方式（`spawned_by`）、运行状态。
- 若子会话属于某个 team run，则显示 run 标记。

### 右侧详情建议改造

- 在父会话消息中，子任务完成回执卡支持“一键打开子会话”。
- 在子会话中，支持“返回父会话”。

这样用户才能真正追踪协同链路。

---

## 4.5 借鉴五：让系统主动推荐正确协同模式

现在工具虽然齐全，但模型和用户都容易“选错武器”。

### 建议补充协同策略提示

在工具说明与系统提示中显式加入如下规则：

- **单任务深挖**：优先 `subagent_control.spawn`
- **多角色并行**：优先 `agent_swarm.batch_send -> wait`
- **外部服务协同**：优先 `a2a@service + a2a_wait`
- **需要汇总归并**：优先 swarm，不要多个独立 subagent
- **需要长期线程复用**：优先 `agent_swarm.send/history/status`

这会比单纯暴露工具 schema 更有效。

---

## 5. 对 wunder 的具体落地建议

## 5.1 第一阶段：只做前端，不动后端协议

### 目标

先把现有后端能力“看得见”。

### 建议事项

1. **把 `team_*` 事件从 generic workflow item 升级为结构化协同视图。**
2. **正式接入 `SwarmPanel`，但不要作为独立孤岛，而是作为消息内卡片/侧边抽屉。**
3. **为 `subagent_announce` 加专门渲染卡片。**
4. **在 session list 中接入 parent-child 展示。**

### 预期收益

- 不改后端协议就能显著提升“协同感”。
- 能快速验证用户是否真的愿意使用多智能体链路。

---

## 5.2 第二阶段：补统一协同视图模型

### 建议新增前端模型

在现有 `workflowItems` 旁新增一种专门结构，例如：

- `coordinationItems`

建议字段：

- `coordinationId`
- `kind`：`subagent | swarm | a2a | merge | session_link`
- `nodeId`
- `parentNodeId`
- `agentId`
- `sessionId`
- `status`
- `title`
- `summary`
- `startedAt`
- `finishedAt`
- `elapsedS`
- `openTarget`

然后把 `team_*`、`subagent_announce`、`a2a_*` 都映射进同一套前端模型。

---

## 5.3 第三阶段：后端补协同聚合接口

### 建议新增接口

1. `GET /wunder/chat/sessions/{session_id}/children`
- 返回子会话树与运行状态。

2. `GET /wunder/chat/sessions/{session_id}/coordination`
- 返回该父会话下所有 `subagent/team/a2a` 协同摘要。

3. `GET /wunder/chat/team_runs/{team_run_id}/graph`（可选）
- 返回 run/task/session 的聚合图。

### 原则

- 不要让前端自己把多来源事件硬拼成全部视图。
- 前端保留流式增量渲染，但首屏应能一次性拿到聚合快照。

---

## 5.4 第四阶段：引入“协同指挥官”

这是最重要但也最晚做的一步。

### 目标

让系统从“有多个协同工具”升级为“能自主选择协同工具”。

### 方式

先做规则版，再考虑模型版：

- 若用户任务包含多个明确子目标、多个文件/模块、多个角色，则建议或自动进入 swarm。
- 若任务是深挖某个问题、需要独立上下文，则建议或自动进入 subagent。
- 若任务需要外部系统能力，则建议 A2A。

### 注意

这层不要一开始就写进 `src/services/tools.rs` 或 `frontend/src/views/MessengerView.vue` 这种超大文件里，应该新建模块承载。

---

## 6. 我对 wunder 的推荐路线

## 路线判断

**不建议模仿 golutra 的“终端就是智能体实体”实现形态；建议借鉴它的“协同可见、协同可控、协同可回放”产品路线。**

原因：

- `golutra` 是桌面优先、终端优先，适合把 terminal 当执行实体。
- `wunder` 是 server/cli/desktop 三形态协同，必须保持后端统一抽象，不能把产品心智绑死在终端上。

所以 `wunder` 最合适的借鉴方式是：

1. **保留现有后端执行抽象**（session / run / task / a2a）。
2. **补一个统一协同控制层**。
3. **前端升级成协同工作台，而非事件日志面板。**

---

## 7. 最小可执行改造清单

## R1：高优先级（建议马上做）

1. `team_*` 事件前端结构化渲染。
2. `subagent_announce` 前端专门卡片化。
3. `SwarmPanel` 接入聊天主路径。
4. 左侧会话支持 parent-child 展开。

## R2：中优先级

1. 新增协同聚合接口。
2. 前端统一 `coordinationItems` 模型。
3. 将 A2A 纳入同一协同面板。

## R3：中长期

1. 协同策略引擎。
2. 智能体角色模板与自动分工。
3. 协同回放/复盘模式。

---

## 8. 一句话总结

`golutra` 的成功点在于：**把多智能体协同做成了用户能感知、能介入、能追踪的主体验。**

`wunder` 当前已经具备更强的后端协同能力，但前端还停留在“事件存在、对象缺席”的阶段。下一步最应该做的，不是再造新工具，而是把已有的 `subagent + swarm + a2a` 收束为一套统一的协同产品视图。
