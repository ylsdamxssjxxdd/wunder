---
title: 形态矩阵
summary: 各程序形态的技术栈、定位、运行方式与系统支持对比，以及按场景选择形态的依据。
updated_at: 2026-10-13
---

# 形态矩阵

同一套引擎对外有多个程序形态，差异只在接入层。下表列出各形态的技术栈、定位、运行方式与系统支持。

| 程序图标 | 形态名称 | 技术栈 | 定位 | 运行方式 | 系统支持 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | 舰体 server | Rust + axum 0.8；PostgreSQL | 用户与智能体管理平台（多租户、用户与单位、智能体应用构建发布、网关接入与调度） | 服务端常驻进程，Docker 部署，HTTP / WebSocket 接入 | Linux / Docker |
| 🐉 | 舰桥 web | 原生 HTML + CSS + JavaScript（`web/`，无构建） | 舰体的治理后台（系统、智能体、调试、文档四组面板） | 浏览器访问，由舰体静态挂载于 `/` | 跨平台浏览器 |
| 🐝 | 蜂巢 web | Vue 3 + TypeScript（Vite 构建，`frontend/`） | 云端 AI 工作台（舰体的用户端） | 浏览器访问；构建产物 `frontend/dist` 由 nginx 容器只读挂载对外服务（默认端口 18001），**不由舰体托管** | 跨平台浏览器 |
| 🍯 | 蛹 desktop | Rust + Slint 1.18（默认前端 `frontend-slint/`，原生界面、软件渲染）；SQLite | 本地 AI 工作台（主推形态，开箱即用） | 本地桌面进程，同进程原生直调引擎（不经本机网络）；Electron / Tauri 壳已停止维护 | Windows 7 x86 起；Linux（ubuntu18.04 基线）x64 / arm64 AppImage |
| ⚙️ | 舵机 cli | Rust 终端 TUI（ratatui + crossterm）；SQLite | 手工本地操作入口（终端、脚本化、自动化） | 本地终端进程，同进程直调引擎，支持 JSONL 输出 | Windows 7 起（GNU 工具链）；Linux amd64 / arm64（ubuntu18.04 sysroot） |

各形态共享同一引擎（wunder-runtime）与核心（wunder-core）：线程、工具、存储抽象、实时事件和权限语义是同一份代码，差异只在接入层。蛹与舵机不依赖舰体，可独立运行与分发。

## 怎么选

- **个人用户** → 蛹（desktop）。本地 AI 工作台，个人用户的默认入口，下载即用。
- **团队或用浏览器访问** → 蜂巢（web）。舰体的用户端，随舰体提供，部署舰体后成员用浏览器访问。
- **终端与脚本** → 舵机（cli）。手工本地操作入口，终端驱动、脚本化、自动化。
- **多人管理与治理** → 舰体（server）。多租户、用户与单位管理、智能体应用构建发布、网关统一接入与调度。治理后台用舰桥（web），随舰体启动，面向管理员。

## 延伸阅读

- [wunder 心舰首页](/docs/zh-CN/index/)
- [说明书总览](/docs/zh-CN/start/hubs/)
- [快速开始](/docs/zh-CN/start/quickstart/)
