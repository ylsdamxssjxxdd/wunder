---
title: 浏览器
summary: 浏览器是交互型工具，用于在 desktop 模式下真实打开页面、点击、输入和截图，与 web_fetch 分工明确。
read_when:
  - 你在做桌面自动化或网页交互
  - 你要区分浏览器和网页抓取
source_docs:
  - src/services/tools/browser_tool.rs
  - src/services/tools/catalog.rs
---

# 浏览器

真实网页交互工具，仅在 Desktop 模式下可用。

---

## 功能说明

`浏览器` 是交互型工具，用于真实打开页面、点击、输入和截图。

**别名**：
- `browser`
- `browser_tool`

**独立动作别名**：
- `browser_navigate`
- `browser_click`
- `browser_type`
- `browser_screenshot`
- `browser_read_page`
- `browser_close`

---

## 参数说明

| 参数名 | 类型 | 必填 | 说明 |
|--------|------|------|------|
| `action` | string | ✅ | 要执行的动作 |
| `url` | string | ❌ | 目标 URL（navigate 动作使用） |
| `selector` | string | ❌ | CSS 选择器（click/type 动作使用） |
| `text` | string | ❌ | 要输入的文本（type 动作使用） |

---

## 支持的动作

| 动作 | 说明 | 必填附加参数 |
|------|------|--------------|
| `navigate` | 导航到 URL | `url` |
| `click` | 点击元素 | `selector` |
| `type` | 输入文本 | `selector`, `text` |
| `screenshot` | 截图 | - |
| `read_page` | 读取页面 | - |
| `close` | 关闭浏览器 | - |

---

## 使用示例

### 导航到页面

```json
{
  "action": "navigate",
  "url": "https://example.com"
}
```

### 点击元素

```json
{
  "action": "click",
  "selector": "#submit-button"
}
```

### 输入文本

```json
{
  "action": "type",
  "selector": "#username",
  "text": "myusername"
}
```

### 截图

```json
{
  "action": "screenshot"
}
```

---

## 完整工作流示例

### 搜索并截图

```json
// 1. 导航到搜索引擎
{
  "action": "navigate",
  "url": "https://www.bing.com"
}

// 2. 输入搜索词
{
  "action": "type",
  "selector": "#sb_form_q",
  "text": "wunder AI"
}

// 3. 点击搜索按钮
{
  "action": "click",
  "selector": "#sb_form_go"
}

// 4. 等待页面加载
{
  "wait_ms": 2000
}

// 5. 截图
{
  "action": "screenshot"
}

// 6. 关闭浏览器
{
  "action": "close"
}
```

---

## 与 web_fetch 的对比

| 特性 | 浏览器 | [网页抓取](/docs/zh-CN/tools/web-fetch/) |
|------|--------|--------------------------------|
| 用途 | 交互型操作 | 读网页正文 |
| 成本 | 较高（需要真实浏览器） | 较低（直接 HTTP 请求） |
| 能力 | 点击、输入、截图 | 提取正文 |
| 推荐使用 | 需要交互时 | 只需要读正文时 |

---

## 适用场景

✅ **适合使用浏览器**：
- 需要真实打开页面
- 需要点击或输入
- 需要截图确认页面状态
- 要在同一个页面会话里继续操作

❌ **不适合使用浏览器**：
- 只是想读一篇文章或帮助页正文
- 不需要交互
- 想尽量减少上下文噪声和执行成本

这些场景优先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)。

---

## 可见性限制

浏览器工具仅在以下条件下可用：
- 运行形态是 `desktop`
- 配置 `tools.browser.enabled = true`

Server 和 CLI 模式下不可用。

---

## 注意事项

1. **不是更强版网页抓取**：
   - 浏览器和 web_fetch 解决的是两类问题
   - 不要混淆使用

2. **会话保持**：
   - 既然已经进入浏览器会话，再退回 web_fetch 读同一页面，通常会丢失交互上下文

3. **使用限制**：
   - 浏览器工具可见，不代表当前运行环境就适合高频自动化
   - 它仍然受 desktop 条件限制

---

## 延伸阅读

- [网页抓取](/docs/zh-CN/tools/web-fetch/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)
- [Desktop 界面](/docs/zh-CN/surfaces/desktop-ui/)
