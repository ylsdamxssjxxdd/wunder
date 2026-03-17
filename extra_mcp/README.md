# MCP Server (FastMCP)

`extra_mcp` 是 wunder 的独立 MCP 服务（Python/FastMCP），用于提供数据库与知识库工具。  
和主服务内置 MCP 的关系如下：

- 内置 `wunder` MCP（Rust，`src/services/mcp.rs`）：端点 `/wunder/mcp`，工具固定为 `excute`、`doc2md`
- 独立 `extra_mcp`（本目录）：默认端点 `/mcp`，工具按 `mcp_config.json` 动态生成（`db_query*`、`db_export*`、`kb_query*`）

## 1. 启动 `extra_mcp`

### 本地运行（推荐）

```bash
set MCP_TRANSPORT=streamable-http
set MCP_HOST=0.0.0.0
set MCP_PORT=9010
python -m extra_mcp.main
```

### Docker Compose

```bash
docker compose -f docker-compose-x86.yml up -d extra-mcp
```

说明：
- 默认读取 `extra_mcp/mcp_config.json`
- 可用 `MCP_CONFIG_PATH` 指定配置路径
- 运行参数优先级：环境变量 > `mcp_config.json` 的 `mcp.transport/host/port` > 默认值
- 当 `extra_mcp` 运行在 Docker 容器内且数据库主机配置为 `127.0.0.1` / `localhost` 时，会先直连本机回环地址；若失败，再自动回退到 `host.docker.internal`，以兼容“宿主机数据库 + 容器内 MCP”场景
- 如需覆盖回退主机名，可设置环境变量 `EXTRA_MCP_LOOPBACK_FALLBACK_HOST`
- 绑定表场景会同时注册 `db_query*` 与 `db_export*`：前者返回小样本与 `query_handle`，后者可直接把同一查询导出为 `xlsx/csv`
- 若 `db_export*` 的 `path` 使用 `/workspaces/{user_id}/exports/...`（提示词里会自动替换成当前工作区根路径），导出文件会直接落到智能体当前工作区，并在结果中返回 `workspace_relative_path` 与 `public_path`
- 可用 `database.export_root` 或环境变量 `EXTRA_MCP_EXPORT_ROOT` 配置导出根目录（默认 `exports/extra_mcp`）；`db_export*` 的 `path` 参数必须是相对该根目录的相对路径
- 若希望 `db_export*` 直接写入 Wunder 工作区，`extra-mcp` 进程必须能看到与 `wunder-server` 相同的工作区根目录；Docker Compose 已通过共享 `wunder_workspaces:/workspaces` 卷打通该路径

## 2. `mcp_config.json` 最小示例

```json
{
  "mcp": {
    "transport": "streamable-http",
    "host": "0.0.0.0",
    "port": 9010
  },
  "database": {
    "db_type": "mysql",
    "host": "127.0.0.1",
    "port": 3306,
    "user": "root",
    "password": "",
    "database": "personnel",
    "export_root": "exports/extra_mcp",
    "tables": {
      "employees": {
        "table": "employees",
        "description": "员工主数据"
      }
    }
  },
  "knowledge": {
    "base_url": "http://127.0.0.1:9380",
    "api_key": "REPLACE_WITH_RAGFLOW_API_KEY",
    "targets": {
      "default": {
        "dataset_ids": ["REPLACE_WITH_DATASET_ID"]
      }
    }
  }
}
```

## 3. 在 Wunder 中接入（重点）

`config/wunder-example.yaml` 的 `mcp.servers` 建议同时保留两个服务：

```yaml
mcp:
  timeout_s: 1200
  servers:
    - name: wunder
      endpoint: http://127.0.0.1:${WUNDER_PORT:-8000}/wunder/mcp
      allow_tools:
        - excute
        - doc2md
      enabled: false
      transport: streamable-http
      headers:
        Authorization: Bearer ${WUNDER_API_KEY}
      tool_specs:
        - name: excute
          description: 执行 wunder 智能体任务并返回最终回复。
          input_schema:
            type: object
            properties:
              task:
                type: string
            required:
              - task
        - name: doc2md
          description: 解析文档并返回 Markdown 文本。
          input_schema:
            type: object
            properties:
              source_url:
                type: string
            required:
              - source_url

    - name: extra_mcp
      endpoint: http://${WUNDER_MCP_HOST:-127.0.0.1}:${WUNDER_MCP_PORT:-9010}/mcp
      allow_tools: []
      enabled: false
      transport: streamable-http
      headers:
        Authorization: Bearer ${WUNDER_API_KEY}
      tool_specs: []
```

首次接入 `extra_mcp` 后，请在管理端 MCP 页面执行一次“连接/刷新工具”，把拉取到的 `tool_specs` 保存到配置中，避免模型侧无可用工具描述。

### 导出型任务推荐流程

1. 先用 `db_query*` 做 `COUNT(*)`、聚合或 `LIMIT 3~5` 小样本校验。
2. 复用返回的 `query_handle` 调 `db_export*`，并把 `path` 设为 `/workspaces/{user_id}/exports/...`，直接生成 `xlsx/csv` 到当前工作区。
3. 如需后续处理，优先使用返回的 `workspace_relative_path` 交给读文件/写文件/文档工具继续处理，而不是重新分页查询数据库。
4. 回复里只保留导出文件路径、行数、字段与筛选口径，不要把分页明细继续贴进上下文。

## 4. `mcpServers` 导入示例（管理端/用户侧）

```json
{
  "mcpServers": {
    "extra_mcp": {
      "type": "streamable-http",
      "baseUrl": "http://127.0.0.1:9010/mcp",
      "headers": {
        "Authorization": "Bearer <WUNDER_API_KEY>"
      },
      "isActive": true
    }
  }
}
```

## 5. 常见检查项

1. `type/transport` 与服务实际传输协议一致（`http` 会被归一化为 `streamable-http`）
2. `extra_mcp` 端点路径是 `/mcp`；内置 `wunder` 端点路径是 `/wunder/mcp`
3. Docker Compose 内 `wunder-server` 访问独立 MCP 请用 `extra-mcp:9010`，不要写 `127.0.0.1:9010`
4. 401/403 时补齐 `headers` 或 `auth`
5. 工具不可见时先检查 `enabled`、`allow_tools`、`tool_specs`
6. 如果 `extra_mcp` 容器要访问宿主机数据库，可继续使用 `127.0.0.1` / `localhost` 配置；连接失败时会自动回退到 `host.docker.internal`
