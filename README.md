# wunder 心舰

wunder（心舰）是一个**智能体调度平台**

## 形态矩阵

| 程序图标 | 形态名称 | 技术栈 | 定位 | 运行方式 | 系统支持 |
| :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | 舰体 server | Rust + axum 0.8；PostgreSQL | 用户与智能体管理平台（多租户、组织治理、网关统一接入与调度） | 服务端常驻进程，Docker 部署，HTTP / WebSocket 接入 | Linux / Docker |
| 🐉 | 舰桥 web | 原生 HTML + CSS + JavaScript（`web/`，无构建，随舰体下发） | 舰体的管理端（治理后台：用户、单位、渠道、监控、评测） | 浏览器访问，由舰体静态托管 | 跨平台浏览器 |
| 🐝 | 蜂巢 web | Vue 3 + TypeScript（Vite 构建） | 舰体的用户端（云端 AI 工作台） | 浏览器访问，由舰体静态托管 | 跨平台浏览器 |
| 🍯 | 蜂窝 desktop | Rust + Slint 1.18（原生界面，软件渲染）；SQLite | 桌面版本地 AI 工作台（主推形态，开箱即用） | 本地桌面进程，进程内直调引擎（不经本机网络） | Windows 7 x86 起；Linux AppImage |
| ⚙️ | 舵机 cli | Rust 终端 TUI（ratatui + crossterm）；SQLite | 本地命令行入口（手工操作、脚本化、自动化） | 本地终端进程，支持 JSONL 输出 | Windows 7 及以上（GNU 工具链） |

舰体、舰桥、蜂巢、蜂窝、舵机共享同一套引擎（wunder-runtime）：线程、工具、存储抽象、实时事件和权限语义是同一份代码，差异只在接入层。

## 性能矩阵

用同一套引擎的不同形态，其性能特征对比如下。数值待实测后填写；采样口径与回退门槛见 [性能标准](docs/性能标准.md)，历史数据见 [性能基线](docs/性能基线/)。

| 程序图标 | 形态名称 | 启动速度 | 内存占用 | CPU 占用 | 包体积 | 并发智能体线程 | 聊天页面性能 |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| 🚀 | 舰体 server | — | — | — | — | — | — |
| 🐉 | 舰桥 web | — | — | — | — | — | — |
| 🐝 | 蜂巢 web | — | — | — | — | — | — |
| 🍯 | 蜂窝 desktop | — | — | — | — | — | — |
| ⚙️ | 舵机 cli | — | — | — | — | — | — |

> 采集脚本见 [scripts/form-bench](scripts/form-bench/README.md)：一次运行即可产出上表全部指标。

## 工具矩阵

| 工具 | 简介 | 分类 |
| :--- | :--- | :--- |
| 计划面板 `plan` | 向前端推送结构化的任务计划面板 | 收尾与前端协同 |
| 问询面板 `question_panel` | 弹出结构化选项，向用户提问并等待选择 | 收尾与前端协同 |
| 会话让出 `sessions_yield` | 主动让出当前回合控制权，稍后再被唤起 | 收尾与前端协同 |
| 列出文件 `list` | 列出目录内容 | 工作区与代码 |
| glob | 按通配模式查找文件路径 | 工作区与代码 |
| 搜索内容 `search` | 在文件中按正则搜索匹配内容 | 工作区与代码 |
| 读取文件 `read` | 读取文件内容，支持分段读取 | 工作区与代码 |
| 写入文件 `write` | 写入或整体覆盖文件 | 工作区与代码 |
| 文本编辑 `edit_file` | 对文件做精确字符串替换 | 工作区与代码 |
| 执行命令 `exec` | 执行 Shell / 系统命令 | 工作区与代码 |
| 命令会话 `command_session` | 管理后台长驻命令会话，支持前后台切换 | 工作区与代码 |
| 技能调用 `skill_call` | 调用已安装的技能 | 工作区与代码 |
| 读图工具 `read_image` | 读取图片内容供模型理解 | 多模态 |
| 图片生成 `generate_image` | 按提示词生成图片 | 多模态 |
| 视频生成 `generate_video` | 按提示词生成视频 | 多模态 |
| 语音生成 `generate_speech` | 文本转语音 | 多模态 |
| 语音转写 `transcribe_speech` | 语音转文字 | 多模态 |
| 网页抓取 `web_fetch` | 抓取并解析网页正文 | Web 与桌面 |
| 浏览器 `browser` | 驱动浏览器完成点击、输入、等待渲染、截图等交互 | Web 与桌面 |
| 桌面监视器 `desktop_monitor` | 读取桌面窗口与屏幕状态 | Web 与桌面 |
| 桌面控制器 `desktop_controller` | 操作本机桌面（鼠标、键盘、窗口） | Web 与桌面 |
| 子智能体控制 `subagent_control` | 在任务中临时派生子智能体，结果回传后回收 | 线程与子智能体 |
| 会话线程控制 `thread_control` | 管理任务线程 / 分支线程 | 线程与子智能体 |
| 自我状态 `self_status` | 查询智能体自身的运行状态 | 系统连接与记忆 |
| 记忆管理 `memory_manager` | 读写长期记忆 | 系统连接与记忆 |
| 定时任务 `schedule_task` | 创建与管理定时 / 周期任务 | 系统连接与记忆 |

## 吞噬矩阵

| 吞噬 | 项目 | 地址 |
| :--- | :--- | :--- |
| 项目原型 | EVA | https://github.com/ylsdamxssjxxdd/eva |
| 智能体基础 | OpenAI Codex | https://github.com/openai/codex |
| 前端基础 | HuLa | https://github.com/HuLaSpark/HuLa |
| 协议基础 | Claude Code | https://github.com/anthropics/claude-code |
| 用户基础 | OpenClaw | https://github.com/openclaw/openclaw |
| 工具基础 | deepseek-harness（dsh） | https://github.com/deepseek-ai/deepseek-harness |