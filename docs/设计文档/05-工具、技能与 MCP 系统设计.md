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

## 7. 当前工具目录与代码落点

当前 model-visible 内置工具以 `src/services/tools/catalog.rs` 的 `builtin_tool_specs_with_language()` 为准，当前共 **31** 个。下面的“默认启用”指默认工具画像与示例配置里的初始集合，当前与 `src/services/default_tool_profile.rs`、`config/wunder-example.yaml` 保持一致。

需要注意：

- desktop 本地模式下，其它内置工具仍可能按运行能力直接可见；“默认启用”主要表示默认智能体和新建智能体的初始勾选集合。
- `读图工具`、`桌面控制器`、`桌面监视器` 还受模型视觉能力过滤。
- `浏览器` 受 `tools.browser.enabled` 控制，并兼容旧的细分入口别名。

### 7.1 默认启用工具画像

| 工具名 | 常用别名 | 主要作用 |
| --- | --- | --- |
| `最终回复` | `final_response` | 输出最终答复并结束当前工具链。 |
| `定时任务` | `schedule_task` | 创建、更新、查询和执行定时任务。 |
| `休眠等待` | `sleep` `sleep_wait` `pause` | 主动等待指定时间后继续执行。 |
| `记忆管理` | `memory_manager` `memory_manage` | 管理长期记忆条目与记忆状态。 |
| `执行命令` | `execute_command` | 执行 shell/终端命令。 |
| `ptc` | `programmatic_tool_call` | 执行程序化工具调用脚本。 |
| `列出文件` | `list_files` | 浏览工作区目录和文件树。 |
| `搜索内容` | `search_content` | 在文件中按关键字或正则搜索内容。 |
| `读取文件` | `read_file` | 读取文件全文或指定片段。 |
| `网页抓取` | `web_fetch` | 抓取网页正文与低噪声摘要。 |
| `技能调用` | `skill_call` `skill_get` | 读取或调用 Skill 的操作说明与内容。 |
| `写入文件` | `write_file` | 创建或覆盖文件内容。 |
| `应用补丁` | `apply_patch` | 以 patch 方式批量修改文件。 |

另有默认技能 `技能创建器`，但它属于 Skill，不计入内置工具总数。

### 7.2 内置工具全量总表

| 工具名 | 常用别名 | 类别 | 默认启用 | 说明 |
| --- | --- | --- | --- | --- |
| `最终回复` | `final_response` | 回复控制 | 是 | 输出最终答复并结束当前工具链。 |
| `a2ui` | - | 回复控制 | 否 | 向前端返回结构化 UI 片段与补充内容。 |
| `计划面板` | `update_plan` | 回复控制 | 否 | 更新任务计划、步骤和进度状态。 |
| `问询面板` | `question_panel` `ask_panel` | 回复控制 | 否 | 向用户发起路线选择、确认或澄清。 |
| `会话让出` | `sessions_yield` `yield` | 回复控制 | 否 | 让出当前会话，等待外部恢复或继续执行。 |
| `定时任务` | `schedule_task` | 调度治理 | 是 | 管理 cron/at/every 定时任务。 |
| `休眠等待` | `sleep` `sleep_wait` `pause` | 调度治理 | 是 | 在当前任务内主动等待。 |
| `用户世界工具` | `user_world` | 平台内协作 | 否 | 在 Wunder 用户域内列用户、发消息。 |
| `渠道工具` | `channel_tool` `channel_send` `channel_contacts` | 外部渠道 | 否 | 查询渠道联系人并向外部对象发消息。 |
| `记忆管理` | `memory_manager` `memory_manage` | 状态与记忆 | 是 | 管理长期记忆条目、写入与删除。 |
| `a2a观察` | `a2a_observe` | 外部协作 | 否 | 观察 A2A 服务任务状态与事件。 |
| `a2a等待` | `a2a_wait` | 外部协作 | 否 | 等待 A2A 服务运行结果。 |
| `执行命令` | `execute_command` | 文件与代码 | 是 | 执行终端命令，受审批、沙盒和白名单控制。 |
| `ptc` | `programmatic_tool_call` | 文件与代码 | 是 | 执行本地程序化脚本工具。 |
| `列出文件` | `list_files` | 文件与代码 | 是 | 列出目录结构和文件树。 |
| `搜索内容` | `search_content` | 文件与代码 | 是 | 搜索代码、配置或文本内容。 |
| `读取文件` | `read_file` | 文件与代码 | 是 | 读取文件全文、多段范围或缩进块。 |
| `读图工具` | `read_image` `view_image` | 文件与代码 | 否 | 读取本地图片供视觉模型理解。 |
| `技能调用` | `skill_call` `skill_get` | 能力组织 | 是 | 读取 Skill 手册或调起技能能力。 |
| `写入文件` | `write_file` | 文件与代码 | 是 | 写入、创建或覆盖文件。 |
| `应用补丁` | `apply_patch` | 文件与代码 | 是 | 以结构化 patch 修改一个或多个文件。 |
| `LSP查询` | `lsp` | 文件与代码 | 否 | 获取语言服务诊断、定位和符号信息。 |
| `子智能体控制` | `subagent_control` | 智能体协作 | 否 | 派生、发送、等待单个子智能体。 |
| `会话线程控制` | `thread_control` `session_thread` | 智能体协作 | 否 | 枚举、切换、创建和等待会话线程。 |
| `智能体蜂群` | `agent_swarm` `swarm_control` | 智能体协作 | 否 | 面向多智能体并发派发与结果聚合。 |
| `节点调用` | `node.invoke` `node_invoke` | 平台集成 | 否 | 调用网关节点或远端节点能力。 |
| `网页抓取` | `web_fetch` | 外部信息 | 是 | 抓取网页正文、链接与页面摘要。 |
| `浏览器` | `browser` `browser_tool` | 外部信息 | 否 | 控制浏览器会话、页面导航、交互与截图；兼容 `browser_navigate`、`browser_click`、`browser_type`、`browser_screenshot`、`browser_read_page`、`browser_close`。 |
| `桌面控制器` | `desktop_controller` `controller` | 桌面能力 | 否 | 执行桌面点击、输入、快捷键等控制动作。 |
| `桌面监视器` | `desktop_monitor` `monitor` | 桌面能力 | 否 | 监视桌面画面、截图和变化状态。 |
| `自我状态` | `self_status` | 状态与记忆 | 否 | 输出当前线程、工具链和运行状态摘要。 |

### 7.3 代码落点

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
