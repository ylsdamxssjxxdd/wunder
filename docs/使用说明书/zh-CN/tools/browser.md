---
title: 浏览器
summary: 浏览器是交互型工具，用于操作浏览器运行时，支持会话、标签页、snapshot/ref/act、截图与读页。
read_when:
  - 你需要真实打开页面并持续交互
  - 你需要通过 snapshot/ref 操作页面元素
  - 你在评估 desktop、server、docker 中的浏览器自动化能力
source_docs:
  - src/services/browser/runtime.rs
  - src/services/tools/browser_tool.rs
  - src/api/browser_control.rs
  - src/services/tools/catalog.rs
---

# 浏览器

`浏览器` 是 Wunder 的浏览器运行时入口，不再只是旧版的 `navigate/click/type` 脚本包装。

它现在有三层职责：

- 对模型暴露统一工具协议
- 维护浏览器会话与标签页
- 提供 `snapshot -> ref -> act` 的交互链路

---

## 能力概览

主工具名：

- `浏览器`
- 英文别名：`browser`

保留的 legacy 别名：

- `browser_navigate`
- `browser_click`
- `browser_type`
- `browser_screenshot`
- `browser_read_page`
- `browser_close`

统一 action：

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
- `click`
- `type`
- `press`
- `hover`
- `wait`
- `screenshot`
- `read_page`

---

## 关键设计

### 1. 会话与标签页

- 同一智能体会话会复用一个浏览器 session
- 每个 session 支持多个标签页
- `tabs/open/focus/close` 用于 tab 生命周期管理

### 2. snapshot/ref/act

- `snapshot` 返回结构化页面快照
- 每个可交互元素会分配 `ref`
- `act` 优先通过 `ref` 定位，不再鼓励模型猜 CSS selector

### 3. legacy 兼容

- 旧的 `browser_click/browser_type/...` 仍可用
- 内部会自动转发到新的浏览器运行时

---

## 常用参数

| 参数 | 类型 | 说明 |
|------|------|------|
| `action` | string | 要执行的动作 |
| `profile` | string | 可选，浏览器 profile，默认 `managed` |
| `browser_session_id` | string | 可选，显式浏览器会话 ID |
| `target_id` | string | 可选，标签页 ID |
| `url` | string | 导航或开新标签页时使用 |
| `format` | string | `snapshot` 格式：`role/aria/ai` |
| `ref` | string | `snapshot` 返回的元素引用 |
| `selector` | string | 兼容入口，仍可直接传 CSS selector |
| `text` | string | 输入文本或等待文本 |
| `key` | string | `press` 动作使用的键值 |
| `request` | object | `act` 的结构化请求 |
| `full_page` | boolean | 截图是否抓整页 |
| `max_chars` | integer | `snapshot/read_page` 最大字符数 |

---

## 推荐智能体循环

### 浏览和交互

1. `start` 或直接 `navigate`
2. `snapshot`
3. 读取 `ref`
4. `act`
5. 必要时再次 `snapshot`

### 多标签页

1. `open`
2. `tabs`
3. `focus`
4. `navigate/act`

---

## 示例

### 打开页面并抓取快照

```json
{
  "action": "navigate",
  "url": "https://example.com"
}
```

```json
{
  "action": "snapshot",
  "format": "role"
}
```

### 用 ref 点击元素

```json
{
  "action": "act",
  "request": {
    "kind": "click",
    "ref": "e1"
  }
}
```

### 输入文本

```json
{
  "action": "act",
  "request": {
    "kind": "type",
    "ref": "e2",
    "text": "wunder browser runtime"
  }
}
```

### 管理标签页

```json
{
  "action": "open",
  "url": "https://www.bing.com"
}
```

```json
{
  "action": "tabs"
}
```

```json
{
  "action": "focus",
  "target_id": "tab-2"
}
```

---

## 配置

模型可见性：

```yaml
tools:
  browser:
    enabled: true
```

浏览器运行时：

```yaml
browser:
  enabled: true
  docker:
    enabled: true
```

不需要再把 `浏览器` 写进 `tools.builtin.enabled`，系统会在 `tools.browser.enabled=true` 时自动挂载工具。

兼容旧版 desktop：

- `server.mode=desktop`
- `tools.browser.enabled=true`

即使没有显式开启 `browser.enabled`，legacy desktop 仍然可用。

---

## Docker 准备

当前实现已经为 Docker 预留了运行条件：

- `PLAYWRIGHT_BROWSERS_PATH`
- `INSTALL_PLAYWRIGHT_BROWSERS`
- `/app/config/data/browser`
- `/app/config/data/browser/downloads`
- `--no-sandbox`
- `--disable-dev-shm-usage`
- `shm_size: 2gb`

当前仓库里的 Docker Compose 默认会：

- 构建时安装 Chromium（`INSTALL_PLAYWRIGHT_BROWSERS=1`）
- 运行时开启浏览器工具和浏览器运行时
- 给 Chromium 预留更大的 `/dev/shm`，避免白屏、崩溃和截图失败

建议：

- server 容器内运行 headless 浏览器
- 通过 volume 持久化浏览器缓存和下载目录
- 在生产环境收紧 `browser.security.allow_private_network`

---

## 与网页抓取的区别

| 工具 | 适合场景 |
|------|----------|
| `浏览器` | 真实交互、点击、输入、截图、跨标签页操作 |
| `网页抓取` | 直接读取网页正文、低噪声内容提取 |

---

## 扩展接口

除了模型工具外，还提供：

- `GET /wunder/browser/health`
- `GET /wunder/browser/status`
- `GET /wunder/browser/profiles`
- `POST /wunder/browser/session/start`
- `POST /wunder/browser/session/stop`
- `GET /wunder/browser/tabs`
- `POST /wunder/browser/tabs/open`
- `POST /wunder/browser/tabs/focus`
- `POST /wunder/browser/tabs/close`
- `POST /wunder/browser/navigate`
- `POST /wunder/browser/snapshot`
- `POST /wunder/browser/act`
- `POST /wunder/browser/screenshot`
- `POST /wunder/browser/read_page`

这些接口主要服务于后续调试面板、sidecar 和 Docker 运行形态。
