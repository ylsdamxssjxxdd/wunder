---
title: Server 部署
summary: 团队和组织的选择。多用户、权限管理、渠道接入、统一治理。
read_when:
  - 你需要多人共用 wunder
  - 你需要管理员后台和统一管理
source_docs:
  - docs/API文档.md
  - packaging/docker/
updated_at: 2026-04-10
---

# Server 部署

Server 是团队和组织的选择。多用户协作、权限管理、渠道接入——都需要先部署 Server。

## 什么时候选 Server

| 场景 | 选 Server |
|------|-----------|
| 多人共用一个系统 | ✅ |
| 需要管理员后台 | ✅ |
| 需要接入飞书、微信等渠道 | ✅ |
| 需要统一治理和审计 | ✅ |
| 只是个人用 | ❌ 选 Desktop |

## 部署前准备

- Docker 和 Docker Compose（推荐方式）
- 至少 4GB 可用内存
- PostgreSQL 数据库（Docker 部署时自动包含）

## 3 步部署

### 1. 获取代码

```bash
git clone <repo-url>
cd wunder
```

### 2. 启动服务

```bash
# x86 架构
docker-compose -f docker-compose-x86.yml up -d

# ARM 架构
docker-compose -f docker-compose-arm.yml up -d
```

### 3. 访问系统

- 用户前端：http://localhost:18002
- 管理端与文档：http://localhost:18000
- 默认管理员：admin / admin

**首次登录后请立即修改默认密码。**

## 部署后必做

1. **修改默认密码**：admin 账号的默认密码不安全
2. **配置模型**：管理端 → 模型配置 → 添加 API Key
3. **创建用户**：管理端 → 用户管理 → 添加用户或开放注册
4. **检查渠道**：如果需要接入外部渠道，先配置凭证

## 核心能力

### 多租户

- 用户和单位分层管理
- 权限按角色分配
- 数据隔离

### 渠道接入

支持的外部渠道：
- 飞书
- 企业微信 / 微信
- QQ Bot
- WhatsApp Cloud
- XMPP

### 可观测性

- 服务健康监控
- 性能指标
- 日志与审计

### 安全

- 令牌鉴权
- 沙盒隔离
- 审批策略
- 请求限制

## 配置文件

主要配置文件位于 `config/` 目录：

- 模型配置
- 工具配置
- 渠道配置
- 安全策略

修改配置后通常需要重启服务。

## 延伸阅读

- [管理端界面](/docs/zh-CN/surfaces/web-admin/)
- [认证与安全](/docs/zh-CN/ops/auth-and-security/)
- [部署与运行](/docs/zh-CN/ops/deployment/)
- [配置说明](/docs/zh-CN/reference/config/)
