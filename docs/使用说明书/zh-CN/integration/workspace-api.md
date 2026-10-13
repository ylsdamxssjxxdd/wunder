---
title: 工作区 API
summary: 工作区不只管文件读写，它把每个用户固定到属于自己的唯一云端目录。
---

# 工作区 API

`/wunder/workspace*` 不只是文件读写，它决定了文件落在谁的用户目录下。

## 工作区与临时目录

- 要长期保留的产物 → 工作区。
- 只是中转、发个临时下载链接 → [临时目录](/docs/zh-CN/integration/temp-dir/)。

## 路由怎么定

只有一条路径：请求带上已登录用户身份 → 服务端解析出该用户的工作区根 → 在这个根下按相对路径定位。所有接口都用相对工作区路径，不暴露服务器真实目录。

要知道的边界：

- 过渡期仍接受容器 ID 参数，但它不再决定目录，传什么值都按用户根解析，前端也不提供容器选择。
- 智能体 ID 不参与目录派生，只影响会话和配置绑定；每个用户只有一个智能体实例。
- 子智能体和主智能体共用同一个用户目录。
- 每个用户只有一个固定云端目录，不能新增或更换。

## 最短链路：常用接口

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

做文件面板时对应起来：

- 目录页：`GET /wunder/workspace`
- 文件预览：`GET /wunder/workspace/content`
- 搜索：`GET /wunder/workspace/search`
- 用量条与概览：`GET /wunder/workspace/stats`
- 写文件：`POST /wunder/workspace/file`
- 上传：`POST /wunder/workspace/upload`
- 导出：`GET /wunder/workspace/download` 或 `archive`

## 关键设置与常见坑

- 这一系列接口直接返回裸对象，不套一层数据包裹。
- 用量统计里没配置配额时，配额字段为空，前端只显示已用量、不渲染配额分母。
- 别把工作区当临时目录用；工作区适合持久产物。
- 别传真实磁盘绝对路径，这里一律用工作区相对路径。
- 传容器 ID 不会切到别的目录。

## 延伸阅读

- [工作区与工作目录](/docs/zh-CN/concepts/workspaces/)
- [工作区路由与文件管理](/docs/zh-CN/reference/workspace-routing/)
- [临时目录与文档转换](/docs/zh-CN/integration/temp-dir/)
