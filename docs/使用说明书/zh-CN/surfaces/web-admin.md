---
title: 舰桥界面
summary: 舰桥是舰体的治理与调试界面（原生 HTML + JS，`web/`），无构建步骤，由舰体挂 `/`。顶层导航 4 组 19 项：系统 4／智能体 6／调试 6／文档 2。
read_when:
  - 管理员第一次打开舰桥，需要知道有哪些面板
  - 想确认某个能力在舰桥上有没有
  - 想知道调试面板的线程轨迹视图
source_docs:
  - web/index.html
  - web/app.js
  - web/modules
  - web/styles
  - crates/wunder-server/src/main.rs
updated_at: 2026-10-13
---

# 舰桥界面

舰桥是**舰体（用户与智能体管理平台）**自带的治理与调试界面，位于 `web/`，原生 HTML + JS 模块。普通用户登录后不会看到它，只有管理员凭 `x-api-key` 或 Bearer + `UserStore::is_admin` 进入（`crates/wunder-server/src/main.rs:517`、`wunder-core/src/auth.rs:1-74`）。

**没有构建步骤**：舰体启动时直接 `mount_static(app, "web", "/")`（`main.rs:117`），端口与舰体一致（默认 18000）。蜂巢与舰桥是**两套界面**，蜂巢的 dist 由独立 nginx 容器 18001 下发。

## 顶层导航：4 组 19 项（`web/index.html:35/60/93/126`）

**系统（4）**

- 内部状态：进程健康、锁与队列、runtime metrics
- 系统设置：`config/wunder.yaml` 的运行中热更（`api/admin.rs:108/490/743`）
- 单位管理：组织树，最深 4 级（`services/org_units.rs:9-10`）
- 用户管理：账号、配额、presence（在线列拆蜂巢 / 蜂窝 / 舵机）

**智能体（6）**

- 模型配置：LLM / TTS / 图像 / 视频模型
- 线程管理：跨用户线程列表、状态、目标态（**只读展示**，goal 无编辑 UI）
- 预设智能体：worker card 模板（`services/worker_card_files.rs:1-2`），可以声明哪些字段允许用户自定义；用户级定时任务与渠道**在这里作为模板存在**（`web/index.html:614/724-730`），不是独立导航项
- 形象管理：`modules/companions.js`
- 工具管理：`TOOL_MANAGER_PANELS`（`web/app.js:196`）把 builtin / mcp / knowledge / skills / toolVisibility 归为同一导航组
- 系统提示词：`config/prompts/` 与 prompt 模板

**调试（6）**

- 吞吐量测试：presets 1024…1 MiB in / 1024…8192 out，`MAX_CONCURRENCY=1024`（`ops/throughput.rs:16-22`）
- 性能测试：性能采样固定用户 `performance_admin`、根 `.wunder_perf`、命令超时 5s（`ops/performance.rs:22-33`）
- 能力评估：wunderbench 与能力打分
- 多模调试：image / video / transcribe / tts 直连测试（`api/admin_multimodal.rs` 4 条）
- 调试面板：线程列表 + 线程详情；**右侧日志区已重构为线程轨迹视图**（`web/modules/trajectory-model.js`、`trajectory-view.js`；`log.js` 的 `appendLog` 退化为控制台兜底）
- LSP：语言服务

**文档（2）**

- 使用说明书：站内嵌 `docs/使用说明书/`
- 接口：API 文档入口

## 面板装配

`web/app.js:156-193` 用 `panelMap` 做 panel ↔ nav 装配；每个模块导出 `initXxxPanel` 与 `loadXxx`，`app.js:4-82` 逐个 import（URL 版本号做缓存失效）。`web/modules/` 共 41 个文件，含：

- `elements.js` 统一 DOM 收集
- `state.js` 全局状态（其中 `panelLoaded.interlink` :26 与 `memories` :24 是**死条目**，没有对应面板实现）
- `api.js` / `api-error.js`
- `i18n.js` + `i18n-config.js`
- `notify.js`
- `log.js`（线程轨迹视图改造后，日志函数退化为控制台兜底）
- `trajectory-model.js` / `trajectory-view.js`
- `throughput.js` → `throughput/controller.js`
- `builtin.js` 与 `monitor.js` 中的 `filterRemovedSwarmTools`（swarm 从引擎与协议层移除，前端标签与兼容垫片仍在，属工程缺口）

`styles/` 6 份按域拆：`base` / `admin` / `extra` / `airlock` / `throughput` / `trajectory`；`shared/` 只有 `tool-visuals.js` 与 `deprecated-tools.js`。

## 舰桥**没有**的东西（避免踩坑）

- **用户级定时任务面板**：没有独立入口；`cron` 只在「预设智能体」里作为模板存在
- **用户级渠道管理**：同上，渠道只在预设模板里
- **长期记忆面板**：`web/modules/state.js:24` 是死条目，没有对应实现
- **互通治理面板**：整个 `web/` 只在 `state.js:26` 残留 `panelLoaded.interlink` 标志；互通的舰队 / 通道 / 命令 / 审计 / 策略治理**当前是后端 API only**（`api/admin_interlink.rs` 7 条），写「舰桥互通治理面板」即为幻觉
- **goal UI**：线程管理只读展示，没有 goal 编辑器；用户面 goal 入口在蜂巢 `MessageGoalBar.vue`、蜂窝 `composer.slint:680`、舵机 `/goal`

## 设计基调

舰桥是治理与调试界面，操作默认**高风险**：

- 清晰的状态、错误反馈、确认边界与权限校验；前端确认不能替代后端鉴权
- 表格、日志与监控面板**必须分页/筛选/限量**，禁止一次性渲染大量 DOM
- 密集、可扫描；不做营销化布局或情绪化装饰
- 新增业务逻辑放 `web/modules/`，样式放 `web/styles/`，共享小工具放 `web/shared/`
- `web/app.js` 只做启动、导航与模块装配，不继续堆业务细节

## 访问方式

- 地址：`http://<host>:18000/`（舰体挂载点，`main.rs:117`；默认端口见 `config/wunder.yaml:23-25`）
- 鉴权：`x-api-key`（admin）或 Bearer + admin；单位负责人（leader）只对 `/wunder/admin/org_units|user_accounts` 开孔（`wunder-core/src/auth.rs:83`）
- 首次登录后立即修改默认密码

## 常见问题

**舰桥能聊天吗？** 不能。它是治理与调试后台。日常对话在蜂巢（成员）或蜂窝（本地）。

**舰桥和蜂巢是一回事吗？** 不是。蜂巢是 Vue3 SPA、nginx 18001 下发，用户面；舰桥是原生 HTML + JS、由舰体挂 `/`，管理员面。

**为什么状态里有 `interlink` / `memories` 但面板是空的？** `web/modules/state.js` 里的这两项是死条目，面板从未落地；互通治理是后端 API only。

**日志区怎么变成线程轨迹了？** 调试面板右侧原日志区在近期迭代里重构为**线程轨迹视图**（`trajectory-view.js`），旧的 `appendLog` 只作为控制台兜底保留。

**吞吐量、性能测试和压测谁在用？** 吞吐量测试与性能测试是舰桥运维面板，走 `ops/throughput.rs` 与 `ops/performance.rs`；面向渲染的线程压测 `POST /wunder/chat/stress-threads` 属于**用户面**（`api/chat/stress.rs:17-25`），三端入口默认 1000×1000。

## 延伸阅读

- [舰体部署](/docs/zh-CN/start/server/)
- [界面概览](/docs/zh-CN/surfaces/index/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [配置说明](/docs/zh-CN/reference/config/)
- [舰桥面板索引](/docs/zh-CN/reference/admin-panels/)
