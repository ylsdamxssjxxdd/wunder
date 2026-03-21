---
title: Server 部署
summary: 需要多用户治理、统一接入层和管理员后台时，再看 `wunder-server` 这条线。
read_when:
  - 你要部署 server 形态
  - 你需要多用户、组织治理和对外接入能力
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# Server 部署

如果你需要**团队协作、多用户治理、统一对外接口**，这页才用看。

`wunder-server` 是 Wunder 的核心服务形态，负责多租户治理、统一接入和管理员后台。

---

## 什么时候必须上 Server？

| 场景 | 选 Server |
|------|-----------|
| 多用户并发访问 | ✅ |
| 组织、单位、租户和管理员治理 | ✅ |
| 统一暴露 `/wunder`、聊天接口和 `A2A` | ✅ |
| 把 Wunder 当平台能力接给别的系统 | ✅ |
| 只是个人本地使用 | ❌ 选 Desktop |

---

## Server 架构概览

```
                    ┌─────────────────┐
                    │   外部访问      │
                    │  (用户/业务方)  │
                    └────────┬────────┘
                             │
                    ┌────────▼────────┐
                    │   Nginx (18001)│
                    │  反向代理 + 静态│
                    └────────┬────────┘
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
    │ Frontend│        │ wunder- │        │ 静态文档 │
    │  (用户) │        │ server  │        │   站     │
    └─────────┘        └────┬────┘        └─────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
    │Postgres │        │ Weaviate│        │ wunder- │
    │  (主库) │        │(向量库) │        │ sandbox │
    └─────────┘        └─────────┘        └─────────┘
         │                                       │
    ┌────▼────┐                             ┌────▼────┐
    │ 工作区  │                             │ extra-  │
    │ 存储    │                             │ mcp     │
    └─────────┘                             └─────────┘
```

### Server 负责什么

| 能力 | 说明 |
|------|------|
| **执行入口** | `/wunder` 底层智能体调度 |
| **会话入口** | `/wunder/chat/*` 完整聊天接口 |
| **A2A 接口** | `/a2a` 系统级智能体互通 |
| **流式能力** | WebSocket + SSE 双链路 |
| **治理能力** | 用户、单位、权限、配额 |
| **渠道接入** | 飞书、微信、QQ、XMPP 等（三种形态均支持） |

---

## 部署前准备

### 硬件要求

| 规模 | CPU | 内存 | 磁盘 |
|------|-----|------|------|
| 小规模（<10 用户） | 2 核 | 4GB | 50GB |
| 中规模（10-50 用户） | 4 核 | 8GB | 100GB |
| 大规模（>50 用户） | 8 核+ | 16GB+ | 200GB+ |

### 软件依赖

- Docker 20.10+
- Docker Compose 2.0+
- 或直接运行 Rust 二进制（需要 PostgreSQL）

### 数据库选择

| 场景 | 数据库 |
|------|--------|
| 生产环境 | PostgreSQL（推荐） |
| 开发测试 | PostgreSQL（Docker 自带） |
| 桌面端 | SQLite3 |

> **注意**：Server 形态**不使用** SQLite，必须用 PostgreSQL。

---

## Docker Compose 部署（推荐）

### 1. 获取代码

```bash
git clone <repo-url>
cd wunder
```

### 2. 配置环境变量（可选）

复制 `.env.example` 为 `.env`，根据需要修改：

```bash
cp .env.example .env
```

关键配置：

| 配置项 | 说明 | 默认值 |
|--------|------|--------|
| `WUNDER_PORT` | Server 端口 | 18000 |
| `WUNDER_TEMP_DIR_ROOT` | 临时文件目录 | ./temp_dir |
| `DATABASE_URL` | PostgreSQL 连接串 | postgres://... |

### 3. 启动服务

**x86 架构：**
```bash
docker-compose -f docker-compose-x86.yml up -d
```

**ARM 架构（Mac M1/M2/M3、树莓派）：**
```bash
docker-compose -f docker-compose-arm.yml up -d
```

### 4. 等待启动

首次启动会：
- 拉取或构建镜像
- 初始化 PostgreSQL 数据库
- 启动 wunder-server、frontend、sandbox 等

等待约 1-2 分钟，检查状态：

```bash
docker-compose -f docker-compose-x86.yml ps
```

### 5. 访问系统

| 服务 | 地址 | 说明 |
|------|------|------|
| 用户前端 | http://localhost:18001 | 普通用户入口 |
| 管理端 | http://localhost:18000 | 管理员后台 |
| 默认管理员 | admin / admin | 首次登录请修改密码 |

---

## 关键配置项

### 配置文件

主配置文件：`config/wunder.yaml`

```yaml
server:
  mode: server
  port: 18000
  max_active_sessions: 100

llm:
  models:
    - name: gpt-4o
      api_key: your-api-key
      endpoint: https://api.openai.com/v1
      max_context: 128000
      max_rounds: 20

database:
  url: postgres://wunder:wunder@postgres:5432/wunder

security:
  api_key: your-admin-api-key
  cors:
    allowed_origins:
      - http://localhost:18001
```

### 持久化数据

| 数据类型 | Docker 卷 | 说明 |
|----------|-----------|------|
| 用户工作区 | `wunder_workspaces` | `/workspaces` |
| PostgreSQL 数据 | `wunder_logs` | PostgreSQL 数据目录 |
| 临时文件 | `./temp_dir` | 临时上传/下载 |

> **重要**：不要把长期业务数据放进 `data/` 目录，容易被清理或覆盖！

---

## 对外访问规划

### 暴露哪些路径

通过 Nginx 反向代理，建议暴露：

| 路径 | 说明 |
|------|------|
| `/` | 用户前端 |
| `/wunder` | 核心 API |
| `/wunder/chat` | 聊天 API |
| `/wunder/admin` | 管理端 API（需保护） |
| `/a2a` | A2A 接口 |
| `/.well-known/agent-card.json` | 智能体发现 |

### 关键约束

| 约束 | 说明 |
|------|------|
| `user_id` 不要求注册 | 可以传任意虚拟标识 |
| 优先 WebSocket | SSE 作为兜底 |
| 业务方接 `/wunder` | 底层执行入口 |
| 稳定调用接 `/wunder/chat/*` | 完整会话入口 |

---

## 生产环境检查清单

- [ ] 使用 PostgreSQL 而非 SQLite
- [ ] 配置数据库备份策略
- [ ] 工作区存储持久化
- [ ] HTTPS 证书配置
- [ ] CORS 策略收紧
- [ ] API Key 复杂度足够
- [ ] 日志收集配置
- [ ] 监控告警设置
- [ ] 资源限制配置

---

## 常见问题

**Q: 能和 Desktop 配合用吗？**  
A: 可以！Desktop 可以接入 Server 作为远端 Gateway。

**Q: user_id 必须先注册吗？**  
A: 不需要！外部调用可以传任意虚拟 user_id。

**Q: 只暴露 /wunder 够吗？**  
A: 建议同时规划聊天域和 WebSocket，体验更好。

**Q: 如何升级？**  
A: 拉取最新代码，重新 `docker-compose up -d` 即可。

---

## 下一步

- 想了解部署细节？→ [部署与运行](/docs/zh-CN/ops/deployment/)
- 要配置安全？→ [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- 想看系统架构？→ [系统架构](/docs/zh-CN/concepts/architecture/)
- 要接入 API？→ [wunder API](/docs/zh-CN/integration/wunder-api/)
