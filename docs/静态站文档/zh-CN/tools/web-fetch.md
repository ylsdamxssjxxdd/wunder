---
title: 网页抓取
summary: "`网页抓取` 的英文别名是 `web_fetch`。它适合读网页正文，不适合代替真实浏览器交互。"
read_when:
  - 你要让模型读取网页正文
  - 你不确定该用网页抓取还是浏览器
source_docs:
  - config/wunder-example.yaml
  - src/services/tools/web_fetch_tool.rs
  - docs/系统介绍.md
---

# 网页抓取

`网页抓取` 的英文别名就是 `web_fetch`。

如果你现在只想让模型读一个网页，不要先打开浏览器，先看这个工具。

## 本页重点

- `web_fetch` 适合什么，不适合什么
- 它最常见的入参和返回是什么
- 它和浏览器的边界到底在哪

## 关键结论

- `web_fetch` 直接通过 HTTP/HTTPS 拉取页面，不维护真实浏览器会话。
- 它会先做正文提取、噪声剔除和字符预算裁剪，再把结果返回给模型。
- 它会拦截私网或内网目标，并对重定向逐跳复校验。

## 什么时候先用它

- 你只想读文章、帮助页、说明页正文
- 你不需要点击、输入、登录和表单提交
- 目标页面主要是静态内容
- 你希望结果尽量短、干净、适合直接进模型上下文

## 什么时候不要先用它

- 你要点按钮
- 你要填表单
- 你要登录后再读页面
- 你要在真实页面状态里连续交互

这些场景优先看 [浏览器](/docs/zh-CN/tools/browser/)。

## 最常用的入参

- `url`
- `extract_mode`
- `max_chars`

兼容别名也已经支持：

- `extractMode`
- `maxChars`

`extract_mode` 当前主要是：

- `markdown`
- `text`

如果你不传，默认会走更适合阅读的 `markdown` 提取。

## 返回里你最该看什么

`web_fetch` 返回的不只是正文，一般还会带这些字段：

- `final_url`
- `status`
- `title`
- `content_type`
- `format`
- `extractor`
- `truncated`
- `warning`
- `cached`
- `fetched_at`
- `content`

最值得先看的通常是：

- `content`：正文结果
- `truncated`：这次有没有被裁剪
- `warning`：是否有响应体截断或正文预算截断
- `cached`：是不是命中了短 TTL 缓存

## 它为什么比直接读原始 HTML 更适合模型

因为它不是简单返回页面源码，而是先做这些事：

- 正文定位
- 导航、页脚、相关推荐等噪声过滤
- 重复块去重
- 字符预算裁剪

所以它更像“网页正文提取器”，不是“网页源码下载器”。

## 配置开关在哪里

核心配置位于 `tools.web.fetch.*`。

当前默认值来自运行配置：

- `enabled=true`
- `timeout_secs=20`
- `max_redirects=3`
- `max_response_bytes=2MB`
- `max_chars=12000`
- `max_chars_cap=30000`
- `cache_ttl_secs=600`

如果你排查“为什么这个工具没出来”或“为什么结果比预期短”，先查这组配置。

## 常见误区

- `web_fetch` 不是浏览器的轻量模式，它根本不维护交互状态。
- `max_chars` 是结果预算，不代表网页原始大小。
- 能抓到页面，不等于适合继续做交互；交互任务仍然应该切到浏览器。
- 目标站点如果是 JS 强交互页面，`web_fetch` 往往不如浏览器可靠。

## 延伸阅读

- [浏览器](/docs/zh-CN/tools/browser/)
- [工具总览](/docs/zh-CN/tools/)
- [接入概览](/docs/zh-CN/integration/)
