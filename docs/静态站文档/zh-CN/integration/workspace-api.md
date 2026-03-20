---
title: 工作区 API
summary: `/wunder/workspace*` 不只是文件接口，它决定文件到底落在哪个隔离工作区。
read_when:
  - 你在做工作区面板、文件上传下载或产物回传
  - 你要理解 `container_id` 为什么会覆盖 `agent_id`
source_docs:
  - docs/API文档.md
  - src/api/workspace.rs
  - docs/设计方案.md
---

# 工作区 API

如果你要做文件树、编辑器、上传下载或产物面板，这页先看。

`/wunder/workspace*` 不只是文件读写接口，它决定文件到底落在哪个隔离空间。

## 这页解决什么

- 工作区相关接口有哪些
- `user_id`、`agent_id`、`container_id` 如何共同决定路由
- 什么时候应该用工作区，而不是 `temp_dir`

## 先记一条路由规则

工作区路由优先级当前很明确：

1. 显式传入 `container_id`
2. 否则看 `agent_id` 绑定的 `sandbox_container_id`
3. 再回退到默认用户工作区或旧兼容路由

所以当你显式传了 `container_id`，它的优先级高于 `agent_id`。

## 这组接口用在什么地方

- 文件树和目录浏览
- 文件预览和编辑
- 上传下载和压缩打包
- 工具产物的持久化回传
- 按容器隔离不同智能体的文件空间

## 最常用的接口怎么分

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

如果你在做一个文件面板，可以这样理解：

- 目录页：`GET /wunder/workspace`
- 文件预览：`GET /wunder/workspace/content`
- 搜索：`GET /wunder/workspace/search`
- 写文件：`POST /wunder/workspace/file`
- 上传：`POST /wunder/workspace/upload`
- 导出：`GET /wunder/workspace/download` 或 `archive`

## 最容易搞错的点

- 把工作区当临时目录。工作区适合持久产物，`temp_dir` 适合中转。
- 直接传真实磁盘绝对路径。这里大多数接口都使用相对工作区路径。
- 以为只有下载接口才需要 `container_id`。实际上整组 `/wunder/workspace*` 都支持显式 `container_id`。

## 相关文档

- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [工作区路由参考](/docs/zh-CN/reference/workspace-routing/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)
