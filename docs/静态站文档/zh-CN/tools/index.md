---
title: 工具
summary: 这组页面只讲工具怎么选、怎么用；如果你现在就要读网页，先看 `web_fetch`，不要先打开浏览器。
read_when:
  - 你要查某个内置工具的动作、参数和适用场景
  - 你想按 OpenClaw 风格浏览 Wunder 的工具清单
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
  - src/services/tools/catalog.rs
---

# 工具

这组页面只讲工具本身。

如果你想知道调度关系、来源边界和提示词挂载方式，先看 [工具体系](/docs/zh-CN/concepts/tools/)。

## 这组页面解决什么

- 某个工具具体能做什么
- 你现在该先用哪个工具
- 什么时候该换另一条工具链

## 先记这几条

- 工具页讲“怎么选、怎么用”，概念页讲“为什么这么设计”。
- 同一个任务里，通常只需要先选对一条工具链，不需要一上来把所有工具都打开。
- `server`、`cli`、`desktop` 下的可见工具并不完全一样。

## 先按问题找工具

### 只想读网页正文

先看 [网页抓取](/docs/zh-CN/tools/web-fetch/)。

它的英文别名就是 `web_fetch`。

适合这些情况：

- 你只想抓一篇文章、帮助页或说明页
- 你不需要点击、输入和登录
- 你想尽量少噪声、少上下文占用

### 要真实操作网页

先看 [浏览器](/docs/zh-CN/tools/browser/)。

适合这些情况：

- 你要导航、点击、输入和截图
- 你要在真实页面会话里继续读页
- 你当前就在 desktop 语境里做网页交互

### 要读写文件、代码和工作区

先看这些：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/workspace-files/">
    <strong>文件与工作区工具</strong>
    <span>列文件、搜内容、读写文件的高频入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/exec/">
    <strong>执行命令</strong>
    <span>运行命令、编译测试和预算化执行。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/ptc/">
    <strong>ptc</strong>
    <span>把 Python 脚本写成程序化产物再执行。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/apply-patch/">
    <strong>应用补丁</strong>
    <span>多文件结构化编辑的主力工具。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/read-image/">
    <strong>读图工具</strong>
    <span>把本地图片注入到多模态后续消息里，而不是按文本读取。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/lsp/">
    <strong>LSP 查询</strong>
    <span>定义跳转、引用搜索、符号树与调用层级。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/skill-call/">
    <strong>技能调用</strong>
    <span>按技能名读取完整 SKILL.md 和技能目录结构。</span>
  </a>
</div>

### 要界面协同或自动化

先看这些：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/panels-and-a2ui/">
    <strong>界面协同工具</strong>
    <span>`a2ui`、计划面板和问询面板如何驱动前端界面。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/schedule-task/">
    <strong>定时任务</strong>
    <span>把延迟执行、周期执行和唤醒式任务做成正式工具。</span>
  </a>
</div>

### 要会话协作、节点和系统桥接

先看这些：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/tools/thread-control/">
    <strong>会话线程控制</strong>
    <span>管理线程树、主线程和派生会话，不只是“切换对话”。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/subagent-control/">
    <strong>子智能体控制</strong>
    <span>查看、发送和派生单个子会话运行。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/agent-swarm/">
    <strong>智能体蜂群</strong>
    <span>单目标发送、多目标并发派发和等待聚合结果。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/memory-manager/">
    <strong>记忆管理</strong>
    <span>结构化记忆碎片的 list/add/update/delete/recall。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/channel/">
    <strong>渠道工具</strong>
    <span>联系人发现与渠道消息发送。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/user-world/">
    <strong>用户世界工具</strong>
    <span>系统内用户目录查询与站内消息发送。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/a2a-tools/">
    <strong>A2A 工具</strong>
    <span>调用 `a2a@服务名`，并用观察/等待工具跟踪任务。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/node-invoke/">
    <strong>节点调用</strong>
    <span>列出可用节点并向指定节点发送命令。</span>
  </a>
</div>

## 最容易搞错的点

- `web_fetch` 不是浏览器的轻量别名，它是另一条工具链。
- 浏览器更偏 desktop 本地能力，不是所有 server 环境都天然可用。
- 同一个系统里的两个会话，可见工具集也可能不同。

这些差异通常来自：

- 请求级 `tool_names`
- 智能体默认挂载
- 用户级 MCP / Skills / 知识库配置
- 运行形态和环境能力过滤

## 其他常见轻量工具

- `最终回复`
- `休眠等待`

可以这样记：

- `最终回复` 把最终文本落回用户轮次。
- `休眠等待` 负责短暂等待，不等于 cron 或 A2A 轮询。

## 相关文档

- [工具体系](/docs/zh-CN/concepts/tools/)
- [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- [API 索引](/docs/zh-CN/reference/api-index/)
