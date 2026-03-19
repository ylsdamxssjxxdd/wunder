---
title: 工作区 API
summary: `/wunder/workspace*` 是 Wunder 的文件控制面，负责目录浏览、内容读写、上传下载和容器路由。
read_when:
  - 你在做工作区面板、文件上传下载或产物回传
  - 你要理解 `container_id` 为什么会覆盖 `agent_id`
source_docs:
  - docs/API文档.md
  - src/api/workspace.rs
  - docs/设计方案.md
---

# 工作区 API

Wunder 的工作区接口不是简单的文件读写接口，而是文件隔离语义的外部控制面。

## 这页解决什么

这页只回答这些问题：

- 工作区相关接口有哪些
- `user_id`、`agent_id`、`container_id` 如何共同决定路由
- 什么时候应该用工作区，而不是 `temp_dir`

## 最常用的接口

- `GET/DELETE /wunder/workspace`
- `GET /wunder/workspace/content`
- `GET /wunder/workspace/search`
- `POST /wunder/workspace/upload`
- `GET /wunder/workspace/download`
- `GET /wunder/workspace/archive`
- `POST /wunder/workspace/dir`
- `POST /wunder/workspace/move`
- `POST /wunder/workspace/copy`
- `POST /wunder/workspace/batch`
- `POST /wunder/workspace/file`

## 什么时候看这组接口

- 你要做文件树、文件预览和编辑器
- 你要把工具产物落盘后继续处理
- 你要按容器隔离不同智能体的产物
- 你要给用户提供压缩下载或目录归档

## 路由规则先记这一条

工作区路由优先级当前很明确：

1. 显式传入 `container_id`
2. 否则看 `agent_id` 绑定的 `sandbox_container_id`
3. 再回退到默认用户工作区或旧兼容路由

所以当你显式传了 `container_id`，它的优先级高于 `agent_id`。

## 这组接口最容易用错的地方

### 把工作区当临时目录

不对。

工作区强调持久化和后续可继续处理，`temp_dir` 强调临时中转。

### 直接传真实磁盘绝对路径

不对。

这里大多数接口都使用相对工作区路径，而不是宿主机绝对路径。

### 以为只有下载接口才需要 `container_id`

不对。

当前 `/wunder/workspace*` 全部接口都支持显式 `container_id`。

## 最短理解方式

如果你在做一个文件面板，可以这样配：

- 目录页：`GET /wunder/workspace`
- 文件预览：`GET /wunder/workspace/content`
- 搜索：`GET /wunder/workspace/search`
- 写文件：`POST /wunder/workspace/file`
- 上传：`POST /wunder/workspace/upload`
- 导出：`GET /wunder/workspace/download` 或 `archive`

## 你最需要记住的点

- 工作区的核心不是文件 API，而是隔离后的文件空间。
- 显式 `container_id` 会覆盖 `agent_id` 推导。
- 工作区适合持久产物，`temp_dir` 适合中转与分发。

## 相关文档

- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [工作区路由参考](/docs/zh-CN/reference/workspace-routing/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)
