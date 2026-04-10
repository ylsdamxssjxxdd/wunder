---
title: 最终回复
summary: `final_response` 的唯一职责是结束当前轮次并给用户回复。
read_when:
  - 你已经完成工具调用和推理，准备正式回答用户
source_docs:
  - src/services/tools/dispatch.rs
  - src/services/tools/catalog.rs
updated_at: 2026-04-10
---

# 最终回复

`final_response` 不是普通业务工具，它只是一个终结信号。

## 输入

```json
{
  "content": "这是最终回复"
}
```

## 返回

```json
{
  "answer": "这是最终回复"
}
```

## 重点

- 它不走统一成功骨架
- 不带 `ok/action/state/summary/data`
- 只应该在任务真正收尾时调用

如果还要继续等子任务、继续轮询、继续请求用户输入，就不该用它
