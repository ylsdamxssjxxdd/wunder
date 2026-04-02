# 工具、技能、MCP 与外部能力

## 1. 定位

工具体系是模型连接真实世界的唯一通道。模型只能看到当前线程允许的 **工具面**，不能直接访问全部后端能力。

## 2. 三层架构

```
执行调度层 (src/orchestrator/tool_exec.rs, tool_calls.rs, tool_parallel.rs)
  工具调用解析、并行调度、重试治理、结果归一化

工具实现层 (src/services/tools/)
  文件操作 / 命令执行 / 浏览器 / 渠道消息 / 工作区 / ...

能力组织层 (src/services/skills.rs, src/services/mcp.rs, src/services/user_tools.rs)
  技能 / MCP / 用户工具的产品化管理
```

执行调度层在 orchestrator 内完成工具调用的完整治理链路；实现层做真正的执行；能力组织层负责产品化接入。

## 3. Tool Surface

Tool surface 决定了当前线程、当前模型能看到哪些工具。它由以下因素共同决定：

- 用户配置的工具授权
- Agent 预设中绑定的工具
- 模型能力（是否支持 vision、parallel tool calls 等）
- 内置工具与用户工具的合并

## 4. 工具治理链路

一个工具调用从发起到完成的完整链路：

```
模型输出工具调用
  → tool_calls.rs: 解析调用，确定目标工具
  → tool_exec.rs: 调度执行
  → tool_parallel.rs: 并行策略
  → 权限检查 + 审批判定
  → 真正执行（src/services/tools/ 中对应实现）
  → retry_governor.rs: 失败重试
  → result_normalizer.rs: 结果归一化
  → 写入 tool_logs 和 stream events
```

新工具如果没有审批策略、沙盒策略和错误模型，就不应直接暴露给模型。

## 5. 当前工具实现

| 文件 | 工具 |
| --- | --- |
| `catalog.rs` | 工具目录与注册 |
| `dispatch.rs` | 工具路由分发 |
| `apply_patch_tool.rs` | 文件补丁应用 |
| `browser_tool.rs` | 浏览器控制 |
| `channel_tool.rs` | 渠道消息工具 |
| `command_*.rs` | Shell 命令执行 |
| `desktop_control.rs` | 桌面控制 |
| `memory_manager_tool.rs` | 记忆管理工具 |
| `read_file_guard.rs` | 文件读取守卫 |
| `read_image_tool.rs` | 图片读取 |
| `search_content_tool.rs` | 内容搜索 |
| `self_status_tool.rs` | 自身状态工具 |
| `sessions_yield_tool.rs` | 会话让出工具 |
| `skill_call.rs` | 技能调用 |
| `sleep_tool.rs` | 延迟工具 |
| `subagent_control.rs` | 子智能体控制 |
| `thread_control_tool.rs` | 线程控制 |
| `web_fetch_tool.rs` | 网页获取 |

## 6. 技能

技能是**模型可读的执行手册 + 流程模板**，用于：

- 固化标准操作流程
- 把一组工具的协作方式稳定下来
- 把组织经验做成可复用的能力单元

技能本质上通过工具和提示材料驱动，**不是**一套平行运行时。

技能内容存放在 `config/skills/` 目录。

## 7. MCP

MCP 负责把外部资源、外部工具或外部服务纳入统一能力治理：

| 层面 | 职责 |
| --- | --- |
| MCP 管理 | 服务配置、启停、认证、目录展示 |
| MCP 使用 | 列资源、读资源、调工具 |

配置位于 `config/mcp_config.json`。

## 8. 外部能力

当前系统中的外部能力：

| 能力 | 说明 |
| --- | --- |
| 浏览器控制 | Puppeteer-based 浏览器操控（`src/services/browser/`） |
| 桌面控制 | Tauri / Electron 桥接的桌面操作 |
| 文件与补丁 | 读写、搜索、diff、patch |
| 命令执行 | Shell 命令（含 guard 和 output 检查） |
| 渠道消息 | 通过 channels 收发外部消息 |
| MCP 资源与工具 | 外部 MCP 服务 |
| 技能调用 | 流程化能力调用 |

## 9. 新增工具步骤

1. 在 `src/services/tools/` 新建独立文件实现逻辑。
2. 在 `src/services/tools/catalog.rs` 注册到工具目录。
3. 补充输入校验、错误模型和英文注释。
4. 明确审批策略、沙盒策略、并行策略和超时。
5. 补齐回归或最小验证。
6. 更新 `docs/API` 与 `docs/功能迭代.md`。

## 10. 新增技能步骤

1. 明确技能目标、输入、输出和流程边界。
2. 将技能内容写入 `config/skills/` 对应目录。
3. 只复用已有工具，不复制业务逻辑。
4. 验证不同语言和上下文下的可读性。

## 11. 新增 MCP 步骤

1. 明确 endpoint、transport、认证方式和工具/资源范围。
2. 在 `config/mcp_config.json` 配置并控制 enable 状态。
3. 验证列资源、读资源和调工具的最小闭环。
4. 确保错误、超时和认证失败可被定位。

## 12. 关联文档

- `docs/设计文档/05-工具、技能与 MCP 系统设计.md`
- `docs/API文档.md`
