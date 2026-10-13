---
title: 首次运行排查
summary: 各形态第一次启动或部署最常见的失败模式与验证点：蜂窝启动、舰体部署、蜂巢访问、舵机执行。定位到具体环节的回查路径。
read_when:
  - 装完启动没界面 / 启动页长时间不结束
  - 部署完访问不对、端口不通
  - 舵机第一次跑没输出
  - 首次配置模型后请求失败
source_docs:
  - crates/wunder-desktop/src/native.rs
  - crates/wunder-desktop/src/desktop_runtime_recovery.rs
  - crates/wunder-server/src/main.rs
  - crates/wunder-cli/main.rs
  - docker-compose-x86.yml
  - frontend-slint/Cargo.toml
updated_at: 2026-10-13
---

# 首次运行排查

按形态分节；每一节先列**验证点**（跑通的样子）再列**失败现象 → 回查**。所有示例值用中性占位（`<user>`、`/path/to/workspace`、`https://example.com/v1`），不要写入真实身份、密钥或绝对路径。

## 蜂窝（desktop）

### 验证点

- `wunder-frontend-slint` 启动后进入工作台，**没有**任何本机监听端口（同进程原生调用，`crates/wunder-desktop/src/args.rs:53-67` `port=0, bridge_only=true, native_runtime=true`）
- `~/.wunder/workspace` 与 `<temp>/config/desktop.settings.json` 已生成
- 设置 → 模型：「测试连接」返回成功
- 发第一条消息能看到工具批次与最终回复

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 启动页长时间不结束 | `~/.wunder` 权限；工作树扫描耗时（`desktop_runtime_recovery.rs:12-45`，限 4096） | [蜂窝入门](/docs/zh-CN/start/desktop/) |
| 窗口出现但工作区空 | 工作目录区是否显示「离线」；`wf_*` 云端工作区状态 | [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/) |
| 模型请求 401/403 | 设置 → 模型 → API Key、接口地址；「测试连接」重跑 | [配置说明](/docs/zh-CN/reference/config/) |
| 运行中再发消息被判「用户忙」 | 会话队列 `agent_queue.enabled` 默认 true；关掉会破坏排队条 | [快速开始](/docs/zh-CN/start/quickstart/) |
| 想要 TTS 播放但没有声音 | **蜂窝 TTS 未实现**，只能配模型；需要 TTS 请用蜂巢 | [蜂窝界面](/docs/zh-CN/surfaces/desktop-ui/) |
| 找不到「设备互通」设置类目 | 类目未渲染；可用面只有侧栏云端工作区 + 远端命令审批弹窗 | [界面概览](/docs/zh-CN/surfaces/index/) |
| 状态混乱 | 设置 → 通用 → 一键重置工作状态（保留技能、知识库、长期记忆、配置与工作区文件） | [蜂窝入门](/docs/zh-CN/start/desktop/) |

## 舰体（server）

### 验证点

- `docker compose ps` 全部 running；`wunder-server`、`wunder-nginx`、`postgres`、`sandbox`、`onlyoffice`、`drawio`、`extra-mcp` 齐
- 健康检查：`GET http://<host>:18000/wunder/i18n` 返回 200
- 舰桥：浏览器打开 `http://<host>:18000/`，看到「4 组 19 项」顶层导航
- 蜂巢：浏览器打开 `http://<host>:18001/`，进入登录页

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 18000 端口不通 | `WUNDER_HOST` / `WUNDER_PORT` 覆盖；`config/wunder.yaml:23-25` 服务器段 | [舰体部署](/docs/zh-CN/start/server/) |
| 舰桥能进但蜂巢 18001 空白 | `frontend/dist` 是否构建（`npm run build:frontend`）；nginx 容器是否起来；`docker compose logs wunder-nginx` | [舰体部署](/docs/zh-CN/start/server/) |
| 打开 18000 却是蜂巢的样子 | 检查是否把两个概念混了——舰体只挂 `web/`（舰桥），蜂巢 dist 由 **nginx 容器 18001** 下发，不由舰体托管 | [界面概览](/docs/zh-CN/surfaces/index/) |
| 启动加载 example.yaml 静默失败回落 default | 已知隐患（`docs/功能迭代.md` 2026-10-12 工程条目）；显式指向 `config/wunder.yaml` 或用 `WUNDER_CONFIG_PATH` | [配置说明](/docs/zh-CN/reference/config/) |
| 客户端 WS 频繁 `slow_client` | 背压水位 `SLOW_CLIENT_QUEUE_QUEUE_SIZE=2`；慢客户端 lossy/strict 两路；先看是否长消息 | [故障排查](/docs/zh-CN/help/troubleshooting/) |
| PostgreSQL 连接失败 | `WUNDER_POSTGRES_DSN` / `WUNDER_POSTGRES_POOL_SIZE`；compose 里 postgres 服务是否 healthy | [舰体部署](/docs/zh-CN/start/server/) |
| 想切回 SQLite | `WUNDER_STORAGE_BACKEND=sqlite` + `WUNDER_SQLITE_DB_PATH=/path/to/wunder.db` | [配置说明](/docs/zh-CN/reference/config/) |
| sandbox 模式启动但监听 9001 | 正常：`WUNDER_SERVER_MODE=sandbox` 走另一条链路（`main.rs:45-51,177-181`） | [舰体部署](/docs/zh-CN/start/server/) |

## 蜂巢（web）

### 验证点

- 浏览器打开 `http://<host>:18001/`（nginx 容器）
- 登录成功后自动跳到 `/app/chat`
- 输入区「已占用/容量」正常更新
- 时间线里工作过程条目能展开

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| 打开后 `/login` 循环 | access token 是 `wund_<uuid>` 不透明串，**不是 JWT**；重放会触发整族吊销（`api/auth.rs:368-412`） | [认证与安全](/docs/zh-CN/ops/auth-and-security/) |
| 长线程打开慢、渲染卡 | 时间线大数据量方案：`docs/蜂巢大数据量线程日志渲染优化方案.md:4` 明写「业务实现待执行」；R1/R2/R3 未解瓶颈 | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |
| WS 断线不恢复 | 补水端点 `/wunder/chat/sessions/{id}/thread-log/{changes,snapshot,events}`；蜂巢只有一条 `/chat/ws`（`api/chat.ts:233`）多路复用 | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |
| 附件上传失败 | temp dir 上限 200 MB（`api/temp_dir.rs:20`），持久附件 10 MB（`services/chat_attachments.rs:17`） | [蜂巢界面](/docs/zh-CN/surfaces/frontend/) |

## 舵机（cli）

### 验证点

- 交互式终端里 `wunder-cli` 进入 ratatui TUI（`Viewport::Inline`）
- 非 TTY（管道 / CI）自动降级为一次性 exec，能退出
- `wunder-cli doctor` 报告绿色
- `wunder-cli exec --json "..."` 每行是合法 JSON

### 失败现象与回查

| 现象 | 优先检查 | 文档 |
| --- | --- | --- |
| TUI 不启动，直接一次性退出 | stdin/stdout/stderr 是否**全部**是 TTY；`main.rs:212-233` | [舵机使用](/docs/zh-CN/start/cli/) |
| `~/.wunder` 权限报错 | `WUNDER_HOME` 覆盖；数据目录默认 `~/.wunder`（`runtime.rs:1018-1029`） | [舵机使用](/docs/zh-CN/start/cli/) |
| 找不到 `/cron` 命令 | 舵机显式关闭定时任务（`runtime.rs:683 config.cron.enabled = false`）；无渠道 / 知识库 / 记忆 / 形象管理界面 | [舵机使用](/docs/zh-CN/start/cli/) |
| JSONL 事件缺字段 | `exec_events.rs` 定义事件契约 `thread` / `turn` / `item.*`；对照解析 | [舵机使用](/docs/zh-CN/start/cli/) |
| 想在脚本里跑单工具 | 用 `wunder-cli tool ...` 直跑，不走智能体循环 | [舵机使用](/docs/zh-CN/start/cli/) |

## 通用检查

- **不要把「停止维护前端壳」理解成「停止维护蜂窝后端」**：Electron / Tauri 前端壳停止迭代，引擎与 `crates/wunder-desktop/` 后端继续开发。
- **示例值口径**：文档、日志、测试与代码里的示例值不使用真实业务名称、真实身份、真实密钥或真实绝对路径。占位统一：`<user>`、`<api-key>`、`/path/to/workspace`、`https://example.com`。
- **性能边界**：任何列表、日志、面板都应有分页/筛选/限量；发现「一次性渲染大量 DOM」或「无界缓存」属工程缺口，报告即可，不用变通方案盖住。

## 延伸阅读

- [快速开始](/docs/zh-CN/start/quickstart/)
- [界面概览](/docs/zh-CN/surfaces/index/)
- [蜂窝入门](/docs/zh-CN/start/desktop/)
- [舰体部署](/docs/zh-CN/start/server/)
- [舵机使用](/docs/zh-CN/start/cli/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
