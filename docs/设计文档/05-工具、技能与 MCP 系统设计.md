# 工具、技能与 MCP 系统设计

## 1. 设计目标

wunder 的核心理念之一是“对开发者来说一切都是接口，对大模型来说一切皆工具”。因此工具系统不是附属模块，而是连接智能体与真实世界的主通道。技能与 MCP 是这个主通道上的两种能力组织方式。

## 2. 系统组成

| 组成 | 说明 | 主要目录 |
| --- | --- | --- |
| 执行调度 | 工具调用解析、并行调度、重试治理、结果归一化 | `src/orchestrator/tool_exec.rs` `src/orchestrator/tool_calls.rs` `src/orchestrator/tool_parallel.rs` |
| 工具实现 | 文件、shell、browser、desktop、memory、channel 等 | `src/services/tools/` |
| 用户工具与预设 | 用户侧启用、限制、预设和工具目录 | `src/services/user_tools.rs` `src/services/default_tool_profile.rs` |
| 技能 | 流程化能力、仓库化技能内容 | `src/services/skills.rs` `config/skills/` |
| MCP | 外部资源与能力接入 | `src/services/mcp.rs` `extra_mcp/` |

## 3. 设计边界

工具系统长期必须守住以下边界：

- 工具可见性由运行时构建，不由 prompt 手工列举兜底。
- 技能不是内核原语，而是 Wunder 对工具与流程的产品化组织。
- MCP 管理属于 Wunder 产品能力，但 MCP 访问语义可以下沉到内核治理。
- 任何高风险能力都必须受 approval、sandbox、timeout、retry 等结构治理。

## 4. 工具面模型

一个标准工具面应包含以下信息：

| 维度 | 含义 |
| --- | --- |
| 可见性 | 当前线程和当前模型是否能看到该工具 |
| 变更性 | 工具是否会修改外部世界 |
| 并行性 | 是否允许并行执行 |
| 审批模式 | 是否需要用户确认 |
| 沙盒策略 | 文件系统、命令、网络等限制 |
| 错误模型 | 超时、重试、可恢复失败和终止失败 |
| 观测信息 | 调用日志、输出截断、ledger 记录 |

## 5. 执行主链

工具系统的一次标准调用应遵循以下过程：

1. 根据线程语言、配置、权限、能力包构建 tool surface。
2. 向模型暴露 model-visible specs，而不是暴露整个后端。
3. 模型选择工具并发起调用。
4. 运行时检查审批、沙盒、超时、可并行性和输入规范。
5. 执行工具并记录 ledger、输出摘要和失败信息。
6. 必要时把结果回写到等待态、恢复链和实时投影。

## 6. 技能与 MCP 的定位

### 6.1 Skills

Skills 用于把稳定流程固化为可复用能力单元，适合：

- 固定的文档生产流程。
- 预定义的外部接入或资料加工步骤。
- 团队约定好的任务模板。

### 6.2 MCP

MCP 用于把外部资源、外部服务或外部知识连接进 wunder，使系统可以跨应用、跨仓库、跨资源域协作。MCP 既是扩展能力，也是统一治理边界的一部分。

## 7. 当前代码落点

| 目录 | 说明 |
| --- | --- |
| `src/services/tools/` | 当前真实工具实现主目录 |
| `src/services/tools/catalog.rs` | 工具目录与注册 |
| `src/services/tools/dispatch.rs` | 工具路由分发 |
| `src/services/skills.rs` | 技能管理与调用 |
| `src/services/mcp.rs` | MCP 管理与接入 |
| `src/services/browser/` | 浏览器桥接 |
| `src/services/bridge/` | 外部桥接服务 |

## 8. 设计重点

后续演进中应优先收敛以下问题：

- 不继续向 `src/services/tools.rs` 追加新功能，而是继续拆分到独立文件。
- 让工具治理统一走 `eva` 主链，减少旧桥接分叉。
- 技能与 MCP 的产品管理面放在 Wunder，但治理与调用语义由运行时统一约束。
- 让桌面、CLI、服务端共享同一套工具安全语义。

## 9. 验收标准

- 工具可见性、审批、沙盒、重试和日志有统一运行时落点。
- 新工具接入不需要修改多个无关层级。
- Skills 与 MCP 可以复用同一治理面，而不是各自绕开内核。
- 高风险工具不会仅靠 prompt 文案限制，而有程序级约束。

## 10. 相关文档

- `docs/API/04-user-tools-skills-and-knowledge.md`
- `docs/API/05-general-capabilities-and-workspace.md`
- `docs/方案/eva运行时设计.md`
