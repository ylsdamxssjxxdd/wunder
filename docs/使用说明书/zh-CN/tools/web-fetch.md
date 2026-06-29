---
title: 网页抓取
summary: `web_search` 与 `web_fetch` 的分工，以及 `web_fetch` 的直接抓取、浏览器回退与成功/失败返回。
read_when:
  - 用户要读取公开网页正文，而不是操控浏览器
source_docs:
  - src/services/tools/web_search_tool.rs
  - src/services/tools/web_fetch_tool.rs
updated_at: 2026-05-15
---

# 网页抓取

## 先分清搜索和抓取

- `web_search`：输入关键词，返回候选网页的标题、URL 和摘要。
- `web_fetch`：输入一个已确认的具体 URL，读取该网页正文。

只有关键词时，先用 `web_search`：

```json
{
  "query": "示例项目 官方 文档 GitHub",
  "count": 5
}
```

如果用户已有一个站点或域名，只想在该来源内搜索，传 `site` 或 `sites`，不要把搜索页 URL 拼好后交给 `web_fetch`：

```json
{
  "site": "example.com",
  "query": "安装 配置",
  "count": 5
}
```

拿到具体 URL 后，再用 `web_fetch` 抓正文。搜索结果页也可以作为线索页抓取，但最终证据应继续回到具体来源页面。

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

## Provider

Docker compose 默认只启用 Wunder 内置网页抓取，不再启动 Firecrawl 自托管服务组。`tools.web.search.enabled` 默认关闭；需要关键词搜索时，可先配置可用搜索 provider，再显式启用 `web_search`。未配置搜索 provider 时，也可以直接用 `web_fetch` 抓取搜索结果页作为线索页。

`tools.web.fetch.provider` 支持 `direct`、`auto`、`firecrawl`。默认值是 `direct`，使用 Wunder 内置 HTTP 抓取器。`auto` 会在配置了 `FIRECRAWL_API_KEY` 或 `FIRECRAWL_BASE_URL` 时优先使用外部 Firecrawl，否则回退到内置抓取器。

Firecrawl Cloud 使用 `https://api.firecrawl.dev`，并且需要 API Key。管理员系统设置页只保存 Wunder 连接外部 Firecrawl 的参数，不启动或停止 Docker 服务。

## 浏览器回退

如果直接 HTTP 抓到的只是前端壳、反爬校验页或无意义 HTML，`web_fetch` 可能会自动尝试浏览器回退。  
成功时用户会看到：

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

## 不适用场景

- 有搜索关键词但没有具体 URL：先用 `web_search`
- 要点击、输入、滚动：用 [浏览器](/docs/zh-CN/tools/browser/)
- 要看本地 HTML 文件：这不是它的场景
