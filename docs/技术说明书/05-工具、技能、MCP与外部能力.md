# 工具、技能、MCP 与外部能力

不是设计文档（边界与理由见 `docs/设计文档/05-工具、技能与 MCP 系统设计.md`），不是用户教程（操作路径见 `docs/使用说明书/`）。§11 是排障入口。

约定：所有路径为仓库相对路径；行号为写作时实测位置，代码变动后以就近符号为准。

## 1. 命名规则

模型看到的工具名**大多是中文规范名**，不是英文函数名。每个规范名可挂多个英文别名，全部经 `resolve_tool_name`（`crates/wunder-runtime/src/services/tools/catalog.rs`）归一到规范名后再进分发。

- 规范名是**唯一的注册键与治理键**：工具画像、审批分档、能力门、前端展示名都按规范名匹配。
- 别名只做兼容解析，不保证出现在工具面（模型看不到别名）。例：`programmatic_tool_call` → `ptc`。
- MCP 工具与打包 MCP 用 `server@tool` 形式，`server` 段来自配置里的服务器名（见 §7）。
- 知识库按库名动态注册，**库名本身就是工具名**（取代旧的单一 `knowledge` 工具）。
- 用户工具的别名进 `UserToolBindings.alias_map`，`dispatch.rs:29-33` 最优先按别名路由，命中即绕过内建解析（见 §9）。

写文档、写文案、写测试断言时必须给出「规范名 + 别名」。只写英文名容易查错对象。

### 1.1 规范名清单

`services/tools/catalog.rs`（ToolSpec 表）与 `services/tools/dispatch.rs:57-110`（分发分支）共同定义当前内置工具面：

| 规范名 | 主要别名 | 作用 |
| --- | --- | --- |
| `计划面板` | `update_plan` | 更新任务计划与步骤状态，仅展示无副作用 |
| `问询面板` | `question_panel` / `ask_panel` | 向用户发起多题问询，执行后停轮 |
| `会话让出` | `sessions_yield` / `yield` | 让出当前会话等待外部恢复 |
| `定时任务` | `schedule_task` | 管理 cron / at / every 定时任务 |
| `记忆管理` | `memory_manager` / `memory_manage` | 长期记忆条目读写 |
| `执行命令` | `execute_command` | 执行 shell 命令 |
| `命令会话` | `command_session` / `write_command_stdin` | 长驻命令会话轮询与 stdin 写入 |
| `ptc` | `programmatic_tool_call` | 程序化工具调用脚本 |
| `列出文件` | `list_files` | 工作区目录列举 |
| `glob` | — | 文件名与路径模式匹配 |
| `搜索内容` | `search_content` | 关键字 / 正则内容搜索 |
| `读取文件` | `read_file` | 文件全文或片段读取 |
| `读图工具` | `read_image` / `view_image` | 图片供视觉模型理解 |
| `写入文件` | `write_file` | 创建或覆盖文件 |
| `文本编辑` | `edit_file` / `apply_patch` / `str_replace_editor` | 替换、补丁、子命令三形态合一 |
| `技能调用` | `skill_call` / `skill_get` | 读取技能正文并做路径替换 |
| `子智能体控制` | `subagent_control` | 派生、发送、等待子智能体 |
| `会话线程控制` | `thread_control` | 枚举、切换、创建、等待会话线程 |
| `网页抓取` | `web_fetch` | 抓取网页正文与低噪声摘要 |
| `网页搜索` | `web_search` | 关键词搜索公开网页（当前临时下架，见 §4.1） |
| `桌面控制器` | `desktop_controller` | 桌面点击、输入、快捷键 |
| `桌面监视器` | `desktop_monitor` | 桌面画面与状态监视 |
| `自我状态` | `self_status` | 当前线程与工具链状态摘要 |
| `get_goal` / `create_goal` / `update_goal` | `goal` → `update_goal` | 常驻目标三件套，由运行时按会话注入 |
| `语音生成` / `声转文` / `图像生成` / `视频生成` | 见 catalog 多模态 spec | 多模态四件套，需对应模型可用 |

浏览器**不是单一工具**：`services/browser/playwright.rs:23-49` 注册 17 个 `browser_*` 原生面工具（导航、点击、输入、截图、读页、关闭等），浏览器能力启用即自动注册；「浏览器」只是文档聚合口径（`services/tools/browser_tool.rs:56-60`）。

## 2. 注册与分发

### 2.1 静态注册：catalog → dispatch

```
services/tools/catalog.rs      ToolSpec 表：规范名、别名、描述、input schema、ToolKind
        ↓ 生成模型可见工具面
orchestrator/tool_calls.rs     解析并清洗模型输出的 tool calls
        ↓
orchestrator/execute_tools.rs  编排：能力门、审批、超时、预算、并行
        ↓
services/tools/dispatch.rs:27  真正分发到实现
```

- `catalog.rs` 是工具面的**唯一定义处**；`builtin_tool_specs_with_language()` 按语言产出 spec，英文描述与中文规范名同处维护。
- 输入 schema 在 `services/tools/schema.rs`；参数修复在 `wunder-core/src/tool_args.rs`（负责 raw 字符串化与嵌套参数还原）。
- `dispatch.rs:57-110` 是内置工具的路由分支表，规范名在这里落到实现文件。
- 实现文件与规范名不是一一对应（如 `文本编辑` 落在 `edit_tool.rs` / `apply_patch_tool/` / `str_replace_editor_tool.rs`），以 dispatch 分支为准。
- `services/tools/registry.rs`、`services/tools.rs` 是注册容器与模块出口，不定义工具语义。

### 2.2 动态注册四类

除内置 spec 外，运行时还会往工具面追加动态工具：

| 类别 | 命名 | 定义处 |
| --- | --- | --- |
| MCP 工具 | `server@tool` | `catalog.rs:63-118` |
| 打包 MCP | `server@__mcp_pack__` | `services/tools/mcp_pack.rs:9-35` |
| 技能 | 技能名本身可直接调用 | `dispatch.rs:34-37` |
| 知识库 | 按库名注册，一库一工具 | `services/tools/knowledge_tool.rs` |

另有第五类**用户工具别名**，不参与 catalog 生成 spec，但在分发时优先级最高：`dispatch.rs:29-33` 先查 `UserToolBindings.alias_map`，命中即用用户侧配置的目标（MCP / 技能 / 知识库）执行。这意味着一个用户工具别名可以「遮盖」同名内置工具，改名或删别名前先确认没有别名冲突。

### 2.3 工具画像与可见性

- `services/tools/capabilities.rs` 决定一个工具在当前线程是否可用（能力门）。
- `services/default_tool_profile.rs` 决定默认勾选档位（见 §3）。
- `services/visibility.rs` 与管理员选择层（abilities）决定治理面可见性（见 §8）。

## 3. 默认与分档

`crates/wunder-runtime/src/services/default_tool_profile.rs` 是唯一入口，四张名单：

| 常量 | 位置 | 内容 | 语义 |
| --- | --- | --- | --- |
| `DEFAULT_BUILTIN_TOOL_NAMES` | `:4-18` | `定时任务`、`记忆管理`、`执行命令`、`命令会话`、`ptc`、`列出文件`、`glob`、`搜索内容`、`读取文件`、`网页抓取`、`技能调用`、`写入文件`、`文本编辑` | 新建智能体的初始勾选集合；另带默认技能 `技能创建器` |
| `DESKTOP_HIDDEN_TOOL_NAMES` | `:23-29` | `get_goal` / `create_goal` / `update_goal`、`会话线程控制`、`会话让出` | 桌面端不可选也不默认启用；goal 三件套由运行时按会话自动注入 |
| `DESKTOP_EXTRA_TOOL_NAMES` | `:32-34` | `读图工具`、`桌面监视器`、`桌面控制器`、`子智能体控制` | 桌面端额外默认启用 |
| `TEMPORARILY_HIDDEN_TOOL_NAMES` | `:75` 附近 | `网页搜索`、`ptc` | **临时下架，不是已移除** |

### 3.1 临时下架要说清楚

`网页搜索` 与 `ptc` 在 `TEMPORARILY_HIDDEN_TOOL_NAMES` 里，源码注释明写「属临时隐藏，恢复时移除对应条目即可」。同时 `catalog.rs:983` 仍注册 `网页搜索`，`dispatch.rs:90` 仍有 `ptc` 分支，`ptc` 也仍在 `DEFAULT_BUILTIN_TOOL_NAMES` 中。

正确表述：**代码保留、当前临时下架、对模型与用户不可见**。把它写成「已移除」是错的（现网 zh-CN/en 的 `tools/index.md`、`tools/web-fetch.md` 在这一条上需要纠正）。恢复方式是从隐藏名单里删掉对应条目，不需要动 catalog 或 dispatch。

### 3.2 可用性门槛

即使工具在默认名单里，仍需满足条件才会出现在模型可见工具面：

| 门槛 | 影响工具 | 出处 |
| --- | --- | --- |
| 模型具备 vision | `读图工具`、`桌面监视器`、`桌面控制器`；不可用时整类过滤 | `catalog.rs:1401` 附近、`capabilities.rs` |
| 对应多模态模型可用 | `语音生成`、`声转文`、`图像生成`、`视频生成` | `catalog.rs:1362-1381`、`services/multimodal_models.rs` |
| 启用网页抓取配置 | `网页抓取`（provider 与开关） | `services/tools/web_fetch_provider.rs`、`wunder-core/src/config.rs` |
| desktop 运行模式 + 显式开关 | 桌面类工具 | `crates/wunder-desktop/`、`capabilities.rs` |
| 浏览器能力启用 | 17 个 `browser_*` | `services/browser/playwright.rs:23-49` |

`自我状态` 不在任何默认名单，按需勾选。

## 4. 工具协议按 provider 解析

工具调用的**线协议不是固定的**，由 `resolve_tool_call_mode`（`services/llm/llm.rs:305-319`）逐 provider 解析：

| 模式 | 含义 |
| --- | --- |
| `FunctionCall` | 标准 OpenAI tools 字段结构化调用；除 `openai` 外的 provider 默认 |
| `FreeformCall` | 文本内嵌调用语法；`openai` 默认 |
| `ToolCall` | tag / xml 文本协议兜底（`llm.rs:175-176`），未知值走这里 |

另有 ChatCompletions / Responses 两套 API 形态（`llm.rs:111`）。模型若在配置里显式声明调用模式，显式配置优先于 provider 默认。

- `services/tools/freeform.rs:4-16/68-69` 的 lark 语法**只为 `文本编辑` 的补丁形态准备**，不是通用自由文本协议；语法文件 `services/tools/tool_apply_patch.lark`。
- `wunder-core/src/tool_args.rs` 负责修复被 raw 字符串化的参数与嵌套参数，解析异常先看这里再怀疑模型。
- **Win7 没有独立协议分支**。唯一与 win7 相关的是互通老节点不声明 `CAP_TOOL_EXEC`（见 §10）。任何「win7 工具协议回退」的说法都没有代码依据，不要写也不要实现。

## 5. 执行与治理

### 5.1 并行与锁

- 并行判定在 `orchestrator/execute_tools.rs:388`。
- 并行工具取读锁、独占工具取写锁，RwLock 守卫在 `execute_tools.rs:784-804`——**独占工具执行期间同线程并行工具被阻塞**。
- 并行度 `resolve_tool_parallelism` 上限 **4**（`execute_support.rs:119,429`、`constants.rs:39`）。
- 可并行白名单在 `orchestrator/tool_parallel.rs:6-45`：不在白名单一律独占。

### 5.2 审批与执行策略

- 按 `ToolKind` 分四档归档审批：`Read` / `Write` / `Exec` / `Control`（`core/approval_registry.rs:10-19`）。
- 批准结果缓存 **600s**（`wunder-core/src/exec_policy.rs:10-18`），同档同类不重复问。
- 审批队列容量 `APPROVAL_QUEUE_CAPACITY = 32`（`core/approval.rs:6`）。
- 执行策略三态 Allow / Audit / Enforce（`wunder-core/src/exec_policy.rs:6-10`），**未配置即 allow**（:21-29）；高危名单（`rm`、`rm -rf`、`mkfs`、`dd`、`chmod 777` 等）在 :59-130/227-246。
- 审批来源为 `ChatWs` 或 `Channel`（`approval_registry.rs:7-115`），前端确认**不能替代后端鉴权**。

### 5.3 预算与超时

| 项 | 值 | 出处 |
| --- | --- | --- |
| 每轮工具预算 | 10000 | `orchestrator/execute_support.rs:119-122` |
| 记忆召回每轮预算 | 2000 | 同上 |
| 工具默认超时 | 120s（下限 1s） | `orchestrator/constants.rs`、`DEFAULT_TOOL_TIMEOUT_S` |
| 编排层裁剪常量 | 字符串头尾各 10000 字符；数组 128 / 48+16 项；分页 500 / 180+60 | `orchestrator/constants.rs:41-50` |

### 5.4 结果契约

工具实现的返回值统一收敛为 `ToolResultPayload`（`orchestrator/tool_result_payload.rs:4`），业务失败识别在 `result_normalizer.rs`。

统一返回骨架为 `ok` / `action` / `state` / `summary` / `data` 五段，失败时带 `error_meta`（错误码、可重试性、提示）。写新工具时：`summary` 给人和模型一眼读懂，`data` 只放结构化事实，不要塞长文本日志。`ToolResultPayload.timestamp` 当前未被使用（`tool_result_payload.rs:8`，仓内已标 dead code），不要依赖。

### 5.5 两条裁剪通道

这是最常见的误解来源，**必须分开看**：

| 通道 | 位置 | 影响 |
| --- | --- | --- |
| 展示通道 | `services/tool_result_display.rs`，preview 2400 字符 / 16 行 | **只影响客户端展示**，不回写后端结果 |
| 模型输入通道 | `finalize_tool_result`（`execute_tools.rs:741`）+ microcompaction + `model_observation` | 决定模型实际看到什么 |

模型输入侧的六层裁剪与统一排障顺序在 [11-工具结果裁剪与观测链路](./11-工具结果裁剪与观测链路.md)，本章不重复展开。判断「模型是不是没看到内容」只看第二条通道；判断「用户界面为什么只显示一半」只看第一条。

### 5.6 工具日志与落库

工具调用与结果作为线程输出项落库，表为 `thread_items` / `thread_turns` / `tool_logs`（`crates/wunder-runtime/src/storage/sqlite/schema.rs:607/626/694`）。一个模型轮次内可以有多个工具调用，**工具调用不单独计模型轮次**。

## 6. MCP：Rust 侧作为服务端

舰体内置 MCP 服务端点 `/wunder/mcp`（`services/mcp.rs:73-85`）。

| 项 | 事实 |
| --- | --- |
| feature 门 | `mcp`；未开启时该端点返回 **501**（`services/mcp.rs:22-32`） |
| 实现 | rmcp，Streamable HTTP，`nest_service` 挂到主路由（`services/mcp_impl.rs:79-88`） |
| 暴露工具 | `wunder@excute`（**源码即此拼写，非笔误可改**）、`doc2md` |

外部 MCP 客户端由此可以「把一个 wunder 智能体当一个工具用」。`doc2md` 走 `services/doc2md.rs`，输入上限 10MB（`api/doc2md.rs:8`）。

## 7. MCP：作为客户端与外部服务

### 7.1 配置

客户端配置在 `wunder-core/src/config.rs:1737-1759`，键为 `config.mcp.servers{...}`：

| 字段 | 作用 |
| --- | --- |
| `name` | 服务器名，决定工具名前缀 |
| `endpoint` | Streamable HTTP 地址 |
| `transport` | 当前只证实 `streamable-http` |
| `headers` / `auth` | 请求头与鉴权 |
| `allow_tools` | 工具白名单 |
| `packaged` | 折叠为单工具模式（见 7.3） |
| `tool_specs` | 本地补充的 spec 描述 |

**stdio 传输未在当前代码中找到实现，不要配置也不要写进文档。**

### 7.2 命名与冲突

工具注册为 `server@tool`（`catalog.rs:63-118`）。同服务器内工具名冲突时追加后缀哈希，保证规范名唯一，因此**在工具面看到的名字可能与远端原始名不同**，排障时按前缀找服务器、按哈希后缀认冲突。

### 7.3 packaged 折叠

`packaged: true` 的服务器折叠为**单个**工具 `server@__mcp_pack__`（`services/tools/mcp_pack.rs:9-35`），动作参数为 `list` / `get` / `call`。目的是把工具面很大的第三方服务器压成恒定 1 个 spec，节省上下文、稳定提示词前缀；代价是模型需要先 `list` 再 `call`，多一次往返。

### 7.4 `extra_mcp/`

`extra_mcp/` 是**独立的 Python FastMCP 服务**，不是 Rust 引擎的一部分：端点 `/mcp`，配置 `config/mcp_config.json`（含 example 模板），提供 `db_query*` / `db_export*` / `kb_query*` / `ppt_*` 一类工具。在 compose 里作为独立服务（`extra-mcp`）部署，Rust 侧通过 §7 的客户端配置接入。

## 8. 技能、能力与知识库

四层职责不同，不要互相代称：

| 层 | 是什么 | 治理面 | 关键出处 |
| --- | --- | --- | --- |
| 工具 | 执行体，有 spec 与 schema | 工具画像 | `services/tools/catalog.rs` |
| 技能 | 可调用知识 + 脚本目录 | 技能管理 | `services/skills.rs` |
| 知识库 | 检索语料，一库一动态工具 | 知识管理 | `services/knowledge.rs` 等 |
| 能力 abilities | **管理员选择层的统一描述符** | 智能体能力配置 | `services/abilities/`、`services/agent_abilities.rs` |

### 8.1 技能

- 技能 = 一个目录，含 `SKILL.md`（frontmatter 描述）与入口脚本 `run.py` / `skill.py` / `main.py`（`services/skills.rs:13-14`）。
- **技能名本身可直接被调用**（`dispatch.rs:34-37`），不必先「读取再执行」。
- `技能调用`（`services/tools/skill_call.rs`）负责读取技能正文并做 `{{SKILL_ROOT}}` 路径替换——技能里写相对路径，运行时替换成真实技能根。
- 内置技能目录由 `repo_assets.rs` 定位到 `config/skills/`。
- 打包导入（压缩档案、重名处理）走 `services/skill_archive.rs`；档案解压语义见 `services/archive_extract.rs`（`.zip` / `.skill` / `.hivepack` 等，GB18030 文件名解码）。
- 管理侧接口在 `services/admin_skills.rs` 与 `api/admin/integration_admin.rs`。

### 8.2 能力 abilities

`source` 取值：`builtin` | `mcp` | `skill` | `knowledge` | `user_*` | `shared`。worker card 的 `abilities` 字段就是这一层的选择结果（见 `02-仓库结构与模块职责.md`），运行时把它翻译成工具面。**工具是执行体，技能是知识加脚本，能力是治理面的选择单元**，三者不是一回事。

### 8.3 用户工具

三形态 `UserToolKind::Mcp | Skill | Knowledge`（`services/user_tools.rs:108`）：

- 别名进 `UserToolBindings.alias_map`，**dispatch 最优先按别名路由**（`dispatch.rs:29-33`）。
- 用户 MCP 仍走 `server@tool` 命名。
- 端点 `/wunder/user_tools/{mcp|skills|knowledge}`（`api/user_tools.rs:96-110`），共 33 条路由，是用户面里规模最大的一组。
- 蜂巢有独立目录 `frontend/src/components/user-tools/`；舰桥在「工具管理」导航下统一装配 builtin / mcp / knowledge / skills / toolVisibility（`web/app.js:196`）。

## 9. 本地能力与执行边界

| 能力 | 边界 | 出处 |
| --- | --- | --- |
| 命令会话 | `services/tools/command_sessions/`（broker / tracker / terminal_pty / desktop）；winpty 库定位 `desktop.rs:59`；**win7 下 GBK 解码** `:1231` | 长驻会话与一次性 `执行命令` 是两套通道 |
| 浏览器 | 域名白名单 + 私网与 `file://` 阻断 | `services/browser/` |
| 桌面工具 | 需 desktop 模式 + 显式开关；vision 不可用时整类过滤 | `capabilities.rs` |
| 文件工具 | 路径围栏 `is_within_root`（`wunder-core/src/path_utils.rs:47-53`） | 被工具层广泛调用（`file_tool.rs:1436`、`edit_tool.rs:203`） |
| 云端执行 | 读/写/执行类工具一律在沙盒内执行，无沙盒端点直接失败、**不回落宿主** | `crates/wunder-runtime/src/sandbox/` |
| 执行策略 | Allow / Audit / Enforce 三态 + 高危名单 | `wunder-core/src/exec_policy.rs` |
| 互通远端执行 | `tool.exec` 属 L3 风险；默认设备能力集不含 L3，win7 构建不声明 `CAP_TOOL_EXEC`；越集命令本地 preflight 直接拒绝；L3 审批不可记忆 | `wunder-core/src/interlink.rs` |

沙盒侧限额（`sandbox/mod.rs:17-24`）：并发命令会话 16、会话保留 5min、增量 256KB、规则缓存 512/600s、命令超时 120s、PTC 60s。

## 10. 新增一个工具的正确落点

按顺序改，缺一处就会出现「注册了但看不见」或「看见了但跑不通」：

| 步骤 | 落点 | 检查点 |
| --- | --- | --- |
| 1 实现 | `services/tools/<新文件>.rs` | 返回 `ToolResultPayload` 骨架，`summary` 短、`data` 结构化 |
| 2 spec | `services/tools/catalog.rs` | 中文规范名 + 英文别名 + 详细描述；描述写清何时用、何时不用 |
| 3 schema | `services/tools/schema.rs` | 参数可修复的走 `tool_args.rs`；避免深层嵌套 |
| 4 分发 | `services/tools/dispatch.rs` | 加分支；确认别名不与用户工具 `alias_map` 冲突 |
| 5 能力门 | `services/tools/capabilities.rs` | vision / 模型 / 配置 / desktop 门槛声明清楚 |
| 6 档位 | `services/default_tool_profile.rs` | 是否进 `DEFAULT_BUILTIN`；要临时下架写进 `TEMPORARILY_HIDDEN`，不要删代码 |
| 7 并行 | `orchestrator/tool_parallel.rs:6-45` | 只读工具才加白名单；写类保持独占 |
| 8 审批 | `ToolKind` 归档 | `Read` / `Write` / `Exec` / `Control` 选对档，决定缓存与提示语义 |
| 9 文案 | 三端展示 | 蜂巢 i18n `frontend/src/i18n/messages/`；舰桥 `web/shared/tool-visuals.js` 与 `web/modules/`；蜂窝 `frontend-slint/src/tool_icons.rs`；Slint 文案由 `scripts/build_i18n.py` 生成 `ui/i18n.slint`，**勿手改生成物** |
| 10 测试 | `crates/wunder-runtime/tests/` | 至少一条 dispatch 与一条结果裁剪边界用例 |

工具的系统描述（description）是模型唯一的选择依据，写模糊就会误调用或不被调用。返回要精简，长内容落文件路径而不是塞回上下文。

## 11. 排障入口

| 症状 | 查哪里 | 常见原因 |
| --- | --- | --- |
| 工具没出现在模型可见面 | `capabilities.rs` → `default_tool_profile.rs` → `catalog.rs` spec 是否生成 | 能力门未过（vision、多模态模型、网页抓取配置）；在 `TEMPORARILY_HIDDEN` 名单里；desktop 开关未开 |
| 模型调了但报未知工具 | `resolve_tool_name` 别名表 + `dispatch.rs:57-110` 分支 | 只加了别名没加分支，或规范名文案不一致 |
| 调用落到意外目标 | `dispatch.rs:29-33` 用户工具别名优先 | 用户工具别名遮盖了内置同名工具 |
| 工具被判需审批或被拒 | `approval_registry.rs`、`exec_policy.rs` | 档位选高；Enforce 模式；命中高危名单；互通侧 L3 越集在本地 preflight 拒绝 |
| 结果被裁 / 模型说看不到 | 先看 `finalize_tool_result`（`execute_tools.rs:741`）与 `model_observation`，再看 `tool_result_display.rs` | 混淆两条通道（§5.5）；只有展示被裁时模型数据其实是完整的 |
| 一批调用串行变慢 | `tool_parallel.rs:6-45`、`execute_tools.rs:784-804` | 独占工具混进并行批次抢写锁；并行度上限 4 |
| 超时 | `constants.rs` 的 `DEFAULT_TOOL_TIMEOUT_S`（120s） | 沙盒侧另有命令 120s / PTC 60s 更内层的限制 |
| MCP 工具接不上 | `config.mcp.servers` 与 `catalog.rs:63-118` 命名 | 名字带 `server@` 前缀或冲突哈希后缀；`packaged: true` 时只有 `__mcp_pack__` 一个工具；stdio 未实现 |
| `/wunder/mcp` 返回 501 | cargo feature `mcp` 未开启（`services/mcp.rs:22-32`） | 构建产物不含该 feature |
| 参数解析失败 | `wunder-core/src/tool_args.rs`、`services/llm/llm.rs:305-319` | provider 的调用模式与预期不符；freeform 语法只覆盖 `文本编辑` |
| win7 上工具行为异常 | 先看能力声明与 GBK 解码，不是协议 | 不存在 win7 协议回退分支 |

## 12. 已删除与已合并

- 已合并：`apply_patch` 并入 `文本编辑`（一个工具三形态，`catalog.rs:1219-1226`）。
- 已删除（无 dispatch 分支，工具文档应移除）：用户世界工具、渠道工具、`a2a_*`、`final_response`、`a2ui`、LSP 查询/节点调用工具、独立的「休眠等待」工具（现在由 `会话让出` 承担）。
- 未实现：**A2A 互操作**。`grep -rniE "a2a|agent2agent" crates/ --include=*.rs` 零命中，舰桥仅残留读不到后端数据的幽灵文案。对外集成面是「外部工作流 `/wunder/external/workflow*` + MCP 端点 + 渠道 webhook」，不要写 A2A。
- 蜂群编排已从引擎与调度面移除，协作形态只剩子智能体；前端标签与兼容垫片仍在（`docs/技术说明书/02-仓库结构与模块职责.md` 记了残留清单）。

## 关联文档

- [02-仓库结构与模块职责](./02-仓库结构与模块职责.md)：模块落点与 crate 分层
- [03-运行时与执行链路](./03-运行时与执行链路.md)：主循环与轮次口径
- [11-工具结果裁剪与观测链路](./11-工具结果裁剪与观测链路.md)：模型输入侧六层裁剪
- [06-数据存储、工作区与配置](./06-数据存储、工作区与配置.md)：`config.mcp.servers` 与配置加载
- [09-开发、扩展与变更流程](./09-开发、扩展与变更流程.md)：改动流程与测试入口
