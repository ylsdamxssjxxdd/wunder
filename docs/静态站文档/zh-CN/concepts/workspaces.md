---
title: "工作区与容器"
summary: "Wunder 的文件隔离不是“一个当前目录”，而是按 `user_id + container_id` 分层组织的持久化工作区。"
read_when:
  - "你要理解文件为什么按用户和容器隔离"
  - "你要判断 `agent_id`、`container_id` 和工作区之间的关系"
source_docs:
  - "docs/系统介绍.md"
  - "docs/设计方案.md"
  - "docs/API文档.md"
---

# 工作区与容器

Wunder 的工作区不是一个简单的“当前目录”概念，而是一套稳定的隔离策略。

你至少要分清两层：

1. 按 `user_id` 隔离不同调用者
2. 按 `container_id` 再把同一调用者的私有空间和智能体运行空间拆开

## 当前约定

- `container_id=0`：用户私有容器
- `container_id=1~10`：智能体运行容器

## `container_id=0` 不只是私有文件夹

用户私有容器同时也是“智能体内见系统”的根目录。

这里不仅能放用户自己的文件，也会承载智能体可见配置：

- `global/tooling.json`
- `skills/`
- `agents/<agent_id>.worker-card.json`

运行时会在请求进入调度前做一次轻量同步和校验，并把有效快照与诊断写入 `.wunder/`。

## `container_id=1~10` 用来做什么

这些容器主要给智能体运行时使用，适合承载：

- 任务中间产物
- 单个智能体的执行目录
- 需要和用户私有目录分开的自动化流程文件

## `agent_id` 和工作区是什么关系

`agent_id` 不等于工作区。

- `agent_id` 决定对话、配置和主线程绑定
- `container_id` 决定文件空间路由

当前系统里，`agent_id` 可以参与容器推导，但它不代表“一个智能体对应一个完整私有目录世界”。

尤其要注意：

- raw `/wunder` 虽然接受 `agent_id`
- 但该字段当前主要用于主会话绑定和工作区/容器路由
- 它不会因此自动补齐完整智能体人格快照

## 对外路径长什么样

对模型和接口来说，公共视角通常表现为：

- `/workspaces/{user_id}/...`

实现层会把不同容器拆到不同目录，例如：

- 容器 0：`/workspaces/{user_id}/`
- 容器 1~10：`/workspaces/{user_id}__c__{container_id}/`

你不需要手工记住底层目录名，但需要理解：同一个 `user_id` 可以同时拥有多块持久化工作区。

## 常见误解

### 误解一：`agent_id` 就等于工作区

不是。

`agent_id` 决定对话和配置绑定，`container_id` 决定文件空间路由。

### 误解二：所有文件都应该放在私有容器

不是。

私有容器更适合放用户级资料和智能体内见配置；任务产物和自动化流程文件更适合放运行容器。

### 误解三：本地模式下就没有容器概念

也不是。

desktop/cli 只是把容器映射到真实本地目录，不会取消容器语义。

## 相关文档

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [wunder API](/docs/zh-CN/integration/wunder-api/)
- [Desktop 本地模式](/docs/zh-CN/ops/desktop-local-mode/)
