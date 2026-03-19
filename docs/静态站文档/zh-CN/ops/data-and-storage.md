---
title: 数据与存储
summary: Wunder 的持久化需要同时分清数据库、工作区、向量存储和临时目录。
read_when:
  - 你要部署或迁移数据
  - 你要理解 PostgreSQL、SQLite、Weaviate、workspaces、temp_dir 分别放什么
source_docs:
  - config/wunder-example.yaml
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
---

# 数据与存储

部署 Wunder 时，最容易乱的是“到底什么该持久化，什么只是临时目录”。

这页只解决这件事。

## 先分四类数据

### 关系型主数据

这是业务核心数据。

例如：

- 会话
- 用户
- 渠道
- 记忆
- 用户世界消息
- 管理配置

### 工作区文件

这是用户和智能体运行过程里产生的持久文件空间。

它和数据库不是一类东西。

### 向量知识库

这是检索相关数据，不应该和普通会话表混为一谈。

### 临时目录

这是中转区，不是长期存储区。

## `storage.backend` 决定什么

配置里当前支持：

- `auto`
- `sqlite`
- `postgres`

它决定的是主业务存储后端。

## server 和 desktop 的典型选择

当前实践上应这样理解：

- 网页端 / server：优先 PostgreSQL
- desktop 本地模式：优先 SQLite

这不是风格问题，而是运行形态决定的：

- server 面向多用户、多租户和持续并发
- desktop 更偏单机、本地和轻量持久化

## Weaviate 放什么

向量知识库相关能力当前使用：

- `vector_store.weaviate`

它主要承接向量检索侧数据。

所以不要把它理解成“主业务数据库的替代品”。

## 工作区放什么

工作区根通常由：

- `workspace.root`

控制。

它承载的是：

- 用户私有文件
- 智能体容器文件
- 任务产物
- 可被后续工具继续处理的落盘结果

如果数据库任务导出到 `/workspaces/{user_id}/...`，它最终也会进入这里。

## `temp_dir` 是什么

`/wunder/temp_dir/*` 对应的是临时目录。

它适合：

- 上传中转
- 下载转发
- 外部客户端取临时文件

不适合：

- 存长期业务资料
- 当正式工作区使用

## 为什么工作区和 temp_dir 一定要分开

因为两者生命周期不一样：

- 工作区强调可持续引用
- temp_dir 强调临时分发和中转

如果混在一起，排障和清理都会很痛苦。

## 典型持久化检查清单

部署后至少确认：

1. 主数据库是你预期的后端
2. 工作区目录有持久卷
3. 向量库有持久卷
4. `temp_dir` 不被当成长存储

## 最常见误区

- 以为 SQLite 和 PostgreSQL 只是“性能差异”
- 把工作区产物只写进 temp_dir
- 忘记给 `/workspaces` 做持久化
- 以为 Weaviate 里会自动保存全部业务数据

## 相关文档

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [工作区与容器](/docs/zh-CN/concepts/workspaces/)
- [配置说明](/docs/zh-CN/reference/config/)
