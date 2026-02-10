# wunder
wunder 是一个面向组织与用户的智能体调度系统，支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，并内置工具链、知识库与长期记忆能力。Rust (Axum) 服务对外暴露统一的 `/wunder` 入口，支持流式与非流式响应，并提供用户侧与管理侧前端、调试控制台与管理接口。
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## 核心理念
对开发者来说，一切皆接口；对大模型来说，一切皆工具。
- 内置工具（动态）：手和脚
- MCP 工具（动态）：刀和剑
- Skills（静态）：流程手册
- 知识工具（静态）：百科全书
- 自建工具（动态）：个人装备
- 共享工具（动态）：装备市场

wunder 可自托管为 MCP 工具（`/wunder/mcp`），便于跨系统调用。

<img width="700" height="380" alt="ayanami" src="https://github.com/user-attachments/assets/8ef1f7f9-f563-4253-8663-238c831d1aa3" />

## 平台能力矩阵
### 用户侧（应用使用者）
- 功能广场 `/home`：新建智能体应用，浏览共享智能体。
- 聊天页默认入口，支持流式过程与最终回复展示。
- 工作区：文件与产物沉淀，支持资源预览。
- 历史会话：回看与继续对话。
- 用户侧前端提供浅/深两套主题。

### 管理侧（组织与运维）
- 用户/单位/权限管理与配额治理。
- 智能体应用生命周期管理（创建、发布、共享、下线）。
- 模型、工具、Skills、MCP 管理与启用。
- 网关统一入口与策略路由（鉴权、限流、审计）。
- 调试监控：会话监控、吞吐压测与性能采样。

### 调度与平台侧（系统能力）
- 自动上下文压缩 + 可选长期记忆，支持长会话。
- 多用户隔离：`user_id` 既是会话键也是工作区键，可为虚拟用户。
- 工具体系：内置 + MCP + Skills + 知识库 + 自建/共享工具，可按需启用。
- UI 与系统提示支持多语言切换。

## 入口与使用
- 管理端调试界面：`http://127.0.0.1:18000`
- 调试前端：`http://127.0.0.1:18001`
- 用户侧前端（开发，默认）：`http://127.0.0.1:18001`
- 用户侧前端（生产静态，启用 Nginx 时）：`http://127.0.0.1:18002`
- API 入口：`/wunder`（支持流式与非流式）

### 使用方式
1. 启动服务后默认打开用户侧前端（开发）：`http://127.0.0.1:18001`（若启用 Nginx 静态部署则使用 `http://127.0.0.1:18002`）
2. 进入 `/home` 新建或选择智能体应用（也可直接进入聊天页）。
3. 在聊天页进行对话，所需资料可先整理到工作区。

### 会话控制命令（用户侧前端 + 渠道入站）
用户侧聊天输入框和渠道入站都支持会话控制命令：

- `/new` 或 `/reset`：新建线程，并切换为当前会话。
- `/stop` 或 `/cancel`：请求停止当前会话执行。
- `/help` 或 `/?`：返回命令帮助说明。

用户侧前端额外支持：

- `/compact`：主动触发当前会话上下文压缩。

补充说明：
- 命令不区分大小写，只解析文本首个 token（例如 `/new hello` 等价 `/new`）。
- 渠道命令解析位于 `src/channels/service.rs`；用户侧命令处理位于 `frontend/src/views/ChatView.vue`。

## 快速开始
### 1) 更新配置
拷贝示例配置：`config/wunder-example.yaml` -> `config/wunder.yaml`
拷贝环境示例：`.env.example` -> `.env`，配置 `WUNDER_API_KEY`、`WUNDER_POSTGRES_DSN`、`WUNDER_SANDBOX_ENDPOINT` 等。
前端 API base：在仓库根 `.env` 中设置 `VITE_API_BASE` 或 `VITE_API_BASE_URL`，并重启前端。

### 2) 启动服务
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
首次启动会拉取基础镜像并编译依赖，可能需要较长时间。

### 3) 打开入口
管理端调试界面：`http://127.0.0.1:18000`
用户侧前端（开发，默认）：`http://127.0.0.1:18001`
用户侧前端（生产静态，启用 Nginx 时）：`http://127.0.0.1:18002`

## 工作区与持久化
- 工作区路径：`workspaces/<user_id>`（提示词使用 `/workspaces/<user_id>/`）。
- 对话历史/工具日志/监控/锁与溢出事件写入数据库（默认 PostgreSQL，可选 SQLite）。
- 管理端覆写配置写入 `data/config/wunder.override.yaml`。

## Skills 与 MCP
- Skills 默认从 `skills/` 与 `EVA_SKILLS/` 加载，通过 `config/wunder.yaml` 或管理端启用。
- MCP 在 `config/wunder.yaml` 或 `data/config/wunder.override.yaml` 中配置，由管理端维护工具清单。

## 项目结构
```
src/                 # Rust 服务模块
  api/               # /wunder、/a2a、admin APIs
  core/              # 配置/鉴权/i18n
  services/          # tools/LLM/MCP/workspace
  ops/               # monitor/evaluation/throughput
  sandbox/           # sandbox client/server
  orchestrator/      # 调度引擎
  storage/           # PostgreSQL/SQLite 持久化
config/              # 基础配置
prompts/             # system/tool/memory 提示词
workspaces/          # 用户工作区
skills/              # 内置技能
EVA_SKILLS/          # 技能目录
knowledge/           # 知识库
temp_dir/            # 临时文件
web/                 # 管理端调试 UI
frontend/            # 用户侧前端 (Vue3)
data/config/         # 管理端覆写
data/throughput/     # 吞吐报告
docs/                # 设计/API/测试文档
```

## 相关文档
- 系统介绍：`docs/系统介绍.md`
- 设计方案：`docs/设计方案.md`
- API 文档：`docs/API文档.md`
- 测试方案：`docs/方案/测试方案.md`

## wunder已吞噬核心
- EVA：<https://github.com/ylsdamxssjxxdd/eva>
- OpenAI Codex：<https://github.com/openai/codex>
- Claude Code：<https://github.com/anthropics/claude-code>
- OpenClaw：<https://github.com/openclaw/openclaw>
- OpenCode：<https://github.com/anomalyco/opencode>

