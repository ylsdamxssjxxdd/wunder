---
title: 核心概览
summary: 用 `docs/总体设计.md` 的 11 个核心快速建立 Wunder 的整体运行模型；细分机制和旧概念页统一转入参考部分。
read_when:
  - 你第一次系统性理解 Wunder
  - 你准备做接入、运维或工具开发，需要统一视角
  - 你已经会用 Wunder，但想理解它为什么这样设计
source_docs:
  - docs/总体设计.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 核心概览

理解 Wunder，先不要把注意力分散到零碎术语。先抓住 `docs/总体设计.md` 定义的 11 个核心，再去看接口、工具和界面，整体会清楚很多。

旧的概念细页没有删除，而是归入 [参考概览](/docs/zh-CN/reference/)。这页只负责建立系统主骨架。

## 11 个核心一览

| 核心 | 一句话理解 | 延伸参考 |
|------|------------|----------|
| 智能体循环 | 让线程稳定完成一轮又一轮“思考、行动、观察、继续” | [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)、[流式执行](/docs/zh-CN/concepts/streaming/) |
| 工具 | 让模型可靠调用能力，而不是只会生成文本 | [工具体系](/docs/zh-CN/concepts/tools/)、[提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/) |
| 蜂群 | 让母蜂调度工蜂并行协作，而不是把一切塞进单线程 | [蜂群协作](/docs/zh-CN/concepts/swarm/) |
| 上下文压缩 | 让长会话继续跑下去，同时保留有效信息 | [边界处理](/docs/zh-CN/concepts/boundary-handling/)、[额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/) |
| 记忆 | 让长期资料可用，但不污染线程核心认知 | [长期记忆](/docs/zh-CN/concepts/memory/)、[工作区与容器](/docs/zh-CN/concepts/workspaces/) |
| 渠道 | 让 server、desktop、cli 和第三方入口共享同一内核 | [系统架构](/docs/zh-CN/concepts/architecture/)、[接入概览](/docs/zh-CN/integration/) |
| 定时任务 | 让周期执行和后台治理进入统一系统能力 | [计划任务工具](/docs/zh-CN/tools/schedule-task/)、[运维概览](/docs/zh-CN/ops/) |
| 多用户管理 | 让组织、租户、权限和配额成为一等能力 | [Server 部署](/docs/zh-CN/start/server/)、[认证与安全](/docs/zh-CN/ops/auth-and-security/) |
| 实时性 | 让前端和外部系统持续感知线程与任务变化 | [流式事件参考](/docs/zh-CN/reference/stream-events/)、[聊天 WebSocket](/docs/zh-CN/integration/chat-ws/) |
| 稳定性 | 让系统在长会话、高并发、多工具下依然能跑 | [边界处理](/docs/zh-CN/concepts/boundary-handling/)、[故障排查](/docs/zh-CN/help/troubleshooting/) |
| 可观测性 | 让系统能解释发生了什么、为什么、怎么复盘 | [流式事件参考](/docs/zh-CN/reference/stream-events/)、[性能与可观测性](/docs/zh-CN/ops/benchmark-and-observability/) |

## 1. 智能体循环

- 设计目标：让智能体稳定执行连续回合，而不是每次请求都像一次孤立问答。
- 核心能力：线程状态机、模型调用、工具调用、重试治理、终态收敛、恢复续跑。
- 关键约束：线程语义必须稳定，不能因为展示层或临时逻辑破坏运行时主链路。
- 延伸参考：[会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/)、[流式执行](/docs/zh-CN/concepts/streaming/)、[运行时与在线状态](/docs/zh-CN/concepts/presence-and-runtime/)

## 2. 工具

- 设计目标：让模型更容易正确调用能力，并可靠消费结果。
- 核心能力：清晰工具描述、结构化参数、统一结果截断、工具工作流展示、失败反馈。
- 关键约束：对大模型来说一切皆工具；工具返回必须精简明确；工具事实与展示投影必须分离。
- 延伸参考：[工具体系](/docs/zh-CN/concepts/tools/)、[提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)、[工具总览](/docs/zh-CN/tools/)

## 3. 蜂群

- 设计目标：支持母蜂调度多个工蜂完成协作任务，并把结果汇回主线程。
- 核心能力：任务拆解、工蜂派发、节点状态同步、汇总结果、父子会话关联。
- 关键约束：母蜂与工蜂职责分离；不能把蜂群误当普通子智能体链路；协作过程必须持续可见。
- 延伸参考：[蜂群协作](/docs/zh-CN/concepts/swarm/)、[子智能体控制](/docs/zh-CN/tools/subagent-control/)、[蜂群工具](/docs/zh-CN/tools/agent-swarm/)

## 4. 上下文压缩

- 设计目标：在长会话中控制上下文规模，同时保留有效信息。
- 核心能力：手动压缩、自动压缩、溢出恢复、压缩摘要回注、压缩前后对比与回放。
- 关键约束：压缩结果必须可追溯；不能伪造事实；压缩过程与终态要可观测。
- 延伸参考：[边界处理](/docs/zh-CN/concepts/boundary-handling/)、[额度与 Token 占用](/docs/zh-CN/concepts/quota-and-token-usage/)、[流式事件参考](/docs/zh-CN/reference/stream-events/)

## 5. 记忆

- 设计目标：支持长会话和长期资料利用，但不污染线程核心认知。
- 核心能力：线程初始化记忆注入、知识库、工作区文件、长期资料读取。
- 关键约束：长期记忆只允许在线程初始化注入一次；线程首次确定后的 `system prompt` 必须冻结。
- 延伸参考：[长期记忆](/docs/zh-CN/concepts/memory/)、[提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/)、[工作区与容器](/docs/zh-CN/concepts/workspaces/)

## 6. 渠道

- 设计目标：支持多入口接入同一运行时能力。
- 核心能力：HTTP、WebSocket、Desktop、CLI、第三方渠道、网关适配。
- 关键约束：多入口共核，只允许接入面差异，不允许演化成多套独立系统。
- 延伸参考：[系统架构](/docs/zh-CN/concepts/architecture/)、[接入概览](/docs/zh-CN/integration/)、[聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)

## 7. 定时任务

- 设计目标：支持系统级周期执行与后台治理。
- 核心能力：定时触发、计划任务、后台巡检、自动维护、异步执行链路。
- 关键约束：定时任务必须与在线线程语义区分；失败重试和执行记录要可追踪。
- 延伸参考：[计划任务工具](/docs/zh-CN/tools/schedule-task/)、[部署与运行](/docs/zh-CN/ops/deployment/)、[运维概览](/docs/zh-CN/ops/)

## 8. 多用户管理

- 设计目标：支持组织、用户、租户和权限治理。
- 核心能力：用户体系、单位与租户隔离、权限控制、资源配额、管理后台。
- 关键约束：默认按多用户并发设计；数据隔离、权限边界和治理能力不能后补。
- 延伸参考：[Server 部署](/docs/zh-CN/start/server/)、[认证与安全](/docs/zh-CN/ops/auth-and-security/)、[管理端面板索引](/docs/zh-CN/reference/admin-panels/)

## 9. 实时性

- 设计目标：让前端和外部系统及时感知线程与任务变化。
- 核心能力：WebSocket 事件流、快照补偿、增量同步、断线重连、回放恢复。
- 关键约束：前端消费事件流但不定义后端真相；实时展示允许投影但不能篡改状态语义。
- 延伸参考：[流式执行](/docs/zh-CN/concepts/streaming/)、[聊天 WebSocket](/docs/zh-CN/integration/chat-ws/)、[流式事件参考](/docs/zh-CN/reference/stream-events/)

## 10. 稳定性

- 设计目标：让系统在长会话、高并发、多工具、多入口场景下持续可运行。
- 核心能力：错误隔离、超时与重试、恢复续跑、资源治理、线程终态收敛、回归验收。
- 关键约束：稳定性依赖程序结构保障，不能只靠 prompt 和人工操作；必须优先控制高风险链路的故障扩散。
- 延伸参考：[边界处理](/docs/zh-CN/concepts/boundary-handling/)、[故障排查](/docs/zh-CN/help/troubleshooting/)、[性能与可观测性](/docs/zh-CN/ops/benchmark-and-observability/)

## 11. 可观测性

- 设计目标：让系统能解释“发生了什么、为什么、如何复盘”。
- 核心能力：线程事实流、时间线回放、管理员画像、吞吐评测、调试导出。
- 关键约束：区分事实层、回放层、画像层；区分请求、结果、观察结果；指标口径统一。
- 延伸参考：[流式事件参考](/docs/zh-CN/reference/stream-events/)、[性能与可观测性](/docs/zh-CN/ops/benchmark-and-observability/)、[管理端面板索引](/docs/zh-CN/reference/admin-panels/)

## 总体原则

| 原则 | 说明 |
|------|------|
| 一切围绕线程 | 会话、工具、压缩、蜂群、回放最终都要落回线程语义上统一治理 |
| 一切围绕事件 | 实时同步、排障、回放、监控都应建立在事件与状态变更之上 |
| 一切围绕约束 | prompt 冻结、记忆一次性注入、指标口径统一、事实与画像分离都是硬约束 |
| 一切围绕性能 | 高并发访问、长会话、工具链执行和多前端同步必须优先考虑速度与资源成本 |

## 下一步怎么读

- 想继续看拆开的运行模型细节：去 [参考概览](/docs/zh-CN/reference/) 的“运行模型参考”。
- 想直接接入系统：去 [接入概览](/docs/zh-CN/integration/)。
- 想理解工具层：去 [工具总览](/docs/zh-CN/tools/)。
- 想排障和看约束落地：去 [运维概览](/docs/zh-CN/ops/) 和 [故障排查](/docs/zh-CN/help/troubleshooting/)。
