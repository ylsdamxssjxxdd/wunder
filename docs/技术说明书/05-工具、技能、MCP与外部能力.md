# 工具、技能、MCP 与外部能力

## 1. 定位

工具体系是模型连接真实世界的唯一通道。模型只能看到当前线程允许的 **tool surface**，不能直接访问全部后端能力。

## 2. 三层架构

```
内核治理层 (crates/eva/src/tools/)
  spec / registry / router / batch planner / preflight / permissions / approvals / sandbox / retry / ledger

宿主适配层 (crates/eva/src/wunder/tooling/)
  catalog / dynamic handler / surface builder / legacy bridge / observed bridge

业务实现层 (src/services/tools/)
  文件操作 / 命令执行 / 浏览器 / 渠道消息 / 工作区 / ...
```

内核层定义治理规则和接口，宿主层把 Wunder 的工具生态装配给内核。业务层做真正的执行。

## 3. Tool Surface

Tool surface 决定了当前线程、当前模型能看到哪些工具。它由以下因素共同决定：

- 用户配置的工具授权
- Agent 预设中绑定的工具
- 模型能力（是否支持 vision、parallel tool calls 等）
- 动态挂载（运行时由其他工具挂载的额外工具）
- 搜索/推荐（工具数超过直接暴露阈值时的检索机制）

关键指标（`ToolSurfaceRuntimeSummary`）：

| 字段 | 含义 |
| --- | --- |
| `model_visible_tool_names` | 模型可见工具名列表 |
| `searchable_tool_count` | 可搜索工具数 |
| `discoverable_tool_count` | 可发现工具数 |
| `mounted_dynamic_tool_names` | 动态挂载的工具 |
| `overflowed` | 是否超过直接暴露阈值 |
| `tool_search_enabled` | 工具搜索是否启用 |

## 4. 工具治理链路

一个工具调用从发起到完成的完整链路：

```
模型输出工具调用
  → Router: 根据 tool name 查找 spec，确定执行路由
  → Batch Planner: 将并行的调用打包成批次
  → Preflight: 前置检查（参数校验、依赖满足）
  → Permissions: 文件系统权限检查
  → Approvals: 审批策略判定（自动/需审批）
  → Sandbox: 沙盒隔离策略
  → Retry Governor: 重试策略与退避
  → Executor: 真正执行
  → Ledger: 记录调用日志
```

新工具如果没有这些治理属性，就不应直接暴露给模型。

## 5. 技能

技能是**模型可读的执行手册 + 流程模板**，用于：

- 固化标准操作流程
- 把一组工具的协作方式稳定下来
- 把组织经验做成可复用的能力单元

技能本质上通过工具和提示材料驱动，**不是**一套平行运行时。

## 6. MCP

MCP 负责把外部资源、外部工具或外部服务纳入统一能力治理：

| 层面 | 职责 |
| --- | --- |
| MCP 管理 | 服务配置、启停、认证、目录展示 |
| MCP 使用 | 列资源、读资源、调工具 |

MCP 工具访问语义下沉到内核治理层，MCP 服务器管理仍属于 Wunder 产品层。

## 7. 外部能力

当前系统中的外部能力：

| 能力 | 说明 |
| --- | --- |
| 浏览器控制 | Puppeteer-based 浏览器操控 |
| 桌面控制 | Tauri / Electron 桥接的桌面操作 |
| 文件与补丁 | 读写、搜索、diff、patch |
| 命令执行 | Shell 命令 |
| 渠道消息 | 通过 channels 收发外部消息 |
| A2A | 智能体互联 |
| MCP 资源与工具 | 外部 MCP 服务 |

## 8. 新增工具步骤

1. 在 `src/services/tools/` 新建独立文件实现逻辑。
2. 补充输入校验、错误模型和英文注释。
3. 接入工具目录和 tool surface 构建链。
4. 明确审批策略、沙盒策略、并行策略和超时。
5. 补齐回归或最小验证。
6. 更新 `docs/API` 与 `docs/功能迭代.md`。

## 9. 新增技能步骤

1. 明确技能目标、输入、输出和流程边界。
2. 将技能内容写入对应 skill 目录。
3. 只复用已有工具，不复制业务逻辑。
4. 验证不同语言和上下文下的可读性。

## 10. 新增 MCP 步骤

1. 明确 endpoint、transport、认证方式和工具/资源范围。
2. 在配置层接入并控制 enable 状态。
3. 验证列资源、读资源和调工具的最小闭环。
4. 确保错误、超时和认证失败可被定位。

## 11. 关联文档

- `docs/设计文档/05-工具、技能与 MCP 系统设计.md`
- `docs/API/04-user-tools-skills-and-knowledge.md`
- `docs/API/05-general-capabilities-and-workspace.md`
