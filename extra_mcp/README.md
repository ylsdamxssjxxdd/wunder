# MCP Server (FastMCP)

`extra_mcp` 是 wunder 的独立 MCP 服务（Python/FastMCP），用于提供数据库与知识库工具。  
和主服务内置 MCP 的关系如下：

- 内置 `wunder` MCP（Rust，`src/services/mcp.rs`）：端点 `/wunder/mcp`，工具固定为 `excute`、`doc2md`
- 独立 `extra_mcp`（本目录）：默认端点 `/mcp`，工具按 `mcp_config.json` 动态生成（`db_query*`、`kb_query*`）

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
