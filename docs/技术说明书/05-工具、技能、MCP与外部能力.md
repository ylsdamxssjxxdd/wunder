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

## 5. 当前工具实现与内置工具目录

### 5.1 实现文件落点

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

### 5.2 默认启用工具画像

以下“默认启用”按当前 `src/services/default_tool_profile.rs` 与 `config/wunder-example.yaml` 整理，表示默认智能体和新建智能体的初始勾选集合，不等于运行时唯一可见工具集合。

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

### 5.3 内置工具全量总表

当前 model-visible 内置工具按 `src/services/tools/catalog.rs` 中的 `builtin_tool_specs_with_language()` 统计，共 **31** 个。

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
