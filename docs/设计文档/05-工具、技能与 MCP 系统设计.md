# 工具、技能与 MCP 系统设计

> 约束范围：本册约束能力面（工具 / 技能 / abilities / 用户工具 / MCP）的分层语义、dispatch 与工具画像的兑现方式、工具协议解析边界、临时下架与已移除的口径分界、结果裁剪双通道。端点清单、配置字段与工具参数表见 `../技术说明书/05-工具、技能、MCP与外部能力.md` 与 `../技术说明书/11-工具结果裁剪与观测链路.md`，本册不复述。

## 1. 四层能力模型

wunder 的能力面是四层，不是一套概念的四个别名。混用任何两层都会导致治理错位：

| 层 | 定义 | 真相位置 | 设计含义 |
| --- | --- | --- | --- |
| 工具 | 模型直接调用的最小执行体 | `services/tools/catalog.rs` 注册、`services/tools/dispatch.rs` 分发 | 有 schema、有审批档、有超时、有结果归一 |
| 技能 | 可复用的知识与脚本包，不是新运行时原语 | `services/skills.rs`：目录 = `SKILL.md`（frontmatter）+ 入口 `run.py`/`skill.py`/`main.py` | 组织工具，不替代工具；技能名本身可被直接调用（`dispatch.rs:34-37`） |
| abilities | 管理员选择层的统一描述符 | `services/abilities/`、`services/agent_abilities.rs`，`source = builtin \| mcp \| skill \| knowledge \| user_* \| shared` | 治理面的选择单元：管理员勾选的是能力，不是执行体 |
| 用户工具 | 开发者自定义接口的入口 | `services/user_tools.rs`，`UserToolKind::Mcp \| Skill \| Knowledge` | 开发者把外部东西接进来；别名进 `UserToolBindings.alias_map` |

「对开发者来说一切都是接口，对大模型来说一切皆工具」在这两层兑现：

- **接口侧**：用户工具的三种形态（MCP / 技能 / 知识库）都是注册进来的接口，不要求写 Rust。
- **工具侧**：无论来源，最终都折叠成模型可见的一个工具名，走同一条 dispatch。

## 2. dispatch 统一入口（不变量）

`services/tools/dispatch.rs:26-45` 是唯一路由事实。优先级固定：

| 序 | 路由 | 依据 |
| --- | --- | --- |
| 1 | 用户工具别名 | `alias_map` 按归一化名命中即转发 `execute_user_tool` |
| 2 | 技能名 | `context.skills` 命中直接 `execute_skill` |
| 3 | MCP 工具 | `server@tool` 形状判定（含打包折叠 `server@__mcp_pack__`） |
| 4 | 知识库名 | 按库动态注册，`find_knowledge_base` 命中转发 |
| 5 | 内置工具 | `execute_builtin_tool` 按规范名 match |

由此推出三条不变量：

1. **没有旁路**。技能、MCP、知识库都从这同一个入口进，因此审批、超时、并行、裁剪、观测对四类能力一致生效；任何「新能力直接挂 handler 不进 dispatch」的做法破坏本册。
2. **动态注册不进内置 match**。知识库名取代旧的 `knowledge` 单工具、MCP 工具名带 `server@` 前缀（冲突加后缀哈希，`catalog.rs:63-118`）、打包 MCP 折叠为单工具（动作为 list/get/call，`tools/mcp_pack.rs`）——它们由 catalog 动态生成，不是 `dispatch.rs` 里的分支。
3. **归一先于路由**。模型侧工具名大多是中文规范名（如 `执行命令`、`文本编辑`、`计划面板`），英文别名经 `resolve_tool_name` 归一（`catalog.rs:1184`，如 `programmatic_tool_call` → `ptc`）。文档与代码提及工具必须给「规范名 + 别名」。

## 3. 工具协议按 provider 解析的边界

`resolve_tool_call_mode`（`services/llm.rs:305-319`，已实测）：

| 优先级 | 规则 |
| --- | --- |
| 1 | 模型显式配置 `tool_call_mode` 优先 |
| 2 | provider 归一为 `openai` → `FreeformCall` |
| 3 | 其余 → `FunctionCall` |
| 4 | 未知配置值兜底 → `ToolCall`（tag/xml 文本协议） |

- freeform（lark 语法）当前只为 `文本编辑` 的 apply_patch 形态准备（`services/tools/freeform.rs`）。
- `文本编辑` 是一个工具三形态（替换 / 补丁 / 子命令），`apply_patch` 已并入，不再是独立工具。
- 另有 ChatCompletions / Responses 双 API 并存；Win7 **没有**独立协议分支，「win7 工具协议回退」未证实，不得书写。
- 模型输出的 tool calls 由 `orchestrator/tool_calls.rs` 解析清洗、`tool_args.rs` 修复 raw 字符串化与嵌套参数，之后才进 dispatch。

## 4. 临时下架 ≠ 已移除（口径分界）

这是本册最容易被写错的一条。以 `services/default_tool_profile.rs` 为唯一口径源：

| 状态 | 判据 | 实例 |
| --- | --- | --- |
| 临时下架 | `TEMPORARILY_HIDDEN_TOOL_NAMES = ["网页搜索", "ptc"]`（:75，注释明写「恢复时移除对应条目即可」）；catalog 仍注册、dispatch 仍有分支 | `网页搜索`、`ptc` |
| 已移除 | 代码无 dispatch 分支、无注册 | 用户世界工具、渠道工具、`a2a_*`、`final_response`、`a2ui`、LSP 查询/节点调用、独立「休眠等待」工具（`sleep` 语义现为 `会话让出`） |
| 已合并 | 旧名归一到新工具 | `apply_patch` → `文本编辑` |

正确表述：临时下架的工具「代码保留、当前对模型与用户不可见，恢复只需移除隐藏条目」；写成「已移除」是错的。

分档规则（同一文件）：默认内置集不含目标类与桌面类；`get_goal`/`create_goal`/`update_goal`、`会话线程控制`、`会话让出` 对桌面隐藏、由运行时按会话自动注入；桌面额外默认 `读图工具`、`桌面监视器`、`桌面控制器`、`子智能体控制`。可用性门槛（模型能力驱动）：读图与桌面类需 vision，多模态四件套（`语音生成`/`声转文`/`图像生成`/`视频生成`）需对应模型可用，`网页抓取` 需配置启用。

「浏览器」不是一个工具：启用浏览器注册的是 17 个 `browser_*` 原生面工具（`services/browser/playwright.rs:23-39`），聚合名只是文档口径。

## 5. 结果裁剪双通道（必须分开建模）

| 通道 | 位置 | 语义 | 约束 |
| --- | --- | --- | --- |
| 展示通道 | `services/tool_result_display.rs`，preview 2400 字符 / 16 行 | 客户端可读摘要 | 只影响人看到的，不影响模型输入；原始结果必须完整可查 |
| 模型输入通道 | `finalize_tool_result`（`orchestrator/execute_tools.rs:741`）+ microcompaction + model_observation | 进入下轮 prompt 的观测 | 受 token 预算、头尾各 10000 字符等裁剪常量（`orchestrator/constants.rs:41-50`）与「只信任 provider 观测到的 token」约束 |

为什么必须分开：模型输入通道直接改变上下文与 prompt 前缀，裁剪策略要与缓存复用、微压缩（旧观测折成 768B 预览、保留最近 5 组）联动演进；展示通道只受人的可读性约束。合并成一条通道会导致「为了展示好看截断模型输入」或「为了模型完整撑爆 UI」两类事故。细节链路见 `../技术说明书/11-工具结果裁剪与观测链路.md`。

## 6. MCP 边界

- 内置 MCP 服务端：streamable-http，端点 `/wunder/mcp`（`services/mcp.rs`）；feature `mcp` 关闭时返回 501。暴露工具名沿用源码拼写（`wunder@excute`、`doc2md`）。
- 客户端侧只证实了 streamable-http 传输；stdio 未证实，不得书写。
- `extra_mcp/` 是独立 Python FastMCP 服务，作为外部 MCP 服务器接入，不受 wunder 进程治理，其工具经 `server@tool` 进入统一 dispatch 后受同等治理。

MCP 的定位是把外部能力接进来，**不绕开**审批、超时、裁剪与观测；也不允许长成第二套执行平面。

## 7. 反例与缺口

| 项 | 现状 | 性质 |
| --- | --- | --- |
| `网页搜索`/`ptc` 被写成「已移除」 | 现有 zh-CN/en 使用说明书 `tools/index.md`、`tools/web_fetch.md` 该条口径错 | 文档缺陷，正确口径见第 4 节 |
| 工具预算与并行度 | 每轮工具预算 10000、并行度 ≤4、可并行白名单（`tool_parallel.rs`）；钩子管线 `tool_pipeline.rs` 默认关闭，不是执行主链 | 勿把 pipeline 写成主链 |
| 协议型工具特例 shape | 少数工具仍保留内部子结果特例形态，未完全进入统一成功/失败包 | 已知待收口 |
| dispatch 未知名兜底 | 返回「未知内置工具」错误而非静默丢弃 | 保持：错误必须可观测 |

## 8. 验收判据

- 新增任何能力（内置工具、技能、MCP、知识库、用户工具）都能在 `dispatch.rs` 的路由序中找到唯一落点，且复用同一审批、超时、裁剪链。
- 文档中每个工具名都能对应 catalog 中「规范名 + 别名」，且状态标注（默认 / 分档 / 临时下架 / 已移除）与 `default_tool_profile.rs` 一致。
- 展示裁剪改动不回归影响模型输入长度；模型输入裁剪改动不回归影响客户端完整结果。

## 9. 相关文档

- 事实层：`../技术说明书/05-工具、技能、MCP与外部能力.md`、`../技术说明书/11-工具结果裁剪与观测链路.md`
- 上层：`01-系统总体设计.md`；相邻：`02-智能体核心设计.md`（执行主链）、`12-安全-权限与隔离设计.md`（审批分档）
