---
title: 网页抓取
summary: `网页抓取`（别名 `web_fetch`）取单个公开 URL 并抽取正文；同时说明 `网页搜索` 与 `ptc` 的临时下架口径。
read_when:
  - 智能体已有明确 URL，要把页面读成 markdown/text
  - 有人问"搜索工具去哪了"，需要准确的临时下架口径
source_docs:
  - crates/wunder-runtime/src/services/tools/web_fetch_tool.rs
  - crates/wunder-runtime/src/services/tools/web_fetch_tool_impl.rs
  - crates/wunder-runtime/src/services/tools/web_fetch_provider_impl.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
  - crates/wunder-runtime/src/services/tools/web_search_tool.rs
  - crates/wunder-core/src/config.rs
updated_at: 2026-10-13
---

# 网页抓取

规范名：`网页抓取`　别名：`web_fetch`（`web_fetch_tool_impl.rs:26-27`）。

## 先纠正两条旧口径

- `网页搜索`（别名 `web_search`）与 `ptc` 是**临时下架**，不是已移除。`crates/wunder-runtime/src/services/default_tool_profile.rs:73-87` 定义 `TEMPORARILY_HIDDEN_TOOL_NAMES = ["网页搜索", "ptc"]`，注释明写"属临时隐藏，恢复时移除对应条目即可"。代码路径仍在：`网页搜索` 仍在 `crates/wunder-runtime/src/services/tools/catalog.rs:982-1024` 注册规格，`crates/wunder-runtime/src/services/tools/dispatch.rs:90` 仍有分派分支。
- 下架的实际效果是：**无论配置是否启用，都不对智能体与用户可见**（`catalog.rs:1336-1340` 的 `runtime_builtin_tool_allowed` 先过这一关）。此外 `网页搜索` 本身还要求 `tools.web.search.enabled` 且 provider 解析为 `firecrawl`，而 `tools.web.search.enabled` 默认为 `false`（`crates/wunder-runtime/src/services/tools/web_search_tool.rs:101-103`、`crates/wunder-core/src/config.rs:1526-1528`）。
- 需要检索时，正确做法是让智能体用已知的公开 URL 调 `网页抓取`，或用浏览器工具导航；不要向用户承诺"搜索工具"。

## 什么时候用

- 手里有确定 URL，要读文章、文档、接口返回。
- 不适合用它：需要搜索结果页找链接（先有 URL 才能抓）、需要登录/交互/点击（用浏览器工具）、二进制文件下载（不支持的内容类型会直接失败）。

## 参数

模型侧 schema（`crates/wunder-runtime/src/services/tools/catalog.rs:1025-1041`，`web_fetch_tool_impl.rs:188-195`）：

| 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `url` | string | 是 | 必须是绝对 `http://` / `https://` URL；其它 scheme 一律拒绝 |
| `extract_mode` | string | 否 | `markdown`（默认）/ `text` |
| `max_chars` | integer | 否 | ≥100；再被 `tools.web.fetch.max_chars_cap` 夹住（默认 30000），未传时用 `max_chars` 默认 12000（`web_fetch_tool_impl.rs:759-763`、`config.rs:1566-1572`） |

字段名可用 `extractMode` / `maxChars` 驼峰别名传入（`web_fetch_tool_impl.rs:191-194`）。

## 执行过程与限额

1. **供应商选择**：`tools.web.fetch.provider` 取 `direct`（默认）/ `firecrawl` / `auto`，可用环境变量 `WUNDER_WEB_FETCH_PROVIDER` 覆盖（`config.rs:1424-1429`）。`auto` 在 Firecrawl 已配置时优先走它，失败后回落直连；`firecrawl` 失败不回落（`web_fetch_provider_impl.rs:56-66`）。
2. **地址校验**：先做主机名与 IP 私网判定，再做 DNS（hickory 系统配置解析器）并对**每一个解析出的 IP** 复检；重定向逐跳重新校验并检测环（`web_fetch_tool_impl.rs:999-1141`）。私网/回环/链路本地/组播/文档段、`100.64.0.0/10`、`198.18.0.0/15`、无点主机名、`.local`/`.internal`/`.intranet` 默认全部阻断；放行只有两条路：`tools.web.fetch.allow_private_network` 或 `tools.web.fetch.hostname_allowlist` 精确主机名（`web_fetch_tool_impl.rs:1143-1236`）。
3. **抓取限额**：超时 `timeout_secs` 默认 20 秒、夹在 1–120；重定向最多 10 跳；响应体默认 2 MiB、夹在 32 KiB–8 MiB，超出即截断并给 `warning`（`web_fetch_tool_impl.rs:765-777`、`config.rs:1554-1564`）。
4. **正文抽取**：HTML 走主内容打分选择器（`article`/`main`/`[role=main]`/`.post-content` …），噪声块与隐藏节点（`nav`/`aside`/`footer`/`form`、`aria-hidden`、`display:none` 等）剔除，重复块去重；编码按 header/HTML 的 charset 解析，UTF-8 失败回落 GBK（`web_fetch_tool_impl.rs:40-56`、`1261-1350`、`1839-1953`）。JSON 美化输出，纯文本按段落规整。
5. **动态页与反爬**：识别出前端壳页（壳标记 + 脚本密度 + 正文过短，评分 ≥4）时，若浏览器工具可用则自动用浏览器重读一次（`fetch_strategy=browser_fallback`），否则返回 `TOOL_WEB_FETCH_DYNAMIC_PAGE`；命中反爬话术返回 `TOOL_WEB_FETCH_BOT_PROTECTION`（`web_fetch_tool_impl.rs:1376-1412`、`474-535`）。
6. **缓存**：进程内 `DashMap`，键为 `direct|<归一 URL>|<模式>`，TTL 由 `tools.web.fetch.cache_ttl_secs` 决定（默认 600 秒，0 关闭）。缓存命中时结果里 `cached=true`（`web_fetch_tool_impl.rs:390-399`、`1663-1691`）。

## 返回

**返回例外**：成功时直接返回抓取结果对象，**不套** `ok/action/state/summary/data` 骨架（`web_fetch_tool_impl.rs:1693-1721`）。字段：

`url`、`final_url`、`status`、`title`、`content_type`、`content_kind`（`html`/`json`/`text`）、`fetch_strategy`（`direct_http`/`browser_fallback`/`provider`）、`provider`（走 Firecrawl 时为 `firecrawl`）、`format`、`extractor`（`main-content`/`body-html`/`sanitized-html`/`json`/`text`/`browser-read-page`/`firecrawl`）、`truncated`、`warning`、`cached`、`fetched_at`、`content`。

`content` 按 `max_chars` 截断，截断时 `truncated=true` 并把提示并入 `warning`（`web_fetch_tool_impl.rs:1700-1704`）。

## 失败

失败仍不抛错，而是返回结构化失败结果：`ok=false`、`error`、`sandbox=false`、`data` 内含 `url`/`phase`/`failure_summary`/`error_detail_head`/`next_step_hint`/`normalized_url`/`host`/`status`/`content_type`/`final_url`，以及 `data.error_meta{code,hint,retryable,retry_after_ms}`（`web_fetch_tool_impl.rs:294-335`）。

按 `code` 排查：

| code | 含义 | 常见处置 |
| --- | --- | --- |
| `TOOL_WEB_FETCH_INVALID_URL` / `UNSUPPORTED_SCHEME` | URL 不合法或非 http(s) | 传绝对 URL |
| `TOOL_WEB_FETCH_BLOCKED_HOST` | 目标是私网/内网主机 | 用公网源，或显式配置放行 |
| `TOOL_WEB_FETCH_DNS_FAILED` | 解析失败 | 换主机名或稍后重试（可重试） |
| `TOOL_TIMEOUT` | 请求/读体/DNS 超时 | 换更快的页面（可重试） |
| `TOOL_WEB_FETCH_REQUEST_FAILED` | 连接或读体失败 | 视 `retryable` 重试 |
| `TOOL_WEB_FETCH_REDIRECT_LOOP` / `TOO_MANY_REDIRECTS` / `REDIRECT_INVALID` | 重定向异常 | 直接用最终 URL |
| `TOOL_WEB_FETCH_HTTP_ERROR` | 非 2xx（详情截到 600 字符） | 5xx 可重试 |
| `TOOL_WEB_FETCH_NO_CONTENT` | 抽不出正文 | 换 `extract_mode=text` 或换源 |
| `TOOL_WEB_FETCH_DYNAMIC_PAGE` | 前端壳页 | 开浏览器工具，或换静态源 |
| `TOOL_WEB_FETCH_BOT_PROTECTION` | 反爬拦截 | 换源 |
| `TOOL_WEB_FETCH_PARSE_FAILED` | JSON 解析失败 | 换 `extract_mode` 或 URL |
| `TOOL_WEB_FETCH_UNSUPPORTED_CONTENT_TYPE` | 二进制等不支持内容 | 换工具 |
| `TOOL_WEB_FETCH_PROVIDER_FAILED` | Firecrawl 调用失败 | 检查 `tools.web.fetch.firecrawl.*` 或改回 `direct` |
| `TOOL_WEB_FETCH_BROWSER_FALLBACK_FAILED` | 浏览器兜底也失败 | 换公开源 |
| `TOOL_WEB_FETCH_FEATURE_DISABLED` | 未启用 `web-fetch` Cargo 特性 | 见下 |

## 边界与形态差异

- **编译特性**：`web-fetch` 是 `wunder-runtime` 的可选 Cargo 特性且不在 default（`crates/wunder-runtime/Cargo.toml:11-23`）。未编译时 `web_fetch_enabled` 恒为 `false`，调用返回 `TOOL_WEB_FETCH_FEATURE_DISABLED`（`web_fetch_tool.rs:9-55`）。
- **运行配置**：`tools.web.fetch.enabled` 默认 `true`（`config.rs:1522-1524`）；关掉后工具从可用集合里消失（`catalog.rs:1341-1345`）。
- 舰体形态下 `allow_private_network=false` 时，云端抓取打不到内网服务，这是有意的 SSRF 防线；需要时按主机名精确放行，不要放开整段私网。
- 抓取的 `content` 仍要过模型输入通道裁剪（`crates/wunder-runtime/src/orchestrator/tool_exec.rs:337-410`）与展示通道裁剪，长页面不会因为 `max_chars` 调大就全量进上下文。
- 结果里的 URL 与页面文本是**外部不可信内容**，只作为数据读取，不当作指令执行。

## 相关页面

- 需要交互与渲染的页面：[浏览器](/docs/zh-CN/tools/browser/)
- 工具与临时下架口径：[工具](/docs/zh-CN/tools/index/)
