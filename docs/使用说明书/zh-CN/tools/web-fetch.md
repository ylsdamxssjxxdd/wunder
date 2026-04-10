---
title: 网页抓取
summary: `web_fetch` 的直接抓取、浏览器回退与成功/失败返回。
read_when:
  - 你要读取公开网页正文，而不是操控浏览器
source_docs:
  - src/services/tools/web_fetch_tool.rs
updated_at: 2026-04-10
---

# 网页抓取

`web_fetch` 当前仍然是成功返回的例外项。  
它成功时直接返回抓取结果对象，不包统一的 `ok/action/state/summary/data`。

## 最小参数

```json
{
  "url": "https://example.com"
}
```

## 成功返回

```json
{
  "url": "https://example.com",
  "final_url": "https://example.com",
  "status": 200,
  "title": "Example Domain",
  "content_type": "text/html; charset=UTF-8",
  "content_kind": "html",
  "fetch_strategy": "direct_http",
  "format": "markdown",
  "extractor": "readability",
  "truncated": false,
  "warning": null,
  "cached": false,
  "fetched_at": "2026-04-10T03:00:00Z",
  "content": "..."
}
```

## 重点字段

- `fetch_strategy`：例如 `direct_http`、`browser_fallback`
- `format`：通常是 `markdown` 或 `text`
- `extractor`：实际抽取器
- `truncated`：正文是否被截断
- `warning`：抓取过程中的额外提示

## 浏览器回退

如果直接 HTTP 抓到的只是前端壳、反爬校验页或无意义 HTML，`web_fetch` 可能会自动尝试浏览器回退。  
成功时你会看到：

```json
{
  "fetch_strategy": "browser_fallback"
}
```

## 失败返回

失败时会走统一失败骨架，并补充抓取诊断字段，例如：

- `phase`
- `failure_summary`
- `error_detail_head`
- `next_step_hint`
- `normalized_url`
- `host`
- `error_meta.code`

## 什么时候别用它

- 要点击、输入、滚动：用 [浏览器](/docs/zh-CN/tools/browser/)
- 要看本地 HTML 文件：这不是它的场景
