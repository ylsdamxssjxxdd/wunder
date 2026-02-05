# wunder
wunder 是一个多用户智能体调度平台，可灵活对接大模型 API、MCP 工具与 Skills 流程，内置知识库。Rust (Axum) 服务对外暴露统一的 `/wunder` 入口，支持流式与非流式响应，并提供调试控制台与管理接口。
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## 核心理念
对开发者来说，一切皆接口；对大模型来说，一切皆工具。
- 内置工具（动态）：双手与脚
- MCP 工具（动态）：刀剑
- Skills（静态）：流程手册
- 知识工具（静态）：百科
- 自建工具（动态）：个人装备
- 共享工具（动态）：装备市场

wunder 可自托管为 MCP 工具（`/wunder/mcp`），便于跨系统调用。

## 用户侧功能与使用
### 用户侧功能
- 聊天页默认入口，支持流式过程与最终回复展示。
- 功能广场 `/home`：新建智能体应用，浏览共享智能体。
- 工作区：文件与产物沉淀，支持资源预览。
- 历史会话：回看与继续对话。

### 使用方式
1. 启动服务后打开用户侧前端：`http://127.0.0.1:18001`
2. 进入 `/home` 新建或选择智能体应用（也可直接进入聊天页）。
3. 在聊天页进行对话，所需资料可先整理到工作区。

## 快速开始（自托管/开发）
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
用户侧前端：`http://127.0.0.1:18001`

## 平台能力概览
- 工具体系：内置 + MCP + Skills + 知识库 + 自建/共享工具，可按需启用。
- 自动上下文压缩 + 可选长期记忆，支持长会话。
- 多用户隔离：`user_id` 既是会话键也是工作区键，可为虚拟用户。
- 配额治理：注册用户按模型调用消耗配额，虚拟 `user_id` 不受限。
- 调试与监控：管理端支持会话监控、吞吐压测与性能采样。
- UI 与系统提示支持多语言切换。

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
- 测试方案：`docs/测试方案.md`
