---
title: 工具总览
summary: 内置工具的规范名与英文别名、八个分类对应的说明页、场景到工具的对照，以及统一结果外壳与不套外壳的例外。
updated_at: 2026-10-16
---

# 工具总览

工具规范名大多为中文，另配英文别名，由 `resolve_tool_name` 归一（`crates/wunder-runtime/src/services/tools/catalog.rs`）。MCP 工具（`server@tool`）、技能名、知识库名与用户工具别名按配置动态注册，不在下表之列。

## 工具矩阵

| 工具（规范名） | 英文别名 | 简介 | 分类 |
| :--- | :--- | :--- | :--- |
| 计划面板 | `update_plan` | 向前端推送结构化的任务计划面板 | 收尾与前端协同 |
| 问询面板 | `question_panel` | 弹出结构化选项，向用户提问并等待选择 | 收尾与前端协同 |
| 会话让出 | `sessions_yield` | 主动让出当前回合控制权，稍后再被唤起 | 收尾与前端协同 |
| 列出文件 | `list_files` | 列出目录内容 | 工作区与代码 |
| glob | — | 按通配模式查找文件路径 | 工作区与代码 |
| 搜索内容 | `search_content` | 在文件中按正则搜索匹配内容 | 工作区与代码 |
| 读取文件 | `read_file` | 读取文件内容，支持分段读取 | 工作区与代码 |
| 写入文件 | `write_file` | 写入或整体覆盖文件 | 工作区与代码 |
| 文本编辑 | `edit` / `edit_file` | 精确替换 / 补丁 / 子命令三形态（`apply_patch` 已并入） | 工作区与代码 |
| 执行命令 | `execute_command` | 执行 Shell / 系统命令 | 工作区与代码 |
| 命令会话 | `command_session` | 管理后台长驻命令会话，支持前后台切换 | 工作区与代码 |
| 技能调用 | `skill_call` | 读取技能正文并执行技能脚本 | 工作区与代码 |
| 网页抓取 | `web_fetch` | 抓取并解析网页正文（需启用配置） | Web |
| 浏览器 | `browser` | 聚合展示名；底层为 17 个 `browser_*` 原生工具（点击、输入、等待、截图等） | Web |
| 读图工具 | `read_image` | 读取图片内容供模型理解（需 vision 能力） | 多模态 |
| 图像生成 | `generate_image` | 按提示词生成图片 | 多模态 |
| 视频生成 | `generate_video` | 按提示词生成视频 | 多模态 |
| 语音生成 | `generate_speech` | 文本转语音 | 多模态 |
| 声转文 | `transcribe_speech` | 语音转文字 | 多模态 |
| 桌面控制 | `desktop_control` | 操作本机桌面并观察（鼠标、键盘、窗口） | 桌面 |
| 子智能体控制 | `subagent_control` | 在任务中临时派生子智能体，结果回传后回收 | 线程与子智能体 |
| 会话线程控制 | `thread_control` | 管理任务线程 / 分支线程（桌面端不开放） | 线程与子智能体 |
| 自我状态 | `self_status` | 查询智能体自身的运行状态 | 系统与记忆 |
| 记忆管理 | `memory_manager` | 读写长期记忆 | 系统与记忆 |
| 定时任务 | `schedule_task` | 创建与管理定时 / 周期任务 | 系统与记忆 |
| get_goal / create_goal / update_goal | — | 目标态三件套，由运行时按会话自动注入，不对桌面端开放选择 | 目标态 |

`网页搜索` 与 `ptc` 属代码保留、当前**临时下架**（对模型与用户不可见，见 `default_tool_profile.rs`），不是已移除。已彻底移除：渠道工具、用户世界工具、`a2a_*` 工具、`final_response`、`a2ui`、LSP 查询类工具。

## 场景与工具

| 场景 | 用什么 |
| --- | --- |
| 手上有明确 URL，要读这一页正文 | [网页抓取](/docs/zh-CN/tools/web-fetch/) |
| 只有关键词，要找公开网页来源 | 网页搜索当前临时下架；拿到 URL 后再走网页抓取 |
| 要点击、输入、等页面动态渲染 | [浏览器](/docs/zh-CN/tools/browser/) |
| 看本地代码 | 先用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 列目录或搜索，再读片段 |
| 小而精确、带上下文匹配的改代码 | `文本编辑` 的补丁形态 |
| 一次精确文本替换 | [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `文本编辑` |
| 复杂替换逻辑 | [执行命令](/docs/zh-CN/tools/exec/) 跑脚本 |
| 整份重写或新建文件 | [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `write_file` |
| 编译、测试、跑脚本 | [执行命令](/docs/zh-CN/tools/exec/) |
| 当前会话里临时拉起子运行 | [子智能体控制](/docs/zh-CN/tools/subagent-control/) |
| 管理任务线程 / 分支线程 | [会话线程控制](/docs/zh-CN/tools/thread-control/) |

## 分类与文档

| 分类 | 说明页 |
| --- | --- |
| 收尾与前端协同 | 计划面板 / 问询面板 / 会话让出：[计划面板 / 问询面板 / 会话让出](/docs/zh-CN/tools/panels/) |
| 工作区与代码 | 列出文件 / glob / 搜索内容 / 读取文件 / 写入文件 / 文本编辑：[工作区文件](/docs/zh-CN/tools/workspace-files/)；执行命令 / 命令会话：[执行命令](/docs/zh-CN/tools/exec/)；技能调用：[技能调用](/docs/zh-CN/tools/skill-call/) |
| Web | 网页抓取：[网页抓取](/docs/zh-CN/tools/web-fetch/)；浏览器：[浏览器工具族](/docs/zh-CN/tools/browser/) |
| 多模态 | 读图工具：[读图工具](/docs/zh-CN/tools/read-image/)；图像生成 / 视频生成 / 语音生成 / 声转文：[多模态生成](/docs/zh-CN/tools/multimodal-generation/) |
| 桌面 | 桌面控制：[桌面控制](/docs/zh-CN/tools/desktop-control/) |
| 线程与子智能体 | 子智能体控制：[子智能体控制](/docs/zh-CN/tools/subagent-control/)；会话线程控制：[会话线程控制](/docs/zh-CN/tools/thread-control/) |
| 系统与记忆 | 自我状态：[自我状态](/docs/zh-CN/tools/self-status/)；记忆管理：[记忆管理](/docs/zh-CN/tools/memory-manager/)；定时任务：[定时任务](/docs/zh-CN/tools/schedule-task/) |
| 目标态 | `get_goal` / `create_goal` / `update_goal` 由运行时按会话自动注入，不对桌面端开放选择 |

## 结果外壳

大多数内置工具成功时返回同一套外壳：是否成功、动作名、状态、一句摘要，以及承载业务结果的数据区。业务结果优先在数据区里取。部分工具还会再补一句“下一步该怎么做”。

常见的状态包括：已完成、仅预演、已接收、运行中、已让出、等待输入、部分完成、超时、无改动。超时不一定代表彻底失败。

失败时也走统一外壳：一句错误、可读的原因说明和修复提示，以及能帮助定位的数据。

不套统一外壳的例外：

- **最终回复**（已移除）：只是终结信号，只回一条给用户的答案。
- **定时任务**：返回压缩后的调度结果。
- **浏览器工具组**：结果采用内容块结构（见 [浏览器](/docs/zh-CN/tools/browser/)）。
- **网页搜索**（临时下架）：曾用于关键词搜公开网页，结果在数据区的 `results` 里；现在要正文直接用网页抓取。
- **网页抓取**：成功时直接返回抓取结果对象。
- **文本编辑**（旧名 `apply_patch`）：成功走统一外壳；补丁形态失败时，错误语义比普通文件工具更严格。
