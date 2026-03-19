---
title: 工作区路由参考
summary: Wunder 的工作区路由不是单纯按 user_id 取目录，而是由 `container_id`、智能体容器配置和 scoped user_id 共同决定。
read_when:
  - 你在排查文件为什么落到了“另一个工作区”
  - 你要准确理解工作区接口的路由优先级
source_docs:
  - docs/API文档.md
  - src/api/workspace.rs
  - docs/设计方案.md
---

# 工作区路由参考

这页不是讲“工作区是什么”，而是讲“请求最后会路由到哪一块工作区”。

## 这页解决什么

这页只回答这些问题：

- `container_id` 和 `agent_id` 谁优先
- 为什么同一个 `user_id` 可能对应多块持久工作区
- scoped user_id 为什么还能直接访问容器工作区

## 路由优先级

当前 `/wunder/workspace*` 的路由优先级很明确：

1. 如果显式传了 `container_id`，优先按它路由
2. 否则如果智能体配置了 `sandbox_container_id`，按该容器路由
3. 再回退到默认 scoped user workspace 兼容策略

这意味着：

- 显式 `container_id` 高于 `agent_id`

## 容器约定

当前约定是：

- `container_id=0`：用户私有容器
- `container_id=1~10`：智能体运行容器

所以你看到“同一个用户为什么会有多块空间”，本质是容器语义在生效。

## 底层路径和公共路径不要混

公共视角里，你通常会看到：

- `/workspaces/{user_id}/...`

但实现层会把不同容器拆到不同真实目录。

所以：

- 公共路径更适合给模型和接口理解
- 真实目录更适合服务端内部隔离

## scoped user_id 是什么

当前已登录用户也可以显式传 scoped `user_id`，例如容器或智能体作用域 ID。

这让某些高级场景下，前端或调试工具可以直接访问对应隔离空间，而不必总是重新推导。

但如果你是新接入方，默认仍建议优先用：

- `user_id + container_id`

而不是自己拼 scoped ID。

## 什么时候最容易踩坑

### 以为 `agent_id` 一定决定工作区

不对。

只要显式传了 `container_id`，它就优先。

### 以为下载和上传才需要考虑容器

不对。

浏览、内容读取、搜索、上传、写文件、移动复制、归档下载都在同一套路由逻辑下。

### 以为本地模式就没有容器

也不对。

本地模式只是把容器映射到真实目录，不会取消容器语义。

## 你最需要记住的点

- 显式 `container_id` 优先级最高。
- `container_id=0` 和 `1~10` 的职责不同。
- 公共路径和真实目录名不是同一个概念。

## 相关文档

- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)
