# wunder
wunder 是一个多用户智能体调度平台，可灵活对接大模型 API、MCP 工具与 Skills 流程，内置知识库。Rust (Axum) 服务对外暴露统一的 `/wunder` 入口，支持 SSE 流式与非流式响应，并提供调试控制台与管理接口。
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

## 1. 功能概览
- 统一入口 `/wunder`：支持 SSE 流式与非流式响应。
- A2A 标准接口 `/a2a` + `/.well-known/agent-card.json` 用于能力发现。
- 工具体系：内置 + MCP + Skills + 知识库 + 自建/共享工具，可通过 `tool_names` 启用。
- 自动上下文压缩 + 可选长期记忆，支持长会话。
- 多用户隔离：`user_id` 既是会话键也是工作区键，可为虚拟用户。
- 配额治理：注册用户按模型调用消耗配额，虚拟 `user_id` 不受限。
- 调试与监控：`/` 调试面板 + `/wunder/admin/monitor` 会话监控。
- 吞吐压测、性能采样与能力评估内置。
- UI 与系统提示支持多语言切换。

## 2. 快速开始
### 2.1 更新配置
拷贝示例配置：`config/wunder-example.yaml` -> `config/wunder.yaml`
拷贝环境示例：`.env.example` -> `.env`，配置 `WUNDER_API_KEY`、`WUNDER_POSTGRES_DSN`、`WUNDER_SANDBOX_ENDPOINT` 等。
前端 API base：在仓库根 `.env` 中设置 `VITE_API_BASE` 或 `VITE_API_BASE_URL`，并重启前端。

### 2.2 启动服务
x86
```bash
docker compose -f docker-compose.rust.x86.yml up
```
arm
```bash
docker compose -f docker-compose.rust.arm.yml up
```
首次启动会拉取基础镜像并编译依赖，可能需要较长时间。

#### 2.2.1 启动时的增量编译判断
docker compose 的 command 会先检查 `CARGO_TARGET_DIR/release/wunder-server` 是否存在，并用 `find src Cargo.toml Cargo.lock -newer <binary>` 判断源码是否比二进制新；如果没有更新就直接 exec 二进制，否则触发 `cargo build --release` 重新编译。

### 2.3 打开管理端调试界面
浏览器打开：
```
http://127.0.0.1:18000
```

### 2.4 打开用户侧前端
浏览器打开：
```
http://127.0.0.1:18001
```

## 3. 请求示例
### 3.1 非流式请求
```
curl -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"你好\",\"stream\":false}"
```

### 3.2 流式 SSE 请求
```
curl -N -X POST http://127.0.0.1:18000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -H "Accept: text/event-stream" ^
  -d "{\"user_id\":\"u001\",\"question\":\"你好\",\"stream\":true,\"debug_payload\":true}"
```
`debug_payload` 仅对 `/wunder` 生效（`/wunder/chat` 不返回完整请求体）。

常见 SSE 事件类型包括：
`progress`、`llm_request`、`llm_output_delta`、`llm_output`、`tool_call`、`tool_output_delta`、`tool_result`、`token_usage`、`context_usage`、`plan_update`、`question_panel`、`a2ui`、`final`、`error`

### 3.3 按需启用工具
```
curl -X GET "http://127.0.0.1:18000/wunder/tools?user_id=u001" ^
  -H "X-API-Key: <your-api-key>"
```

## 4. API 入口概览
核心接口：
- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`
- `GET /wunder/i18n`

用户侧接口：
- `/wunder/auth`
- `/wunder/chat/*`
- `/wunder/workspace/*`
- `/wunder/user_tools/*`

管理与运维：
- `/wunder/admin/*`
- `/wunder/admin/throughput/*`
- `/wunder/admin/evaluation/*`
- `/wunder/admin/performance/sample`
- `/wunder/admin/memory/*`

其他入口：
- `/a2a` + `/.well-known/agent-card.json`
- `/wunder/mcp`
- `/wunder/doc2md/convert`
- `/wunder/attachments/convert`
- `/wunder/temp_dir/*`

详见 `docs/API文档.md`。

## 5. 工作区与持久化
- 工作区路径：`workspaces/<user_id>`（提示词使用 `/workspaces/<user_id>/`）。
- 对话历史/工具日志/监控/锁与溢出事件写入数据库（默认 PostgreSQL，可选 SQLite）。
- 旧版 `data/historys/` 仅保留迁移用途。
- 管理端覆写配置写入 `data/config/wunder.override.yaml`。
- 吞吐报告输出到 `data/throughput`。

同一 `user_id` 并发请求会被拒绝（HTTP 429）。

## 6. Skills 与 MCP
### 6.1 Skills
- Skills 默认从 `skills/` 与 `EVA_SKILLS/` 加载。
- `SKILL.md` 需包含 YAML frontmatter，字段 `name/description/input_schema`（也支持中文字段名）。
- 入口脚本为 `run.py` / `skill.py` / `main.py`，使用 `run(payload)` 调用。
- 通过 `config/wunder.yaml` 或 `/wunder/admin/skills` 启用。

### 6.2 MCP
- 在 `config/wunder.yaml` 与 `data/config/wunder.override.yaml` 中配置 `mcp.servers`。
- 通过 `/wunder/admin/mcp` 与 `/wunder/admin/mcp/tools` 管理。

## 7. 项目结构
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

## 8. 相关文档
- 系统介绍：`docs/系统介绍.md`
- 设计方案：`docs/设计方案.md`
- API 文档：`docs/API文档.md`
- 请求示例：`docs/请求示例.md`
- 测试方案：`docs/测试方案.md`
