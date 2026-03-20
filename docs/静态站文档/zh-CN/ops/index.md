---
title: 运维概览
summary: Wunder 运维包含部署、存储、安全、渠道运行态与可观测；目标是让系统可持续稳定运行，而不只是“启动成功”。
read_when:
  - 你在部署或接手 Wunder 运行环境
  - 你要快速定位运维与治理入口
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# 运维概览

把 Wunder 运维理解成五条主线会更清晰：部署、存储、安全、渠道、观测。

## 运维入口

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/"><strong>部署与运行</strong><span>启动路径、依赖与健康检查。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/data-and-storage/"><strong>数据与存储</strong><span>Postgres/SQLite/Weaviate/workspaces/temp_dir。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/auth-and-security/"><strong>认证与安全</strong><span>令牌体系、边界与审批策略。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/channel-runtime/"><strong>渠道运行态</strong><span>Webhook、长连接、outbox 与恢复。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/benchmark-and-observability/"><strong>监控与 Benchmark</strong><span>吞吐、延迟、错误和容量评估。</span></a>
  <a class="docs-card" href="/docs/zh-CN/ops/desktop-local-mode/"><strong>Desktop 本地模式</strong><span>本地运行边界与治理要点。</span></a>
</div>

## 排障优先级建议

1. 入口是否可达（HTTP/WS）
2. 鉴权是否匹配（API Key / 用户 Token）
3. 依赖是否就绪（数据库、sandbox、MCP）
4. 运行态是否健康（会话 runtime、渠道状态、outbox）
5. 指标是否异常（吞吐、延迟、错误率）

## 常见误区

- 进程活着不等于服务可用，必须看入口和依赖。
- 渠道问题常在接入层，不一定是模型问题。
- `temp_dir` 不是长期存储目录，不能当业务仓库。

## 延伸阅读

- [帮助中心](/docs/zh-CN/help/)
- [参考概览](/docs/zh-CN/reference/)
