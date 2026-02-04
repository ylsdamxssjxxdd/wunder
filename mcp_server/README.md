# MCP Server (FastMCP)

本目录用于运行独立的 MCP 服务（当前内置数据库与知识库工具，后续可扩展更多工具），支持 MySQL/PostgreSQL，支持多目标数据库配置。

## 1. 运行方式

### 本地运行

```bash
# 推荐以 streamable-http 方式对外提供服务
set MCP_TRANSPORT=streamable-http
set MCP_HOST=0.0.0.0
set MCP_PORT=9010
python -m mcp_server.main
```

### Docker Compose

`docker-compose-x86.yml`/`docker-compose-arm.yml` 已内置 `wunder_mcp` 服务，默认端口 `9010`。

```bash
# 在项目根目录
set MCP_PORT=9010
set MCP_HOST=0.0.0.0
# 启动
docker compose -f docker-compose-x86.yml up -d wunder_mcp
```

## 2. MCP 配置文件

默认读取 `mcp_server/mcp_config.json`，可用 `MCP_CONFIG_PATH` 指定路径。数据库/知识库配置仅使用该配置文件；运行参数通过 `MCP_TRANSPORT/MCP_HOST/MCP_PORT` 环境变量配置。

```json
{
  "database": {
    "db_type": "mysql",
    "host": "127.0.0.1",
    "port": 3306,
    "user": "root",
    "password": "",
    "database": "personnel",
    "description": "人员与组织信息库",
    "connect_timeout": 5
  },
  "knowledge": {
    "base_url": "http://127.0.0.1:9380",
    "api_key": "REPLACE_WITH_RAGFLOW_API_KEY",
    "default_key": "default",
    "targets": {
      "default": {
        "dataset_ids": [
          "REPLACE_WITH_DATASET_ID"
        ],
        "description": "默认知识库"
      }
    },
    "request": {
      "page_size": 20,
      "similarity_threshold": 0.2,
      "vector_similarity_weight": 0.3,
      "top_k": 1024,
      "keyword": false,
      "highlight": false,
      "use_kg": false,
      "toc_enhance": false
    }
  }
}
```

如需多库配置，可在 `database.targets` 中配置多个目标，并可为每个目标增加 `description` 说明用途。
也可以使用 `database.target_descriptions`（或 `database.descriptions`）仅维护 key->用途说明的映射，用于补全工具描述。
建议在 `database.description` 中补充核心表及用途（如 `employees=人员主数据`、`departments=部门`），帮助模型更准确选表；保持简洁，避免把所有表都塞进去。

知识库检索基于 RAGFlow 的 `/api/v1/retrieval` 接口，需要配置 `knowledge.base_url`、`knowledge.api_key` 与 `dataset_ids`。
多知识库可在 `knowledge.targets` 中配置多个目标，并可为每个目标增加 `description` 说明用途。
也可以使用 `knowledge.target_descriptions`（或 `knowledge.descriptions`）仅维护 key->用途说明的映射，用于补全工具描述。

## 3. MCP 连接示例

```json
{
  "mcpServers": {
    "wunder_mcp": {
      "type": "streamable-http",
      "description": "Wunder MCP 服务（当前内置数据库工具）。",
      "isActive": false,
      "name": "wunder_mcp",
      "baseUrl": "http://127.0.0.1:9010/mcp",
      "headers": {}
    }
  }
}
```

> 如通过网关或反向代理加鉴权，请在 `headers` 中补充对应的认证头。当前提供工具：`db_get_schema`、`db_query`、`kb_query`。

## 4. 服务器部署时的 IP / 端口配置要点

- **MCP_HOST 只决定监听地址**：生产部署一般设置 `0.0.0.0`。
- **对外访问地址 = 服务器 IP / 域名 + MCP_PORT**：
  - 例如 `http://<server-ip>:9010/mcp`。
- **防火墙/安全组**：需要放行 `MCP_PORT` 对外访问。
- **Docker 端口映射**：当前 compose 使用同端口映射（`host:container` 相同）。如需只改外部端口，需要修改 compose 的 `ports`。

## 5. 宿主机 / 容器网络常见配置

- **MySQL/Postgres 在同一 compose 网络**：
  - 在 `mcp_config.json` 的 `database.host` 或 `database.targets.*.host` 中填写容器服务名（如 `postgres`/`mysql`）。
- **数据库在宿主机**：
  - Windows/Mac：`host.docker.internal` 可直接用（配置到 `database.host`）。
  - Linux：建议直接用宿主机 IP（如 `172.17.0.1`）。
- **数据库在远程服务器**：直接填外网/内网 IP + 端口，并确保安全组放行。

## 6. 安全建议

- MCP 服务默认无鉴权，建议仅内网使用或通过网关反向代理加鉴权。
- `db_query` 仅允许只读 SQL 查询。

## 7. 目录结构

- `main.py`：FastMCP 服务入口。
- `runtime.py`：运行时配置读取（MCP_HOST/MCP_PORT/MCP_TRANSPORT）。
- `tools/`：每类 MCP 工具一个子目录。
  - `tools/database/`：数据库工具实现（`db_get_schema`/`db_query`）。
- 新增工具后在 `tools/__init__.py` 注册即可生效。
- `common/`：通用工具方法。
