---
title: wunder 心舰
summary: wunder 是智能体调度平台。蜂窝（desktop）是本地 AI 工作台，蜂巢（web）是云端 AI 工作台，舵机（cli）是手工本地操作入口，舰体（server）是用户与智能体管理平台。
read_when:
  - 第一次接触 wunder，需要先判断从哪里开始
  - 想快速回忆四个形态的分工与入口
source_docs:
  - AGENTS.md
  - crates/wunder-desktop/src/lib.rs
  - crates/wunder-cli/main.rs
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-13
---

# wunder 心舰

<p class="docs-eyebrow">智能体调度平台</p>

wunder 把目标交给智能体，它拆解任务、调用工具、交付结果。四个形态共享同一个引擎（wunder-runtime）与核心（wunder-core），差别只在接入层。

## 四个形态，各自解决什么

| 形态 | 定位 | 进程与前端 | 什么时候选它 |
| --- | --- | --- | --- |
| **蜂窝（desktop）** | 本地 AI 工作台，个人用户默认入口，主推应用 | 本地桌面进程，默认前端 `frontend-slint/`（Rust + Slint），同进程原生调用引擎 | 想开箱即用、直接操作本机文件与桌面 |
| **蜂巢（web）** | 云端 AI 工作台 | 无独立进程，`frontend/`（Vue3 + TypeScript），浏览器访问，随舰体提供 | 团队部署舰体后，成员在浏览器里工作 |
| **舵机（cli）** | 手工本地操作入口 | 本地进程 `crates/wunder-cli/`，TTY 下进 ratatui TUI，非 TTY 降级为一次性 exec | 终端驱动、脚本化、CI/CD 与自动化 |
| **舰体（server）** | 用户与智能体管理平台 | `crates/wunder-server/` + 引擎，PostgreSQL；同时承载舰桥与蜂巢 | 需要多租户、权限治理、渠道接入与统一发布 |

**舰桥**是舰体自带的治理界面（`web/`，原生 HTML + JS），面向管理员，随舰体一起启动。

蜂窝和舵机不依赖舰体，可独立运行与分发。Electron / Tauri 前端壳已停止日常维护与功能迭代，不再作为当前能力描述。

## 从蜂窝开始

个人用户默认使用**蜂窝**：下载安装，打开就是本地 AI 工作台，描述目标即可让智能体拆解任务、调用工具、交付结果。

```text
下载蜂窝 → 配置模型 → 描述目标 → 拿到结果
```

细节见 [快速开始](/docs/zh-CN/start/quickstart/) 与 [蜂窝入门](/docs/zh-CN/start/desktop/)。

## 一个智能体，多个线程，多个工作区

- **一个智能体可以同时拥有多个独立任务线程**：调度、锁与实时状态都按线程隔离，用户可以在同一个智能体下并行推进多个目标。
- **工作区可以有多个**：本地工作目录（蜂窝、舵机）+ 云端工作区（舰体/蜂巢）+ 通过设备互通访问的远程设备工作区，三者在界面上共存、按名称区分。
- 会话队列（`agent_queue.enabled`）默认开启，运行中再次发消息会进入排队条而不是被判「用户忙」。关掉它会导致运行中再发被判忙且不进排队条。

## 能做什么

- **文件与代码**：读取、编辑、执行命令、重构项目
- **办公自动化**：整理文档、生成报告、处理表格、做会议纪要
- **子智能体协作**：任务需要并行推进时，由主智能体临时派生子智能体分头执行、结果回传后汇总（任务结束即回收）
- **持续任务**：定时巡检、周期提醒、跨渠道消息处理（舰体形态；蜂窝与蜂巢各自有独立入口，舵机当前关闭）
- **系统集成**：连接外部服务，把常用流程固化成技能
- **设备互通（interlink）**：本地设备接入云端舰体后，云端可以在获得人类审批的前提下向本地节点下发命令，风险按 L0–L3 分层

## 界面速览

- [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/)：主窗只有两页（对话、设置），设置内 12 个分类；其余能力以面板与对话框形式呈现
- [蜂巢界面](/docs/zh-CN/surfaces/frontend/)：SPA 路由，单条 `/chat/ws` WebSocket 多路复用；设置内含工具、记忆、渠道、定时、设备、归档
- [舰桥界面](/docs/zh-CN/surfaces/web-admin/)：顶层导航 4 组 19 项（系统 4／智能体 6／调试 6／文档 2），面向治理与调试
- [界面概览](/docs/zh-CN/surfaces/index/) 与能力矩阵对比

## 需要更多？三种可选形态

蜂窝覆盖绝大多数场景。按需组合其它形态：

- **舰体（server）**：多人共用、权限管理、渠道接入、统一发布智能体应用。见 [舰体部署](/docs/zh-CN/start/server/)。
- **蜂巢（web）**：部署舰体后，成员通过浏览器访问云端 AI 工作台；dist 由独立的 nginx 容器（默认 18001）下发。
- **舵机（cli）**：终端与自动化场景使用，脚本化 JSONL 输出、云端互通子命令全套。见 [舵机使用](/docs/zh-CN/start/cli/)。

## 快速导航

- **第一次使用** → [快速开始](/docs/zh-CN/start/quickstart/)
- **下载安装** → [蜂窝入门](/docs/zh-CN/start/desktop/)
- **界面地图** → [界面概览](/docs/zh-CN/surfaces/index/)
- **深入理解系统** → [核心概览](/docs/zh-CN/concepts/)
- **接入现有系统** → [接入概览](/docs/zh-CN/integration/)
- **说明书导航** → [说明书总览](/docs/zh-CN/start/hubs/)
- **文档生态导航** → [常用说明目录](/docs/zh-CN/start/docs-directory/)
- **首次启动失败** → [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)
- **遇到问题** → [故障排查](/docs/zh-CN/help/troubleshooting/) 或 [FAQ](/docs/zh-CN/help/faq/)
