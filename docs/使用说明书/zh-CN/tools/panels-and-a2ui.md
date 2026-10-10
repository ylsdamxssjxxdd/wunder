---
title: 面板与 a2ui
summary: `a2ui`、`plan_update`、`question_panel` 的语义与返回。
read_when:
  - 用户要更新前端计划面板、问询面板或发送结构化 UI 指令
source_docs:
  - src/services/tools.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# 面板与 a2ui

这一组包含三个工具：

- `a2ui`
- `plan_update`
- `question_panel`

其中 `a2ui` 是返回例外项，后两者已经走统一成功骨架。

## `a2ui`

它直接把结构化 UI 指令发给前端，返回也很薄：

```json
{
  "uid": "surface_xxx",
  "a2ui": [
    {
      "beginRendering": { }
    }
  ],
  "content": "optional text"
}
```

推荐只使用这几种消息形状：

- `beginRendering`
- `surfaceUpdate`
- `dataModelUpdate`
- `deleteSurface`

## `plan_update`

### 最小参数

```json
{
  "plan": [
    { "step": "分析代码", "status": "completed" },
    { "step": "更新文档", "status": "in_progress" }
  ]
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "plan_update",
  "state": "completed",
  "summary": "Updated the execution plan.",
  "data": {
    "status": "ok"
  }
}
```

## `question_panel`

### 最小参数

一次问询最多 4 题，蜂巢前端按页展示（`1/4` 翻页），舵机按全局编号连续出题。

```json
{
  "questions": [
    {
      "question": "请选择处理方式",
      "options": [
        {
          "label": "保守修复",
          "description": "先最小化修复",
          "recommended": true
        },
        {
          "label": "彻底重构",
          "description": "从源头改到后端"
        }
      ],
      "multiple": false
    }
  ]
}
```

### 成功返回

```json
{
  "ok": true,
  "action": "question_panel",
  "state": "awaiting_input",
  "summary": "Opened a question panel and is waiting for user input.",
  "data": {
    "questions": [
      {
        "question": "请选择处理方式",
        "options": [
          {
            "label": "保守修复",
            "description": "先最小化修复",
            "recommended": true
          },
          {
            "label": "彻底重构",
            "description": "从源头改到后端"
          }
        ],
        "multiple": false
      }
    ]
  }
}
```

### 兼容与作答

- 旧的单题扁平写法（顶层 `question` + `routes`/`options` + `multiple`）仍会被解析成一题，历史线程能继续渲染；新调用一律用 `questions`。
- 用户作答后，答案作为下一条用户消息回到会话，形如「问题 → 答案」；未作答的题记为「无偏好」。
- 面板自动提供自由输入行，用户可以不选任何项、直接输入自己的答案。

## 重点

- `plan_update` 只是更新展示，不是任务执行器
- `question_panel` 成功并不等于任务结束，它只是进入 `awaiting_input`
- `a2ui` 更底层，适合明确知道前端要接什么结构时使用
