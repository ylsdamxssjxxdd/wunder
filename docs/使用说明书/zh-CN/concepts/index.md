---
title: 核心概览
summary: 11 个核心不是 11 个功能点，而是 Wunder 为了长期运行、多人并发和多入口接入而主动建立的系统骨架。
read_when:
  - 你第一次系统性理解 Wunder
  - 你准备做接入、运维或工具开发，需要统一视角
  - 你已经会用 Wunder，但想理解它为什么这样设计
source_docs:
  - docs/总体设计.md
---

# 核心概览

理解 Wunder，先不要先背术语、接口和页面名称。先抓住 11 个核心，因为它们定义的不是“系统里有什么模块”，而是“系统必须守住哪些稳定边界”。

旧的概念细页没有删除，而是归入 [参考概览](/docs/zh-CN/reference/) 的“运行模型参考”。这页负责建立系统主骨架，并把你带到 11 个核心细页。

![Wunder 的 11 个核心分层图：执行内核、接入与治理、交付保障三层结构](/docs/assets/manual/core-overview-map.svg)

## 先记住四条总判断

- Wunder 不是以“单次回答”为中心，而是以“线程能否长期稳定运行”为中心。
- Wunder 不是把能力散落在模型提示词里，而是尽量把能力收口成工具、事件和治理约束。
- Wunder 不是只做一个聊天入口，而是要让 server、desktop、cli 和渠道共享同一套内核。
- Wunder 不是先做功能再补治理，而是默认按多用户并发、长会话和高风险工具链来设计。

## 为什么这 11 个是核心，而不是普通功能

| 判断维度 | 如果没有独立成核心，会发生什么 |
|------|------|
| 执行链路 | 系统会退化成“一问一答”，线程语义无法稳定收敛 |
| 能力组织 | 工具、记忆、蜂群、渠道会各自为政，模型和前端都很难消费 |
| 治理边界 | 多用户、权限、定时任务和外部接入会在后期互相污染 |
| 运维与复盘 | 只能看到“结果不对”，看不到“为什么不对、卡在哪、如何回放” |

## 11 个核心如何分层

| 层级 | 核心 | 真正要解决的问题 |
|------|------|------|
| 执行内核 | 智能体循环、工具、蜂群、上下文压缩、记忆 | 让线程能持续跑、能调能力、能协作、能控制上下文、能利用长期资料 |
| 接入与治理 | 渠道、定时任务、多用户管理 | 让不同入口、后台任务和多用户边界共享同一套系统口径 |
| 交付保障 | 实时性、稳定性、可观测性 | 让用户看得见、系统扛得住、管理员能复盘 |

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

## 11 个核心细页

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/core-agent-loop/"><strong>智能体循环</strong><span>看线程状态机、终态收敛与恢复续跑。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-tools/"><strong>工具</strong><span>看工具描述、结构化参数与结果约束。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-swarm/"><strong>蜂群</strong><span>看母蜂、工蜂和协作边界。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-context-compression/"><strong>上下文压缩</strong><span>看长会话压缩、摘要回注与可追溯性。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-memory/"><strong>记忆</strong><span>看线程初始化注入与冻结约束。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-channels/"><strong>渠道</strong><span>看多入口共核与接入面边界。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-scheduled-tasks/"><strong>定时任务</strong><span>看周期执行、后台治理与记录追踪。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-multi-user-management/"><strong>多用户管理</strong><span>看租户、权限、配额和治理面板。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-realtime/"><strong>实时性</strong><span>看事件流、快照补偿和断线恢复。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-stability/"><strong>稳定性</strong><span>看错误隔离、重试、恢复和回归验收。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-observability/"><strong>可观测性</strong><span>看事实流、回放、画像与指标口径。</span></a>
</div>

## 建议阅读顺序

- 如果你在做接入：先看 [智能体循环](/docs/zh-CN/concepts/core-agent-loop/)、[渠道](/docs/zh-CN/concepts/core-channels/)、[实时性](/docs/zh-CN/concepts/core-realtime/)。
- 如果你在做工具或智能体能力：先看 [工具](/docs/zh-CN/concepts/core-tools/)、[上下文压缩](/docs/zh-CN/concepts/core-context-compression/)、[记忆](/docs/zh-CN/concepts/core-memory/)。
- 如果你在做管理员侧或上线治理：先看 [多用户管理](/docs/zh-CN/concepts/core-multi-user-management/)、[稳定性](/docs/zh-CN/concepts/core-stability/)、[可观测性](/docs/zh-CN/concepts/core-observability/)。
- 如果你在做协作与自动化：先看 [蜂群](/docs/zh-CN/concepts/core-swarm/) 和 [定时任务](/docs/zh-CN/concepts/core-scheduled-tasks/)。

## 总体原则

| 原则 | 说明 |
|------|------|
| 一切围绕线程 | 会话、工具、压缩、蜂群、回放最终都要落回线程语义上统一治理 |
| 一切围绕事件 | 实时同步、排障、回放、监控都应建立在事件与状态变更之上 |
| 一切围绕约束 | prompt 冻结、记忆一次性注入、指标口径统一、事实与画像分离都是硬约束 |
| 一切围绕性能 | 高并发访问、长会话、工具链执行和多前端同步必须优先考虑速度与资源成本 |

## 下一步怎么读

- 想逐个看 11 个核心：直接从上面的卡片进入细页。
- 想继续看原先按主题拆开的运行模型细节：去 [参考概览](/docs/zh-CN/reference/) 的“运行模型参考”。
- 想直接接入系统：去 [接入概览](/docs/zh-CN/integration/)。
- 想理解工具层：去 [工具总览](/docs/zh-CN/tools/)。
- 想排障和看约束落地：去 [运维概览](/docs/zh-CN/ops/) 和 [故障排查](/docs/zh-CN/help/troubleshooting/)。
