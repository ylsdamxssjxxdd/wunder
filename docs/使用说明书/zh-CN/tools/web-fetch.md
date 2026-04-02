---
title: 网页抓取
summary: web_fetch 适合读取网页正文，通过 HTTP/HTTPS 拉取页面，不维护浏览器会话，与浏览器工具分工明确。
read_when:
  - 你要让模型读取网页正文
  - 你不确定该用网页抓取还是浏览器
source_docs:
  - src/services/tools/web_fetch_tool.rs
  - src/services/tools/catalog.rs
---

# 网页抓取

网页正文提取工具，直接通过 HTTP/HTTPS 拉取页面。

---

## 功能说明

`web_fetch` 通过 HTTP/HTTPS 拉取页面，先做正文提取、噪声剔除和字符预算裁剪，再返回给模型。

**别名**：
- `web_fetch`
- `web_fetch_tool`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `url` | string | ✅ | 目标 URL |
| `extract_mode` | string | ❌ | 提取模式：`markdown` 或 `text`，默认 `markdown` |
| `max_chars` | integer | ❌ | 最大字符数，最小 100 |

---

## 使用示例

### 简单抓取

```json
{
  "url": "https://example.com"
}
```

### 指定提取模式

```json
{
  "url": "https://example.com",
  "extract_mode": "text"
}
```

### 限制输出长度

```json
{
  "url": "https://example.com",
  "max_chars": 5000
}
```

---

## 返回字段

| 字段 | 说明 |
|------|------|
| `final_url` | 最终 URL（跟随重定向后） |
| `status` | HTTP 状态码 |
| `title` | 页面标题 |
| `content_type` | 内容类型 |
| `format` | 返回格式 |
| `extractor` | 使用的提取器 |
| `truncated` | 是否被截断 |
| `warning` | 警告信息 |
| `cached` | 是否命中缓存 |
| `fetched_at` | 获取时间 |
| `content` | 正文内容 |

---

## 处理流程

1. 拦截私网或内网目标
2. 对重定向逐跳复校验
3. 正文定位和提取
4. 过滤导航、页脚、相关推荐等噪声
5. 重复块去重
6. 字符预算裁剪

---

## 与浏览器的对比

| 特性 | 网页抓取 | [浏览器](/docs/zh-CN/tools/browser/) |
|------|----------|--------------------------------|
| 方式 | HTTP/HTTPS 请求 | 真实浏览器会话 |
| 成本 | 较低 | 较高 |
| 能力 | 提取正文 | 点击、输入、截图、交互 |
| 推荐使用 | 只需要读正文 | 需要交互 |

---

## 适用场景

✅ **适合使用 web_fetch**：
- 读文章、帮助页、说明页正文
- 不需要点击、输入、登录和表单提交
- 目标页面主要是静态内容
- 希望结果尽量短、干净、适合直接进模型上下文

❌ **不适合使用 web_fetch**：
- 要点按钮、填表单
- 要登录后再读页面
- 要在真实页面状态里连续交互
- JS 强交互页面

这些场景优先用 [浏览器](/docs/zh-CN/tools/browser/)。

---

## 配置说明

核心配置位于 `tools.web.fetch.*`：

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `enabled` | `true` | 是否启用 |
| `timeout_secs` | `20` | 超时时间（秒） |
| `max_redirects` | `3` | 最大重定向次数 |
| `max_response_bytes` | `2MB` | 最大响应字节数 |
| `max_chars` | `12000` | 默认最大字符数 |
| `max_chars_cap` | `30000` | 最大字符数上限 |
| `cache_ttl_secs` | `600` | 缓存 TTL（秒） |

---

## 注意事项

1. **不是浏览器的轻量模式**：
   - web_fetch 根本不维护交互状态
   - 不要与浏览器工具混淆

2. **max_chars 是结果预算**：
   - 不代表网页原始大小
   - 超过会被截断

3. **JS 强交互页面**：
   - web_fetch 往往不如浏览器可靠
   - 建议用浏览器工具

---

## 延伸阅读

- [浏览器](/docs/zh-CN/tools/browser/)
- [工具总览](/docs/zh-CN/tools/)
