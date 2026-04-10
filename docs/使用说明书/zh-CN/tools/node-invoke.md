---
title: 节点调用
summary: 网关节点列表与命令调用。
read_when:
  - 你要列出可用节点或向指定节点发送命令
source_docs:
  - src/services/tools.rs
updated_at: 2026-04-10
---

# 节点调用

`node_invoke` 主要两个动作：

- `list`
- `invoke`

## `list`

```json
{
  "ok": true,
  "action": "list",
  "state": "completed",
  "summary": "Listed 6 gateway nodes.",
  "data": {
    "state_version": 42,
    "count": 6,
    "nodes": [ ... ]
  }
}
```

## `invoke`

```json
{
  "ok": true,
  "action": "invoke",
  "state": "completed",
  "summary": "Invoked command ping on node node_a.",
  "data": {
    "node_id": "node_a",
    "command": "ping",
    "result": { ... }
  }
}
```

## 重点

- 先 `list` 再 `invoke`
- 真正的命令执行结果在 `data.result`
