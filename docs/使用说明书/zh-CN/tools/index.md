---
title: 工具总览
summary: 工具页解决的是「现在该用哪个工具」，而不是重复概念解释；按任务分组可最快命中正确工具链。
read_when:
  - 你要查某个工具的动作、参数和适用场景
  - 你想按任务选择最短工具链
source_docs:
  - docs/API文档.md
  - docs/设计方案.md
  - docs/系统介绍.md
  - config/wunder-example.yaml
  - src/services/tools/catalog.rs
---

# 工具总览

把这组文档当成「**任务选型手册**」更准确。

---

## 实际实现的内置工具

根据源码 `src/services/tools/catalog.rs`，wunder 目前实际实现了以下 **29 个内置工具**：

### 1. 基础控制工具

| 工具名 | 说明 | 适用场景 |
|--------|------|----------|
| [最终回复](/docs/zh-CN/tools/final-response/) | 输出最终答案 | 结束当前轮次，给出结论 |
| [a2ui](/docs/zh-CN/tools/panels-and-a2ui/) | 界面协同工具 | 与用户交互，展示 UI 组件 |
| [计划面板](/docs/zh-CN/tools/panels-and-a2ui/) | 展示执行计划 | 分步展示任务计划和进度 |
| [问询面板](/docs/zh-CN/tools/panels-and-a2ui/) | 向用户提问 | 获取用户选择或输入 |
| [定时任务](/docs/zh-CN/tools/schedule-task/) | 延迟/周期执行 | 定时巡检、周期提醒 |
| [休眠等待](/docs/zh-CN/tools/sleep/) | 同步等待 | 等待外部条件就绪 |

---

### 2. 文件与代码工具

| 工具名 | 说明 | 适用场景 |
|--------|------|----------|
| [列出文件](/docs/zh-CN/tools/workspace-files/) | 浏览目录结构 | 查看工作区文件 |
| [搜索内容](/docs/zh-CN/tools/workspace-files/) | 在文件中搜索 | 查找代码、文本 |
| [读取文件](/docs/zh-CN/tools/workspace-files/) | 读取文件内容 | 查看代码、文档 |
| [写入文件](/docs/zh-CN/tools/workspace-files/) | 创建/覆盖文件 | 新建文件、全量替换 |
| [应用补丁](/docs/zh-CN/tools/apply-patch/) | 结构化多文件改动 | 精确编辑代码 |
| [LSP查询](/docs/zh-CN/tools/lsp/) | 代码符号查询 | 找定义、引用、调用链 |
| [执行命令](/docs/zh-CN/tools/exec/) | 运行 shell 命令 | 编译、测试、脚本 |
| [ptc](/docs/zh-CN/tools/ptc/) | Python 程序化执行 | 复杂 Python 逻辑、图表生成 |
| [读图工具](/docs/zh-CN/tools/read-image/) | 分析本地图片 | 多模态理解图片内容 |

---

### 3. 网页与桌面工具

| 工具名 | 说明 | 适用场景 |
|--------|------|----------|
| [网页抓取](/docs/zh-CN/tools/web-fetch/) | 提取网页正文 | 读新闻、文档、博客 |
| [浏览器](/docs/zh-CN/tools/browser/) | 网页自动化 | 点击、输入、截图、导航 |
| [桌面控制器](/docs/zh-CN/tools/desktop-control/) | 桌面操作 | 鼠标、键盘、窗口控制 |
| [桌面监视器](/docs/zh-CN/tools/desktop-control/) | 观察桌面变化 | 截图、监视屏幕 |

---

### 4. 协作与编排工具

| 工具名 | 说明 | 适用场景 |
|--------|------|----------|
| [会话线程控制](/docs/zh-CN/tools/thread-control/) | 管理会话线程 | 分叉、切换、归档会话 |
| [子智能体控制](/docs/zh-CN/tools/subagent-control/) | 派生子智能体 | 分工协作、任务分解 |
| [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) | 多智能体并发 | 并行派发、结果聚合 |
| [记忆管理](/docs/zh-CN/tools/memory-manager/) | 管理长期记忆 | 增删改查记忆片段 |
| [技能调用](/docs/zh-CN/tools/skill-call/) | 调用固化流程 | 复用 SKILL.md 定义的流程 |

---

### 5. 系统桥接工具

| 工具名 | 说明 | 适用场景 |
|--------|------|----------|
| [用户世界工具](/docs/zh-CN/tools/user-world/) | 站内用户通信 | 列出用户、发送消息 |
| [渠道工具](/docs/zh-CN/tools/channel/) | 外部渠道发信 | 飞书、微信、QQ、XMPP |
| [a2a观察](/docs/zh-CN/tools/a2a-tools/) | 观察远程任务 | 查看 A2A 任务状态 |
| [a2a等待](/docs/zh-CN/tools/a2a-tools/) | 等待远程任务 | 等待 A2A 任务完成 |
| [节点调用](/docs/zh-CN/tools/node-invoke/) | 调用分布式节点 | 列节点、下发命令 |

---

## 先做选型：按任务找工具

| 你要做什么 | 首选工具 | 备选工具 |
|------------|----------|----------|
| 读网页正文 | [网页抓取](/docs/zh-CN/tools/web-fetch/) | - |
| 操作网页（点击、输入） | [浏览器](/docs/zh-CN/tools/browser/) | - |
| 读取、搜索文件 | [列出文件](/docs/zh-CN/tools/workspace-files/) + [搜索内容](/docs/zh-CN/tools/workspace-files/) | - |
| 编辑代码、多文件改动 | [应用补丁](/docs/zh-CN/tools/apply-patch/) | [写入文件](/docs/zh-CN/tools/workspace-files/) |
| 执行命令、脚本 | [执行命令](/docs/zh-CN/tools/exec/) | - |
| Python 程序化执行 | [ptc](/docs/zh-CN/tools/ptc/) | - |
| 分析图片 | [读图工具](/docs/zh-CN/tools/read-image/) | - |
| 代码符号查询 | [LSP查询](/docs/zh-CN/tools/lsp/) | - |
| 固化流程、复用步骤 | [技能调用](/docs/zh-CN/tools/skill-call/) | - |
| 管理多个会话 | [会话线程控制](/docs/zh-CN/tools/thread-control/) | - |
| 派生子智能体 | [子智能体控制](/docs/zh-CN/tools/subagent-control/) | - |
| 多智能体并行协作 | [智能体蜂群](/docs/zh-CN/tools/agent-swarm/) | - |
| 管理长期记忆 | [记忆管理](/docs/zh-CN/tools/memory-manager/) | - |
| UI 交互、计划面板 | [a2ui](/docs/zh-CN/tools/panels-and-a2ui/) + [计划面板](/docs/zh-CN/tools/panels-and-a2ui/) + [问询面板](/docs/zh-CN/tools/panels-and-a2ui/) | - |
| 定时、延迟执行 | [定时任务](/docs/zh-CN/tools/schedule-task/) | [休眠等待](/docs/zh-CN/tools/sleep/) |
| 操作桌面、窗口 | [桌面控制器](/docs/zh-CN/tools/desktop-control/) + [桌面监视器](/docs/zh-CN/tools/desktop-control/) | - |
| 发送渠道消息 | [渠道工具](/docs/zh-CN/tools/channel/) | - |
| 系统内用户通信 | [用户世界工具](/docs/zh-CN/tools/user-world/) | - |
| 调用远程智能体 | [a2a观察](/docs/zh-CN/tools/a2a-tools/) + [a2a等待](/docs/zh-CN/tools/a2a-tools/) | - |

---

## 工具生态四层模型

```
┌─────────────────────────────────────┐
│   第四层：Skills（固化流程）       │
│   - 可复用的步骤组合              │
│   - SKILL.md 定义                  │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   第三层：MCP（外部工具）          │
│   - 第三方服务接入                 │
│   - 标准 MCP 协议                  │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   第二层：内置工具（核心能力）     │
│   - 文件、命令、网页、桌面         │
│   - 会话、协作、记忆               │
└─────────────────────────────────────┘
              ↑
┌─────────────────────────────────────┐
│   第一层：模型调度（Orchestrator） │
│   - 工具选择、结果归并             │
│   - 流式执行、错误处理             │
└─────────────────────────────────────┘
```

---

## 关键工具特性

### 工具结果防爆机制

- 执行层先按预算裁剪
- MCP observation 入模前二次压缩
- 返回 `truncated/continuation_required` 信号
- 驱动模型自动分页或缩小范围

### 原子写入策略

- `写入文件` 与 `应用补丁` 采用临时文件 + rename
- 降低异常中断与并发覆盖风险
- 支持回滚保护

### dry_run 预演

- `执行命令`、`写入文件`、`应用补丁` 支持预演
- 先返回计划或摘要，再决定是否执行
- 降低误操作风险

### 预算控制

- `time_budget_ms`：时间预算
- `output_budget_bytes`：输出预算
- `max_commands`：命令数量限制
- 预算回传到结果中

### 工具别名机制

很多工具支持多个别名，例如：
- `final_response` → `最终回复`
- `list_files` → `列出文件`
- `read_file` → `读取文件`
- `write_file` → `写入文件`
- `search_content` → `搜索内容`
- `execute_command` → `执行命令`
- `programmatic_tool_call` → `ptc`
- 等等...

---

## 工具可见性规则

部分工具的可见性受运行形态和配置影响：

| 工具 | Desktop | Server | CLI | 配置要求 |
|------|---------|--------|-----|----------|
| 浏览器工具 | ✅ | ❌ | ❌ | Desktop 模式 |
| 桌面控制器 | ✅ | ❌ | ❌ | Desktop 模式 |
| 桌面监视器 | ✅ | ❌ | ❌ | Desktop 模式 |
| 其他工具 | ✅ | ✅ | ✅ | 无 |

---

## 常见误区澄清

| 误区 | 正确理解 |
|------|----------|
| `web_fetch` 是浏览器精简版 | ❌ 是独立链路，专门用于读网页正文 |
| 工具对所有智能体都可见 | ❌ 受运行形态、智能体挂载、参数影响 |
| `休眠等待` = 定时任务 | ❌ `休眠等待` 是同步等待，定时任务是异步调度 |
| 工具越多越好 | ❌ 优先用内置工具，必要时才扩展 MCP/Skills |
| 所有工具都在所有形态可用 | ❌ 浏览器、桌面工具仅 Desktop 模式可用 |

---

## 下一步

- 想了解工具原理？→ [工具体系](/docs/zh-CN/concepts/tools/)
- 想扩展工具？→ [MCP 入口](/docs/zh-CN/integration/mcp-endpoint/)
- 想看 API？→ [API 索引](/docs/zh-CN/reference/api-index/)
