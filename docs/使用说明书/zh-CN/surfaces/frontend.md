---
title: 蜂巢界面
summary: 蜂巢是云端 AI 工作台（Vue3 + TS SPA），随舰体提供，浏览器访问。单条 `/chat/ws` WebSocket 多路复用；设置内含工具、记忆、渠道、定时、设备、归档；双向语音（含 TTS），没有交互式终端。
read_when:
  - 舰体部署完成，第一次用浏览器进入蜂巢
  - 想知道某个能力在蜂巢有没有
  - 想把蜂窝用法迁移到蜂巢
source_docs:
  - frontend/src/router/index.ts
  - frontend/src/api/chat.ts
  - frontend/src/utils/ws.ts
  - frontend/src/stores/chatWatcher.ts
  - frontend/src/views/messenger
updated_at: 2026-10-13
---

# 蜂巢界面

蜂巢（web）是 wunder 的**云端 AI 工作台**：Vue3 + TypeScript 单页应用（`frontend/`），随舰体提供、浏览器访问。它没有独立进程，构建产物 `frontend/dist` 由**独立的 nginx 容器**下发（`docker-compose-x86.yml:265-283`，`${NGINX_PORT:-18001}:80`），**不由舰体静态托管**——舰体只挂 `web/`（舰桥）与头像/ppt 静态目录（`crates/wunder-server/src/main.rs:101-119`）。

## 路由（`frontend/src/router/index.ts:234-298`）

顶层：`/`（自动跳登录或 chat）、`/login`、`/register`、`/home`（跳默认智能体 chat）、`/portal`（同）、`/demo/*`（演示壳）。

用户面 `/app/*`（`UserLayout`）：

- `home`、`chat`：Messenger 主体
- `embed/chat`：嵌入式对话页
- `external/:linkId`：外部工作流落地页
- `settings`：同一 Messenger 视图，`category` 决定进入哪个设置类目
- `trajectory`：线程轨迹视图 `ThreadTrajectoryView.vue`

治理面 `/admin/*`（`AdminLayout`，`requiresAdmin`）：`users`、`agents`、`system`；另有 `/admin/login`。

6 条 `/__e2e/*` 路由是压测壳（`router/index.ts:202-227`），仅 E2E 环境启用。

## 主区：messenger

`views/messenger/` 与 `components/chat/`、`components/messenger/` 组成三栏：

- **左栏**：新任务、搜索、工作区与线程树、**工作目录区**（`views/messenger/workspace/*`）、设置入口
- **右栏**：聊天顶栏 + 时间线 + 输入区
- **浮层**：文件预览、在线编辑、形象浮层（`components/companions/CompanionFloatingLayer.vue`）

一个智能体可以同时拥有多个独立任务线程；工作区在蜂巢可以有多份：本用户云端工作区（呈现为 `/workspaces/<user>/workspace`）+ 通过设备互通访问的远程设备工作区（`views/messenger/interlink/*`）。

## 通信

- **REST**：`api/http.ts` 基础层 + `api/{chat,agents,auth,channels,cron,interlink,memory,userTools,workspace,admin}.ts`
- **WebSocket 只有一条**：`/chat/ws`（`api/chat.ts:233`），由 `utils/ws.ts` 做**多路复用**，`stores/chatWatcher.ts:227` 分派业务；订阅、心跳、补水、慢客户端事件都走这条通道
- 断线重连与状态补水靠 HTTP 端点 `/wunder/chat/sessions/{id}/thread-log/{changes,snapshot,events}`（`api/chat/events.rs:30-56`）

**不要**在文档里说蜂巢同时开多条 WS。

## 大数据量渲染（性能口径）

时间线采用虚拟窗口 + worker 解析：

- `views/messenger/messageVirtualWindow.ts`、`messageViewportRuntime.ts`
- `workers/chatRender.worker.ts`、`workers/chatThreadSnapshot.worker.ts`
- `realtime/chat/threadBodyReader.ts` + `components/chat/ThreadBodyViewport.vue`
- `stores/threadViewLoader.ts` 走 `/view/bootstrap`、`/view/chat-page`、`/view/search` 三个投影端点

服务端读模型分页常量：聊天页 `CHAT_PAGE_SIZE=12`（上限 20），轨迹页 `TRAJECTORY_PAGE_SIZE=80`（上限 100），响应预算 128 KB / 192 KB（`wunder-core/src/thread_view.rs:5-11`）。

**状态提醒**：`docs/蜂巢大数据量线程日志渲染优化方案.md:4` 明写「方案重写完成，业务实现待执行」，R1/R2/R3 仍是未解瓶颈——写文档时不要把大数据量渲染当成已完成能力。

## 设置分类

蜂巢设置以类目形式挂在 `views/messenger/settings/*`，与蜂窝共享同一后端 API。包含：

- **通用 / 账户 / 模型 / 智能体 / 形象 / 提示词包 / 帮助**（与蜂窝同）
- **工具（SettingsToolsCategory）**：tab `builtin / mcp / skill / knowledge`
- **记忆（SettingsMemoryCategory）**：`api/memory.ts`
- **渠道（SettingsChannelsCategory）**：`api/channels.ts` + `UserChannelSettingsPanel.vue`
- **定时（SettingsCronCategory）**：`api/cron.ts`
- **设备（SettingsDevicesCategory）**：`views/messenger/interlink/*`，是设备互通治理入口
- **归档（SettingsArchivedCategory）**

## 与蜂窝的差异

| 项 | 蜂巢 | 蜂窝 |
| --- | --- | --- |
| 前端 | Vue3 + TS | Rust + Slint |
| 后端 | 舰体，nginx 18001 | 同进程原生调用，无端口 |
| WebSocket | 只有一条 `/chat/ws` 多路复用 | 无（façade 直连） |
| 交互式终端 | ❌ 未实现 | ✅ `terminal_view.slint` + winpty |
| 语音输入 / TTS | 双向 ✅（`ChatComposer.vue:299` 语音输入；TTS 见 `messengerControllerInstall.ts:9523`） | 输入 ✅ / **TTS ❌** |
| 设备互通 | ✅ 设置设备类目 + `views/messenger/interlink/*` | ⚠️ 侧栏云端工作区 + 审批弹窗；**设置类目未渲染** |
| 托盘 / 系统通知 / 全局热键 / 截图 | ❌ | ✅ |
| 附件上传上限 | 200 MB（temp dir，`api/temp_dir.rs:20`）；持久附件 10 MB（`services/chat_attachments.rs:17`） | 本地文件直接引用 |

## 输入区要点

- **消息输入**：自然语言 + 斜杠命令入口
- **附件**：本地文件选择或拖拽
- **发送/停止**：未运行时发送，运行中停止
- **模型切换**：直接在输入区，不跳设置页
- **审批模式**：suggest / auto_edit / full_auto（默认 full_auto，`services/worker_card_settings.rs:16,53-60`）
- **思考等级**：底部脑图标，灰色=跟随模型、无思考=空、中等=半满、最高=全满
- **工作线程面板**：与「工作目录」同一外层面板 + 内层列表；标题右侧常驻状态图标与活动数量；筛选按钮在无活动全部结束后仍保留，可随时退出筛选；筛选期间仍可拖拽排序

## 时间线与状态

- 每轮回复下方「工作过程」条目折叠中间思考、工具批次、中间打印
- 状态：🔄 运行中 / ⏳ 等待（需要输入或审批） / ✅ 已完成
- 上下文占用与耗时对应**发起该工具的那次模型请求**；同一次请求的并行工具共享数字，不逐条相加
- 极快的调用可以显示 `0ms`；失败与取消也记录耗时

## 斜杠命令（消息输入）

`/new` 新建线程；`/stop` 停止当前执行；`/compact` 主动压缩对话；`/help` 打开帮助文档；`/goal` 目标态管理（`api/chat_goal.rs:14-19`）。

`/compact` 也算一次用户轮次；压缩摘要直接显示在命令后的智能体气泡中，切换线程或刷新页面后保留。

## 保护机制

- **附件处理中**：发送按钮禁用，等处理完再发
- **后台运行**：切换、新建、刷新或暂时离线不取消已有工作线程；重连后从持久化事件恢复状态与待处理审批
- **空线程复用**：新建线程优先复用当前智能体的未使用空线程，已发送消息、正在运行或子智能体派生的线程不复用
- **删除同步**：管理员删除线程后，用户端会在列表刷新或切换时清理条目和缓存；点击失效条目会提示并回到输入页

## 常见问题

**蜂巢与舰桥共用一套界面吗？** 不共用。蜂巢面向使用者，舰桥面向治理者；蜂巢走 nginx 18001，舰桥由舰体挂 `/`。

**蜂巢能多人共用吗？** 蜂巢本身就是团队/多用户场景，成员通过舰体账号登录。个人单机场景推荐蜂窝。

**蜂巢有终端吗？** 没有。需要 shell 时用蜂窝（`terminal_view.slint`）或舵机。

**语音输出（TTS）能用吗？** 可以。蜂巢双向语音（含 TTS）；蜂窝只有输入没有播放。

**设备互通怎么用？** 蜂巢是完整的入口：`SettingsDevicesCategory` + `views/messenger/interlink/*`；命令审批在时间线内联弹出。

## 延伸阅读

- [界面概览](/docs/zh-CN/surfaces/index/)
- [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/)
- [舰桥界面](/docs/zh-CN/surfaces/web-admin/)
- [舰体部署](/docs/zh-CN/start/server/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
