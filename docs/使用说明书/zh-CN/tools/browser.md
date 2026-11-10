---
title: 浏览器工具
summary: 单一 `browser` action 工具的启用条件、严格动作 schema、批处理、跨刷新 ref 恢复、降噪后的观察输出、截图回传与限额。
updated_at: 2026-11-10
---

# 浏览器工具

启用后，模型只调用一个 `browser` 工具。每次调用必须选择一个 `action`；动作分支独立声明允许字段、必填字段和互斥条件，服务端会在调用 Playwright bridge 前再次校验。旧 `browser_*` 工具名不能调用。

静态公开页面优先使用[网页抓取](/docs/zh-CN/tools/web-fetch/)。需要导航、标签页、点击、输入、等待动态渲染、截图或读取可操作页面引用时使用浏览器工具。

## 启用条件

需要同时启用浏览器工具和浏览器运行时开关。启用后 `browser` 自动注册，无需加入内置工具白名单。

## 动作

| action | 用途 | 要点 |
| --- | --- | --- |
| `navigate` / `back` | 打开 URL 或后退 | `navigate` 需要 http(s) URL；默认不附页面正文 |
| `tabs_list` / `tabs_new` / `tabs_select` / `tabs_close` | 列出、创建、选择或关闭标签页 | select/close 需要 `target_id` 或 `index` |
| `observe_refs` | 读取可访问性快照 | 返回后续点击和输入使用的 element ref；同页重渲染或刷新后仍会按 selector 自动恢复 |
| `observe_text` | 分页读取当前页正文 | 可使用同一未变化页面返回的 `next_cursor` |
| `screenshot` | 截图 | 可指定工作区相对 `path`；保存为工作区附件，并把图像追加给模型查看 |
| `click` / `type` / `hover` / `select` / `drag` / `press` | 页面交互 | 元素动作提供且只提供一种 `ref` 或 `selector`；`drag` 提供完整来源和目标；type/select/press 有专属必填字段 |
| `wait` | 等加载、选择器、文本、URL 或有界时间 | 已知成功条件优先于固定等待 |
| `batch` | 顺序执行 1–10 个依赖步骤 | 每步使用严格 kind schema，不能嵌套 batch；步骤末尾可用 `back` / `observe_text` 直接取证 |

典型流程：`navigate` → `observe_refs` → 交互或 `batch` → `wait` → `observe_text` 验证。普通 DOM 更新、SPA 重渲染或同 URL 自刷新不会让 ref 直接失效：动作会按 ref 记录的 selector 重新定位，页面短暂刷新时最多等待约 0.8 秒。只有真正导航到另一个文档、且 selector 也找不到时，才需要重新观察。

`observe_refs` 的文本会跳过没有可访问名称的链接/图标按钮，并合并同角色、同名称、同状态的重复控件（如导航栏里重复 20 次的同一个链接），重复数量只记在 `stats.duplicates`，不再逐条输出。

## 批处理

```json
{
  "action": "batch",
  "steps": [
    {"kind": "type", "ref": "e1", "text": "sample"},
    {"kind": "click", "ref": "e2"},
    {"kind": "wait", "url": "**/complete", "timeout_ms": 5000},
    {"kind": "observe_text", "max_chars": 2000}
  ]
}
```

批处理内的 `observe_text` 与顶层同名 action 行为一致，正文按每批 12,000 字符预算返回，并保留 `truncated`、`has_more` 与 `next_cursor`。

## 结果与限额

模型结果按 action 投影为下一步需要的最小数据。观察只保留快照正文、截断状态和 cursor；快照文本本身已经带有每个 ref，不再重复返回一整份 refs 数组。交互保留动作、目标引用、URL 和页面世代，并在跨刷新恢复 ref 时带 `ref_recovered`。截图移除 base64，保存为工作区附件，同时追加一条带图像的用户消息供模型查看；事件和审计保留完整结构化结果。

页面正文 cursor 带签名并绑定标签页、页面身份和 DOM 修订摘要。页面内容变化或 bridge 重启后旧 cursor 会被拒绝，必须重新观察。运行时不保存无界页面全文缓存。

只允许 http/https；`file://` 和内网地址默认被拦截。快照与正文默认 12,000 字符、最多请求 40,000 字符，批处理最多 10 步，浏览器会话、标签页、下载和超时也由运行时配置限制。

`evaluate`、原始 JavaScript、浏览器状态和关闭浏览器会话不属于模型 action；它们只保留给控制面接口。
