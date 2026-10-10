---
title: 浏览器
summary: 浏览器自动化的 17 个原生工具、最小参数、返回结构（content blocks）与和 `web_fetch` 的分工。
read_when:
  - 用户要打开网页、点击、输入、截图或读取动态页面
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/browser/provider.rs
  - src/services/browser/providers/playwright.rs
  - src/services/browser/runtime.rs
updated_at: 2026-10-10
---

# 浏览器

浏览器能力是一组**原生工具**（工具面与上游 harness 对齐），而不是一个带 `action` 开关的单一工具。每个动作都是一个独立工具名，模型直接按名字调用。

工具由 `BrowserProvider` 驱动，默认实现是 `playwright`（复用既有 Playwright 桥接引擎）。可通过配置 `browser.provider` 切换 provider；未知 provider 会回退到默认实现并打印警告。

## 工具清单

| 工具 | 作用 | 最小参数 |
| --- | --- | --- |
| `browser_navigate` | 当前标签跳转到 URL | `url` |
| `browser_navigate_back` | 后退 | 无 |
| `browser_tabs` | 列出 / 新建 / 选择 / 关闭标签 | `action`（`list`/`new`/`select`/`close`） |
| `browser_snapshot` | 抓无障碍快照，产出可供 click/type 使用的元素 ref | 无 |
| `browser_click` | 点击元素 | `ref` 或 `selector` |
| `browser_type` | 向元素输入文本 | `text`（配合 `ref`/`selector`） |
| `browser_hover` | 悬停元素 | `ref` 或 `selector` |
| `browser_select_option` | 选择 `<select>` 选项 | `ref`/`selector` + `value` |
| `browser_drag` | 拖拽一个元素到另一个 | `from_ref`/`from_selector` + `to_ref`/`to_selector` |
| `browser_press_key` | 按键，如 `Enter`、`Tab`、`ArrowDown` | `key` |
| `browser_evaluate` | 在页面上下文执行 JS 表达式并返回值 | `expression` |
| `browser_wait_for` | 等待固定时长、加载状态或文本出现 | `wait_ms` / `load_state` / `text` 之一 |
| `browser_take_screenshot` | 截图并以图片块返回 | 无（可选 `path`、`full_page`） |
| `browser_read_page` | 以 Markdown / 文本读取当前页 | 无（可选 `max_chars`） |
| `browser_batch` | 按顺序执行一批交互步骤（最多 10 步） | `steps` |
| `browser_close` | 关闭当前会话的浏览器 | 无 |
| `browser_status` | 上报运行时状态与已打开的会话 | 无 |

多标签操作（`browser_navigate_back` / `browser_click` / `browser_type` 等）默认作用于当前活动标签；需要指定标签时传 `target_id`。

## 适用场景

- 页面是动态渲染的
- 需要点击、输入、按键、等待
- 需要浏览器截图
- `web_fetch` 抓不到有效正文

## 最小参数示例

导航：

```json
{
  "url": "https://example.com"
}
```

抓快照后再点击（ref 来自快照）：

```json
{
  "ref": "e12"
}
```

输入并提交：

```json
{
  "ref": "e3",
  "text": "hello",
  "submit": true
}
```

读取页面：

```json
{
  "max_chars": 12000
}
```

截图到指定工作区相对路径：

```json
{
  "path": "browser/screenshots/home.png",
  "full_page": true
}
```

一次批量交互：

```json
{
  "steps": [
    { "kind": "click", "ref": "e1" },
    { "kind": "type", "ref": "e2", "text": "wunder" }
  ]
}
```

## 返回结构解读

成功结果采用 MCP 风格的内容块结构，并额外带一个 `provider` 字段标识实际使用的 provider：

```json
{
  "provider": "playwright",
  "ok": true,
  "content": [
    { "type": "text", "text": "Navigated to https://example.com." }
  ],
  "data": { "...": "provider 透传的运行时数据" }
}
```

- `content`：内容块数组。文本块为 `{ "type": "text", "text": "..." }`；图片块为 `{ "type": "image", "data": "<base64>", "mime_type": "image/png" }`。
- `error`：失败时出现的错误信息。
- `data`：provider 透传的原始运行时数据，字段随动作而定。
- 文本内容过长时会被截断，并在文本中标记已截断。

### `browser_status`

更像运行时状态，`data` 中通常包含：

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
  "sessions": ["sess_xxx"]
}
```

模型侧浏览器工具不会返回本地控制端点字段。舰桥 HTTP 状态接口可能包含 `control.host` / `control.port`，它们只是 wunder 内部浏览器控制配置，不是文件下载地址，也不应让模型拿去访问。

### `browser_take_screenshot`

截图会以图片内容块返回，同时把桥接层回传的图片保存到当前智能体工作区，默认路径为 `browser/screenshots/browser_shot_<id>.png`；可传 `path` 指定工作区相对路径（自动补 `.png` 扩展名）。

如果直接调用浏览器 HTTP 控制接口 `/wunder/browser/screenshot`，返回仍会写入 `temp_dir` 并提供 `/wunder/temp_dir/download?...` 下载链接，用于舰桥或调试端临时取图。

### 其他动作

`browser_read_page` / `browser_snapshot` / `browser_navigate` / `browser_tabs` 等的具体字段由 provider / 桥接层决定，通常至少包含 `ok: true` 与动作相关数据，统一放在 `data` 中。

## 运行时与依赖（对齐 dsh）

浏览器工具由 `playwright` provider 驱动，桥接进程用 Python Playwright 启动或接管浏览器。可通过 `browser.playwright` 选择浏览器来源，行为对齐 dsh 的 `launch` / `attach` 两种模式：

```yaml
browser:
  provider: playwright
  playwright:
    mode: launch            # launch=另起受控浏览器；attach=接管已在运行的浏览器
    channel: null           # 复用本机浏览器：chrome / msedge / chromium ...
    executable_path: null   # 直接指定浏览器可执行文件（优先级高于 channel）
    attach_endpoint: null   # mode=attach 时的 CDP 端点（http(s)://host:port 或 ws://...）
    browsers_path: null     # 留空时自动探测 <python_root>/ms-playwright（补充包自带位置）
    headless: true
    viewport_width: 1280
    viewport_height: 720
    timeout_secs: 60
```

- `mode: launch`（默认）：另起一个受控浏览器实例。**默认（`channel` / `executable_path` 均留空）即使用补充包内置的 Chromium**，不依赖用户本机浏览器；只有需要复用本机已安装浏览器时才设 `channel` 或 `executable_path`。
- `mode: attach`：连接到一个已在运行的浏览器（`attach_endpoint`），复用它的标签页与登录态；wunder 只接管页面，不会关闭用户自己的浏览器。
- `browser_status` 的 `data.playwright` 会回显 `launch_mode` / `channel` / `executable_path`，并以 `attach_endpoint_configured` 布尔值表示是否配置了接管端点（出于安全不返回端点原文，避免泄露内嵌令牌）。

依赖说明：

- 需要 Python 版 `playwright` 驱动包。桌面端「补充包」已在 `requirements-*-common.txt` 中固定 `playwright`；Linux 补充包还会把 **Chromium** 一并下载到 `opt/python/ms-playwright`，并被桥接层自动探测为默认浏览器（`browsers_path` 留空即可），开箱即用、可离线运行，**无需用户另装浏览器**。
- 补充包内置的是 **Chromium（开源）**，不是 Google Chrome 品牌版：Chrome 二进制受其许可协议限制、不允许随产品再分发，而 Chromium 与 Chrome 同一渲染引擎，可自由分发。
- Win7 补充包只带驱动、不带 Chromium（Playwright 自带 Chromium 需要 Windows 10+，Win7 无法运行），Win7 上请通过 `mode: attach` 或 `channel` / `executable_path` 复用本机浏览器；若要让 Windows 侧也「开箱自带浏览器」，需另出 Windows 10+ x64 补充包。

## 与 `web_fetch` 的区别

- `web_fetch`：优先读静态正文，成本更低
- 浏览器工具：优先解决交互、动态渲染、页面自动化

如果只是读公开网页正文，先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)。  
只有在页面依赖前端渲染、验证流程或必须交互时，再切到浏览器。

浏览器动作默认超时为 60 秒。对容易超时的页面，优先复用已有会话，再执行 `browser_navigate`，并按需传 `timeout_ms` 覆盖单次调用。