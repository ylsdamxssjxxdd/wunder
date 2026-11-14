---
title: 形态矩阵
summary: 六个程序形态的技术栈、定位、运行方式与系统支持，以及按场景选择形态的依据。
updated_at: 2026-11-14
---

# 形态矩阵

wunder 是通用型人工智能操作系统（模型 + 驾驭框架 + 运行环境），同一套引擎对外有多个程序形态，差异只在接入层。

| 程序图标 | 形态名称 | 技术栈 | 定位 | 运行方式 | 系统支持 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | wunder 服务端 | Rust + axum 0.8；PostgreSQL | 用户与智能体管理平台（多租户、用户与单位、智能体应用构建发布、网关接入与调度） | 服务端常驻进程，Docker 部署，HTTP / WebSocket 接入 | Linux / Docker |
| 🐉 | wunder 管理端 | 原生 HTML + CSS + JavaScript（`web/`，无构建） | 服务端的治理后台（系统、智能体、调试、文档四组面板） | 浏览器访问，由服务端静态挂载于 `/` | 跨平台浏览器 |
| 🐝 | wunder 云端版 | Vue 3 + TypeScript（Vite 构建，`frontend/`） | 云端 AI 工作台（服务端的用户端） | 浏览器访问；构建产物 `frontend/dist` 由 nginx 容器只读挂载对外服务（默认端口 18001），**不由服务端托管** | 跨平台浏览器 |
| 🍯 | wunder 桌面版 | Rust + Slint 1.18（默认前端 `crates/wunder-desktop/frontend/`，原生界面、软件渲染）；SQLite | 本地 AI 工作台（主推形态，个人用户默认入口） | 本地桌面进程，同进程原生直调引擎（不经本机网络）；Electron / Tauri 壳已停止维护 | Windows 7 x86 起；Linux（ubuntu18.04 基线）x64 / arm64（`.run` 安装包） |
| 📱 | wunder 安卓版 | Rust + Slint 1.18（`crates/wunder-android/` 单 Activity 原生壳，复用桌面版主题、时间线、输入组件；Skia 渲染）；SQLite | 桌面版的 Android 形态，本地 AI 工作台（手机端） | APK 安装后打开，同进程直调共享引擎；默认内置 Ubuntu 运行环境（arm64） | Android 8.0（API 26）起；arm64-v8a（x86_64 切片仅用于模拟器验证） |
| ⚙️ | wunder 命令行 | Rust 终端 TUI（ratatui + crossterm）；SQLite | 手工本地操作入口（终端、脚本化、自动化） | 本地终端进程，同进程直调引擎，支持 JSONL 输出 | Windows 7 起（GNU 工具链）；Linux x64 / arm64（ubuntu18.04 sysroot） |

各形态共享同一引擎（wunder-runtime）与核心（wunder-core）：线程、工具、存储抽象、实时事件和权限语义是同一份代码，差异只在接入层。桌面版、安卓版与命令行不依赖服务端，可独立运行与分发。

## 选择依据

- **个人用户** → 桌面版（desktop）。本地 AI 工作台，个人用户的默认入口，装好即用。
- **手机与移动场景** → 安卓版（android）。桌面版的 Android 形态，安装 APK 即可在手机上使用同一套引擎和能力。
- **团队或用浏览器访问** → 云端版（web）。服务端的用户端，随服务端提供，部署服务端后成员用浏览器访问。
- **终端与脚本** → 命令行（cli）。手工本地操作入口，终端驱动、脚本化、自动化。
- **多人管理与治理** → 服务端（server）。多租户、用户与单位管理、智能体应用构建发布、网关统一接入与调度。治理后台用管理端（web），随服务端启动，面向管理员。
