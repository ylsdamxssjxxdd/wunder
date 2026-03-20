---
title: 浏览器
summary: "`浏览器` 是 Wunder 在 desktop 模式下提供的交互型工具；需要真实页面会话时用它，不要用 `web_fetch` 硬顶。"
read_when:
  - 你在做桌面自动化或网页交互
  - 你要区分浏览器和网页抓取
source_docs:
  - docs/API文档.md
  - docs/系统介绍.md
  - src/services/tools/browser_tool.rs
---

# 浏览器

`浏览器` 是交互型工具，不是正文抓取工具。

如果你要真实打开页面、点击、输入和截图，这页先看。

## 这页解决什么

- 浏览器工具能做哪些动作
- 它和 `web_fetch` 的区别到底是什么
- 为什么它通常只在 desktop 语境下出现

## 先记这几条

- 浏览器工具更偏 desktop 本地能力。
- 它要求真实页面会话，所以成本比 `web_fetch` 更高，但交互能力也更强。
- 当你已经在浏览器会话里操作页面时，继续用浏览器链路通常更自然。

## 什么时候先用它

- 你需要真的打开页面
- 你需要点击或输入
- 你需要截图确认页面状态
- 你要在同一个页面会话里继续 `read_page`

## 当前动作

浏览器工具当前统一通过 `action` 驱动，核心动作包括：

- `navigate`
- `click`
- `type`
- `screenshot`
- `read_page`
- `close`

常用参数也很直接：

- `url`
- `selector`
- `text`

## 什么时候不要先用它

- 你只是想读一篇文章或帮助页正文
- 你不需要交互
- 你想尽量减少上下文噪声和执行成本

这些场景优先看 [网页抓取](/docs/zh-CN/tools/web-fetch/)。

## 它为什么通常只在 desktop 下可用

当前可见性条件不是“代码里有这个工具就行”，而是：

- 运行形态是 `desktop`
- `tools.browser.enabled=true`

所以文档里默认应把它理解成 desktop 本地交互工具，而不是所有 server 环境都天然可用。

## `read_page` 和 `web_fetch` 的区别

两者都能“读页面”，但语义不同：

- `web_fetch`：直接提取正文
- `browser.read_page`：建立在真实页面会话上的读取

可以这样选：

- 先读正文，优先 `web_fetch`
- 先交互再读页，优先浏览器

## 最容易搞错的点

- 浏览器不是“更强版网页抓取”，它们解决的是两类问题。
- 既然已经进入浏览器会话，再退回 `web_fetch` 读同一页面，通常会丢失交互上下文。
- 浏览器工具可见，不代表当前运行环境就适合高频自动化；它仍然受 desktop 条件限制。

## 相关文档

- [网页抓取](/docs/zh-CN/tools/web-fetch/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
