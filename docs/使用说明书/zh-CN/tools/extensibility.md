---
title: 扩展与动态工具
summary: 工具/技能/能力/用户工具四层关系，以及 MCP `server@tool`、`server@__mcp_pack__`、技能名直调、知识库按名注册与别名优先路由。
read_when:
  - 要接 MCP、加技能、加知识库，或困惑于"工具名单里为什么多出一个名字"
  - 需要判断某个能力该配在哪一层
source_docs:
  - crates/wunder-runtime/src/services/tools/dispatch.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/tools/mcp_pack.rs
  - crates/wunder-runtime/src/services/tools/user_tool_dispatch.rs
  - crates/wunder-runtime/src/services/tools/knowledge_tool.rs
  - crates/wunder-runtime/src/services/tools/skill_call.rs
  - crates/wunder-runtime/src/services/user_tools.rs
  - crates/wunder-runtime/src/services/agent_abilities.rs
  - crates/wunder-runtime/src/services/mcp.rs
  - crates/wunder-runtime/src/services/mcp_impl.rs
  - crates/wunder-core/src/config.rs
updated_at: 2026-10-13
---

# 扩展与动态工具

模型看到的工具名，一部分来自内置表，另一部分是**运行时按配置动态注册**出来的。理解这四层，就能解释"我明明没勾这个工具，它为什么在"或"配了却看不见"。

## 四层分别是什么

| 层 | 是什么 | 谁在用 |
| --- | --- | --- |
| **工具** | 执行体：有 schema、有分派分支、能产生副作用或读取数据 | 模型调用，运行时执行 |
| **技能** | 知识 + 脚本：一个目录，含 `SKILL.md`（frontmatter）与入口脚本 `run.py` / `skill.py` / `main.py` | 既可被当工具调用，也可只读取其正文当资料 |
| **能力（abilities）** | **管理员选择层的描述符**，不是执行体：把工具与技能统一成可选条目 | 舰桥/蜂巢勾选、授权、共享 |
| **用户工具** | 用户自带的三类扩展：MCP / 技能 / 知识库，带别名路由 | 用户面配置，运行面按别名命中 |

能力描述符的 `source` 取值固定八种：`builtin`、`mcp`、`skill`、`knowledge`、`user_mcp`、`user_skill`、`user_knowledge`、`shared`；`kind` 只有 `Tool` 与 `Skill`；`id` 形如 `<source>[:<owner>]:<runtime_name>`（`crates/wunder-runtime/src/services/agent_abilities.rs:13-43`、`crates/wunder-runtime/src/services/abilities/mod.rs:15-41`）。所以"选了一个能力"= 把一个工具名或技能名放进该智能体的选择集，运行仍由工具/技能层执行。

用户工具三形态为 `UserToolKind::Mcp | Skill | Knowledge`（`crates/wunder-runtime/src/services/user_tools.rs:107-119`），管理 API 前缀 `/wunder/user_tools/mcp`、`/wunder/user_tools/skills`、`/wunder/user_tools/knowledge`（`crates/wunder-runtime/src/api/user_tools.rs:96-216`）。

## 分派顺序：别名最优先

`execute_tool` 的命中顺序是固定的（`crates/wunder-runtime/src/services/tools/dispatch.rs:26-46`）：

1. `resolve_tool_name` 把英文别名归一到中文规范名。
2. **用户工具别名表命中** → 直接按别名路由到用户 MCP / 用户技能 / 用户知识库。别名格式为 `<owner_id>@<工具名>`；自己的技能可直接用技能名，别人的技能用 `<owner>@<技能名>`（`user_tools.rs:178-197`）。**这一步优先于内置工具**，因此同名时用户工具会改变路由。
3. **技能名命中** → 直接执行技能入口脚本（超时 60 秒），执行后标记工作区树脏。
4. **名字含 `@`** → 按 MCP 处理（`user_tool_dispatch.rs:107-109`）。
5. **知识库名命中** → 按库执行检索。
6. 其余走内置工具分派表（`dispatch.rs:48-111`），参数先过统一别名归一管线。

## MCP

### 客户端：`server@tool`

- 配置项：`config.mcp.servers[]`，字段含 `name`、`display_name`、`description`、`enabled`、`endpoint`、`transport`、`headers`、`auth`、`allow_tools`、`packaged`、`tool_specs[]`（`crates/wunder-core/src/config.rs:1737-1759`）。
- 注册规则：每个启用服务器里、被 `allow_tools` 放行的（`allow_tools` 为空表示全放行）每个工具，注册成 `<server>@<tool>`（`crates/wunder-runtime/src/services/tools/catalog.rs:1581-1601`）。
- **打包模式**：`packaged: true` 的服务器折叠成**一个**工具 `<server>@__mcp_pack__`，动作只有三个（`crates/wunder-runtime/src/services/tools/mcp_pack.rs:8-37`）：

| 动作 | 参数 | 语义 |
| --- | --- | --- |
| `list` | — | 返回可用工具的精简清单（描述截到 180 字符） |
| `get` | `tool` | 返回单个工具的 schema |
| `call` | `tool`、`arguments` | 调用远端工具 |

  返回骨架是 `ok / action="mcp_package" / package_action / state="completed" / summary / data`。折叠的意义是少占提示词：几十个工作细节不进上下文，模型按需 `list`/`get`。

- 展示名与冲突：运行时名是 `server@tool`，管理面展示名用工具 `title`，同名工具加 `__<server>` 后缀，仍冲突再追加 6 位哈希（`catalog.rs:30-118`）。
- **传输只有 streamable-http**。`transport` 缺省与 `http` 都归一成 `streamable-http`，其它值在列工具与调工具时都被拒：`不再支持旧版 MCP 传输类型: <值>，请改用 streamable-http`（`mcp_impl.rs:47-56`、`537-542`、`625-630`、`715-722`）。stdio 在 wunder 侧**不支持**。
- 工具列表有缓存：TTL 30 秒、最多 128 条，键含服务器名/端点/传输/放行表/headers/auth（`mcp_impl.rs:44-45`、`440-466`）。
- 参数：MCP 只接受对象参数，非对象（含 `null` 之外）统一视为无参数（`mcp_impl.rs:645-652`）。
- 返回：全局配置的 MCP 直连调用返回 `{content, structured_content, meta, is_error}` 形态（`mcp_impl.rs:654-671`），**不是**统一骨架；经用户工具别名路由的 MCP 调用会再包一层 `ok/action="mcp_call"/state="completed"/summary/data`，并在 `is_error=true` 时报错（`user_tool_dispatch.rs:95-105`）。
- 需要 `mcp` Cargo 特性；未编译时内置端点返回 501 并提示 `rebuild with --features mcp`（`crates/wunder-runtime/src/services/mcp.rs:22-33`、`Cargo.toml:11-19`）。

### 服务端：`/wunder/mcp`

舰体/蜂窝在 `/wunder/mcp` 挂一个 streamable-http MCP 服务，服务器名 `wunder`，暴露两个工具：`excute`（执行智能体任务并返回最终回复）与 `doc2md`（解析文档返回 Markdown）（`mcp_impl.rs:36-42`、`78-89`）。工具名 `excute` 是代码里的实际拼写，对接时以它为准。

端点侧还有一道防自环的名单：`excute` 跑任务时给智能体的工具集由 `build_allowed_tool_names` 现算（`config.tools.builtin.enabled` + 别名 + MCP 注册名，浏览器族在未可用时剔除），并**显式移除 `wunder@excute` 与 `a2ui`**，避免端点驱动的智能体再回调端点本身或调用已删除的工具（`mcp_impl.rs:134-169`、`222-232`）。

### 独立服务 `extra_mcp/`

仓库根的 `extra_mcp/` 是一个**独立的 Python FastMCP 服务**，不属于 Rust 运行时：配置默认读 `config/mcp_config.json`（可用 `MCP_CONFIG_PATH` 覆盖），传输由 `MCP_TRANSPORT` 决定（`stdio` / `sse` / `streamable-http`，默认 `stdio`）。注册的工具名为 `db_query`、`db_export`、`kb_query`、`ppt_read`、`ppt_write`、`ppt_delete`、`ppt_refine`、`ppt_template_read`（`extra_mcp/main.py:1-16`、`extra_mcp/tools/__init__.py:10-13`）。要连它，就在 `config.mcp.servers[]` 里按上面的规则配一个 streamable-http 端点。

## 技能

- 目录式：一个技能 = 一个目录，`SKILL.md` 携带 frontmatter，入口脚本按 `run.py` → `skill.py` → `main.py` 顺序探测（`crates/wunder-runtime/src/services/skills.rs:13-14`）。
- **技能名本身就是可调用工具名**：分派第 3 步直接执行（`dispatch.rs:34-38`），无需经 `技能调用`。
- `技能调用`（别名 `skill_call`，参数只有 `name`）负责的是"读正文"：返回 `data.{name,description,path,root,content,tree}`，并把正文里的 `{{SKILL_ROOT}}` 替换为真实技能根（`crates/wunder-runtime/src/services/tools/skill_call.rs:12-20`、`45-129`）。名字可写成 `<owner>@<技能名>` 或裸技能名，多主人重名时返回歧义错误并列出候选。
- 打包导入（含 `.skill` 压缩档案与重名处理）在 `crates/wunder-runtime/src/services/skill_archive.rs`。
- 全局技能目录由 `skills.paths` 提供；脚本执行器路径可用 `WUNDER_SKILL_RUNNER_PATH` 覆盖（`skills.rs:15`）。

## 知识库

- **按库名动态注册**：每个 `knowledge.bases[]` 中 `enabled` 且 `root` 非空的库，其库名直接成为一个工具名；与技能同名时让位给技能（`catalog.rs:1608-1620`）。旧的单一 `knowledge` 工具已不是入口形态，返回骨架里的 `action` 仍记为 `knowledge`。
- 参数面：`query`（或 `keywords` 数组，二者取其一）、`limit`；`keywords` 优先，多关键词时按词分组检索并去重（`crates/wunder-runtime/src/services/tools/knowledge_tool.rs:561-595`）。
- 三型由 `base_type` 归一为 Literal / Vector / Ragflow，分派分别走词面+LLM 检索、内置向量库、外部 RAGFlow（`knowledge_tool.rs:72-89`）。向量检索失败或没有可用嵌入档案时**回落词面检索**，结果里 `fallback_mode` 可见（`knowledge_tool.rs:274-347`）。
- 返回 `data.{knowledge_base,vector,count,documents[,query,keywords,embedding_model,queries]}`，命中 0 条时带 `next_step_hint` 提示收窄查询；文档项被压缩成少数字段以省上下文（`knowledge_tool.rs:462-559`）。
- 事件：每次检索发 `knowledge_request`（debug 日志级别才带请求载荷）。

## 失败与排查

| 症状 | 定位 |
| --- | --- |
| 配了 MCP 却没有工具 | 服务器 `enabled`？`allow_tools` 是否把它挡了？`transport` 是不是写了旧值？ |
| `不再支持旧版 MCP 传输类型` | 改成 `streamable-http` |
| `未知内置工具: <名>` | 名字既不是规范名/别名，也不含 `@`、不是技能名、不是库名 |
| 技能调用失败 `tool.invoke.skill_failed` | 入口脚本缺失或非零退出；先看 `技能调用` 能否读到正文 |
| 知识库名调不到 | 该库 `enabled=false`、`root` 为空，或名字被同名技能占用 |
| 用户工具"抢"了内置名 | 别名路由在第 2 步，优先于内置分派表，属预期行为 |
| 工具整体消失 | 临时下架名单（`网页搜索`、`ptc`）或桌面隐藏名单，见 [工具](/docs/zh-CN/tools/index/) |

## 边界

- 动态注册的名字与内置名共用一张表：给技能/知识库取名时避开内置规范名与别名，否则语义会被别名归一改写。
- MCP 工具列表与调用都受 `allow_tools` 白名单约束，`packaged` 折叠后仍然逐次校验（`mcp_pack.rs:173-200`）。
- 技能执行是**子进程跑 Python 脚本**，60 秒超时；它继承平台的路径与权限边界，不要把特权操作写进技能脚本。
- 扩展项进入模型上下文前仍受能力面选择集与模型可用性过滤（视觉类、多模态类、桌面类的门槛各自不同，见 [工具](/docs/zh-CN/tools/index/)）。

## 相关页面

- 内置工具与档位：[工具](/docs/zh-CN/tools/index/)
- 技能正文读取：[技能调用](/docs/zh-CN/tools/skill-call/)
- 对外 MCP 端点：[MCP 端点接入](/docs/zh-CN/integration/mcp-endpoint/)
