---
title: 工作区 API
summary: `/wunder/workspace*` 不只是文件接口，它把每个用户固定到属于自己的唯一云端目录。
read_when:
  - 用户在做工作区面板、文件上传下载或产物回传
  - 用户想知道 `container_id` 为什么不再起作用
source_docs:
  - docs/API文档.md
  - docs/设计文档/01-系统总体设计.md
---

# 工作区 API

做文件树、编辑器、上传下载或产物面板，先看这页。

`/wunder/workspace*` 不只是文件读写接口，它决定文件落在哪个用户目录。

## 本页重点

- 工作区相关接口有哪些
- 路由如何收敛到「每个用户唯一目录」
- 什么时候应该用工作区，而不是 `temp_dir`

## 路由规则（先看）

工作区路由现在只有一条路径：

1. 请求带已登录用户身份
2. 服务端解析出该用户的工作区根
3. 在该根下按相对路径定位条目

补充：

- `container_id` 已收敛：过渡期仍接受该参数，但不再决定目录，传任何值都按用户根解析；前端不提供容器选择。
- `agent_id` 不再参与目录派生，只影响会话与配置绑定；每个用户只有一个智能体实例。
- 子智能体与主智能体共用同一个用户目录。

## 接口使用场景

- 文件树和目录浏览
- 文件预览和编辑
- 上传下载和压缩打包
- 工具产物的持久化回传
- 目录用量统计（文件数、目录数、已用容量、最近修改文件）

## 常用接口分类

- `GET/DELETE /wunder/workspace`
- `GET /wunder/workspace/content`
- `GET /wunder/workspace/search`
- `GET /wunder/workspace/stats`
- `POST /wunder/workspace/upload`
- `GET /wunder/workspace/download`
- `GET /wunder/workspace/archive`
- `POST /wunder/workspace/dir`
- `POST /wunder/workspace/move`
- `POST /wunder/workspace/copy`
- `POST /wunder/workspace/batch`
- `POST /wunder/workspace/file`

做文件面板时，可以这样对应：

- 目录页：`GET /wunder/workspace`
- 文件预览：`GET /wunder/workspace/content`
- 搜索：`GET /wunder/workspace/search`
- 用量条与欢迎页概览：`GET /wunder/workspace/stats`
- 写文件：`POST /wunder/workspace/file`
- 上传：`POST /wunder/workspace/upload`
- 导出：`GET /wunder/workspace/download` 或 `archive`

`/workspace*` 系列返回裸对象（不加 `data` 包裹）。`stats` 的 `quota_bytes` 为 `null` 表示未配置配额，此时前端只显示已用量，不渲染配额分母。

## 常见误区

- 把工作区当临时目录。工作区适合持久产物，`temp_dir` 适合中转。
- 直接传真实磁盘绝对路径。这里所有接口都使用相对工作区路径。
- 以为传 `container_id` 能切到另一个目录。不会，服务端统一按用户根解析。
- 以为可以新增或更换工作区。不能，每个用户只有一个固定云端目录。

## 延伸阅读

- [工作区与工作目录](/docs/zh-CN/concepts/workspaces/)
- [工作区路由与文件管理](/docs/zh-CN/reference/workspace-routing/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)
