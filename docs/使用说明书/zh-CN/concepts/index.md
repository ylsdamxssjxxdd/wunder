---
title: 核心概览
summary: 理解 Wunder，先抓住 11 个核心。它们定义的不是"系统里有什么功能"，而是"系统必须守住哪些设计原则"。
---

# 核心概览

Wunder 不是一个简单的聊天工具。它要长期稳定运行、支持多用户并发、让智能体真正执行任务而不仅仅是说话。为了实现这些目标，系统建立了 11 个核心设计原则。

理解这些核心，不是为了背诵术语，而是为了在使用 Wunder 时知道"为什么会这样"、"遇到问题时该从哪里想"。

## Wunder 的四条设计哲学

在了解 11 个核心之前，先记住这四条总原则：

**1. 一切围绕线程**

Wunder 不是以"一次问答"为中心，而是以"一条持续运行的线程"为中心。你跟智能体的每一次对话、智能体执行的每一个任务，都发生在线程里。线程是 Wunder 最重要的执行单元。

**2. 一切皆工具**

对智能体来说，它能做的所有事情——读文件、写代码、搜索网页、控制桌面——都是"工具"。这种统一视角让智能体可以灵活组合各种能力完成任务。

**3. 多入口共享内核**

不管你是用桌面应用、网页端、命令行还是飞书微信跟智能体对话，背后都是同一套系统。体验不同，但能力一致。

**4. 安全与治理优先**

Wunder 默认按多用户、长会话、高风险工具来设计。安全限制、资源配额、操作审计不是事后补的，而是一开始就在系统骨架里。

## 11 个核心如何分层

这 11 个核心可以分为三层，从内到外：

| 层级 | 包含的核心 | 解决什么问题 |
|------|-----------|-------------|
| **执行内核** | 智能体循环、工具、蜂群、上下文压缩、记忆 | 让线程能持续跑、能调用能力、能协作、能控制长度、能利用长期资料 |
| **接入与治理** | 渠道、定时任务、多用户管理 | 让不同入口、后台任务和多用户共享同一套系统规则 |
| **交付保障** | 实时性、稳定性、可观测性 | 让用户看得见过程、系统扛得住压力、管理员能追溯问题 |

## 11 个核心一览

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/core-agent-loop/"><strong>智能体循环</strong><span>线程如何稳定地"思考→行动→观察→继续"。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-tools/"><strong>工具</strong><span>智能体不只是说话，而是能做事。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-swarm/"><strong>蜂群</strong><span>多个智能体协作完成复杂任务。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-context-compression/"><strong>上下文压缩</strong><span>长对话不会因为太长而中断。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-memory/"><strong>记忆</strong><span>长期资料如何安全地参与执行。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-channels/"><strong>渠道</strong><span>不同入口共享同一套核心能力。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-scheduled-tasks/"><strong>定时任务</strong><span>周期性任务自动执行。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-multi-user-management/"><strong>多用户管理</strong><span>组织、权限和资源治理。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-realtime/"><strong>实时性</strong><span>执行过程实时可见、断线可恢复。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-stability/"><strong>稳定性</strong><span>出错不扩散、能恢复。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-observability/"><strong>可观测性</strong><span>系统能解释自己发生了什么。</span></a>
</div>

## 按角色推荐阅读

### 普通用户

优先看：
- 智能体循环 → 理解对话是怎么工作的
- 工具 → 了解智能体能帮你做什么
- 蜂群 → 多智能体协作的原理
- 记忆 → 你的长期资料怎么被使用
- 上下文压缩 → 为什么长对话不会断

### 系统管理员

优先看：
- 多用户管理 → 理解权限和治理
- 稳定性 → 了解系统的容错机制
- 可观测性 → 学会追溯和分析问题
- 渠道 → 多入口接入的原理

### 延伸主题

以下主题页面在核心基础上展开更多实践细节：

- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) —— 对话的组织方式
- [工具体系](/docs/zh-CN/concepts/tools/) —— 工具来源和使用
- [蜂群协作](/docs/zh-CN/concepts/swarm/) —— 蜂群的实际运作
- [长期记忆](/docs/zh-CN/concepts/memory/) —— 记忆的形态和管理
- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/) —— 定制智能体风格
- [工作区与容器](/docs/zh-CN/concepts/workspaces/) —— 文件空间的管理
- [Token 与配额](/docs/zh-CN/concepts/quota-and-token-usage/) —— 资源使用统计
- [流式执行](/docs/zh-CN/concepts/streaming/) —— 实时输出的原理
- [运行时状态](/docs/zh-CN/concepts/presence-and-runtime/) —— 状态指示器的含义
- [边界处理](/docs/zh-CN/concepts/boundary-handling/) —— 异常情况下系统的行为