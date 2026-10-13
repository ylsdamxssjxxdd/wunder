---
title: 工具
summary: 智能体做事的能力来自四层结构：执行体工具、可调用知识与脚本的技能、管理员选择层的能力、用户自己接入的工具。理解分层才不会把「能选」和「能调」混为一谈。
read_when:
  - 不清楚为什么某个工具在舰桥里可选、智能体却调不到
  - 想知道工具名为什么大多是中文，别名怎么用
  - 要给智能体配置能力，但不确定该选内置工具、MCP、技能还是知识库
source_docs:
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/tools/dispatch.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
  - crates/wunder-core/src/schemas.rs
  - crates/wunder-runtime/src/services/user_tools.rs
  - crates/wunder-runtime/src/services/tool_result_display.rs
  - crates/wunder-runtime/src/orchestrator/execute_tools.rs
  - crates/wunder-runtime/src/core/approval_registry.rs
  - crates/wunder-runtime/src/services/tools/registry.rs
updated_at: 2026-10-13
---

# 工具

智能体不只会说话，还能做事。做事的能力统称「工具」，但**工具、技能、能力、用户工具是四层不同的东西**，混用是配置问题最常见的原因。

## 四层模型

| 层 | 是什么 | 谁在用 | 判据 |
| --- | --- | --- | --- |
| **工具** | 执行体：一次调用会读文件、跑命令、发请求 | 模型直接调用，引擎分发执行 | 有 `ToolSpec` 与分发分支（`services/tools/catalog.rs`、`dispatch.rs`） |
| **技能** | 知识与脚本包：目录 + `SKILL.md` 正文 + 入口脚本（`run.py`/`skill.py`/`main.py`） | 模型可直接按技能名调用，也可用 `技能调用` 读取正文 | `services/skills.rs` |
| **能力（abilities）** | 治理面的选择单元：管理员给智能体挑「能用哪些」的统一描述符 | 管理侧配置，不进执行链 | `source = builtin / mcp / skill / knowledge / user_mcp / user_skill / user_knowledge / shared`（`wunder-core/src/schemas.rs:103`） |
| **用户工具** | 用户自己接入的扩展，三形态 `Mcp \| Skill \| Knowledge` | 按用户挂载，别名进路由表 | `services/user_tools.rs:108` |

一句话：**工具是执行体，技能是可调用知识与脚本，能力是管理员的选择层，用户工具是用户侧的三类接入**。能力被选中不等于执行体存在；执行体存在不等于模型能看见。

## 名字：规范名 + 别名

模型看到的工具名**大多是中文规范名**，另有英文别名，调用前经 `resolve_tool_name` 归一（`services/tools/catalog.rs`）。

| 规范名 | 常见别名 | 规范名 | 常见别名 |
| --- | --- | --- | --- |
| `执行命令` | `execute_command` | `读取文件` | `read_file` |
| `命令会话` | `command_session`、`write_command_stdin` | `写入文件` | `write_file` |
| `ptc` | `programmatic_tool_call` | `文本编辑` | `edit`、`patch`、`apply_patch`、`应用补丁` |
| `列出文件` | `list_files` | `搜索内容` | `search_content` |
| `glob` | — | `技能调用` | `skill_call`、`skill_get` |
| `计划面板` | `update_plan` | `问询面板` | `question_panel`、`ask_panel` |
| `定时任务` | `schedule_task` | `记忆管理` | `memory_manager`、`memory_manage` |
| `子智能体控制` | `subagent_control` | `网页抓取` | `web_fetch` |

配置、日志、文档里写工具时按「规范名 + 别名」两栏写，只写一个容易对不上。

## 动态注册：模型侧名字不止来自内置清单

内置工具只是其中一类。真正进入模型工具列表的名字由四类动态注册构成（`services/tools/dispatch.rs:21-44`）：

| 来源 | 模型侧名字 | 说明 |
| --- | --- | --- |
| MCP 服务器 | `server@tool` | 逐工具注册；重名时展示名追加服务端名或哈希后缀（`catalog.rs:63-118`） |
| 打包 MCP | `server@__mcp_pack__` | `packaged: true` 的服务器折叠为单工具，动作为 `list` / `get` / `call` |
| 技能 | 技能名本身 | 技能名可直接被调用；`技能调用` 负责读取正文并替换 `{{SKILL_ROOT}}` |
| 知识库 | 知识库名 | **按库逐个注册**，取代旧的单一 `knowledge` 工具 |
| 用户工具别名 | 别名 | 分发时**最优先按别名路由**，命中即走用户工具 |

分发顺序：别名 → 技能 → MCP → 知识库 → 内置工具。前面的命中会短路后面的。

## 默认集与形态差异

默认工具集、桌面隐藏集与桌面额外集都在 `services/default_tool_profile.rs`，不要在文档或配置里另记一份。

| 集合 | 内容 |
| --- | --- |
| 默认内置（13） | `定时任务`、`记忆管理`、`执行命令`、`命令会话`、`ptc`、`列出文件`、`glob`、`搜索内容`、`读取文件`、`网页抓取`、`技能调用`、`写入文件`、`文本编辑` |
| 默认技能 | `技能创建器` |
| 桌面隐藏（不可选也不默认启用） | `get_goal`、`create_goal`、`update_goal`（由运行时按会话自动注入）、`会话线程控制`、`会话让出` |
| 桌面额外默认 | `读图工具`、`桌面监视器`、`桌面控制器`、`子智能体控制` |

**临时下架，不是已移除**：`TEMPORARILY_HIDDEN_TOOL_NAMES = ["网页搜索", "ptc"]`（`default_tool_profile.rs:75`）。代码保留、`catalog.rs` 仍注册、`dispatch.rs` 仍有分支，只是在可用性判定处一律返回不可用（`catalog.rs:1338`），对模型与用户都不可见。恢复时删掉对应条目即可。文档不得把这两个写成「已删除」。

可见性还有几道真实门槛：

| 门槛 | 影响 |
| --- | --- |
| 模型无 vision 能力 | 过滤 `读图工具` 与桌面类（`catalog.rs:1401` 附近） |
| 对应模型/服务未配置 | 多模态四件套 `语音生成`/`声转文`/`图像生成`/`视频生成` 逐个不可用 |
| 配置未启用 | `网页抓取`、`网页搜索`、`browser_*` 17 个浏览器原生工具、桌面类工具 |
| 形态 | 桌面模式过滤隐藏集 |

浏览器不是单一工具：启用即自动注册 17 个 `browser_*` 原生面工具（`services/browser/providers/playwright.rs:23-39`），聚合展示名「浏览器」只是文档口径。

## 结果有两条独立裁剪通道

这是最容易被误解的一条：**展示裁剪与模型输入裁剪是两套东西，互不影响**。

| 通道 | 位置 | 口径 | 影响谁 |
| --- | --- | --- | --- |
| 展示通道 | `services/tool_result_display.rs` | preview 2400 字符 / 16 行，超出加省略号 | **只影响客户端展示**，原始结果仍完整保留、可复制 |
| 模型输入通道 | `finalize_tool_result`（`orchestrator/execute_tools.rs:741`）+ microcompaction | 按预算裁剪、压缩可复现的旧观测 | 影响模型下一步看到什么 |

界面上看到「…」**不代表**模型看到的是被截断的那份；反之模型上下文被 microcompaction 收成预览时，界面仍可能显示完整内容。判断上下文压力看压缩页与用量页，不看界面折叠。

## 审批分档

审批按工具的风险类别归档，类别由 `ToolKind` 定义（`services/tools/registry.rs:10-19`）：`Read`（只读）｜`Write`（写文件）｜`Exec`（跑命令/脚本）｜`Control`（控制宿主能力）。

- 待批请求登记在 `core/approval_registry.rs`，来源分 `ChatWs` 与 `Channel`。
- 同一会话内「批准一次、短时免批」的缓存 TTL 为 **600 秒**（`core/exec_policy.rs:18`）。
- 执行策略三态 `Allow / Audit / Enforce`（`wunder-core/src/exec_policy.rs:6-10`）；`Enforce` 下高危动作必须有人类批准。
- 前端确认不能替代后端鉴权。

## 常见误解

- **「舰桥勾了就等于智能体会用」** — 不等于。能力是选择层，还要过名字解析、可用性门槛与形态过滤。
- **「`网页搜索` / `ptc` 已经删掉了」** — 没有。是临时下架，代码与注册都还在。
- **「知识库是一个叫 `knowledge` 的工具」** — 已经改成按库名逐个注册。
- **「技能要靠 `技能调用` 才能用」** — 技能名本身就能被直接调用，`技能调用` 是读取正文与做路径替换的那条路。
- **「界面显示截断 = 模型上下文也被截断」** — 两条通道独立，见上表。
- **「工具有个英文名就行」** — 规范名多为中文，别名只用于归一，写配置时两栏都注明。

## 延伸阅读

- [工具体系](/docs/zh-CN/concepts/tools/) —— 分发链路、裁剪常量与协议差异
- [工具总览](/docs/zh-CN/tools/) —— 逐个工具的参数与用法
- [上下文压缩](/docs/zh-CN/concepts/core-context-compression/) —— 模型输入通道怎么省 token
- [提示词与技能](/docs/zh-CN/concepts/prompt-and-skills/) —— `SKILL.md` 与技能注入
- [多用户与能力管理](/docs/zh-CN/concepts/core-multi-user-management/) —— 管理员怎么选能力
- [配置参考](/docs/zh-CN/reference/config/) —— 工具与能力相关配置项
