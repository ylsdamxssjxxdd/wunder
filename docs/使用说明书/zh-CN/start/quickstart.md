---
title: 快速开始
summary: 用最短路径把蜂窝跑起来：下载、启动、配模型、发第一条消息、验证结果。个人用户默认入口，无需部署服务。
read_when:
  - 第一次使用 wunder
  - 需要一条能验证到结果的通路
source_docs:
  - frontend-slint/Cargo.toml
  - frontend-slint/run.bat
  - frontend-slint/src/main.rs
  - crates/wunder-desktop/src/native.rs
  - crates/wunder-desktop/src/args.rs
updated_at: 2026-10-13
---

# 快速开始

目标只有一个：打开蜂窝，跑通第一个任务。个人用户默认走**蜂窝（desktop）**——本地 AI 工作台，下载即用，不需要先搭服务器。

## 4 步跑通

### 1. 下载安装

从 Releases 下载匹配系统的安装包。蜂窝面向 32 位 Win7 与 Ubuntu 18.04，也覆盖现代 Windows / Linux。安装或解压后启动即可。

开发环境下用 `frontend-slint/run.bat`（Windows）或直接执行 `wunder-frontend-slint` 也可以；它同进程原生调用引擎，**不需要**本机 HTTP/WebSocket（`crates/wunder-desktop/src/args.rs:53-67` `native_defaults()` = `port=0, bridge_only=true, native_runtime=true`）。

### 2. 配置模型

打开「设置 → 模型」，填入 API Key 与接口地址（形如 `https://example.com/v1`），保存前用「测试连接」验证一次；模型名与参数按提供方指引填。

数据留在本机：默认本地 SQLite，配置目录 `~/.wunder`；模型请求走你配置的远程端点。

### 3. 发第一条消息

回到对话页，输入：

```
帮我列出当前工作目录的文件
```

回车发送即可。

### 4. 观察执行过程

一次执行按 **用户轮次 / 模型轮次** 展示：

- 用户轮次 = 你每发送一条消息记 1
- 模型轮次 = 引擎主循环的**一次模型调用**（可含一批工具调用），工具调用不单独计模型轮次

看到时间线出现工具批次、每轮的「工作过程」条目与最终回复，就说明跑通了。

## 蜂窝开箱能力

- 本地优先：数据留在本机，可离线；网络只用于访问模型与网页
- 桌面控制：`桌面控制器` / `桌面监视器`（需 vision 模型）
- 交互式终端：`terminal_view.slint` + winpty（Windows）/ PTY
- 截图采集、语音输入、托盘、系统通知与全局热键
- 一个智能体可以同时拥有**多个独立任务线程**，工作区也可以有多个（本地 + 云端 + 通过设备互通访问的远程设备工作区）
- 直接编辑智能体：随时改系统提示词、审批模式与工具集合

## 验收清单

- [ ] 可以成功发起一次对话
- [ ] 可以看到中间步骤和工具调用批次
- [ ] 可以拿到最终结果并复制
- [ ] 上滚查看历史时不被后续输出打断（虚拟列表 + 增量行更新）
- [ ] 知道失败时该看哪一页

## 出错回哪儿查

| 现象 | 先看 |
| --- | --- |
| 启动页长时间不结束 | [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/) |
| 模型请求 401/403 | 设置 → 模型 → 测试连接；[配置说明](/docs/zh-CN/reference/config/) |
| 运行中发消息被判「用户忙」 | 会话队列 `agent_queue.enabled` 是否被关闭 |
| 界面卡、消息不刷新 | [故障排查](/docs/zh-CN/help/troubleshooting/) |

## 其他使用方式

蜂窝覆盖绝大多数场景。以下三种可选形态按需取用：

- **舰体（server）**：多人共用、权限管理、渠道接入、智能体应用发布与治理。见 [舰体部署](/docs/zh-CN/start/server/)。
- **蜂巢（web）**：随舰体提供的云端 AI 工作台，浏览器访问；`frontend/dist` 由独立 nginx 容器（默认 18001）下发。见 [蜂巢界面](/docs/zh-CN/surfaces/frontend/)。
- **舵机（cli）**：手工本地操作入口，终端与自动化。见 [舵机使用](/docs/zh-CN/start/cli/)。

## 下一步

- 下载安装细节 → [蜂窝入门](/docs/zh-CN/start/desktop/)
- 界面地图 → [界面概览](/docs/zh-CN/surfaces/index/)
- 核心概念 → [核心概览](/docs/zh-CN/concepts/)
- 接入现有系统 → [接入概览](/docs/zh-CN/integration/)
- 所有工具 → [工具总览](/docs/zh-CN/tools/)
