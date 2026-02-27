# wunder
wunder 是一个面向组织或用户的智能体调度系统，wunder拥有三种运行形态：server（服务，云端）、cli（命令行，本地）、desktop（桌面，本地），三种形态可各自独立运行或分发。server是项目核心，支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，并内置工具链、知识库与长期记忆能力。cli与desktop基于server构建。

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

## 运行形态与能力
| 形态 | 适用场景 | 核心能力 | 默认持久化 |
| --- | --- | --- | --- |
| `wunder-server` | 团队协作、多租户、统一网关接入 | `/wunder` 统一 API、用户与单位管理、应用发布、渠道接入、监控与评估 | `workspaces/<user_id>` + PostgreSQL（默认） |
| `wunder-cli` | 本地开发、自动化脚本、轻量对话 | 交互式 TUI、`ask/chat/resume`、`exec/tool/mcp/skills/config/doctor`、JSONL 事件输出、`tool_call/function_call` 切换 | 启动目录下 `WUNDER_TEMP/`（SQLite + 配置 + 会话） |
| `wunder-desktop` | 本地普通用户、可视化操作 | Tauri 桌面窗口（可选 Electron/AppImage）+ 本地桥接服务、复用用户侧 UI、MCP/Skills/工具管理、WebSocket 优先 + SSE 兜底 | 默认程序同级 `WUNDER_TEMPD/` + `WUNDER_WORK/`（Electron 版使用 userData 路径） |

## 平台能力矩阵
### 用户侧（frontend）
- 功能广场 `/home`：新建智能体应用，浏览共享应用。
- 聊天页默认入口，支持流式中间过程与最终回复展示。
- 工作区：文件与产物沉淀，支持资源预览与继续编辑。
- 历史会话：回看、续聊、恢复上下文。
- 用户侧前端提供浅色/深色双主题。

### 管理侧（web）
- 用户/单位/权限管理与配额治理。
- 智能体应用生命周期管理（创建、发布、共享、下线）。
- 模型、工具、Skills、MCP 配置与启用。
- 网关统一入口与策略路由（鉴权、限流、审计）。
- 调试监控：会话监控、吞吐压测与性能采样。

### 调度与编排侧（Rust Core）
- 自动上下文压缩 + 可选长期记忆，支持长会话稳定运行。
- 多用户隔离：`user_id` 既是会话键也是工作区键，可使用虚拟用户。
- 工具体系：内置 + MCP + Skills + 知识库 + 自建/共享工具按需组合。
- 会话轮次拆分为“用户轮次/模型轮次”，便于观测与治理。
- 通讯策略以 WebSocket 为先，SSE 作为兜底恢复链路。

## 入口与使用
### 角色与访问方式（建议）
- **管理员**：使用管理员前端（`web`）进行模型、工具、权限、监控与治理操作。
- **用户**：可按场景选择用户网页前端（`frontend`）、`wunder-desktop` 或 `wunder-cli` 访问智能体能力。
- **渠道接入**：也可通过渠道（如飞书/WhatsApp/QQ 等）接入，与同一调度链路会话互通。

### server 入口（多用户 / 平台部署）
- 管理端调试界面：`http://127.0.0.1:18000`
- 用户侧前端（开发，默认）：`http://127.0.0.1:18001`
- 用户侧前端（生产静态，启用 Nginx 时）：`http://127.0.0.1:18002`
- API 入口：`/wunder`（支持流式与非流式）
- MCP 入口：`/wunder/mcp`

### 本地形态说明（CLI / Desktop）
- `wunder-cli`：本地命令行形态，默认将状态写入启动目录下 `WUNDER_TEMP/`。
- `wunder-desktop`：本地图形界面形态，默认将状态写入程序同级 `WUNDER_TEMPD/`，工作目录为 `WUNDER_WORK/`；Electron 版改为 userData 路径（通过 `--temp-root/--workspace` 注入）。
- 本地形态启动示例统一放在“快速开始”章节，避免与 server 部署流程混淆。

### 会话控制命令
#### 用户前端 + 渠道入站
- `/new` 或 `/reset`：新建线程并切换为当前会话。
- `/stop` 或 `/cancel`：请求停止当前会话执行。
- `/help` 或 `/?`：返回命令帮助说明。
- `/compact`（用户前端）：主动触发当前会话上下文压缩。

补充说明：
- 命令不区分大小写，只解析文本首个 token（例如 `/new hello` 等价 `/new`）。
- 渠道命令解析位于 `src/channels/service.rs`；用户侧命令处理位于 `frontend/src/views/ChatView.vue`。

#### CLI 交互命令（摘要）
- `/help`、`/status`、`/model`、`/tool-call-mode`（`/mode`）
- `/session`、`/system`、`/config`
- `/new`、`/exit`（以及 TUI 中 `/mouse` 等交互命令）

## 快速开始
### 路径 A：wunder-server（多用户 / 平台部署）
#### 1) 配置（可选）
- 开箱即用：无需 `.env` 或 `config/wunder.yaml`，缺少 `config/wunder.yaml` 时会自动回退使用 `config/wunder-example.yaml`。
- 需要自定义时再拷贝示例：
  - `config/wunder-example.yaml` -> `config/wunder.yaml`
  - `.env.example` -> `.env`（如需覆盖 `WUNDER_API_KEY`、`WUNDER_POSTGRES_DSN`、`WUNDER_SANDBOX_ENDPOINT` 等）
- 前端 API base：如需修改，在仓库根 `.env` 中设置 `VITE_API_BASE` 或 `VITE_API_BASE_URL`，并重启前端。

#### 2) 启动 server
x86
```bash
docker compose -f docker-compose-x86.yml up
```
arm
```bash
docker compose -f docker-compose-arm.yml up
```
首次启动会拉取基础镜像并编译依赖，可能需要较长时间。

#### 3) 打开 server 入口
- 管理端调试界面：`http://127.0.0.1:18000`
- 用户侧前端（开发）：`http://127.0.0.1:18001`
- 用户侧前端（生产静态）：`http://127.0.0.1:18002`

### 路径 B：wunder-cli（本地命令行）
```bash
# 进入交互模式（TTY 下默认 TUI）
cargo run --bin wunder-cli

# 单次提问
cargo run --bin wunder-cli -- ask "请总结当前目录项目结构"

# 恢复最近会话
cargo run --bin wunder-cli -- resume --last

# 查看可用工具
cargo run --bin wunder-cli -- tool list
```

### 路径 C：wunder-desktop（本地图形界面）
```bash
# 启动桌面窗口（默认本地桥接 127.0.0.1:18000）
cargo run --features desktop --bin wunder-desktop

# 使用随机可用端口启动桥接
cargo run --features desktop --bin wunder-desktop -- --port 0

# 仅桥接模式（不拉起桌面窗口）
cargo run --features desktop --bin wunder-desktop -- --bridge-only --open

# 仅桥接二进制（无需 Tauri，用于 Electron 壳）
cargo run --bin wunder-desktop-bridge -- --open
```

## 持久化与目录约定
- server 工作区：`workspaces/<user_id>`（提示词中使用 `/workspaces/<user_id>/`）。
- Docker Compose 默认使用两个 named volume：
  - `wunder_workspaces`：挂载到 `/workspaces`（用户工作区）
  - `wunder_logs`：挂载到 PostgreSQL/Weaviate 数据目录（`/var/lib/postgresql/data`、`/var/lib/weaviate`）
- `temp_dir` 默认落在本地 `./temp_dir`（容器内 `/app/temp_dir`；可用 `WUNDER_TEMP_DIR_ROOT` 覆盖）
- 其他运行态配置保留在仓库本地目录（bind mount）：
  - `./data/config`、`./data/prompt_templates`、`./data/user_tools` 等继续存放在本地 `data/`
- 构建与依赖缓存（`target/`、`.cargo/`、`frontend/node_modules/`）保持写入仓库目录（bind mount），便于本地清理与管理。
- 注意：`docker compose down -v` 会删除 `wunder_workspaces` 与 `wunder_logs`；不会删除仓库本地 `data/`。
- CLI 持久化：`WUNDER_TEMP/`（默认包含 SQLite、配置覆盖、会话状态、用户工具数据）。
- Desktop 持久化：`WUNDER_TEMPD/`；默认工作目录 `WUNDER_WORK/`（Electron 版使用 userData 路径）。
- 对话历史、工具日志、监控事件等写入数据库（server 默认 PostgreSQL，本地形态默认 SQLite）。
- 管理端覆写配置路径：`data/config/wunder.override.yaml`（运行态覆盖文件，可重建）。

## Skills 与 MCP
- Skills 默认从 `skills/` 加载，可通过 `config/wunder.yaml` 或管理端启用。
- MCP 服务可在 `config/wunder.yaml` 或 `data/config/wunder.override.yaml` 配置，并由管理端维护工具清单。
- server / cli / desktop 共享同一套工具协议与编排能力，降低跨形态迁移成本。

## 项目结构
```text
src/                 # Rust 核心服务（API/调度/工具/存储）
  api/               # /wunder、/a2a、admin 等接口
  channels/          # 外部渠道接入与分发
  gateway/           # 网关控制面能力
  orchestrator/      # 调度引擎
  services/          # tools/LLM/MCP/workspace 等服务
  storage/           # PostgreSQL/SQLite 持久化
  core/              # 配置/鉴权/i18n/状态
wunder-cli/          # CLI 运行形态（TUI + 命令）
wunder-desktop/      # Desktop 运行形态（Tauri + 本地桥接）
wunder-desktop-electron/ # Electron 桌面壳（可选，AppImage 友好）
frontend/            # 用户侧前端（Vue3）
web/                 # 管理员侧前端（调试/治理）
config/              # 基础配置
prompts/             # system/tool/memory 提示词
skills/              # 内置技能
knowledge/           # 知识库
scripts/             # 开发与维护脚本
docs/                # 设计/API/方案文档
```

## 相关文档
- 系统介绍：`docs/系统介绍.md`
- 设计方案：`docs/设计方案.md`
- API 文档：`docs/API文档.md`
- wunder-cli 方案：`docs/方案/wunder-cli实现方案.md`
- wunder-desktop 方案：`docs/方案/wunder-desktop实现方案.md`
- 测试方案：`docs/方案/测试方案.md`

## wunder已吞噬核心
| 吞噬 | 项目名称 | GitHub 地址 |
| :--- | :--- | :--- |
| 智能体基础 | EVA | https://github.com/ylsdamxssjxxdd/eva |
| rust基础 | OpenAI Codex | https://github.com/openai/codex |
| 前端基础 | HuLa | https://github.com/HuLaSpark/HuLa |
| MCP/SKILLS | Claude Code | https://github.com/anthropics/claude-code |
| 网关/渠道/定时任务 | OpenClaw | https://github.com/openclaw/openclaw |
| 智能体LSP | OpenCode | https://github.com/anomalyco/opencode |