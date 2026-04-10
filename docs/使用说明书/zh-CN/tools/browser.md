---
title: 浏览器
summary: 浏览器自动化工具的动作、返回特征与和 `web_fetch` 的分工。
read_when:
  - 你要打开网页、点击、输入、截图或读取动态页面
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/browser/runtime.rs
updated_at: 2026-04-10
---

# 浏览器

浏览器工具的最大特点是：**成功结果主要透传浏览器运行时，而不是统一包一层 `summary/data`。**

所以看它的返回时，不要机械地期待所有动作都长成统一骨架。

## 什么时候用它

- 页面是动态渲染的
- 需要点击、输入、按键、等待
- 需要浏览器截图
- `web_fetch` 抓不到有效正文

## 常用动作

- `status`
- `profiles`
- `start`
- `stop`
- `tabs`
- `open`
- `focus`
- `close`
- `navigate`
- `snapshot`
- `act`
- `screenshot`
- `read_page`
- 快捷动作：`click`、`type`、`press`、`hover`、`wait`

## 最小参数示例

导航：

```json
{
  "action": "navigate",
  "browser_session_id": "sess_xxx",
  "url": "https://example.com"
}
```

读取页面：

```json
{
  "action": "read_page",
  "browser_session_id": "sess_xxx",
  "max_chars": 12000
}
```

## 返回结构怎么理解

### `status`

更像运行时状态：

```json
{
  "ok": true,
  "enabled": true,
  "tool_visible": true,
  "default_profile": "default",
  "profiles": ["default"],
  "limits": { ... },
  "playwright": { ... },
  "docker": { ... },
  "control": { ... },
  "sessions": ["sess_xxx"]
}
```

### `stop`

```json
{
  "ok": true,
  "closed": true,
  "browser_session_id": "sess_xxx"
}
```

### `screenshot`

会把桥接层回传的 `image_base64` 落成文件，再返回下载信息，典型字段包括：

```json
{
  "ok": true,
  "filename": "browser_xxx.png",
  "download_url": "/wunder/temp_dir/download?...",
  "...": "other browser runtime fields"
}
```

### `read_page` / `snapshot` / `navigate` / `tabs`

这些字段由浏览器桥接层决定，通常至少会有 `ok: true` 和动作相关数据。

## 与 `web_fetch` 的区别

- `web_fetch`：优先读静态正文，成本更低
- `browser`：优先解决交互、动态渲染、页面自动化

如果只是读公开网页正文，先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)。  
只有在页面依赖前端渲染、验证流程或必须交互时，再切到浏览器。
