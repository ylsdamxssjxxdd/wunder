---
title: 运维概览
summary: Wunder 运维包含部署、存储、安全、渠道运行态与可观测；目标是让系统可持续稳定运行，而不只是「启动成功」。
read_when:
  - 你在部署或接手 Wunder 运行环境
  - 你要快速定位运维与治理入口
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# 运维概览

把 Wunder 运维理解成**五条主线**会更清晰：部署、存储、安全、渠道、观测。

目标是：让系统**可持续稳定运行 10 年以上**，而不只是「启动成功」。

---

## 运维入口总览

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/">
    <strong>部署与运行</strong>
    <span>启动路径、依赖与健康检查。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/data-and-storage/">
    <strong>数据与存储</strong>
    <span>Postgres/SQLite/Weaviate/workspaces/temp_dir。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/auth-and-security/">
    <strong>认证与安全</strong>
    <span>令牌体系、边界与审批策略。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/channel-runtime/">
    <strong>渠道运行态</strong>
    <span>Webhook、长连接、outbox 与恢复。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/benchmark-and-observability/">
    <strong>监控与 Benchmark</strong>
    <span>吞吐、延迟、错误和容量评估。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/desktop-local-mode/">
    <strong>Desktop 本地模式</strong>
    <span>本地运行边界与治理要点。</span>
  </a>
</div>

---

## 五条运维主线

### 1. 部署与运行

**目标**：一键启动、健康检查、优雅升级

**关键点**：
- Docker Compose 是推荐部署方式
- 分离构建、依赖、运行态
- 健康检查：前端等待后端、Nginx 等待前端
- 升级策略：滚动更新、数据迁移自动处理

**检查清单**：
- [ ] Docker 和 Docker Compose 版本足够新
- [ ] 端口不冲突（18000、18002；若直接暴露前端开发服务，再检查 18001）
- [ ] 数据卷挂载正确
- [ ] 健康检查通过

---

### 2. 数据与存储

**目标**：不丢数据、快速恢复、性能足够

**存储分层**：

| 层级 | 技术 | 用途 | 备份策略 |
|------|------|------|----------|
| **关系数据** | PostgreSQL | 用户、会话、配置 | 定期全量 + binlog |
| **向量数据** | Weaviate | 知识库检索 | 随 PostgreSQL 备份 |
| **文件存储** | 本地磁盘/NAS | 用户工作区 | 文件系统备份 |
| **临时数据** | 本地磁盘 | 上传、下载、转换 | 定期清理 |

**关键提醒**：
- ❌ 不要把长期业务数据放进 `data/` 目录
- ❌ 不要把 `temp_dir` 当业务仓库
- ✅ 用户工作区要持久化
- ✅ PostgreSQL 必须有备份策略

---

### 3. 认证与安全

**目标**：防止越权、审计可追溯、数据安全

**安全分层**：

```
┌─────────────────────────────────────┐
│   应用层：工具审批、权限控制       │
│   - approval_mode                  │
│   - 工具白名单/黑名单              │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   接口层：鉴权、CORS、限流         │
│   - API Key / Bearer Token         │
│   - CORS 策略                       │
│   - 速率限制                        │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   网络层：HTTPS、反向代理           │
│   - Nginx TLS 终止                 │
│   - Origin 校验                     │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   基础设施：网络隔离、容器安全      │
│   - 网络策略                        │
│   - 容器只读根文件系统              │
└─────────────────────────────────────┘
```

**关键配置**：
- `security.api_key`：管理员 API Key（要复杂！）
- `security.cors.allowed_origins`：收紧 CORS 策略
- `tools.builtin.enabled`：控制内置工具可见性
- `approval_mode`：工具审批级别

---

### 4. 渠道运行态

**目标**：消息不丢、自动重连、可观测

**渠道架构**：

```
外部渠道（飞书/微信/QQ/XMPP）
        ↓ Webhook / 长连接
   ChannelHub（统一入口）
        ↓
   入站队列（快速回执）
        ↓
   调度执行（异步处理）
        ↓
   channel_outbox（出站队列）
        ↓
   渠道适配器（投递 + 重试）
```

**监控要点**：
- 入站成功/失败/重试计数
- 出站成功/失败/重试计数
- 长连接状态（在线/离线）
- outbox 积压深度

**恢复机制**：
- Webhook：幂等处理 + 重试
- 长连接：自动重连 + 心跳
- outbox：持久化 + 后台 worker

---

### 5. 监控与 Benchmark

**目标**：提前发现问题、容量规划、性能调优

**关键指标**：

| 指标 | 说明 | 告警阈值 |
|------|------|----------|
| **活跃会话数** | 当前正在执行的会话 | > max_active_sessions * 0.8 |
| **队列深度** | agent_tasks 待执行数 | > 100 |
| **错误率** | 失败请求 / 总请求 | > 5% |
| **平均延迟** | 端到端响应时间 | > 30s |
| **Token 消耗** | 上下文占用，优先看 `round_usage.total_tokens` | 接近 max_context |

**内置能力**：
- 吞吐量压测接口
- 性能采样接口
- PinchBench 能力评估
- 会话监控面板

---

## 排障优先级建议

当系统出问题时，按这个顺序排查：

1. **入口是否可达**
   - HTTP 状态码？
   - WebSocket 能连接吗？
   - DNS 解析正常吗？

2. **鉴权是否匹配**
   - API Key 对吗？
   - Token 过期了吗？
   - CORS 报错吗？

3. **依赖是否就绪**
   - PostgreSQL 正常吗？
   - Weaviate 正常吗？
   - sandbox 正常吗？
   - MCP 服务正常吗？

4. **运行态是否健康**
   - 会话 runtime 状态？
   - 渠道连接状态？
   - outbox 积压吗？

5. **指标是否异常**
   - 错误率突增？
   - 延迟突增？
   - 队列突增？

---

## 生产环境检查清单

- [ ] 使用 PostgreSQL 而非 SQLite
- [ ] 配置数据库自动备份
- [ ] 工作区存储持久化（NAS/网络存储）
- [ ] HTTPS 证书配置并自动续期
- [ ] CORS 策略收紧到必要域名
- [ ] API Key 复杂度足够（>16 位、混合字符）
- [ ] 日志收集配置（ELK/Loki 等）
- [ ] 监控告警设置（Prometheus/Grafana 等）
- [ ] 资源限制配置（Docker cgroups）
- [ ] 渠道 outbox worker 启用
- [ ] 定期清理 temp_dir 的策略

---

## 常见误区澄清

| 误区 | 正确理解 |
|------|----------|
| 进程活着 = 服务可用 | ❌ 必须检查入口和依赖健康 |
| 渠道问题 = 模型问题 | ❌ 先查接入层，再查模型 |
| temp_dir 可以存长期数据 | ❌ temp_dir 是临时存储，会被清理 |
| 部署完就没事了 | ❌ 要监控、要备份、要升级 |

---

## 下一步

- 遇到问题？→ [故障排查](/docs/zh-CN/help/troubleshooting/)
- 想看参考？→ [参考概览](/docs/zh-CN/reference/)
- 想问问题？→ [FAQ](/docs/zh-CN/help/faq/)
