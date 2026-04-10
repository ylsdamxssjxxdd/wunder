---
title: 快速开始
summary: 用最短路径跑通 wunder 的第一条可用链路；默认推荐 desktop，其次按 server 或 cli 分流。
read_when:
  - 你第一次使用 wunder
  - 你需要在 10 分钟内跑通一个可验证结果
source_docs:
  - README.md
  - docs/系统介绍.md
  - docs/设计方案.md
---

# 快速开始

这页只做一件事：**帮你在 10 分钟内跑通第一条可用链路**。

## 第一步：选对你的路径

| 你的情况 | 推荐入口 | 为什么？ |
|----------|----------|----------|
| 我只是想马上用起来 | [Desktop 入门](/docs/zh-CN/start/desktop/) | 门槛最低，下载即用 |
| 我需要团队协作和管理 | [Server 部署](/docs/zh-CN/start/server/) | 多用户、权限、统一治理 |
| 我是开发者，要做自动化 | [CLI 使用](/docs/zh-CN/start/cli/) | 终端驱动、脚本化、流水线 |

---

## 最短路径：Desktop（推荐）

适合：个人用户、本地演示、桌面工作台

### 5 步跑通

1. **下载安装**
   - 去 Releases 下载匹配你系统的 `wunder-desktop`
   - 安装或解压后启动

2. **配置模型**
   - 打开「系统设置」→「模型配置」
   - 填入你的 API Key 和端点
   - 保存并测试连接

3. **发起第一次对话**
   - 回到聊天界面
   - 输入：`帮我列出当前目录的文件`
   - 回车发送

4. **观察执行过程**
   - 你会看到：
     - 模型思考
     - 工具调用（列出文件）
     - 中间结果
     - 最终回复

5. **验收通过**
   - 如果看到了完整的执行过程和结果
   - 恭喜，你的核心链路已经通了！

### Desktop 特有能力

- **本地优先**：默认本地运行，也可接远端 gateway
- **桌面控制**：可操作本地窗口、文件、浏览器
- **持久工作区**：文件不会被 24 小时自动清理
- **智能体内见**：可直接编辑智能体配置和提示词

---

## 团队路径：Server

适合：多用户协作、组织治理、统一接入

### 启动前准备

- Docker 和 Docker Compose（推荐）
- PostgreSQL 数据库（或用 compose 自带）
- 至少 4GB 可用内存

### 3 步部署

1. **获取代码**
   ```bash
   git clone <repo-url>
   cd wunder
   ```

2. **启动服务**
   ```bash
   # x86 架构
   docker-compose -f docker-compose-x86.yml up -d
   
   # ARM 架构
   docker-compose -f docker-compose-arm.yml up -d
   ```

3. **访问系统**
   - 用户前端：http://localhost:18002
   - 管理端与文档：http://localhost:18000
   - 前端开发服务（仅联调时直接访问）：http://localhost:18001
   - 默认管理员：admin / admin

### Server 核心能力

- **多租户**：用户、单位、权限分层治理
- **统一接口**：`/wunder`、`/wunder/chat/*`、`/a2a`
- **渠道接入**：飞书、微信、QQ、XMPP 等（三种形态均支持）
- **可观测性**：监控、压测、能力评估

---

## 开发路径：CLI

适合：开发者、自动化脚本、终端任务

### 安装与运行

```bash
# 编译（需要 Rust）
cargo build --release

# 运行
./target/release/wunder-cli
```

### 第一次会话

```bash
# 启动并进入交互模式
wunder-cli

# 输入任务
> 帮我写一个 Hello World 的 Python 脚本
```

### CLI 特有能力

- **TUI 界面**：类似 Codex 的终端交互
- **会话管理**：`/fork` 分叉、`/compact` 压缩、`/resume` 恢复
- **调试工具**：`/debug-config`、`/statusline`
- **JSONL 输出**：便于管道和自动化集成

---

## 验收清单

不管选哪条路径，确认以下几点：

- [ ] 可以成功发起一次执行请求
- [ ] 可以看到流式过程（中间步骤、工具调用）
- [ ] 可以拿到最终结果
- [ ] 知道下一步该看什么文档

---

## 下一步

- 想深入理解系统？→ [核心概览](/docs/zh-CN/concepts/)
- 要接入到你的系统？→ [接入概览](/docs/zh-CN/integration/)
- 遇到问题了？→ [故障排查](/docs/zh-CN/help/troubleshooting/)
- 想看所有工具？→ [工具总览](/docs/zh-CN/tools/)
