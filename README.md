# wunder
wunder是对eva改装构建的一个面向多用户的智能体调度平台，支持对接大模型 API、MCP 工具与 Skills 流程并提供基础的字面知识库检索功能。通过 FastAPI 暴露统一入口 `/wunder`，可流式返回中间过程与最终回复，并提供调试页面与基础运维接口。
<img width="1000" height="563" alt="wunder" src="https://github.com/user-attachments/assets/4e589030-f1fc-4e0c-91a7-3419eb39a046" />

## 核心理念
对开发者来说一切都是接口，对大模型来说一切皆工具
- 内置工具（动态）：手和脚
- MCP工具（动态）：刀和剑
- 技能工具（静态）：通关攻略
- 知识工具（静态）：百科全书
- 自建工具（动态）：私人装备
- 共享工具（动态）：装备市场

wunder可以暴露自身作为mcp工具，成为最终武器

## 1. 功能概览
- 内置了一套大模型驱动的自动化流程+灵活的提示词构建+自动上下文压缩
- 统一入口 `/wunder`：支持流式 SSE 与非流式响应。
- 工具链：内置工具 + MCP 工具 + Skills，可按 `tool_names` 精准启用。
- 多用户隔离：按 user_id 创建独立工作区，数据持久化。
- 调试与监控：/wunder/web 调试页面，/wunder/admin/monitor 资源与会话监控。

## 2. 快速开始
### 2.1 构建基础镜像
x86
```bash
docker buildx build --platform linux/x86_64 -t wunder:20251224-x86 -f Dockerfile .
```
arm
```bash
docker buildx build --platform linux/arm64 -t wunder:20251224-arm64 -f Dockerfile .
```
### 2.2 修改配置文件
先将示例配置复制为正式配置：`config/wunder-example.yaml` -> `config/wunder.yaml`
设置api_key，将ylsdamxssjxxdd改成你自己的

### 2.3 启动服务
```bash
docker compose up
```

### 2.4 打开系统设置页面
浏览器访问：
```
http://127.0.0.1:8000/wunder/web
```
点击系统设置页面，填入api地址和key，自动会接上后端

### 2.5 打开模型配置页面
新增模型并保存，可点击自动探测按钮快速获取最大上下文长度

### 2.6 打开调试面板页面
点击调试面板，进行提问测试


## 3. 请求示例
### 3.1 非流式请求
```
curl -X POST http://127.0.0.1:8000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"你好\",\"stream\":false}"
```

### 3.2 流式 SSE 请求
```
curl -N -X POST http://127.0.0.1:8000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"你好\",\"stream\":true}"
```

SSE 事件类型包括：
`progress`、`llm_request`、`llm_output`、`tool_call`、`tool_result`、`final`、`error`

### 3.3 按需启用工具
```
curl -X POST http://127.0.0.1:8000/wunder ^
  -H "Content-Type: application/json" ^
  -H "X-API-Key: <your-api-key>" ^
  -d "{\"user_id\":\"u001\",\"question\":\"列出当前目录\",\"tool_names\":[\"列出文件\"],\"stream\":false}"
```

工具清单请先调用：
```
curl -X GET http://127.0.0.1:8000/wunder/tools ^
  -H "X-API-Key: <your-api-key>"
```

## 4. API 入口一览
详细说明见 `docs/API文档.md`

核心入口：
- `POST /wunder`
- `POST /wunder/system_prompt`
- `GET /wunder/tools`

管理与运维：
- `GET/POST /wunder/admin/llm`
- `GET/POST /wunder/admin/mcp`
- `POST /wunder/admin/mcp/tools`
- `GET/POST /wunder/admin/skills`
- `POST /wunder/admin/skills/upload`
- `GET/POST /wunder/admin/tools`
- `GET /wunder/admin/monitor`
- `GET /wunder/admin/monitor/{session_id}`
- `POST /wunder/admin/monitor/{session_id}/cancel`

临时工作区管理：
- `GET /wunder/workspace`
- `POST /wunder/workspace/upload`
- `GET /wunder/workspace/download`
- `DELETE /wunder/workspace`

## 5. 工作区与历史记录
- 工作区目录：`data/workspaces/{user_id}/files`
- 历史记录：`data/historys/{user_id}/chat_history.jsonl`
- 工具日志：`data/historys/{user_id}/tool_log.jsonl`

同一 `user_id` 并发请求会被拒绝（HTTP 429）。

## 6. Skills 与 MCP
### 6.1 Skills
- Skills 默认从`EVA_SKILLS/` 目录读取。
- 每个技能目录需包含 `SKILL.md`，并在 YAML frontmatter 中声明 `name`、`description` 与 `input_schema`。
- 通过 `config/wunder.yaml` 或 `/wunder/admin/skills` 启用。

### 6.2 MCP
- 在 `config/wunder.yaml` 中配置 `mcp.servers`。
- 可通过 `/wunder/admin/mcp` 管理配置。
- `/wunder/admin/mcp/tools` 可探测并缓存工具清单。

## 7. 配置说明
请先将示例配置复制为正式配置：`config/wunder-example.yaml` -> `config/wunder.yaml`
基础配置文件：`config/wunder.yaml`
持久化覆盖：`data/config/wunder.override.yaml`（管理端修改会写入此文件）
其余如 LLM/MCP/工具等建议在管理端配置，保存后会写入覆盖文件。
- `server`：服务端口与流式分块大小
- `llm`：模型服务配置
- `mcp`：MCP 服务配置
- `skills`：技能目录与启用列表
- `tools`：内置工具启用列表
- `workspace`：工作区与历史策略
- `security`：命令白名单与路径黑名单
- `observability`：日志级别与日志路径
- `cors`：跨域配置

## 8. 测试与压测
完整测试方案见 `docs/测试方案.md`

## 9. 项目结构
```
app/                 # FastAPI 入口与核心逻辑
  api/               # 路由与接口
  orchestrator/      # 编排引擎与提示词构建
  tools/             # 内置工具与 MCP 适配
  skills/            # Skills 加载与注册
  memory/            # 工作区与历史管理
  monitor/           # 监控与会话状态
config/              # 配置文件
data/                # 工作区、历史、日志
docs/                # 设计/API/测试文档
web/                 # 调试页面静态资源
tests/               # 功能测试与压测脚本
```

## 10. 相关文档
- 设计方案：`docs/设计方案.md`
- API 文档：`docs/API文档.md`
- 测试方案：`docs/测试方案.md`
