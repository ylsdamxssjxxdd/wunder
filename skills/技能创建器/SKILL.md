---
name: 技能创建器
description: 创建新技能、修改/优化已有技能并评估效果。当用户希望从零创建技能、更新或优化现有技能、运行评测验证技能、做基准对比/方差分析，或优化技能描述以提升触发准确性时使用。本技能也适用于任何与“技能设计、测试、迭代、触发”相关的请求。
---

# 技能创建器

用于创建新技能并进行迭代改进的技能。

从高层看，创建技能的大致流程如下：

- 明确技能要做什么，以及大致如何做
- 写出技能草案
- 编写少量测试提示词，并用“可访问该技能的 Claude”运行测试
- 帮助用户从定性与定量角度评估结果
  - 在运行进行时，如果还没有定量评测，就起草一些（如果已有，可沿用或必要时修改），并向用户解释
  - 使用 `eval-viewer/generate_review.py` 生成可视化评审界面，方便用户查看结果与指标
- 基于用户反馈和定量问题改写技能
- 重复以上流程，直到满意
- 扩充测试集并在更大规模上再跑一轮

你的任务是判断用户当前处于流程的哪一步，并帮助他们推进下一步。比如：
- 用户说“我想做一个 X 的技能”，你可以帮他们梳理目标、写草案、准备测试、跑评测并迭代。
- 若用户已有草案，则可直接进入评测/迭代环节。

当然也要灵活：若用户说“不要评测，先随便做做”，也可以先按他们的节奏来。

另外，在技能完成后（流程灵活），你还可以运行“技能描述优化”脚本，用于提高触发准确性。

---

## 与用户沟通

技能创建器会被不同熟悉程度的用户使用——有的人非常懂技术，也有人几乎不懂术语。你需要根据上下文选择表达方式。默认情况下：

- “评测”“基准”是边缘术语，但可以使用
- “JSON”“断言”等术语，只有在用户明显熟悉时才不解释

如果不确定，就简短解释；必要时给出一句定义。

---

## 创建技能

### 捕捉意图

先理解用户的意图。当前对话可能已经包含他们想固化的流程（例如“把这段对话变成技能”）。这时先从对话历史中提取关键信息：用到的工具、步骤顺序、用户修正、输入/输出格式等。缺口由用户补全，并在下一步前确认。

1. 这个技能要让 Claude 做什么？
2. 什么时候应该触发该技能？（用户会用什么说法/上下文）
3. 期望输出的格式是什么？
4. 是否需要测试用例验证？
   - 可验证输出（文件转换、数据抽取、代码生成、固定流程）通常建议做测试
   - 主观输出（写作风格、艺术）往往不需要
   - 给出合适建议，但让用户决定

### 访谈与研究

主动询问边界条件、输入/输出格式、示例文件、成功标准与依赖。不要在这些不清晰时就写测试提示词。

检查可用 MCP：如果研究有帮助（查文档、找类似技能、最佳实践），可以并行调用子代理；否则就直接在当前对话中完成。尽量带着上下文来，减少用户负担。

### 编写 SKILL.md

基于访谈结果，填写以下组件：

- **name**：技能标识符
- **description**：触发场景 + 技能作用。描述是主要触发机制，所以必须同时写“做什么”和“什么时候用”。
  - 注意：当前模型倾向于“触发不足”。为了避免漏触发，描述要更“积极”。
  - 例如：不要写“展示公司数据的仪表盘”，而是写“凡是用户提到仪表盘/可视化/内部指标/展示公司数据，即使没说‘仪表盘’也要触发”。
- **compatibility**：依赖/工具（可选，通常不必）
- **技能正文内容**

---

## 技能写作指南

### 技能结构

```
skill-name/
├── SKILL.md (required)
│   ├── YAML frontmatter (name, description required)
│   └── Markdown instructions
└── Bundled Resources (optional)
    ├── {{SKILL_ROOT}}/scripts/    - Executable code for deterministic/repetitive tasks
    ├── {{SKILL_ROOT}}/references/ - Docs loaded into context as needed
    └── {{SKILL_ROOT}}/assets/     - Files used in output (templates, icons, fonts)
```

### 渐进式加载（Progressive Disclosure）

技能采用三层加载机制：
1. **元数据**（name + description）— 永远在上下文（~100 词）
2. **SKILL.md 正文** — 技能触发时加载（建议 <500 行）
3. **资源包** — 按需加载（无上限，脚本可直接执行无需加载）

**关键模式：**
- SKILL.md 尽量 <500 行，接近上限时拆层级并明确引用路径
- 在 SKILL.md 中清楚说明何时读取引用文件
- 参考文件很大（>300 行）时，提供目录

**多域组织**：多框架/多域时按变体组织：
```
cloud-deploy/
├── SKILL.md (workflow + selection)
└── references/
    ├── aws.md
    ├── gcp.md
    └── azure.md
```
只加载当前需要的 reference 文件。

### “避免惊吓”原则

技能不得包含恶意内容、利用漏洞或危害安全的代码。技能内容必须符合用户意图，不能暗含“越权/误导”。“角色扮演”类技能可以接受，但不得欺骗用户。

### 写作模式

优先使用祈使语态（直接指令）。

**定义输出格式**：
```markdown
## 报告结构
必须使用以下模板：
# [Title]
## Executive summary
## Key findings
## Recommendations
```

**示例格式**：
```markdown
## Commit message format
**Example 1:**
Input: Added user authentication with JWT tokens
Output: feat(auth): implement JWT-based authentication
```

### 写作风格

尽量解释“为什么”，而不是堆砌 MUST。强调理论依据与用户心理模型，让技能更通用，避免过拟合。

---

## 测试用例

完成技能草案后，给出 2-3 条真实测试提示词。和用户确认：
“这里是建议的测试用例，你看看是否合适，要不要加？”

保存到 `evals/evals.json`。先只写 prompt，断言留到后面。

```json
{
  "skill_name": "example-skill",
  "evals": [
    {
      "id": 1,
      "prompt": "User's task prompt",
      "expected_output": "Description of expected result",
      "files": []
    }
  ]
}
```

完整 schema 见 `references/schemas.md`（包含 `assertions` 字段）。

---

## 运行与评测测试用例

本节是一条连续流程，不要中途停。**不要使用 `/skill-test`** 或其他测试技能。

将结果放在与技能目录同级的 `<skill-name>-workspace/`。组织方式：
- `iteration-1/`, `iteration-2/`...
- 每个 eval 建一个目录：`eval-0/`, `eval-1/`...
- 不要提前创建全部目录，边跑边建

### 步骤 1：同一轮中同时发起 with-skill 与 baseline

每个测试用例必须同时启动两个子代理：一个带技能，一个不带（或旧版本）。不要先跑 with-skill 再跑 baseline。

**With-skill 运行格式：**

```
Execute this task:
- Skill path: <path-to-skill>
- Task: <eval prompt>
- Input files: <eval files if any, or "none">
- Save outputs to: <workspace>/iteration-<N>/eval-<ID>/with_skill/outputs/
- Outputs to save: <what the user cares about — e.g., "the .docx file", "the final CSV">
```

**Baseline 规则：**
- **新技能**：无技能（same prompt, no skill）输出到 `without_skill/outputs/`
- **改进现有技能**：先快照旧技能（`cp -r <skill-path> <workspace>/skill-snapshot/`），baseline 用快照，输出到 `old_skill/outputs/`

每个测试用例要写 `eval_metadata.json`，名字要描述测试内容（不要叫 eval-0）。目录名也用该名字。若新增或修改 eval prompt，必须为该 eval 生成 metadata 文件，不要假设会沿用上次。

```json
{
  "eval_id": 0,
  "eval_name": "descriptive-name-here",
  "prompt": "The user's task prompt",
  "assertions": []
}
```

### 步骤 2：运行中起草断言

不要干等运行结束。为每个测试用例起草可量化断言并向用户解释；如果 `evals/evals.json` 已有断言，则审阅并解释。

好的断言应该可客观验证，并且名称直观，方便在评测界面一眼看懂。主观任务（写作、设计）不必强行写断言。

完成后更新 `eval_metadata.json` 与 `evals/evals.json`，并告诉用户评测界面会显示哪些质检指标。

### 步骤 3：记录 timing 数据

每个子代理完成时，会返回 `total_tokens` 与 `duration_ms`。这信息不会再次出现，必须立即写入 `timing.json`：

```json
{
  "total_tokens": 84852,
  "duration_ms": 23332,
  "total_duration_seconds": 23.3
}
```

### 步骤 4：评分、聚合、生成评审界面

全部运行结束后：

1. **评分**：用 grader 子代理（或人工）读取 `agents/grader.md` 对照断言打分。保存 `grading.json`。
   - 评分结果数组字段必须是 `text`、`passed`、`evidence`，不要用其他字段名。
   - 若断言可程序化检查，写脚本执行，而不要主观判断。

2. **聚合基准**：在 skill-creator 目录运行：
   ```bash
   python -m scripts.aggregate_benchmark <workspace>/iteration-N --skill-name <name>
   ```
   生成 `benchmark.json` 与 `benchmark.md`（包含均值±方差与对比）。若手动生成 benchmark，遵循 `references/schemas.md`。
   *顺序要求：with_skill 放在 baseline 前面。*

3. **分析者视角**：阅读 benchmark 结果，找出隐藏问题（例如断言全部通过但不区分优劣、某些 eval 波动大、耗时/成本权衡）。参考 `agents/analyzer.md` 的 “Analyzing Benchmark Results”。

4. **启动评审界面**：
   ```bash
   nohup python <skill-creator-path>/eval-viewer/generate_review.py \
     <workspace>/iteration-N \
     --skill-name "my-skill" \
     --benchmark <workspace>/iteration-N/benchmark.json \
     > /dev/null 2>&1 &
   VIEWER_PID=$!
   ```
   如果是 iteration 2+，加 `--previous-workspace <workspace>/iteration-<N-1>`。

   **Cowork / 无 GUI 环境**：若 `webbrowser.open()` 不可用或无显示，使用 `--static <output_path>` 生成独立 HTML 文件。用户点“Submit All Reviews”会下载 `feedback.json`；把它拷回 workspace。

   **注意**：必须使用 `generate_review.py`，无需自建 HTML。

5. **提示用户**：告诉他们已打开评审页面，解释 “Outputs” 与 “Benchmark” 两个标签页，并让他们评审后回到对话。

### 评审界面内容

“Outputs” 页：
- **Prompt**：测试任务
- **Output**：输出文件（可视化渲染）
- **Previous Output**（iteration 2+）
- **Formal Grades**（断言评分）
- **Feedback**：用户评论
- **Previous Feedback**（iteration 2+）

“Benchmark” 页显示：各配置的通过率、耗时与 token，对每个 eval 的拆分与分析结论。

### 步骤 5：读取反馈

用户完成后读取 `feedback.json`：

```json
{
  "reviews": [
    {"run_id": "eval-0-with_skill", "feedback": "the chart is missing axis labels", "timestamp": "..."},
    {"run_id": "eval-1-with_skill", "feedback": "", "timestamp": "..."},
    {"run_id": "eval-2-with_skill", "feedback": "perfect, love this", "timestamp": "..."}
  ],
  "status": "complete"
}
```

空反馈表示用户满意，重点改进有明确问题的用例。

结束后关闭评审服务：
```bash
kill $VIEWER_PID 2>/dev/null
```

---

## 改进技能

这是迭代核心：用户评审后，你需要根据反馈改技能。

### 改进思路

1. **抽象反馈，避免过拟合**：不要只对某个示例做死修正，思考更通用的改法。
2. **保持精简**：删掉不必要的内容，避免指令冗余。
3. **解释“为什么”**：与其写 MUST，不如让模型理解为什么重要。
4. **识别重复劳动**：若多个测试都生成类似脚本，应收敛成 `{{SKILL_ROOT}}/scripts/` 并在技能里引用。

### 迭代流程

改进后：
1. 应用修改
2. 重新运行全部测试（新 iteration 目录），并包含 baseline
3. 使用 `--previous-workspace` 启动评审
4. 等用户评审
5. 读取反馈并继续改

停止条件：
- 用户满意
- 反馈为空
- 迭代不再带来改进

---

## 高阶：盲测比较

当用户要求严谨比较（例如“新版本真的更好吗？”）时，可使用盲测。阅读 `agents/comparator.md` 与 `agents/analyzer.md`。这一步可选，多数用户不需要。

---

## 描述优化（Description Optimization）

技能 frontmatter 中的 `description` 是触发机制。完成技能后，可优化描述提升触发率。

### 步骤 1：生成触发评测集

生成 20 条查询（混合触发/不触发）并保存为 JSON：

```json
[
  {"query": "the user prompt", "should_trigger": true},
  {"query": "another prompt", "should_trigger": false}
]
```

要求：
- 真实用户语气，包含细节（文件名、字段、URL、背景等）
- should-trigger 8-10 条，涵盖多种表述与边缘情况
- should-not-trigger 8-10 条，必须是“近似但不该触发”的难例
- 不要用明显无关的负例

### 步骤 2：用户审阅

用 HTML 模板让用户审阅并导出：
1. 读取 `assets/eval_review.html`
2. 替换占位符：
   - `__EVAL_DATA_PLACEHOLDER__` → JSON 数组
   - `__SKILL_NAME_PLACEHOLDER__` → 技能名
   - `__SKILL_DESCRIPTION_PLACEHOLDER__` → 当前描述
3. 写到临时文件并打开：`open /tmp/eval_review_<skill-name>.html`
4. 用户调整后点击 “Export Eval Set”
5. 导出文件在 `~/Downloads/eval_set.json`（注意可能有重复文件）

### 步骤 3：运行优化循环

告诉用户：“这需要一些时间，我会后台运行并定期汇报。”

运行：

```bash
python -m scripts.run_loop \
  --eval-set <path-to-trigger-eval.json> \
  --skill-path <path-to-skill> \
  --model <model-id-powering-this-session> \
  --max-iterations 5 \
  --verbose
```

使用当前会话的模型 ID，确保触发评测一致。

脚本会：
- 60% 训练 / 40% 测试
- 每条查询跑 3 次
- 迭代优化描述
- 最终输出 `best_description`（以测试集为准）

### 步骤 4：应用结果

用 `best_description` 更新 SKILL.md frontmatter，并向用户展示前后对比与评分。

---

## 技能触发机制说明

Claude 只会在需要技能时才触发。过于简单的请求（如“读这个 PDF”）即使描述匹配，也可能不触发，因为它可直接完成。因此测试查询必须足够具体且复杂。

---

## 打包与交付（仅当 present_files 可用）

如果具备 `present_files` 工具，可打包并交付 `.skill` 文件：

```bash
python -m scripts.package_skill <path/to/skill-folder>
```

打包后告知用户 `.skill` 文件路径。

---

## Claude.ai 专用说明

Claude.ai 无子代理，流程需调整：

- **测试用例**：逐条执行，使用技能完成任务。跳过 baseline。
- **评审**：无法打开浏览器时，直接在对话中展示结果并询问反馈。
- **量化评测**：无 baseline，跳过。
- **迭代**：继续改进 → 重跑 → 收反馈。
- **描述优化**：依赖 `claude -p` CLI，仅 Claude Code 可用。
- **盲测**：需要子代理，跳过。
- **打包**：可用 `package_skill.py`。

---

## Cowork 专用说明

- 有子代理，可并行跑测试（若超时可退化为串行）。
- 无浏览器，`generate_review.py` 用 `--static` 输出 HTML。
- 评审后 `feedback.json` 会下载，需放回 workspace。
- `run_loop.py` 可用，但应在技能稳定后再跑。

---

## 参考文件

- `agents/grader.md` — 断言评分说明
- `agents/comparator.md` — 盲测对比
- `agents/analyzer.md` — 结果分析
- `references/schemas.md` — evals / grading / benchmark schema

---

再次强调核心流程：

- 明确技能目标
- 写草案
- 跑测试
- 生成评审界面 + 定量评测
- 迭代改进
- 打包交付

如果你有 TodoList，请把关键步骤写进去（尤其是“创建 evals JSON 与生成评审界面”）。

Good luck!
