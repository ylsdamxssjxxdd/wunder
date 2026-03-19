---
title: 运维概览
summary: Wunder 的运维面要同时处理部署、存储、安全、渠道运行态、观测和 benchmark，不只是把服务启动起来。
read_when:
  - 你在部署或接手 Wunder 运行环境
  - 你要快速找到监控、性能、渠道和数据治理入口
source_docs:
  - docs/系统介绍.md
  - docs/设计方案.md
  - docs/API文档.md
---

# 运维概览

如果你在做 Wunder 的上线、观测和治理，这里是总入口。

## 先看这些

<div class="docs-card-grid docs-card-grid-compact">
  <a class="docs-card" href="/docs/zh-CN/ops/deployment/">
    <strong>部署与运行</strong>
    <span>先把 server、desktop、本地开发三条运行路径分清。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/data-and-storage/">
    <strong>数据与存储</strong>
    <span>区分 PostgreSQL、SQLite、Weaviate、workspaces 和 temp_dir。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/auth-and-security/">
    <strong>认证与安全</strong>
    <span>API Key、用户 Token、外链鉴权和沙盒边界。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/benchmark-and-observability/">
    <strong>监控与 Benchmark</strong>
    <span>会话监控、工具统计、性能采样和能力评估入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/channel-runtime/">
    <strong>渠道运行态</strong>
    <span>账号状态、长连接、入出站和运行日志的排障入口。</span>
  </a>
  <a class="docs-card" href="/docs/zh-CN/ops/desktop-local-mode/">
    <strong>Desktop 本地模式</strong>
    <span>本地运行时的存储、工作区和能力边界。</span>
  </a>
</div>

## 按问题找页面

### 你在做上线

- [部署与运行](/docs/zh-CN/ops/deployment/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)

### 你在做权限和治理

- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [提示词模板参考](/docs/zh-CN/reference/prompt-templates/)

### 你在做可观测

- [监控与 Benchmark](/docs/zh-CN/ops/benchmark-and-observability/)
- [流式事件参考](/docs/zh-CN/reference/stream-events/)

### 你在排查渠道

- [渠道运行态](/docs/zh-CN/ops/channel-runtime/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)

## 你最需要记住的点

- Wunder 的运维不只是进程运维，还包括会话治理、渠道运行态和模型链路观测。
- 工作区、数据库、向量库和临时目录必须分开治理。
- 监控、吞吐、性能采样和 benchmark 各自解决的问题不同，不要混用。

## 相关文档

- [参考概览](/docs/zh-CN/reference/)
- [帮助](/docs/zh-CN/help/)
