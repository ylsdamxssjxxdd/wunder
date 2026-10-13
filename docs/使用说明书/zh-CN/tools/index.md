---
title: 工具总览
summary: wunder 的工具按用途分成几类；选型入口、结果形态约定，以及不套统一外壳的几个例外。
updated_at: 2026-10-10
---

# 工具总览

wunder 的工具体系覆盖文件、命令、Web、桌面、协作、系统连接等场景。大多数内置工具的成功和失败结果都收敛到一套统一外壳，少数工具例外。

## 按场景选工具

### 只有关键词，想找网页来源

网页搜索已经移除；手上有明确的 URL 时，直接用 [网页抓取](/docs/zh-CN/tools/web-fetch/) 读正文。

### 只读网页正文

先用 [网页抓取](/docs/zh-CN/tools/web-fetch/)，不要一上来就用浏览器。

### 要点击、输入、等页面动态渲染

用 [浏览器](/docs/zh-CN/tools/browser/)。

### 要看本地代码

通常顺序是：先用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 列目录或搜索，再读片段。

### 要改代码

- 小而精确、带上下文匹配的修改：用 [应用补丁（已并入 文本编辑）](/docs/zh-CN/tools/apply-patch/)
- 一次精确文本替换：用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `文本编辑`
- 复杂替换逻辑：用 [执行命令](/docs/zh-CN/tools/exec/) 跑脚本
- 整文件重写：用 [工作区文件](/docs/zh-CN/tools/workspace-files/) 里的 `write_file`
- 要编译、测试、跑脚本：用 [执行命令](/docs/zh-CN/tools/exec/)

### 要做协作

- 当前会话里临时拉起子运行：用 [子智能体控制](/docs/zh-CN/tools/subagent-control/)
- 管理任务线程 / 分支线程：用 [会话线程控制](/docs/zh-CN/tools/thread-control/)

## 工具分组

### 收尾与前端协同

- [最终回复（已移除）](/docs/zh-CN/tools/final-response/)
- [计划面板 / 问询面板 / 会话让出](/docs/zh-CN/tools/panels-and-a2ui/)
- [会话让出](/docs/zh-CN/tools/sleep/)

### 工作区与代码

- [工作区文件](/docs/zh-CN/tools/workspace-files/)
- [执行命令](/docs/zh-CN/tools/exec/)
- [应用补丁（已并入 文本编辑）](/docs/zh-CN/tools/apply-patch/)
- [技能调用](/docs/zh-CN/tools/skill-call/)
- [读图工具](/docs/zh-CN/tools/read-image/)
- [ptc（已移除）](/docs/zh-CN/tools/ptc/)

### Web 与桌面

- [网页抓取](/docs/zh-CN/tools/web-fetch/)
- [浏览器](/docs/zh-CN/tools/browser/)
- [桌面控制](/docs/zh-CN/tools/desktop-control/)

### 线程与子智能体

- [会话线程控制](/docs/zh-CN/tools/thread-control/)
- [子智能体控制](/docs/zh-CN/tools/subagent-control/)

### 系统连接与记忆

- [自我状态](/docs/zh-CN/tools/self-status/)
- [记忆管理](/docs/zh-CN/tools/memory-manager/)
- [用户世界工具](/docs/zh-CN/tools/user-world/)
- [渠道工具（已移除）](/docs/zh-CN/tools/channel/)
- [A2A 工具（已移除）](/docs/zh-CN/tools/a2a-tools/)
- [定时任务](/docs/zh-CN/tools/schedule-task/)

## 结果怎么读

大多数内置工具成功时返回一套统一外壳：是否成功、动作名、状态、一句摘要，以及承载业务结果的数据区。业务结果优先在数据区里取。部分工具还会再补一句“下一步该怎么做”。

常见的状态包括：已完成、仅预演、已接收、运行中、已让出、等待输入、部分完成、超时、无改动。超时不一定代表彻底失败。

失败时也走统一外壳：一句错误、可读的原因说明和修复提示，以及能帮助定位的数据。

几个不套统一外壳的例外：

- **最终回复**（已移除）：只是终结信号，只回一条给用户的答案。
- **`a2ui`**：向前端发结构化界面指令，返回是薄包装。
- **定时任务**：返回压缩后的调度结果。
- **浏览器工具组**：结果采用内容块结构（见 [浏览器](/docs/zh-CN/tools/browser/)）。
- **网页搜索**（已移除）：曾用于关键词搜公开网页，结果在数据区的 `results` 里；现在要正文直接用网页抓取。
- **网页抓取**：成功时直接返回抓取结果对象。
- **文本编辑**（旧名 `apply_patch`）：成功走统一外壳；补丁形态失败时，错误语义比普通文件工具更严格。

## 延伸阅读

- 文件和代码的读写：[工作区文件](/docs/zh-CN/tools/workspace-files/)
- 子智能体的派生与回收：[子智能体控制](/docs/zh-CN/tools/subagent-control/)
- 单个工具的返回：见对应工具页
