---
title: 工具
summary: 工具页解决的是“现在该用哪个工具”，而不是重复概念解释；按任务分组可最快命中正确工具链。
read_when:
  - 你要查某个工具的动作、参数和适用场景
  - 你想按任务选择最短工具链
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
  - src/services/tools/catalog.rs
---

# 工具

把这组文档当成“任务选型手册”更准确。

## 先做选型

- 读网页正文：先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)
- 真实点按网页：用 [浏览器](/docs/zh-CN/tools/browser/)
- 改代码和文件：从 [文件与工作区工具](/docs/zh-CN/tools/workspace-files/) 起步
- 跑命令与测试：看 [执行命令](/docs/zh-CN/tools/exec/)
- 跨会话协作：看 [会话线程控制](/docs/zh-CN/tools/thread-control/) 与 [子智能体控制](/docs/zh-CN/tools/subagent-control/)

## 文件与代码工具

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/workspace-files/"><strong>文件与工作区</strong><span>列、搜、读、写文件。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/exec/"><strong>执行命令</strong><span>编译、测试、脚本执行。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/ptc/"><strong>ptc</strong><span>Python 程序化执行与产物落地。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/apply-patch/"><strong>应用补丁</strong><span>结构化多文件改动。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/read-image/"><strong>读图工具</strong><span>本地图片多模态读取。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/lsp/"><strong>LSP 查询</strong><span>符号、定义、引用、调用层级。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/skill-call/"><strong>技能调用</strong><span>按技能读取 SKILL.md 与资源。</span></a>
</div>

## 网页与桌面工具

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/web-fetch/"><strong>网页抓取</strong><span>正文提取优先工具。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/browser/"><strong>浏览器</strong><span>点击、输入、导航、截图。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/desktop-control/"><strong>桌面控制</strong><span>桌面自动化动作与观察。</span></a>
</div>

## 协作与编排工具

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/thread-control/"><strong>会话线程控制</strong><span>线程树、主线程与派生会话管理。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/subagent-control/"><strong>子智能体控制</strong><span>子会话查看、发送、派生。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/agent-swarm/"><strong>智能体蜂群</strong><span>并发派发与聚合结果。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/memory-manager/"><strong>记忆管理</strong><span>记忆片段的增删改查与召回。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/panels-and-a2ui/"><strong>界面协同工具</strong><span>A2UI、计划面板、问询面板。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/schedule-task/"><strong>定时任务</strong><span>延迟执行、周期执行、巡检任务。</span></a>
</div>

## 系统桥接工具

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/channel/"><strong>渠道工具</strong><span>联系人发现与渠道发信。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/user-world/"><strong>用户世界工具</strong><span>站内用户目录与消息发送。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/a2a-tools/"><strong>A2A 工具</strong><span>远程智能体服务调用与等待。</span></a>
  <a class="docs-card" href="/docs/zh-CN/tools/node-invoke/"><strong>节点调用</strong><span>列节点并下发命令。</span></a>
</div>

## 常见误区

- `web_fetch` 不是浏览器精简版，它是另一条链路。
- 工具可见性会受运行形态、智能体挂载和请求参数影响。
- `休眠等待` 不等于定时任务调度。

## 延伸阅读

- [工具体系](/docs/zh-CN/concepts/tools/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [API 索引](/docs/zh-CN/reference/api-index/)
