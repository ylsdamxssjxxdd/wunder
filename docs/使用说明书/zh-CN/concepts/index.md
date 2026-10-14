---
title: 核心概览
summary: wunder 为什么这样表现——四个核心（快速、稳定、通用、轻量），以及四个形态与深入主题。
updated_at: 2026-10-13
---

# 核心概览

wunder（心舰）是智能体调度平台：把目标交给智能体，它拆解任务、调用工具、交付结果。这四个核心说的是**系统要成为什么样**，不是它能做什么的清单。功能清单看[工具](/docs/zh-CN/tools/)与[能力速查](/docs/zh-CN/capabilities/api-index/)。

## 四条设计原则

| 原则 | 含义 |
| --- | --- |
| **一切围绕线程** | 线程（=会话）是执行主体，不是「一次问答」。一个智能体可以同时拥有多个独立任务线程，调度、锁与实时状态都按线程隔离 |
| **一切皆工具** | 模型能做的事统一暴露为工具接口，规范名多为中文（`读取文件`、`执行命令`、`记忆管理`），英文别名先归一再路由 |
| **多形态共享引擎** | 核心与引擎只有一份，四个形态的差别在接入层 |
| **边界与治理内置** | 提示词冻结、审批分档、日配额、有界视图与分页都在骨架里，不靠上层自觉 |

## 四个形态

| 形态 | 定位 |
| --- | --- |
| **蛹**（desktop） | 本地 AI 工作台，个人默认入口；本地桌面进程，同进程直接调用引擎，本地 SQLite |
| **蜂巢**（web） | 云端 AI 工作台，浏览器访问；无独立进程，用舰体的存储 |
| **舵机**（cli） | 手工本地操作入口，终端驱动、便于脚本化；本地进程，本地 SQLite |
| **舰体**（server） | 用户与智能体管理平台：多租户、单位、应用构建发布、网关接入；服务进程 + 引擎，自带舰桥 |

蛹与舵机不依赖舰体，可独立运行与分发。Electron / Tauri 前端壳已停止日常维护，不再作为能力描述。

## 四个核心

| 核心 | 含义 |
| --- | --- |
| **快速** | 启动速度快，响应速度快 |
| **稳定** | 智能体可长时间工作，不卡顿、不内存泄漏，程序运行正常，CPU 占用低 |
| **通用** | 可部署任意系统平台，可接入任意主流模型，完成各种复杂任务 |
| **轻量** | 程序本体小，提示词设计、工具设计等负担轻，token 消耗低 |

（原「9 个核心」的逐项详情页见[能力 › 核心能力](/docs/zh-CN/capabilities/)。）

## 深入主题

按主题的深入页面（都在[能力 › 深入了解](/docs/zh-CN/capabilities/)里）：

| 主题 | 页面 |
| --- | --- |
| 一次消息穿过哪些环节 | [系统架构](/docs/zh-CN/concepts/architecture/) |
| 会话、线程、轮次怎么编号 | [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) |
| 实时推送与断线续传 | [流式执行](/docs/zh-CN/concepts/streaming/) |
| 工作区与文件安全 | [工作区与工作目录](/docs/zh-CN/concepts/workspaces/) |
| 状态指示器 | [运行时状态](/docs/zh-CN/concepts/presence-and-runtime/) |
| 工具来源与治理 | [工具体系](/docs/zh-CN/concepts/tools/) |
| 提示词、技能、能力四层 | [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/) |
| 记忆形态与排查 | [长期记忆](/docs/zh-CN/concepts/memory/) |
| Token 与配额 | [Token 与配额](/docs/zh-CN/concepts/quota-and-token-usage/) |
| 持续目标态 | [目标态系统](/docs/zh-CN/concepts/goal-mode/) |
| 动画形象 | [形象系统](/docs/zh-CN/concepts/companion/) |
| 异常与截断行为 | [边界处理](/docs/zh-CN/concepts/boundary-handling/) |
| 线程一轮一轮如何推进 | [智能体循环](/docs/zh-CN/concepts/core-agent-loop/) |
| 能力的四层结构 | [工具](/docs/zh-CN/concepts/core-tools/) |
| 过程可见、断线可续 | [实时性](/docs/zh-CN/concepts/core-realtime/) |
| 有界、预算与熔断 | [稳定性](/docs/zh-CN/concepts/core-stability/) |
| 日志、指标与线程轨迹 | [可观测性](/docs/zh-CN/concepts/core-observability/) |

## 按角色推荐阅读

- **第一次用蛹**：[快速上手](/docs/zh-CN/start/quickstart/) → 智能体循环 → 工具 → 记忆 → 上下文压缩。
- **管理员 / 治理**：多用户管理 → 稳定性 → 可观测性 → 渠道 → [配置参考](/docs/zh-CN/capabilities/config/)。
- **接入与开发**：[系统架构](/docs/zh-CN/concepts/architecture/) → [聊天 WebSocket](/docs/zh-CN/capabilities/chat-ws/) → 流式执行。

## 常见误解

- **「多个智能体可以组蜂群协作」** —— 不能。协作形态只有**子智能体**：主智能体临时创建的工作单元，任务结束即回收。蜂群编排（蜂群工具、蜂房、编排态、团队运行、蜂群包）已从引擎与调度面移除；个别终端文案与界面词条仍留有旧字样，那是待清理残留，不代表能力。
- **「列出来的工具都能用」** —— 不能。默认可用集是固定档位，且 `网页搜索` 与 `ptc` 当前属**临时下架**（代码保留、对模型与用户不可见），不是已删除。
- **「改提示词会立刻影响当前对话」** —— 不会。线程首次确定系统提示词后即冻结。
- **「界面显示的就是后端真相」** —— 界面消费的是有界视图（分页 + 字节预算），不是全量历史。看到的内容异常时以线程台账为准。
- **「停了维护前端壳=停了蛹后端」** —— 两回事。蛹默认走本地原生界面 + 同进程原生调用，执行、工具与存储语义仍复用引擎。

## 延伸阅读

- [系统架构](/docs/zh-CN/concepts/architecture/) —— 环节与链路
- [会话与轮次](/docs/zh-CN/concepts/sessions-and-rounds/) —— 计数口径
- [术语表](/docs/zh-CN/help/glossary/) —— 用词统一
