---
title: WunderBench 模型评测
summary: 使用全量题库自动验证模型在 Wunder 真实智能体链路中的任务完成质量，并为题库扩展、回归检测和模型选型提供依据。
read_when:
  - 用户需要评估模型在 Wunder 中的真实任务完成能力
  - 用户需要导出完整评测记录，复盘模型日志、工具调用和工作区结果
  - 用户准备为 WunderBench 新增或调整评测题目
source_docs:
  - docs/API文档.md
  - config/benchmark/tasks
  - src/ops/benchmark
---

# WunderBench 模型评测

WunderBench 是 Wunder 内置的模型能力评测系统。它不只是向模型提问，而是复用 Wunder 的真实智能体执行链路：准备工作区、下发任务、允许模型调用工具、记录产物、自动评分，并生成可导出的完整评测记录。

它适合回答这些问题：

- 当前模型能不能稳定完成 Wunder 中的真实任务？
- 新模型、新提示词、新工具策略上线前是否有能力退化？
- 哪些任务组最薄弱，应该优先优化模型、工具还是系统链路？
- 一次失败到底是模型理解问题、工具调用问题、工作区准备问题，还是裁判评分问题？

## 入口

在管理端控制台打开 **调试 / WunderBench** 页面。

页面上主要有四类操作：

- **运行全量题库**：默认执行全部可用 WunderBench 题目，不再区分快速、标准和全量档位。
- **选择被测模型**：真正执行任务的模型。
- **选择裁判模型**：只用于 `llm_judge` 和 `hybrid` 题目的主观评分。
- **导出评测记录**：下载包含运行过程、attempt、题目规格和模型日志的 JSON 文件。

也可以通过管理端 API 调用：

| 操作 | API |
|------|-----|
| 查看档位 | `GET /wunder/admin/wunderbench/profiles` |
| 查看任务 | `GET /wunder/admin/wunderbench/tasks` |
| 启动评测 | `POST /wunder/admin/wunderbench/start` |
| 查看运行列表 | `GET /wunder/admin/wunderbench/runs` |
| 查看运行详情 | `GET /wunder/admin/wunderbench/runs/{run_id}` |
| 导出记录 | `GET /wunder/admin/wunderbench/runs/{run_id}/export` |
| 取消运行 | `POST /wunder/admin/wunderbench/runs/{run_id}/cancel` |

## 评测范围

WunderBench 现在只保留一个评测范围：`full`。它会运行全部可用任务，避免同一模型在不同档位下得到难以比较的结果。

| 范围 | 用途 | 选择规则 | 推荐次数 |
|------|------|----------|----------|
| `full` | 模型选型、发布前验证、回归对比 | 运行全部可用任务 | 2 次 |

兼容说明：

- `/wunder/admin/wunderbench/profiles` 只返回 `full`。
- 旧客户端或脚本传入 `quick`、`core`、`standard` 等值时，后端会兼容归一为 `full`。
- 仍可通过 `suite_ids` 或 `task_ids` 手动筛选任务，用于定位单个任务组或复查失败题。

## 被测模型与裁判模型

**被测模型** 是参与任务执行的模型。它会读取题目、调用工具、修改工作区并生成最终产物。

**裁判模型** 是评分辅助模型。它不会替代自动检查，也不会参与任务执行，只在以下题型中使用：

- `llm_judge`：主要依赖裁判模型按 rubric 评分。
- `hybrid`：先运行自动检查，再用裁判模型补充判断表达质量、完整性、推理合理性等自动脚本难以覆盖的维度。

`automated` 题目只依赖脚本检查，不需要裁判模型参与。为了减少偏差，正式对比时建议使用稳定、能力较强、与被测模型相对独立的裁判模型；如果只是本地冒烟，也可以临时使用同一个模型。

## 当前题库覆盖

当前内置题目位于 `config/benchmark/tasks/*.md`，素材目录为 `config/benchmark/assets/`。

内置任务组覆盖以下方向：

| 任务组 | 主要能力 | 当前重点 |
|--------|----------|----------|
| `workspace-core` | 工作区读写、文件盘点、配置修复 | 能否正确读取输入、生成结构化输出、保持工作区边界 |
| `coding-agent` | 代码理解、缺陷修复、命令验证 | 能否定位问题、修改代码、运行检查并说明修复点 |
| `office-workflow` | 办公写作、信息分拣、回复草拟 | 能否理解约束、提炼优先级、生成可用文本 |
| `knowledge-memory` | 多文档对比、摘要、结构化提炼 | 能否抽取变化、识别影响、输出简洁结论 |
| `data-analysis` | CSV 指标计算、阈值识别、结构化洞察 | 能否稳定完成表格聚合、风险标记和摘要输出 |
| `ops-observability` | 日志异常分析、运行手册生成 | 能否解析日志、套用规则并产出排障建议 |
| `devops-workflow` | CI 配置修复、流水线约束维护 | 能否最小修改配置并保持关键步骤 |
| `security-triage` | 依赖风险分级、处置建议 | 能否按策略分配优先级和动作 |

当前题库更偏向 Wunder 的基础智能体能力：文件与工作区、代码修复、办公流程、知识摘要。它已经适合作为早期模型选型和回归基线，但还不是完整的通用模型排行榜。后续应继续补充长上下文、多轮协作、浏览器操作、外部渠道、复杂工具链和失败恢复类题目。

## 阅读结果

运行概览会给出文本化摘要，重点看这些字段：

| 字段 | 含义 |
|------|------|
| `readiness` | 可用性结论：`production_ready`、`usable`、`risky`、`not_ready` |
| `overall_score` | 所有任务的平均得分 |
| `reliability_score` | 通过率口径的稳定性 |
| `tool_success_score` | 工具调用结果成功率 |
| `stability_score` | 完成率和多次运行波动 |
| `efficiency_score` | 执行耗时相关的效率分 |
| `weakest_suites` | 当前最薄弱的任务组 |
| `top_failures` | 最值得优先复查的失败 attempt |

可用性结论的理解方式：

- `production_ready`：总体分、通过率和工具成功率都较高，可进入更严格的发布验证。
- `usable`：可用但仍有明显短板，适合继续观察或限定场景使用。
- `risky`：能力不稳定，需要先复盘失败任务。
- `not_ready`：不适合进入默认模型或生产链路。

不要只看总分。一次 `overall_score` 较高但 `tool_success_score` 低的评测，通常说明模型可能会“答得像对的”，但工具链路或文件操作存在风险。

## Attempt、产物与日志

一次评测运行包含多个 task，每个 task 可以有多个 attempt。attempt 是排查问题的最小单位。

每个 attempt 会记录：

- 使用的任务、模型、裁判模型和耗时。
- 工作区相对路径，例如 `benchmark/{run_id}/{task_id}/attempt_{attempt_no}`。
- 模型 transcript、工具调用、工具结果和最终输出。
- 自动评分明细、裁判评分明细和最终分数。
- 错误信息、产物摘要和 token / 速度统计。

新启动的 WunderBench 会为评测线程启用管理员调试日志。模型 attempt 的 monitor session id 形如：

```text
bench-{run_id}-{task_id}-{attempt_no}
```

裁判模型对应的 monitor session id 形如：

```text
bench-{run_id}-{task_id}-{attempt_no}-judge
```

这些日志会在导出文件中一起带出，便于复盘模型请求、模型输出、工具调用、工具返回、工作区更新和运行性能。

## 导出评测记录

在 WunderBench 页面点击 **导出评测记录**，或调用：

```http
GET /wunder/admin/wunderbench/runs/{run_id}/export
```

导出的 JSON 是一次评测的复盘包，主要包含：

| 字段 | 内容 |
|------|------|
| `run` | 运行基本信息、状态、模型、profile、总分和 scorecard |
| `task_aggregates` | 每个任务的聚合分数、通过率、波动情况和轻量 `attempt_refs` |
| `attempts` | 每次 attempt 的执行结果、评分、产物摘要和 transcript |
| `task_specs` | 当时使用的题目规格，避免题库后续变化导致无法复盘 |
| `attempt_logs` | 对应 monitor 日志与轻量 `attempt_ref`，包含模型和工具链路事件 |
| `diagnostics` | 导出说明、缺失日志提示和兼容性提示 |

建议在这些场景导出：

- 模型评测失败，需要反馈给系统开发者分析。
- 发布前需要留存基线。
- 对比两个模型或两版提示词，需要保存完整证据链。
- 怀疑工作区准备、工具调用或裁判评分存在异常。

## 新增题目

新增题目时，在 `config/benchmark/tasks/` 下创建一个 Markdown 文件。每个题目由 YAML frontmatter 和固定 Markdown 区块组成。

基本结构：

````markdown
---
id: task_sample
name: 示例任务
suite: workspace-core
category: filesystem
grading_type: automated
timeout_seconds: 180
runs_recommended: 2
difficulty: easy
required_tools:
  - read_file
  - write_file
tags:
  - filesystem
languages:
  - zh-CN
workspace_files:
  - path: input/source.txt
    content: |
      sample input
---

## Prompt

请在 `{attempt_root}` 范围内完成任务。

## Expected Behavior

说明模型应该完成什么结果。

## Grading Criteria

- [ ] 关键检查点一
- [ ] 关键检查点二

## Automated Checks

```python
def grade(transcript, workspace_path):
    return {"check_name": 1.0}
```
````

常用 frontmatter 字段：

| 字段 | 说明 |
|------|------|
| `id` | 全局唯一任务 ID，建议使用 `task_` 前缀 |
| `name` | 页面展示名称 |
| `suite` | 任务组，用于手动筛选与弱项聚合 |
| `category` | 任务分类 |
| `grading_type` | `automated`、`llm_judge` 或 `hybrid` |
| `timeout_seconds` | 单次 attempt 超时时间，最低会按 30 秒处理 |
| `runs_recommended` | 推荐重复次数 |
| `difficulty` | `easy`、`medium`、`hard` |
| `required_tools` | 期望模型使用或可能需要的工具 |
| `tags` | 任务标签 |
| `languages` | 题目语言 |
| `workspace_files` | attempt 开始前写入工作区的文件 |

`workspace_files` 支持两种形式：

- `path + content`：直接写入内联文件内容。
- `source + dest`：从 `config/benchmark/assets/` 复制素材到 attempt 工作区。

## 评分方式

WunderBench 支持三种评分：

| 类型 | 适用场景 | 特点 |
|------|----------|------|
| `automated` | 文件是否生成、JSON 是否合法、测试是否通过、结构化结果是否正确 | 稳定、可重复，适合作为主基线 |
| `llm_judge` | 写作质量、摘要质量、推理完整性、表达是否符合约束 | 灵活，但依赖裁判模型稳定性 |
| `hybrid` | 既有可脚本检查的硬指标，也有需要语义判断的软指标 | 推荐用于复杂任务 |

自动评分函数约定：

- 函数名必须是 `grade(transcript, workspace_path)`。
- 返回值是 `{检查项: 0.0~1.0}` 的对象。
- `workspace_path` 指向 attempt 工作区根目录。
- `transcript` 包含本次执行过程，适合检查模型是否调用了关键工具或是否出现异常。
- 检查项越细，失败定位越容易。

裁判评分建议：

- rubric 要写清楚“什么算满分、什么算部分完成、什么必须扣分”。
- 不要把可以脚本判断的内容交给裁判模型。
- 重要发布基线不要频繁更换裁判模型，否则历史分数不可直接比较。

## 题目设计原则

好的 WunderBench 题目应满足：

- **真实**：任务接近 Wunder 用户会让智能体完成的工作。
- **可复现**：输入固定、预期稳定，不依赖外部网络或实时数据。
- **可评分**：至少有一部分结果能被自动脚本验证。
- **边界清晰**：明确要求模型只能在 `{attempt_root}` 范围内读写。
- **失败可定位**：评分项拆得足够细，能区分理解失败、工具失败、格式失败和产物缺失。
- **成本可控**：单题不要过长，避免全量运行被少数大题拖慢。

不建议的题目：

- 依赖当前日期、新闻、外部网页等不稳定输入。
- 只有开放式主观评价，没有硬性检查点。
- 需要人工确认才能继续。
- 对模型隐含要求过多，但 Prompt 没有写清。
- 评分脚本读取 attempt 工作区以外的路径。

## 排障

### 模型说目录为空

优先检查：

- 题目的 `workspace_files` 是否正确声明。
- `source` 素材是否存在于 `config/benchmark/assets/`。
- Prompt 中是否使用了 `{attempt_root}`，并要求模型只在该路径内操作。
- 导出记录里的 `attempts[].artifacts`、`attempts[].transcript` 和 `attempt_logs` 是否显示工作区创建成功。

如果导出记录显示工作区文件已创建，但模型仍说目录为空，通常是模型没有正确使用路径或工具参数；如果导出记录也没有文件，则检查题目素材声明和工作区准备链路。

### 自动评分全是 0

优先检查：

- 产物是否写到了题目要求的路径。
- JSON、Markdown、代码文件是否格式合法。
- 自动评分脚本路径是否与 Prompt 中的产物路径一致。
- 评分脚本是否假设了不存在的外部依赖。

### 裁判分数波动大

优先检查：

- rubric 是否足够具体。
- 是否把可自动判断的硬指标错误地交给裁判模型。
- 裁判模型是否与历史评测保持一致。
- 单题是否需要提高 `runs_recommended`，用多次运行抵消偶发波动。

### 运行卡住或耗时过长

优先检查：

- 单题 `timeout_seconds` 是否合理。
- 模型是否反复调用失败工具。
- 工作区文件是否过大。
- 任务是否要求了不必要的长推理或大范围搜索。

## 与其他观测能力的关系

WunderBench 看“任务完成质量”，不是压测。

| 能力 | 关注点 |
|------|--------|
| 会话监控 | 线上线程当前状态、事件、token 和工具调用 |
| 工具统计 | 哪些工具调用最多、成功率如何 |
| 性能采样 | 单请求链路延迟 |
| 吞吐压测 | 并发承载能力 |
| WunderBench | 模型在真实任务中的完成质量和回归情况 |

## 延伸阅读

- [监控与 WunderBench](/docs/zh-CN/ops/benchmark-and-observability/)
- [管理端面板指南](/docs/zh-CN/reference/admin-panels/)
- [API 文档](/docs/zh-CN/reference/api-index/)
