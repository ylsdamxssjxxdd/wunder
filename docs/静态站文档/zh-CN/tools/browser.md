---
title: 浏览器
summary: `浏览器` 是 Wunder 在 desktop 模式下提供的页面交互工具，支持导航、点击、输入、截图和读页。
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

## 这页解决什么

这页只解释：

- 浏览器工具能做哪些动作
- 它和网页抓取有什么区别
- 为什么它通常只在 desktop 语境下出现

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

## 什么时候用它

- 需要真的打开页面
- 需要点击或输入
- 需要截图确认页面状态

## 为什么它通常是 desktop 工具

因为当前 Wunder 的浏览器链路更偏本地桌面运行能力。

所以文档里应默认理解为：

- 这是 desktop 模式下的交互工具

而不是所有 server 环境都天然可用。

## `read_page` 和网页抓取的区别

两者都能“读页面”，但语义不同：

- `网页抓取`：直接提取正文
- `浏览器.read_page`：建立在真实页面会话上的读取

如果你已经在浏览器会话里交互了，通常继续用浏览器链路更自然。

## 你最需要记住的点

- 交互用浏览器，正文抓取用网页抓取。
- 当前浏览器工具更偏 desktop 本地能力。
- 核心动作是导航、点击、输入、截图和读页。

## 相关文档

- [网页抓取](/docs/zh-CN/tools/web-fetch/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
