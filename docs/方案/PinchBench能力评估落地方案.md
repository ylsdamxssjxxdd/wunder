# PinchBench 能力评估落地方案

## 1. 文档目标
- 目标：将 wunder 当前“题库式能力评估”直接替换为对齐 PinchBench 方法论的“真实任务基准评估（benchmark）”体系。
- 本方案默认**不考虑旧评估链路兼容性**，实施时允许直接删除旧的 `evaluation` 用例、接口、页面与存储结构。
- 评估对象不再是“答对几道题”，而是“智能体是否能在真实工作区中完成任务，并以可复核证据证明完成质量”。

## 2. 对齐原则

### 2.1 与 PinchBench 对齐的核心点
- 任务以**真实任务**为中心，而不是选择题、常识题或简单 checker 题。
- 每个任务拥有**独立工作区**、独立素材、独立 transcript、独立评分结果。
- 评分支持 `automated` / `llm_judge` / `hybrid` 三种模式。
- 支持**多次运行**，统计 `mean/std/min/max`，避免单次运气影响结果。
- 同时评估**结果质量、过程质量、稳定性、效率**，而不是只看一个总分。
- 报告必须可追溯：任务提示、工作区产物、工具调用轨迹、最终输出、评分拆解都能回看。

### 2.2 wunder 侧补充原则
- 后端保持薄 API + 清晰模块边界，避免继续堆入单文件。
- 评估运行优先复用现有 `/wunder` 主链路，保证与真实智能体行为一致。
- 评分子系统要可隔离、可超时、可审计，避免任意脚本直接污染主进程。
- token 指标记录遵循 wunder 约束：优先记录**上下文占用量**，并与账单成本拆开统计。
- 管理端页面以 `web/` 为落点，不在 `frontend/` 增加评估实现。

## 3. 替换策略
- 旧 `evaluation` 模块视为一次性替换对象，不保留兼容层。
- 旧评估用例目录 `config/evaluation/cases` 在新方案落地后删除。
- 旧后端模块 `src/api/evaluation.rs`、`src/ops/evaluation.rs`、`src/ops/evaluation_runner.rs` 在切换完成后删除。
- 旧表 `evaluation_runs` / `evaluation_items` 不做迁移，直接用新的 benchmark 表替代。
- 管理端“能力评估”导航保留文案，但页面内容直接替换为 PinchBench 风格的 benchmark 中心。

## 4. 总体架构

### 4.1 领域模型
- `BenchmarkSuite`：任务套件，例如 `workspace-core`、`coding-agent`、`office-workflow`。
- `BenchmarkTask`：单个真实任务定义。
- `BenchmarkRun`：一次套件执行。
- `BenchmarkAttempt`：某任务的某次尝试（用于 `runs_per_task > 1`）。
- `BenchmarkTaskAggregate`：某任务在单次 run 中的聚合结果（`mean/std/min/max`）。
- `BenchmarkReport`：整次 run 的汇总报告与效率统计。

### 4.2 目录落点
- 任务定义：`config/benchmark/tasks/`
- 任务素材：`config/benchmark/assets/`
- 后端 API：`src/api/benchmark.rs`
- 后端执行器：`src/ops/benchmark/`
- 管理端页面逻辑：`web/modules/benchmark/`
- 管理端样式：`web/styles/benchmark.css`
- 测试：`tests/benchmark/`

### 4.3 后端模块拆分
- `src/ops/benchmark/spec.rs`：任务 frontmatter 与 markdown section 结构定义。
- `src/ops/benchmark/loader.rs`：扫描、解析、校验任务文件。
- `src/ops/benchmark/workspace.rs`：准备 task workspace、复制 fixtures、产物清单生成。
- `src/ops/benchmark/executor.rs`：调用 `/wunder` 主链路并采集 stream/transcript。
- `src/ops/benchmark/transcript.rs`：规范化事件、工具调用、最终输出、usage 统计。
- `src/ops/benchmark/grader_auto.rs`：自动评分执行器。
- `src/ops/benchmark/grader_judge.rs`：LLM judge 评分器。
- `src/ops/benchmark/aggregate.rs`：attempt/task/run 聚合与效率计算。
- `src/ops/benchmark/manager.rs`：运行编排、取消、SSE 推送、状态机。
- `src/ops/benchmark/models.rs`：对外 JSON 结构与存储 payload 结构。

## 5. 任务规范设计

### 5.1 任务文件格式
- 直接对齐 PinchBench：使用 **Markdown + YAML frontmatter**。
- 一个任务一个文件，命名建议：`task_XX_slug.md`。
- frontmatter 管元信息；正文 section 管 prompt、期望行为、评分规则与 rubric。

### 5.2 frontmatter 字段
- `id`：任务唯一 ID。
- `name`：展示名称。
- `suite`：所属套件。
- `category`：任务类别，如 `workspace` / `coding` / `knowledge` / `office`。
- `grading_type`：`automated | llm_judge | hybrid`。
- `timeout_seconds`：单次任务执行超时。
- `runs_recommended`：建议重复次数，默认 3。
- `grading_weights`：混合评分权重，如 `{ automated: 0.4, llm_judge: 0.6 }`。
- `workspace_files`：预置素材列表；支持 inline content 和 asset copy。
- `difficulty`：`easy | medium | hard`。
- `required_tools`：建议或必须可用的工具集合。
- `tags`：检索标签。
- `languages`：支持语言列表。

### 5.3 正文 section 约定
- `## Prompt`：真正发给智能体的任务提示。
- `## Expected Behavior`：任务完成的合理路径、接受标准、可接受替代方案。
- `## Grading Criteria`：细粒度 checklist，用于 automated 和 judge 共同参考。
- `## Automated Checks`：Python 评分函数。
- `## LLM Judge Rubric`：面向 judge 模型的质量 rubric。
- `## Additional Notes`：可选，记录任务作者说明。

### 5.4 标准任务模板
```md
---
id: task_01_workspace_repair
name: Workspace Repair
suite: workspace-core
category: workspace
grading_type: hybrid
timeout_seconds: 180
runs_recommended: 3
grading_weights:
  automated: 0.5
  llm_judge: 0.5
workspace_files:
  - source: assets/workspace/broken_config.json
    dest: input/broken_config.json
  - path: input/README.txt
    content: |
      这是任务说明。
difficulty: medium
required_tools:
  - 读文件
  - 写文件
  - 替换内容
tags:
  - workspace
  - repair
languages:
  - zh-CN
---

## Prompt

修复 `input/broken_config.json` 中的配置问题，并将修复说明写入 `result.md`。

## Expected Behavior

1. 读取输入文件。
2. 识别错误字段并修复。
3. 输出结果文件与说明。

## Grading Criteria

- [ ] 修复后的 JSON 可以被解析
- [ ] 必要字段齐全
- [ ] `result.md` 已生成
- [ ] 修复说明包含原因与修改项

## Automated Checks

```python
def grade(transcript: list, workspace_path: str) -> dict:
    from pathlib import Path
    import json
    scores = {}
    workspace = Path(workspace_path)
    fixed = workspace / "input" / "broken_config.json"
    report = workspace / "result.md"
    try:
        payload = json.loads(fixed.read_text(encoding="utf-8"))
        scores["json_valid"] = 1.0
        scores["required_fields"] = 1.0 if all(k in payload for k in ["name", "version"]) else 0.0
    except Exception:
        scores["json_valid"] = 0.0
        scores["required_fields"] = 0.0
    scores["report_created"] = 1.0 if report.exists() else 0.0
    scores["report_has_reason"] = 1.0 if report.exists() and "修改" in report.read_text(encoding="utf-8") else 0.0
    return scores
```

## LLM Judge Rubric

- 是否高效完成任务，避免无关操作
- 是否准确理解错误原因
- 是否给出清晰、可执行的修复说明
```

### 5.5 `workspace_files` 支持的两种模式
- 资产复制：
  - `source`: `config/benchmark/assets/...`
  - `dest`: 复制到任务工作区的相对路径
- 内联写入：
  - `path`: 目标路径
  - `content`: 文本内容

### 5.6 任务编写约束
- 一个任务只验证一个真实目标，不混入过多互不相关目标。
- automated checks 必须可重跑、无随机副作用。
- judge rubric 必须围绕“完成质量”而不是主观偏好。
- 任务优先依赖 wunder 内置工具链，避免强依赖外部不稳定环境。
- 任务 prompt 禁止显式提示“请输出工具调用 JSON”，要尽量保持真实用户表达。

## 6. 执行链路设计

### 6.1 Run 级执行流程
1. 创建 `BenchmarkRun`，写入 `status=queued`。
2. 解析 suite 与 task 列表，校验任务文件合法性。
3. 为 run 生成运行目录根：`workspace/<user_id>/benchmark/<run_id>/`。
4. 对每个 task 按 `runs_per_task` 生成多个 attempt。
5. 每个 attempt 独立准备 workspace、发起 `/wunder` 请求、采集结果、执行评分。
6. 每个 task 的所有 attempt 完成后，聚合出 task 级 `mean/std/min/max`。
7. 所有 task 完成后，聚合出 run 级总分、分项指标、效率指标。
8. 推送 `benchmark_finished` SSE 事件，并持久化最终报告。

### 6.2 Attempt 级执行流程
1. 清理 attempt workspace。
2. 根据 `workspace_files` 准备输入素材。
3. 生成 `session_id`，以真实 `/wunder` 请求执行任务。
4. 持续监听流式事件，收集：
   - 模型中间输出
   - 工具调用参数
   - 工具结果摘要
   - 最终回答
   - error/cancel 信息
5. 生成 transcript summary 与 artifact manifest。
6. 执行自动评分。
7. 若任务配置需要 judge，则构造 judge prompt 并执行 judge scoring。
8. 合成最终得分并写回 attempt。

### 6.3 Transcript 采集范围
- `user_prompt`
- `llm_output` 增量与最终 `final_answer`
- `tool_call`：工具名、参数、时间戳、耗时
- `tool_result`：结果摘要、截断后的预览、错误信息
- `error`：错误码、错误消息、detail
- `usage`：上下文 token 占用、输入输出 token、请求次数、账单成本（若可得）
- `turns`：用户轮次、模型轮次、工具轮次

### 6.4 Artifact Manifest
- 记录 attempt 工作区中所有新增/修改文件：
  - 相对路径
  - 文件大小
  - SHA-256
  - 文本摘要/前 500 字预览
  - MIME/是否可预览
- 报告层默认只展示白名单文件摘要，避免大文件拖垮页面。

## 7. 评分设计

### 7.1 自动评分（Automated Checks）
- 直接对齐 PinchBench：任务文件内写 Python `grade(transcript, workspace_path) -> dict`。
- 每个返回项是 `0.0 ~ 1.0` 分值，支持部分得分。
- 最终自动评分取所有 criterion 的平均值；后续可升级为加权平均。

### 7.2 自动评分执行隔离
- Python 评分代码不在 Rust 主进程中执行。
- 采用独立 Python 子进程或 sandbox service 执行，输入为：
  - `transcript.json`
  - `workspace_path`
  - `task metadata`
- 执行限制：
  - 禁网
  - CPU/内存/超时限制
  - 工作目录固定到 attempt workspace
  - 只允许访问评估工作区与临时目录
- 返回标准 JSON：`scores / notes / error`。

### 7.3 LLM Judge
- judge 输入严格参照 PinchBench：
  - Task Prompt
  - Expected Behavior
  - Grading Criteria
  - Transcript Summary
  - Artifact Summary
  - Rubric
- judge 输出只允许 JSON：
  - `scores`
  - `total`
  - `notes`
- judge prompt 中明确要求：
  - 不允许调用工具
  - 不允许写文件
  - 只输出 JSON
  - 以 0.6~0.7 作为一般可接受完成质量基线

### 7.4 Hybrid 评分
- `hybrid` 模式最终分数：
  - `final_score = (automated_score * automated_weight + judge_score * judge_weight) / total_weight`
- 默认权重建议：
  - `automated: 0.4`
  - `llm_judge: 0.6`
- 对产物强约束任务（如文件、代码修复）可反过来设为 `0.7 / 0.3`。

### 7.5 稳定性与效率指标
- `mean_score`：同 task 多次运行平均分
- `std_score`：同 task 多次运行标准差
- `pass_rate`：达到阈值（如 `>= 0.8`）的比例
- `total_elapsed_s`
- `context_tokens_total`
- `score_per_1k_context_tokens`
- `score_per_minute`
- `tool_calls_per_success`

## 8. 存储设计

### 8.1 表设计
- `benchmark_runs`
  - `run_id`
  - `user_id`
  - `suite_ids_json`
  - `status`
  - `model_name`
  - `judge_model_name`
  - `runs_per_task`
  - `task_count`
  - `attempt_count`
  - `summary_payload`
  - `started_time`
  - `finished_time`

- `benchmark_attempts`
  - `id`
  - `run_id`
  - `task_id`
  - `attempt_no`
  - `status`
  - `execution_payload`
  - `grading_payload`
  - `usage_payload`
  - `started_time`
  - `finished_time`

- `benchmark_task_aggregates`
  - `id`
  - `run_id`
  - `task_id`
  - `aggregate_payload`

### 8.2 payload 结构原则
- 采用强结构字段 + JSON payload 混合模式。
- 列字段用于筛选、排序、统计；payload 用于承载 transcript 摘要、artifact 清单、评分明细。
- PostgreSQL 与 SQLite 统一保留同样的逻辑字段，避免两端行为分叉。

## 9. API 设计

### 9.1 路由前缀
- 新接口统一使用 `/wunder/admin/benchmark/*`。
- 管理端页面保留“能力评估”文案，但实际调用 benchmark API。

### 9.2 核心接口
- `GET /wunder/admin/benchmark/suites`
  - 返回 suite 列表、任务数、类别分布、推荐运行次数。

- `GET /wunder/admin/benchmark/tasks`
  - 支持按 `suite/category/grading_type/tag` 过滤。

- `POST /wunder/admin/benchmark/start`
  - 入参：
    - `user_id`
    - `model_name`
    - `judge_model_name`
    - `suite_ids`
    - `task_ids`
    - `runs_per_task`
    - `capture_artifacts`
    - `capture_transcript`
    - `config_overrides`

- `POST /wunder/admin/benchmark/runs/{run_id}/cancel`

- `GET /wunder/admin/benchmark/runs`

- `GET /wunder/admin/benchmark/runs/{run_id}`

- `GET /wunder/admin/benchmark/runs/{run_id}/stream`
  - SSE 事件：
    - `benchmark_started`
    - `task_attempt_started`
    - `task_attempt_progress`
    - `task_attempt_finished`
    - `task_aggregated`
    - `benchmark_finished`
    - `benchmark_log`

### 9.3 详情接口返回重点
- run 基础信息
- task aggregate 列表
- attempt 详情
- automated / judge / hybrid 分数拆解
- notes / rubric 命中情况
- transcript summary
- artifact manifest
- usage & efficiency

## 10. 管理端页面设计

### 10.1 页面目标
- 从“开始评估 + 历史记录”升级为“benchmark 控制台”。
- 既能发起执行，也能像 PinchBench 一样看 task 级拆解、attempt 级细节、效率与稳定性。

### 10.2 页面布局
- 左侧：
  - suite 选择
  - task 过滤
  - model / judge model 选择
  - `runs_per_task`
  - 启动按钮

- 右侧：
  - 总体摘要卡片
  - task 分数表
  - 稳定性图表
  - 效率指标卡片
  - 历史 run 列表

### 10.3 task 详情抽屉
- 任务 prompt
- expected behavior
- automated criteria 明细
- judge rubric 与 notes
- attempt 列表
- artifact 预览
- transcript summary
- 工具调用时间线

## 11. 套件规划

### 11.1 第一批套件
- `workspace-core`
  - 文件读取、搜索、编辑、结构化输出、修复类任务
- `coding-agent`
  - 小型代码修复、配置修改、脚本生成、测试修复类任务
- `knowledge-memory`
  - 知识检索、摘要归纳、长期记忆召回、跨文档综合类任务
- `office-workflow`
  - 通知撰写、材料汇总、表格/文档整理、流程判断类任务

### 11.2 第一阶段任务数量建议
- 每个 suite 先做 5 个任务，共 20 个任务。
- 第一批任务构成建议：
  - `automated`：8 个
  - `llm_judge`：4 个
  - `hybrid`：8 个

### 11.3 任务入选标准
- 能代表真实智能体工作。
- 能在隔离工作区内复现。
- 可以沉淀可复核证据。
- 不依赖频繁变化的外部网页结果。
- 不要求接入真实第三方生产账号。

## 12. 实施节点（Milestones）

### M0：方案冻结与任务模板确定（0.5 ~ 1 天）
- 产出：
  - 本文档评审通过
  - 任务 markdown 模板冻结
  - 评分 JSON 协议冻结
- 验收：
  - 任务作者可以按模板独立编写任务
  - 后端与前端对同一数据模型没有歧义

### M1：Task Spec Loader（1 ~ 2 天）
- 开发内容：
  - 实现 `spec.rs` / `loader.rs`
  - 支持 frontmatter + section 解析
  - 支持 `workspace_files` 校验
  - 提供 `GET /benchmark/suites` 与 `GET /benchmark/tasks`
- 验收：
  - 能扫描并加载 20 个任务定义
  - 非法任务文件能给出明确错误

### M2：Workspace & Executor（2 ~ 3 天）
- 开发内容：
  - 实现 task workspace 准备
  - 复用 `/wunder` 主链路执行任务
  - 采集 stream/transcript/final answer/tool calls
- 验收：
  - 单任务可稳定完成一次 attempt
  - workspace 素材注入正确
  - transcript/usage 可追溯

### M3：Automated Grading Sandbox（2 天）
- 开发内容：
  - 实现 Python grade 子进程执行器
  - 限制网络、CPU、内存、超时
  - 规范输出 `scores/notes/error`
- 验收：
  - automated 任务可返回细粒度分数
  - 非法评分脚本不会拖垮主进程

### M4：Judge & Hybrid Grading（2 天）
- 开发内容：
  - 实现 transcript summary / artifact summary
  - 实现 judge prompt builder
  - 实现 `llm_judge` 与 `hybrid` 合成
- 验收：
  - judge 结果可解析为稳定 JSON
  - `automated/hybrid/llm_judge` 三模式全部可跑通

### M5：Run 聚合与存储（1 ~ 2 天）
- 开发内容：
  - 新建 benchmark 存储表
  - 实现 task aggregate 与 run summary
  - 实现 `mean/std/min/max/pass_rate` 与效率指标
- 验收：
  - 同 task 多次运行能正确聚合
  - 历史列表与详情可加载

### M6：Admin API + SSE（1 ~ 2 天）
- 开发内容：
  - 实现 `start/cancel/list/detail/stream`
  - 增加 run 与 attempt 粒度事件推送
- 验收：
  - 页面可实时看到 task 级进度
  - cancel 后状态一致、数据可回看

### M7：管理端页面替换（2 ~ 3 天）
- 开发内容：
  - 替换旧能力评估面板
  - 增加 benchmark 控制台、详情抽屉、历史比较
- 验收：
  - 能发起、观察、回看 benchmark run
  - 页面支持查看 task/attempt/评分细节

### M8：任务库首批落地（3 ~ 5 天）
- 开发内容：
  - 补齐 20 个任务
  - 覆盖四个 suite
  - 形成基线分数
- 验收：
  - 每个 suite ≥ 5 个任务
  - 三种 grading type 都有代表任务
  - 可生成一份完整 benchmark 报告

### M9：切换与清理（1 天）
- 开发内容：
  - 删除旧 `evaluation` 模块、旧页面逻辑、旧配置目录
  - 更新 `docs/设计方案.md`、`docs/API文档.md`、`docs/系统介绍.md`
- 验收：
  - 仓库内只保留新的 benchmark 体系
  - 文档、接口、页面一致

## 13. 测试方案

### 13.1 单元测试
- task markdown 解析
- workspace_files 素材复制与 inline 写入
- judge JSON 解析与归一化
- hybrid 权重聚合
- 效率指标计算

### 13.2 集成测试
- 单任务完整 attempt 跑通
- `automated` / `llm_judge` / `hybrid` 三类任务各一条
- SSE 事件顺序正确
- cancel 流程正确

### 13.3 回归测试
- 固定一组基准模型，生成 baseline
- 每次 benchmark 改动后至少复跑 smoke suite
- 重点观察：分数波动、std 放大、耗时异常、token 占用突增

## 14. 风险与控制
- 风险：judge 输出非 JSON
  - 控制：加入严格 prompt、解析归一化与 fallback 逻辑。

- 风险：自动评分脚本执行不安全
  - 控制：独立 Python 子进程 + sandbox + 超时 + 禁网。

- 风险：任务依赖外部不稳定环境
  - 控制：第一批任务不依赖实时互联网结果，优先本地素材任务。

- 风险：页面一次性展示过多 transcript 导致卡顿
  - 控制：只展示摘要，详情按需加载。

- 风险：单模型波动大，分数不稳定
  - 控制：默认 `runs_per_task=3`，报告中强制展示 `std`。

## 15. 验收标准
- 成功替换旧能力评估为 benchmark 体系。
- 至少支持 20 个任务、4 个 suite、3 种 grading type。
- 每个 task 支持独立 workspace、transcript、artifact、attempt 明细。
- 支持多次运行并输出 `mean/std/min/max/pass_rate`。
- 支持效率指标：耗时、上下文 token 占用、分数效率。
- 管理端可完成发起、取消、进度观察、历史回看、task 明细查看。
- 后端 `cargo check`、相关测试与 smoke benchmark 全部通过。

## 16. 最终结论
- 本次不是在旧评估系统上“继续加 checker”，而是**直接以 PinchBench 方法论重建 benchmark 框架**。
- 交付顺序建议按 `M0 -> M9` 推进；只有 `M5` 完成后，系统才算具备基础可用性；`M8` 完成后，系统才具备真正可对比的评估价值。
- 实施阶段应优先保证：**任务真实性 > 评分可追溯性 > 稳定性统计 > 页面美化**。
