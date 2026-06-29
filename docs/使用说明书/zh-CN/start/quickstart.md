---
title: 快速开始
summary: 用最短路径跑通 wunder 的第一个任务。默认推荐 desktop，其次按 server 或 cli 分流。
read_when:
  - 用户第一次使用 wunder
  - 用户需要快速跑通一个可验证结果
source_docs:
  - README.md
  - docs/设计文档/01-系统总体设计.md
---

# 快速开始

这页帮用户完成第一个任务，默认推荐 Desktop，其次按 Server 或 CLI 分流。

## 第一步：选对路径

| 场景 | 推荐入口 | 原因 |
|----------|----------|--------|
| 想直接在本地用起来 | [Desktop 入门](/docs/zh-CN/start/desktop/) | 门槛最低，下载即用 |
| 需要团队协作和管理 | [Server 部署](/docs/zh-CN/start/server/) | 多用户、权限、统一管理 |
| 做自动化或脚本集成 | [CLI 使用](/docs/zh-CN/start/cli/) | 终端驱动、脚本化 |

---

## 最短路径：Desktop（推荐）

适合：个人用户、本地演示

### 5 步跑通

1. **下载安装**
   - 去 Releases 下载匹配用户系统的安装包
   - 安装或解压后启动

2. **配置模型**
   - 打开「系统设置」→「模型配置」
   - 填入用户的 API Key 和接口地址
   - 保存前点「测试连接」确认能用

3. **发起第一次对话**
   - 回到聊天界面
   - 输入：`帮我列出当前目录的文件`
   - 回车发送

4. **观察执行过程**
   - 用户会看到模型思考 → 调用工具 → 展示结果 → 给出回复

5. **验收通过**
   - 看到完整的执行过程和结果，说明已跑通。

### Desktop 特有能力

- **本地优先**：默认本地运行，也可接远端
- **桌面控制**：可操作本地窗口、文件、浏览器
- **持久工作区**：文件不会被自动清理
- **直接编辑智能体**：可随时调整智能体配置和提示词

---

## 团队路径：Server

适合：多用户协作、组织治理

### 启动前准备

- Docker 和 Docker Compose（推荐）
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
   - 默认管理员：admin / admin

### Server 核心能力

- **多租户**：用户、单位、权限分层管理
- **渠道接入**：飞书、微信、QQ 等
- **可观测性**：监控、压测、能力评估

---

## 开发路径：CLI

适合：开发者、自动化脚本

### 安装与运行

```bash
# 编译（需要 Rust）
cargo build --release

# 运行
./target/release/wunder-cli
```

### 第一次会话

```bash
wunder-cli
> 帮我写一个 Hello World 的 Python 脚本
```

### CLI 特有能力

- **TUI 界面**：类似 Codex 的终端交互
- **会话管理**：`/fork` 分叉、`/compact` 压缩、`/resume` 恢复
- **JSONL 输出**：便于管道和自动化集成

---

## 验收清单

不管选哪条路径，确认以下几点：

- [ ] 可以成功发起一次对话
- [ ] 可以看到中间步骤和工具调用
- [ ] 可以拿到最终结果
- [ ] 知道下一步该看什么文档

---

## 下一步

- 想深入理解系统？→ [核心概览](/docs/zh-CN/concepts/)
- 要接入到现有系统？→ [接入概览](/docs/zh-CN/integration/)
- 遇到问题了？→ [故障排查](/docs/zh-CN/help/troubleshooting/)
- 想看所有工具？→ [工具总览](/docs/zh-CN/tools/)
