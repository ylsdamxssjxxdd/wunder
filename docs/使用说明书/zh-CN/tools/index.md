---
title: 工具总览
summary: 当前 wunder 工具体系、统一返回骨架、状态语义、例外项与选型建议。
read_when:
  - 你要先判断该用哪一类工具
  - 你要理解工具最新的返回结构
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
  - src/services/tools/tool_error.rs
  - docs/工具返回内容优化表.md
updated_at: 2026-04-10
---

# 工具总览

这组文档现在应该按下面的心智模型来读：

- 先看“这件事该用哪类工具”
- 再看“这个工具成功/失败到底怎么返回”
- 最后看“这个工具的关键动作和最小必填参数”

## 先记住这件事

wunder 的内置工具已经不再是“每个工具各回各的格式”。大多数工具现在都收敛到统一骨架，模型和前端都应优先读取：

- 顶层 `ok`
- 顶层 `action`
- 顶层 `state`
- 顶层 `summary`
- 顶层 `data`

也就是说，**业务结果优先在 `data` 里取**。`src/services/tools.rs` 里已经有 `tool_result_data()` / `tool_result_field()` 这种兼容读取方式，文档也应按这个约定来理解。

## 统一成功返回

绝大多数内置工具成功时返回：

```json
{
  "ok": true,
  "action": "tool_action",
  "state": "completed",
  "summary": "Human-readable summary.",
  "data": {
    "tool_specific": "payload"
  }
}
```

部分工具还会附带：

```json
{
  "next_step_hint": "What to do next"
}
```

### 常见 `state`

- `completed`：本次动作已经完成
- `dry_run`：只做校验或预演，没有真正执行
- `accepted`：任务已接收，但还没拿到最终结果
- `running`：仍在运行中
- `yielded`：已让出当前轮次控制权，不是最终回复
- `awaiting_input`：已经打开前端面板，正在等用户输入
- `partial`：只完成了一部分，通常需要继续跟进
- `timeout`：等待超时，但并不一定代表目标彻底失败
- `noop`：动作合法，但没有实际改动

## 统一失败返回

大多数内置工具失败时返回：

```json
{
  "ok": false,
  "error": "Human-readable error message.",
  "sandbox": false,
  "data": {
    "tool_specific": "debug payload",
    "error_meta": {
      "code": "TOOL_EXAMPLE_ERROR",
      "hint": "What to fix before retrying.",
      "retryable": false,
      "retry_after_ms": null
    }
  },
  "error_meta": {
    "code": "TOOL_EXAMPLE_ERROR",
    "hint": "What to fix before retrying.",
    "retryable": false,
    "retry_after_ms": null
  }
}
```

阅读失败结果时优先看：

1. `error_meta.code`
2. `error_meta.hint`
3. `data` 里的上下文

## 不是完全统一骨架的例外

下面这些工具要单独记：

### `final_response`

它不是标准工具结果，而是一个非常薄的终结信号：

```json
{
  "answer": "给用户的最终回复"
}
```

### `a2ui`

它本质上是向前端发结构化 UI 指令，返回也保持薄包装：

```json
{
  "uid": "optional-surface-id",
  "a2ui": [ ... ],
  "content": "optional text"
}
```

### `schedule_task`

它当前走的是压缩后的调度返回，不带统一的 `ok/action/state/summary/data` 骨架。常见成功结果更像：

```json
{
  "action": "add",
  "job": {
    "job_id": "job_xxx",
    "name": "日报提醒",
    "enabled": true,
    "schedule": {
      "kind": "every",
      "every_ms": 300000
    },
    "next_run_at": "2026-04-10T10:00:00+08:00",
    "last_run_at": null
  },
  "deduped": false
}
```

### `browser`

浏览器工具主要透传浏览器运行时返回，成功时通常会有 `ok: true`，但不一定带统一的 `summary/data` 外壳。不同动作的字段差异很大。

### `web_fetch`

`web_fetch` 的成功结果目前也是直接返回抓取结果对象，而不是统一成功骨架：

```json
{
  "url": "https://example.com",
  "final_url": "https://example.com",
  "status": 200,
  "title": "Example",
  "content_type": "text/html",
  "content_kind": "html",
  "fetch_strategy": "direct_http",
  "format": "markdown",
  "extractor": "readability",
  "truncated": false,
  "cached": false,
  "fetched_at": "2026-04-10T03:00:00Z",
  "content": "..."
}
```

### `apply_patch`

`apply_patch` 成功时已经走统一骨架，但失败时会有自己额外的 patch 错误码和 hint，错误语义比普通文件工具更严格。

## 现在的工具分层

## 1. 基础收尾与前端协同

- [最终回复](/docs/zh-CN/tools/final-response/)
- [面板与 a2ui](/docs/zh-CN/tools/panels-and-a2ui/)
- [会话让出](/docs/zh-CN/tools/sleep/)：这里的 `sessions_yield` 也归入回合控制思路

## 2. 工作区与代码

- [工作区文件](/docs/zh-CN/tools/workspace-files/)
- [执行命令](/docs/zh-CN/tools/exec/)
- [应用补丁](/docs/zh-CN/tools/apply-patch/)
- [ptc](/docs/zh-CN/tools/ptc/)
- [LSP 查询](/docs/zh-CN/tools/lsp/)
- [技能调用](/docs/zh-CN/tools/skill-call/)
- [读图工具](/docs/zh-CN/tools/read-image/)

## 3. Web 与桌面

- [网页抓取](/docs/zh-CN/tools/web-fetch/)
- [浏览器](/docs/zh-CN/tools/browser/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)

## 4. 线程、子智能体、蜂群

- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)

## 5. 系统连接与记忆

- [自我状态](/docs/zh-CN/tools/self-status/)
- [记忆管理](/docs/zh-CN/tools/memory-manager/)
- [用户世界工具](/docs/zh-CN/tools/user-world/)
- [渠道工具](/docs/zh-CN/tools/channel/)
- [A2A 工具](/docs/zh-CN/tools/a2a-tools/)
- [节点调用](/docs/zh-CN/tools/node-invoke/)
- [定时任务](/docs/zh-CN/tools/schedule-task/)
- [睡眠等待](/docs/zh-CN/tools/sleep/)

## 选型建议

### 只读网页正文

先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)，不要一上来就用浏览器。

### 要点击、输入、等页面动态渲染

用 [浏览器](/docs/zh-CN/tools/browser/)。

### 要看本地代码

通常顺序是：

1. [工作区文件](/docs/zh-CN/tools/workspace-files/) 先列目录或搜索
2. [工作区文件](/docs/zh-CN/tools/workspace-files/) 再读片段
3. 需要符号级理解时再用 [LSP 查询](/docs/zh-CN/tools/lsp/)

### 要改代码

- 小而精确的修改：用 [应用补丁](/docs/zh-CN/tools/apply-patch/)
- 整文件重写：用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `write_file`
- 要编译、测试、跑脚本：用 [执行命令](/docs/zh-CN/tools/exec/)
- 纯 Python 临时程序：用 [ptc](/docs/zh-CN/tools/ptc/)

### 要做协作

- 当前会话里临时拉起子运行：用 [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- 调度用户已经拥有的其他智能体：用 [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- 管理主线程/分支线程：用 [会话线程控制](/docs/zh-CN/tools/thread-control/)

## 这次工具改版的重点变化

- 大多数内置工具已经统一到了 `ok/action/state/summary/data`
- `data` 成为主要结果承载区
- 很多工具的 schema 明显收紧，模型侧更强调扁平、显式、`additionalProperties: false`
- `subagent_control`、`thread_control`、`agent_swarm` 都已经转成“动作明确、状态明确、后续 hint 明确”的风格
- `schedule_task`、`browser`、`web_fetch`、`final_response`、`a2ui` 仍然属于需要单独记忆的例外

## 下一步

- 如果你先关心“文件和代码怎么读写”，直接看 [工作区文件](/docs/zh-CN/tools/workspace-files/)
- 如果你先关心“子智能体和蜂群怎么协作”，直接看 [子智能体控制](/docs/zh-CN/tools/subagent-control/) 和 [智能体蜂群](/docs/zh-CN/tools/agent-swarm/)
- 如果你先关心“当前某个工具具体返回什么”，跳到对应单工具页，那里已经按最新实现列出成功返回结构
