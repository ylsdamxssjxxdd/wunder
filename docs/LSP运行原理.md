# LSP 运行原理与使用说明

## 1. 目标与定位

- LSP 用于给模型提供更可靠的代码语义信息，例如定义跳转、引用查找、悬浮提示、符号列表与诊断信息。
- Wunder 以“工具化”方式封装 LSP，让它既能被管理员调试，也能被模型通过 `LSP查询` 工具调用。

## 2. 启用与配置

### 2.1 配置文件

在 `config/wunder.yaml` 或运行时写入的覆盖文件 `data/config/wunder.override.yaml` 中配置：

- `lsp.enabled`：总开关。
- `lsp.timeout_s`：请求超时（秒）。
- `lsp.diagnostics_debounce_ms`：诊断去抖（毫秒）。
- `lsp.idle_ttl_s`：空闲回收时间（秒，0 表示不回收）。
- `lsp.servers`：语言服务器列表。

单个服务器配置字段：

- `id`：服务唯一标识。
- `name`：展示名称（可选）。
- `command`：启动命令，数组形式（如 `["rust-analyzer"]`）。
- `env`：环境变量。
- `extensions`：支持的文件后缀（为空表示匹配所有文件）。
- `root_markers`：用于识别项目根目录的标记文件（如 `Cargo.toml`）。
- `initialization_options`：初始化参数（YAML/JSON 对象）。
- `enabled`：是否启用该服务。

### 2.2 管理员侧配置

管理员侧 LSP 页面支持读取/保存配置并查看状态。保存操作会调用 `POST /wunder/admin/lsp`，并写入覆盖配置文件（`data/config/wunder.override.yaml`）。

注意：页面里的“服务器列表”需要填写 JSON 数组（不是 YAML），示例：

```json
[
  {
    "id": "rust-analyzer",
    "name": "Rust Analyzer",
    "command": ["rust-analyzer"],
    "env": {},
    "extensions": ["rs"],
    "root_markers": ["Cargo.toml"],
    "initialization_options": {},
    "enabled": true
  }
]
```

### 2.3 LSP 工具启用

模型若要主动调用 LSP，需要在 `tools.builtin.enabled` 中启用 `LSP查询`（或别名 `lsp`）。  
即使不启用该工具，只要 `lsp.enabled=true`，文件写入/替换/编辑仍会触发诊断回传。

## 3. 运行时架构

### 3.1 客户端管理

- Wunder 使用 `LspManager` 统一管理 LSP 进程。
- 进程 key = `user_id + project_root + server_id`，确保不同用户或不同项目根目录相互隔离。
- 每次请求会根据文件后缀与 `root_markers` 选择匹配的服务器，并按需拉起 LSP 进程。

### 3.2 进程启动与初始化

LSP 客户端以 JSON-RPC over stdio 的方式运行：

1. 启动命令（`command`）拉起语言服务器进程。
2. 发送 `initialize` 请求。
3. 发送 `initialized` 通知。
4. 若配置了 `initialization_options`，再发送 `workspace/didChangeConfiguration`。

### 3.3 文件同步

- 首次打开文件：`textDocument/didOpen`。
- 后续写入：`textDocument/didChange`，并维护文件版本号。
- 同时发送 `workspace/didChangeWatchedFiles`，增强服务器的文件状态感知。

## 4. 诊断与缓存机制

- 服务器的 `textDocument/publishDiagnostics` 会被缓存到内存并按路径归档。
- 写入/替换/编辑文件后会等待一小段去抖时间，合并稳定后的诊断结果。
- 工具输出会追加 `lsp` 字段：`enabled` / `matched` / `touched` / `diagnostics` / `error`。
- `diagnostics` 为空表示未发现诊断；`error` 会提示未匹配服务、路径越界或启动失败等原因。
- 读文件时只触发“触摸”但不等待诊断，避免阻塞。

## 5. LSP 查询能力（工具侧）

工具名：`LSP查询`（别名 `lsp`）。常用字段：

- `operation`：`definition` / `references` / `hover` / `documentSymbol` / `workspaceSymbol` / `implementation` / `callHierarchy`
- `path`：工作区内相对路径。
- `line` / `character`：仅对定位类操作必填，**1-based**。
- `query`：`workspaceSymbol` 必填。
- `call_hierarchy_direction`：`incoming` / `outgoing`（默认 `incoming`）。

返回结果按服务器聚合，包含 `server_id`/`server_name` 与 `result`。

## 6. 管理员调试流程

1. 打开管理员侧 LSP 页面，点击“LSP 配置”启用并配置服务器。
2. 保存后通过状态灯或“连接状态”弹窗确认服务连接情况。
3. 在顶部输入 `user_id` 并回车/失焦同步，浏览文件树，双击文件在右侧编辑并保存。
4. 保存后会自动触发 LSP，在右侧结果区查看诊断摘要与返回数据。
5. 若返回错误，通常是命令不可用、路径不在工作区或 LSP 未正确启用。

## 7. 注意事项与最佳实践

- 语言服务器需在运行环境中可执行（例如 `rust-analyzer`、`pyright-langserver`）。
- 使用 `root_markers` 精确定位项目根目录，避免 LSP 误扫过大目录。
- LSP 进程会在空闲超时后自动回收，长时间不用可降低 `idle_ttl_s`。
- 访问路径必须在用户工作区内，越界会被拒绝，确保安全隔离。
