---
title: 工具体系
summary: 工具从注册、选择、分发到结果落地的完整链路，以及并行度、超时、预算与裁剪常量。排查「工具为什么没被调到、结果为什么被裁」时看这页。
read_when:
  - 工具没出现在模型可见列表里，要逐层定位卡在哪一环
  - 工具结果被裁剪或被压缩，想知道是哪一层做的
  - 要接 MCP、技能或知识库，需要命名与路由规则
  - 判断工具能否并行、超时和预算是多少
source_docs:
  - crates/wunder-runtime/src/orchestrator/tool_calls.rs
  - crates/wunder-runtime/src/orchestrator/execute_tools.rs
  - crates/wunder-runtime/src/services/tools/dispatch.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/orchestrator/constants.rs
  - crates/wunder-runtime/src/orchestrator/tool_parallel.rs
  - crates/wunder-runtime/src/orchestrator/tool_result_payload.rs
  - crates/wunder-runtime/src/services/tool_result_display.rs
  - crates/wunder-runtime/src/services/llm.rs
  - crates/wunder-runtime/src/services/mcp.rs
updated_at: 2026-10-13
---

# 工具体系

这页讲机制链路。分层概念、默认集与命名口径见 [工具（核心概念）](/docs/zh-CN/concepts/core-tools/)，逐个工具的用法见 [工具总览](/docs/zh-CN/tools/)。

## 一次工具调用的链路

```
模型输出
  → tool_calls.rs        解析与清洗 tool calls（参数修复、raw 字符串化还原）
  → execute_tools.rs     编排：策略与审批、钩子、超时、预算、并行判定
  → tools/dispatch.rs    真正分发（别名 → 技能 → MCP → 知识库 → 内置）
  → ToolResultPayload    统一结果结构（orchestrator/tool_result_payload.rs）
  → finalize_tool_result 模型输入通道裁剪（execute_tools.rs:741）
  → 线程日志落库 + 事件推送（展示通道另算）
```

两个容易写错的点：

- `tool_pipeline.rs` 只是 `PreExecute → Execute → PostExecute → Result` 的四阶段模型，**钩子默认关闭**，它不是执行主链。
- 业务失败的识别在 `orchestrator/result_normalizer.rs`，不是分发层报错。「工具返回了但业务没成」是正常路径，模型会读到失败原因。

## 工具不可见的逐层定位

| 卡在哪 | 判据 | 位置 |
| --- | --- | --- |
| 没被注册 | 名字不在规范名/别名表里，也没有对应 MCP/技能/知识库 | `services/tools/catalog.rs` |
| 被临时下架 | `网页搜索`、`ptc`：代码保留但一律不可见 | `services/default_tool_profile.rs:75`、`catalog.rs:1338` |
| 不在能力选择层 | 管理员未给该智能体选中这个能力 | `services/agent_abilities.rs`、`services/abilities/` |
| 被形态过滤 | 桌面模式隐藏 `get_goal`/`create_goal`/`update_goal`/`会话线程控制`/`会话让出` | `default_tool_profile.rs:23-29` |
| 被能力门槛过滤 | 无 vision 时过滤读图与桌面类；多模态四件套需对应模型；`网页抓取`/浏览器需启用配置 | `catalog.rs:1336-1376`、`1401` 附近 |
| 名字被短路 | 别名与技能名同名时，前面的路由先命中 | `dispatch.rs:21-44` |

## 命名与动态注册

- MCP 工具：`server@tool`；`packaged: true` 的服务器折叠为单工具 `server@__mcp_pack__`，动作 `list` / `get` / `call`（`services/tools/mcp_pack.rs:9-35`）。
- 展示名冲突处理：工具名跨服务器重名时追加服务端名段，同服务器内重名再加运行时名哈希后缀（`catalog.rs:63-118`）。
- 技能名本身可直接调用；知识库**按库名逐个注册**，没有单一 `knowledge` 工具。
- 用户工具别名进 `UserToolBindings.alias_map`，**dispatch 最优先按别名路由**。

## 并行、超时与预算

| 项 | 值 | 出处 |
| --- | --- | --- |
| 并行度上限 | 4 | `orchestrator/constants.rs:39`、`execute_support.rs:119` |
| 并行判定 | 一批 tool calls 是否可并行在编排层判定 | `execute_tools.rs:388` |
| 锁语义 | 可并行工具取读锁、独占工具取写锁（RwLock 守卫） | `execute_tools.rs:784-804`、`tool_parallel.rs:6-45` |
| 工具默认超时 | 120 秒（下限 1 秒） | `constants.rs:36-37` |
| 工具调用预算 | 每轮 10000 | `execute_support.rs:118` |
| 记忆召回预算 | 每轮 2000 | `execute_support.rs:121` |

并行白名单之外的工具串行执行，这是刻意的：写类与执行类要独占。

## 结果的两条裁剪通道

| 通道 | 常量与行为 | 影响 |
| --- | --- | --- |
| 展示 | `services/tool_result_display.rs`：preview 2400 字符、16 行，超出追加省略号 | 只影响客户端渲染；原始消息与复制内容完整保留 |
| 模型输入 | `finalize_tool_result`（`execute_tools.rs:741`）→ 按预算截断头尾；旧的可复现观测交给 microcompaction | 决定模型下一步看到什么 |

入模预算在 `orchestrator/constants.rs:41-50`：头尾各 10000 字符；数组最多 128 项（头 48 + 尾 16）；分页型结果 500 项（头 180 + 尾 60）。裁剪会带 `...(truncated)...` 标记，模型能看到「被裁了」。

microcompaction 的口径：保留最近 5 组，旧观测收成 768 字节预览，且至少省下 256 token 才动手（`orchestrator/microcompaction.rs:7-9`）。

## MCP 接入事实

- 客户端配置字段：`name / endpoint / transport / headers / auth / allow_tools / packaged / tool_specs`（`wunder-core/src/config.rs:1737-1759`）。
- 内置 MCP 服务端：`/wunder/mcp`，streamable-http（`services/mcp_impl.rs:80-88`），暴露 `wunder@excute` 与 `doc2md`（源码里就是 `excute` 这个拼写）；`mcp` feature 关闭时返回 501（`services/mcp.rs:22-32`）。
- `extra_mcp/` 是独立 Python FastMCP 服务，自带配置与工具集，不属于本引擎。

## 协议与 provider 差异

`resolve_tool_call_mode`（`services/llm.rs:305-319`）：

| 情况 | 模式 |
| --- | --- |
| 模型显式配置 `tool_call_mode` | 按配置 |
| provider 为 `openai` | FreeformCall（文本协议工具调用） |
| 其余 provider | FunctionCall（原生函数调用） |
| 无法识别的值 | 兜底 ToolCall（tag/xml 文本协议） |

freeform 的 lark 语法目前只为 `文本编辑` 的补丁形态准备（`services/tools/freeform.rs`）；参数修复与嵌套还原在 `wunder-core/src/tool_args.rs`。

Win7 没有独立的工具协议分支；已证实的差异只在互通侧——老 Win7 节点不声明 `tool.exec` 能力。

## 已移除 / 已合并（写文档与配置时按此对齐）

| 状态 | 项 |
| --- | --- |
| 已移除 | 用户世界工具、渠道工具、`a2a_*` 工具、`final_response`、`a2ui` 工具、LSP 查询/节点调用工具、独立「休眠等待」工具 |
| 已合并 | `apply_patch` 并入 **`文本编辑`**（同一工具三形态：替换 / 补丁 / 子命令） |
| 语义变更 | `会话让出` 取代旧「休眠等待」的说法 |
| 临时下架 | `网页搜索`、`ptc`（保留代码，不可见） |

## 常见误解

- **「`tool_pipeline.rs` 是执行主链」** — 不是，它是四阶段模型，钩子默认关闭。
- **「结果被截了就是模型看不到了」** — 展示通道与模型输入通道分开，见上表。
- **「工具越多越聪明」** — 相反。名字多、描述含糊会提高选错率，默认集刻意收敛在 13 个内置 + 1 个技能。
- **「工具报错就是调用失败」** — 业务失败会作为正常结果返回给模型，带原因与下一步提示。
- **「所有工具都能并行」** — 只有并行白名单内的读类可以，写与执行独占。
- **「Win7 有工具协议回退」** — 未证实，不存在这条分支。

## 延伸阅读

- [工具（核心概念）](/docs/zh-CN/concepts/core-tools/) —— 四层模型与默认集
- [工具总览](/docs/zh-CN/tools/) —— 逐个工具的参数说明
- [上下文压缩](/docs/zh-CN/concepts/core-context-compression/) —— microcompaction 与历史压缩
- [实时性](/docs/zh-CN/concepts/core-realtime/) —— 工具事件怎么推给客户端
- [稳定性](/docs/zh-CN/concepts/core-stability/) —— 超时、预算与熔断
- [配置参考](/docs/zh-CN/reference/config/) —— MCP 与工具开关配置项
