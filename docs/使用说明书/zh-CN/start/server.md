---
title: 舰体（server）部署
summary: 可选形态。多租户治理、渠道接入、智能体应用发布；18000 挂舰桥、18001 下发蜂巢，以及 compose 与 cargo 两种启动方式、首次登录与部署后清单。
updated_at: 2026-10-16
---

# 舰体（server）部署

舰体（server）是用户与智能体管理平台，属于可选形态。个人用装蛹（desktop）就够了；需要多租户、用户与单位管理、渠道接入、智能体应用构建发布与网关统一调度时才部署舰体。

## 端口分布

舰体部署后同时提供 **三个访问入口**（默认端口，均可用环境变量覆盖）：

| 界面 | 载体 | 默认端口 |
| --- | --- | --- |
| **舰体 API** | 舰体服务本体 | **18000**（可用 `WUNDER_HOST` / `WUNDER_PORT` 覆盖） |
| **舰桥** | 舰体自带的原生管理页面，无需构建 | 与舰体同端口 18000 |
| **蜂巢** | 独立前端应用，构建后由网页服务容器下发 | 独立容器 **18001**（可用 `NGINX_PORT` 覆盖） |

注意：**蜂巢不由舰体托管**。舰体进程只负责舰桥页面与头像、ppt 等目录；蜂巢由独立的网页服务容器在 18001 下发。

其它相关服务端口：沙箱模式 9001、onlyoffice 18080、drawio 18081、PostgreSQL 5432、extra-mcp 18004。

## 适用条件

| 场景 | 选舰体 |
| --- | --- |
| 多人共用一个系统 | ✅ |
| 需要管理员后台（舰桥） | ✅ |
| 需要接入飞书、企业微信、QQ Bot 等渠道 | ✅ |
| 需要统一治理、审计与配额 | ✅ |
| 需要构建并发布智能体应用 | ✅ |
| 只是个人用 | ❌ 选蛹（desktop） |

## 前提条件

- Docker 与 Docker Compose（推荐方式）
- 至少 4 GB 可用内存
- PostgreSQL（compose 已内置；单机也可以改用 SQLite）
- 时区：compose 默认 `Asia/Shanghai`

## docker compose 部署

```bash
# 获取代码
git clone <repo-url>
cd wunder

# x86 架构
docker compose -f docker-compose-x86.yml up -d

# ARM 架构
docker compose -f docker-compose-arm.yml up -d
```

一次会起这些服务：舰体服务（默认 18000）、沙箱（9001，带 `/health`）、onlyoffice（18080）、drawio（18081）、extra-mcp、PostgreSQL（5432）、蜂巢前端构建容器、网页服务容器（18001）。

## 本机运行（cargo）

```bash
# 1. 构建蜂巢前端
npm run build:frontend

# 2. 启动舰体（默认读 config/wunder.yaml，或 WUNDER_CONFIG_PATH 指定）
cargo run -p wunder-server

# 覆盖端口
WUNDER_HOST=127.0.0.1 WUNDER_PORT=18000 cargo run -p wunder-server
```

配置读取顺序：先用 `WUNDER_CONFIG_PATH` 指定的文件，没指定就读 `config/wunder.yaml`；YAML 里的 `${ENV}` 占位会展开；环境变量优先级最高、最后生效；缺失的字段会用示例模板补齐。

已知问题：直接拿 `config/wunder-example.yaml` 启动会解析失败，并静默回落到默认配置——请显式指向 `config/wunder.yaml`，或用 `WUNDER_CONFIG_PATH` 指定。

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

管理员凭据在 `config/wunder.yaml` 里，首次登录后请立即修改管理员口令。

口令用 Argon2 加密存储；登录令牌是不透明随机串（形如 `wund_<uuid>`、`wundr_<uuid>`、`wundc_<uuid>`，**不是 JWT**）；刷新按族轮换，同一个刷新令牌被重放会整族吊销。

## 部署后清单

1. **改管理员口令**
2. **舰桥 → 模型配置**：添加 LLM / TTS / 图像 / 视频模型端点与 API Key
3. **舰桥 → 单位管理**：确认组织树（最深 4 级），必要时导入 `config/org_units.json`
4. **舰桥 → 预设智能体**：创建预设智能体卡片模板，声明哪些字段允许用户自定义
5. **舰桥 → 用户管理**：添加用户，或开放自助注册
6. **如需接入渠道**：飞书 / 企业微信 / 微信 / QQ Bot / WhatsApp Cloud / weixin / xmpp（默认提供 7 个）
7. **观察健康**：舰桥「内部状态」页，以及 `/wunder/admin/monitor` 接口

## 多租户与治理

- 用户与单位分层管理；权限按角色分配（admin / leader / user），数据按用户与智能体隔离
- 在线状态分蜂巢 / 蛹 / 舵机三列展示
- 用户等级与配额（上限 200 级）

## 渠道接入

- 默认注册 7 个：WhatsApp Cloud / 飞书 / QQ Bot / 微信 / 微信公众号 / weixin / xmpp
- 其中飞书 / QQ Bot / xmpp / weixin 走长连接，其余只走 webhook
- **Telegram / Discord / Slack / LINE / 钉钉未支持**，只在目录里作为通用 webhook 条目存在
- 入站队列容量 512、并发 8、入队超时 80ms；出站消息持久化并做指数退避重试

## 可观测性

- 日志同时输出到控制台与 JSONL 文件（按天滚动），默认写在 `./config/data/logs/server`，级别默认 `info`，默认永不清理（保留期 0）
- 运行时指标：阻塞、队列、长任务、循环节拍，外加告警与阈值
- 渠道运行日志：内存里保留最近 300 条，并对 20 秒内的重复洪泛去重

## 安全与隔离

- 云端的读 / 写 / 执行类工具一律**在沙盒内运行**；没有可用沙盒端点时直接失败、**不回落到宿主**。每个用户一个容器，只挂本用户目录，租户隔离由沙盒承担。
- exec 策略三态：allow / audit / enforce
- 审批模式 suggest / auto_edit / full_auto，默认 full_auto
- 高危命令名单：rm、rm -rf、mkfs、dd、chmod 777
- 沙箱容器资源：CPU 8 核、内存 8096MB、进程数 256、超时 300s

## 配置文件

`config/` 目录里有引擎主配置 `wunder.yaml`、示例配置、沙箱运行时配置、MCP 配置、组织单位、国际化文案、版本号、字体与绘图配置等，另有 `benchmark/`、`data/`、`fonts/`、`ppt_templates/`、`preset_worker_cards/`、`prompts/`、`skills/` 等子目录。

配置支持运行中热更新，改动会直接写回 YAML 文件。
