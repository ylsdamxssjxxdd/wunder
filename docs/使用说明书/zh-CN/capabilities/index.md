---
title: 能力概览
summary: 本分区 10 个页面的入口清单，以及 9 项核心能力在 concepts 分区的对应页。
updated_at: 2026-10-16
---

# 能力概览

本分区 10 页按功能入口组织：会话入口、会话状态、提示词与个性化、系统设置、工作区路由、舰桥面板、微信 iLink 渠道、聊天 WebSocket、MCP 入口、三端互通。

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/capabilities/api-index/"><strong>功能与能力速查</strong><span>wunder 能做什么，每个能力在哪使用。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/stream-events/"><strong>会话状态说明</strong><span>聊天过程中各种状态的含义。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/prompt-templates/"><strong>提示词与个性化</strong><span>如何定制智能体的回答风格。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/config/"><strong>系统设置参考</strong><span>常用设置项的含义与推荐值。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/workspace-routing/"><strong>工作区路由与文件管理</strong><span>唯一云端目录、路径约定与管理技巧。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/admin-panels/"><strong>舰桥面板指南</strong><span>管理员后台各面板的功能定位。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/weixin-channel/"><strong>微信 iLink 渠道</strong><span>配 `weixin` 渠道，以及它跟旧微信渠道的区别。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/chat-ws/"><strong>聊天 WebSocket</strong><span>实时通道也是会话控制通道：启动、续传、观察、取消、回传审批都在同一条连接上。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/mcp-endpoint/"><strong>MCP 入口</strong><span>分清两个方向：把 wunder 暴露成 MCP 服务，还是接入外部 MCP 服务。</span></a>
  <a class="docs-card" href="/docs/zh-CN/capabilities/interlink/"><strong>三端互通</strong><span>蜂巢、蛹、舵机统一在线状态、跨端工作区互见与双向远程控制。</span></a>
</div>

## 核心能力

9 项核心能力的机制说明放在 concepts 分区：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/core-goal/"><strong>目标</strong><span>让智能体进入目标态并持续完成一项任务。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-compatibility/"><strong>兼容性</strong><span>让蛹与舵机在旧操作系统环境中可运行并被明确约束。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-context-compression/"><strong>上下文压缩</strong><span>在长会话中控制上下文规模并保留有效信息。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-subagent/"><strong>子智能体</strong><span>在单一线程内派生子任务，由独立执行体完成并回传结果。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-scheduled-tasks/"><strong>定时任务</strong><span>支持系统级周期执行与后台治理。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-channels/"><strong>渠道通信</strong><span>多入口接入同一运行时能力。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-memory/"><strong>记忆</strong><span>支持长会话与长期资料利用，但不污染线程核心认知。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-multithreading/"><strong>多线程</strong><span>在单进程内通过并发控制提升吞吐与资源利用。</span></a>
  <a class="docs-card" href="/docs/zh-CN/concepts/core-multi-user-management/"><strong>多租户</strong><span>支持舰体的组织、用户、租户与权限治理。</span></a>
</div>
