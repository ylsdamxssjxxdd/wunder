---
title: 运维概览
summary: 部署、存储、安全、渠道、监控五条运维主线，以及排障优先级与生产环境核对项。
updated_at: 2026-10-13
---

运维板块面向负责部署与维护系统的人员，覆盖部署、数据与存储、账户与安全、渠道管理、监控与模型评测五条主线。

## 运维入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/"><strong>部署与运行</strong><span>选择形态、准备环境、启动检查。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/data-and-storage/"><strong>数据与存储</strong><span>数据库、工作区、知识库的数据管理。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/auth-and-security/"><strong>账户与安全</strong><span>账户体系、权限边界、安全配置。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/channel-runtime/"><strong>渠道管理</strong><span>飞书、微信等外部渠道的接入与运维。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/benchmark-and-observability/"><strong>监控与 WunderBench</strong><span>系统健康、性能观测、模型评测。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/wunderbench/"><strong>WunderBench 模型评测</strong><span>全量题库评测、评分、导出与题库扩展。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/desktop-local-mode/"><strong>蜂窝本地模式</strong><span>蜂窝本地运行的特点与维护。</span></a>
</div>

## 五条主线

### 1. 部署与运行

**目标**：正确选择运行形态，顺利启动，健康检查通过。

**关键决策**：

- 个人使用还是团队使用
- 用 PostgreSQL 还是 SQLite
- 工作区放在哪里

### 2. 数据与存储

**目标**：不丢数据、快速恢复、存储性能足够。

**存储分层**：

| 存储层 | 用途 | 备份建议 |
|--------|------|----------|
| 数据库 | 用户、会话、配置 | 定期全量备份 |
| 工作区 | 用户文件和产物 | 文件系统备份 |
| 向量库 | 知识库检索 | 随数据库备份 |
| 临时目录 | 上传下载中转 | 定期清理，不需要备份 |

### 3. 账户与安全

**目标**：权限清晰、访问受控、审计可追溯。

**安全要点**：

- API 密钥足够复杂
- 路径和命令访问受限
- 审批模式按需配置

### 4. 渠道管理

**目标**：消息不丢、连接稳定、异常可发现。

**监控要点**：

- 账号在线状态
- 消息收发正常性
- 出站队列是否积压

### 5. 监控与 WunderBench

**目标**：提前发现问题、量化系统表现。

**观测重点**：

- 活跃会话数和队列深度
- 错误率和响应延迟
- Token 消耗趋势

## 排障优先级

系统出问题时，按这个顺序排查：

1. **入口是否可达**：页面能打开、连接能建立。
2. **账户权限是否正常**：登录有效、Token 未过期。
3. **依赖服务是否正常**：数据库可用、MCP 服务连通。
4. **运行状态是否健康**：会话没有卡住、队列没有积压。
5. **指标是否异常**：错误率、延迟没有突增。

## 生产环境要做的事

### 必做项

- [ ] 使用 PostgreSQL（不要用 SQLite）
- [ ] 数据库自动备份已配置
- [ ] 工作区存储持久化（NAS/网络存储）
- [ ] HTTPS 证书配置
- [ ] API 密钥复杂度足够

### 推荐项

- [ ] CORS 策略已收紧
- [ ] 日志收集已配置
- [ ] 监控告警已设置
- [ ] 定期清理临时目录

## 按场景的运维建议

### 个人蜂窝使用

蜂窝模式下运维工作较少：定期更新应用，控制工作区文件大小，异常时使用重置功能。

### 小团队舰体（server）

关注数据库备份、用户管理和 Token 发放，以及基本监控。

### 组织生产环境

需要全面运维：完整的备份策略、监控告警体系、安全审计、渠道管理和性能调优。

## 常见问题

| 现象 | 正确理解 |
|------|----------|
| 进程活着就代表服务可用 | 还要确认入口和依赖健康 |
| 渠道问题当成模型问题 | 先查接入层，再查模型 |
| 拿临时目录存长期数据 | 临时目录会被清理 |
| 部署完就没事了 | 还要监控、备份和维护 |

## 延伸阅读

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [WunderBench 模型评测](/docs/zh-CN/ops/wunderbench/)
- [故障排查](/docs/zh-CN/help/troubleshooting/)
- [参考概览](/docs/zh-CN/reference/)
