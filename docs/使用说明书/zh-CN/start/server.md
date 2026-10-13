---
title: 舰体（server）部署
summary: 可选形态。用户与智能体管理平台：多租户、权限治理、渠道接入、智能体应用发布。舰体启动后同端口挂舰桥（18000），蜂巢 dist 走独立 nginx 容器（18001）。
read_when:
  - 需要多人共用 wunder
  - 需要管理员后台与统一治理
  - 需要接入渠道或对外发布智能体应用
source_docs:
  - crates/wunder-server/src/main.rs
  - docker-compose-x86.yml
  - config/wunder.yaml
  - frontend/nginx.conf
  - builders/build.sh
updated_at: 2026-10-13
---

# 舰体（server）部署

舰体（server）是 wunder 的**用户与智能体管理平台**（`crates/wunder-server/` + 引擎），属于可选形态：个人用装蜂窝（desktop）就够了，只有在需要多租户、用户与单位管理、渠道接入、智能体应用构建发布与网关统一调度时才部署舰体。

## 三件套与端口

舰体部署后同时提供 **三个访问入口**（默认端口，均可用环境变量覆盖）：

| 界面 | 载体 | 默认端口 | 来源 |
| --- | --- | --- | --- |
| **舰体 API** | `crates/wunder-server/`（Rust bin） | **18000**（`WUNDER_HOST` / `WUNDER_PORT` 覆盖；`main.rs:151-159`） | `config/wunder.yaml:23-25` `host: 0.0.0.0, port: 18000, mode: api` |
| **舰桥** | 原生 HTML + JS（`web/`），**无构建步骤** | 与舰体同端口 18000（`main.rs:117` `mount_static(app, "web", "/")`） | — |
| **蜂巢** | Vue3 SPA（`frontend/`），构建产物 `frontend/dist` | 独立 **nginx 容器 18001**（`${NGINX_PORT:-18001}:80`，`docker-compose-x86.yml:265-283`） | 配置 `frontend/nginx.conf` |

注意：**蜂巢的 dist 不是由舰体静态托管的**——旧 README 与旧文档写「由舰体静态托管」是错的；舰体进程只挂 `web`（舰桥）与头像/ppt 目录。

其它相关服务：sandbox 模式使用默认端口 **9001**（`WUNDER_SERVER_MODE=sandbox`，`main.rs:45-51,177-181`）；onlyoffice 18080；drawio 18081；PostgreSQL 5432；extra-mcp 18004。

## 适用场景

| 场景 | 选舰体 |
| --- | --- |
| 多人共用一个系统 | ✅ |
| 需要管理员后台（舰桥） | ✅ |
| 需要接入飞书、企业微信、QQ Bot 等渠道 | ✅ |
| 需要统一治理、审计与配额 | ✅ |
| 需要构建并发布智能体应用 | ✅ |
| 只是个人用 | ❌ 选蜂窝（desktop） |

## 部署前准备

- Docker 与 Docker Compose（推荐方式）
- 至少 4 GB 可用内存
- PostgreSQL（compose 内含 `postgres:16`；单机也可切 `WUNDER_STORAGE_BACKEND=sqlite`）
- 时间区：compose 默认 `TZ=Asia/Shanghai`

## 最短路径：docker compose 部署

```bash
# 获取代码
git clone <repo-url>
cd wunder

# x86 架构
docker compose -f docker-compose-x86.yml up -d

# ARM 架构
docker compose -f docker-compose-arm.yml up -d
```

compose 服务（x86/arm 同构，`docker-compose-x86.yml`）：`wunder-server`（`${WUNDER_PORT:-18000}`，入口 `docker-rust-entry.sh run-or-build`）、`sandbox`（9001 `/health`）、`onlyoffice`（18080）、`drawio`（18081）、`extra-mcp`、`postgres:16`（5432）、`wunder-frontend`（node:22.14 构建 dist）、`wunder-nginx`（18001）。

## 本机运行（cargo）

```bash
# 1. 构建蜂巢前端
npm run build:frontend

# 2. 启动舰体（默认读 config/wunder.yaml，或 WUNDER_CONFIG_PATH 指定）
cargo run -p wunder-server

# 覆盖端口
WUNDER_HOST=127.0.0.1 WUNDER_PORT=18000 cargo run -p wunder-server
```

配置加载优先级（`wunder-core/src/config.rs:2478-2518`）：`WUNDER_CONFIG_PATH` 或 `config/wunder.yaml` → YAML 内 `${ENV}` 展开 → 环境变量最后生效（`WUNDER_STORAGE_BACKEND` / `WUNDER_SQLITE_DB_PATH` / `WUNDER_POSTGRES_DSN` / `WUNDER_POSTGRES_POOL_SIZE`）→ 缺失字段与 example 模板做稀疏合并。

已知隐患：`config/wunder-example.yaml` 用 `load_config_from_path` 解析会失败并静默回落到 `Config::default()`（`docs/功能迭代.md` 2026-10-12 工程条目）。

## 访问入口

| 入口 | 地址 |
| --- | --- |
| 舰桥（管理界面） | `http://<host>:18000/` |
| API | `http://<host>:18000/wunder/*` |
| 蜂巢（用户界面） | `http://<host>:18001/` |
| MCP 服务端 | `http://<host>:18000/wunder/mcp` |
| 外部工作流 | `http://<host>:18000/wunder/external/workflow*` |
| 健康检查 | `GET http://<host>:18000/wunder/i18n` |

## 首次登录

管理员凭据从 `config/wunder.yaml` 与初始化管理读取；示例值请以中性占位写：`<admin-user>` / 部署环境注入的密钥或口令，**不写默认密码或真实凭据**。首次登录后立即修改管理员口令。

Argon2 存口令（`services/user_store.rs:223-234`）；token 是不透明随机串（`wund_<uuid>` / `wundr_<uuid>` / `wundc_<uuid>`，`services/user_store.rs:27,28`），**不是 JWT**；刷新是 OAuth 式族轮换，重放即整族吊销（`api/auth.rs:368-412`）。

## 部署后必做

1. **改管理员口令**（Argon2 落库）
2. **舰桥 → 模型配置**：添加 LLM / TTS / 图像 / 视频模型端点与 API Key（示例用 `<api-key>` 占位，不用真实值）
3. **舰桥 → 单位管理**：确认组织树（最深 4 级，`services/org_units.rs:9-10`），必要时导入 `config/org_units.json`
4. **舰桥 → 预设智能体**：创建 worker card 模板，声明哪些字段允许用户自定义（`services/worker_card_files.rs:1-2`）
5. **舰桥 → 用户管理**：添加用户或开放注册（`config.allow_user_registration`）
6. **如需接入渠道**：飞书 / 企业微信 / 微信 / QQ Bot / WhatsApp Cloud / weixin / xmpp（默认 7 个 adapter，`channels/registry.rs:63-71`）
7. **观察健康**：舰桥「内部状态」+ `/wunder/admin/monitor`

## 主要能力

### 多租户与治理

- 用户与单位分层管理；权限按角色分配（admin / leader / user），数据按 `user_id` / `agent_id` 隔离
- presence 拆蜂巢 / 蜂窝 / 舵机三列（`api/admin/identity_admin.rs:686-735`）
- 等级与配额（`MAX_USER_LEVEL=200`，`services/user_leveling.rs:3-30`）

### 渠道接入

- 默认注册 7 个 adapter：whatsapp_cloud / feishu / qqbot / wechat / wechat_mp / weixin / xmpp（`channels/registry.rs:63-71`）
- 长连接 supervisor 只有 feishu / qqbot / xmpp / weixin 四个，其余是 webhook-only
- **`telegram` / `discord` / `slack` / `LINE` / `DingTalk` 未支持**，只在 catalog 里作为通用 webhook 目录项存在
- 入站有界队列容量 512、并发 8、入队超时 80ms；出站持久 outbox + 指数退避

### 可观测性

- 日志：控制台彩色层 + JSONL 文件层（daily rolling，`server.jsonl`）；目录 `observability.server_log_dir` 默认 `./config/data/logs/server`；级别默认 `info`；**保留期默认 0 = 永不清理**（`wunder-core/src/config.rs:1943-1945`）
- 运行时指标：blocking / queues / long_tasks / loop_ticks + alerts + thresholds（`core/runtime_metrics.rs`）
- WS 日志 `target="wunder.ws"`（`api/ws_log.rs`）
- 渠道运行日志：内存环形 300 条 + 20s 洪泛去重窗（`channels/service.rs:78-79,212-213`）

### 安全与隔离

- 云端读/写/执行类工具一律**在沙盒内执行**（`execute_in_sandbox`），无沙盒端点直接失败、**不回落宿主**；每用户一个容器只挂本用户目录，租户隔离由沙盒承担（2026-10-13 口径）
- exec 策略三态 allow / audit / enforce（`wunder-core/src/exec_policy.rs:6-10`）
- 审批模式 suggest / auto_edit / full_auto，默认 full_auto
- 高危命令名单：rm、rm -rf、mkfs、dd、chmod 777
- 沙箱容器 cpu 8.0 / mem 8096MB / pids 256 / timeout 300s

## 配置文件

`config/` 内容：`wunder.yaml`（引擎主配置）、`wunder-example.yaml`、`sandbox-runtime.yaml`、`mcp_config.json(+example)`、`org_units.json(+example)`、`i18n.messages.json`、`app_version.json`、`fonts.conf`、`matplotlibrc`；子目录 `benchmark/ data/ fonts/ ppt_templates/ preset_worker_cards/ prompts/ skills/`。

管理 API 的 `update()` 支持运行中热更（`api/admin.rs:108/490/743`），ConfigStore 单文件 YAML 回写 + 原子 version 计数（`core/config_store.rs:60-90`）。

## 延伸阅读

- [舰桥界面](/docs/zh-CN/surfaces/web-admin/)
- [蜂巢界面](/docs/zh-CN/surfaces/frontend/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
- [配置说明](/docs/zh-CN/reference/config/)
- [首次运行排查](/docs/zh-CN/start/troubleshooting-first-run/)
