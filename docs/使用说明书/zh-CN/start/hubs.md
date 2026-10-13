---
title: 说明书总览
summary: 说明书导航页：从蜂窝起步，按需进入界面地图、其它形态与参考。列本轮 start 与 surfaces 两组的全部页面。
read_when:
  - 不确定先看哪一页
  - 想一眼扫到所有入口
source_docs:
  - docs/使用说明书/zh-CN/index.md
  - docs/使用说明书/zh-CN/surfaces
  - docs/使用说明书/zh-CN/start
updated_at: 2026-10-13
---

# 说明书总览

所有人都从[蜂窝（desktop）](/docs/zh-CN/start/desktop/)开始：下载安装，打开就是你的本地 AI 工作台。以下入口按使用频率与本轮定稿页面排列。

## 先看这里：用起来

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/start/quickstart/">
    <strong>快速开始</strong>
    <span>4 步跑通第一个任务。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/desktop/">
    <strong>蜂窝入门</strong>
    <span>下载安装，打开即用；本地 SQLite，无端口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/troubleshooting-first-run/">
    <strong>首次运行排查</strong>
    <span>装完启动没界面 / 端口不通 / 模型 401 都从这里查。</span>
  </a>
</div>

## 界面地图

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/surfaces/index/">
    <strong>界面概览</strong>
    <span>四个形态的界面分工与能力矩阵。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/desktop-ui/">
    <strong>蜂窝界面</strong>
    <span>主窗两页 + 12 个设置分类 + 面板与对话框。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/frontend/">
    <strong>蜂巢界面</strong>
    <span>SPA 路由；单条 `/chat/ws` 多路复用；含 TTS 双向语音。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/surfaces/web-admin/">
    <strong>舰桥界面</strong>
    <span>4 组 19 项导航；调试面板右侧已是线程轨迹视图。</span>
  </a>
</div>

## 用得更深

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/concepts/">
    <strong>核心概览</strong>
    <span>理解系统运行模型。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/tools/">
    <strong>工具总览</strong>
    <span>按任务选工具。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/integration/">
    <strong>接入概览</strong>
    <span>外部工作流、MCP 端点、渠道 webhook。</span>
  </a>
</div>

## 其它形态（可选）

个人用户默认只需蜂窝；下列是团队协作或自动化场景才需要：

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/start/server/">
    <strong>舰体部署</strong>
    <span>18000 API + 舰桥；18001 nginx 下发蜂巢；sandbox 9001。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/start/cli/">
    <strong>舵机使用</strong>
    <span>TTY ratatui TUI；`exec --json` JSONL；`cloud` 全套。</span>
  </a>
</div>

## 我在排障

- [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)：按形态定位装 / 启 / 访 三类失败
- [帮助中心](/docs/zh-CN/help/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
- [FAQ](/docs/zh-CN/help/faq/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)

## 文档生态

- [常用说明目录](/docs/zh-CN/start/docs-directory/)：说明书 / 设计文档 / 技术说明书 / API 文档 / 性能标准与评分 / 功能迭代 / 经验教训 的分工与读法
- [API 索引](/docs/zh-CN/reference/api-index/)

## 首页

回到 [wunder 心舰首页](/docs/zh-CN/index/)。
