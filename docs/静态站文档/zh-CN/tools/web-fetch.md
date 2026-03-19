---
title: 网页抓取
summary: `网页抓取` 直接获取网页正文内容，适合读文本，不适合代替真正的浏览器自动化。
read_when:
  - 你要让模型读取网页正文
  - 你不确定该用网页抓取还是浏览器
source_docs:
  - config/wunder-example.yaml
  - src/services/tools/web_fetch_tool.rs
  - docs/系统介绍.md
---

# 网页抓取

`网页抓取` 是 Wunder 当前的轻量网页读取工具。

## 它解决什么

它用于：

- 抓取一个 URL
- 提取网页正文
- 以更适合模型消费的文本返回

## 常用参数

- `url`
- `extract_mode`
- `max_chars`

其中 `extract_mode` 当前主要是：

- `markdown`
- `text`

## 什么时候优先用它

- 你只想读文章、帮助页、说明页正文
- 你不需要点击、输入和交互
- 目标页面主要是静态内容

## 什么时候不要用它

如果你要：

- 点按钮
- 填表单
- 登录
- 处理强交互页面

那应该看：

- [浏览器](/docs/zh-CN/tools/browser/)

## 为什么它不等于浏览器

因为它不维护真实交互状态，只做页面抓取和正文提取。

所以它更轻，也更快，但能力边界更窄。

## 开关在哪里

当前它受：

- `tools.web_fetch.enabled`

控制。

## 你最需要记住的点

- `网页抓取` 适合读网页，不适合操控网页。
- 静态正文优先用它，交互页面优先用浏览器。
- 抓取结果仍会受正文提取和字符预算限制。

## 相关文档

- [浏览器](/docs/zh-CN/tools/browser/)
- [接入概览](/docs/zh-CN/integration/)
