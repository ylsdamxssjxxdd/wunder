# freeform_call 落地方案

## 1. 目标与范围

### 1.1 目标
- 在现有 `tool_call`、`function_call` 之外新增第三种工具调用模式：`freeform_call`。
- 让用户可按模型能力选择最优方式：
  - 弱/兼容模型：`tool_call`
  - 标准函数调用模型：`function_call`
  - 强模型（尤其 GPT-5 类）：`freeform_call`
- 将当前“编辑文件”工具彻底替换为“应用补丁（apply_patch）”工具，不保留旧别名，不考虑旧调用兼容。

### 1.2 约束
- 仅维护 Rust `src` 代码；`app` 不纳入实现范围。
- 工具链设计以稳定运行为第一优先：强校验、可观测、可回放。
- 对齐 Codex 的关键体验：`结构化函数工具 + freeform 语法工具` 共存。

---

## 2. 现状评估（代码基线）

### 2.1 模式层
- 当前模式枚举只有两种：`ToolCall` 与 `FunctionCall`（`src/services/llm.rs`）。
- API 参数校验仅接受 `tool_call|function_call`（`src/api/chat.rs`）。

### 2.2 提示词层
- 仅 `tool_call` 模式注入 `<tool_call>...</tool_call>` 协议块（`src/services/prompting.rs` + `prompts/*/system/tools_protocol.txt`）。
- `function_call` 模式默认省略工具协议块，依赖 LLM API 自带工具定义。

### 2.3 工具定义层
- `ToolSpec` 目前是单形态：`name + description + input_schema`（`src/core/schemas.rs`）。
- 无法表达 freeform 工具需要的 `format(grammar/lark/definition)` 元信息。

### 2.4 调用解析层
- 调用统一归一到 JSON 参数（`ToolCall { name, arguments: Value }`），缺少 raw 文本载荷通道（`src/orchestrator/tool_calls.rs`）。
- 对 Responses API 的提取主要聚焦 `function_call`，未形成 `custom_tool_call` 一等支持。

### 2.5 文件编辑工具层
- 当前仍是行级 JSON 编辑模型 `edit_file`（`src/services/tools.rs`）。
- 与 Codex `apply_patch` 的 freeform 语法和失败反馈模式差异较大。

---

## 3. 目标架构（新增 freeform_call）

## 3.1 三模式并存
- `tool_call`：继续走 `<tool_call>` 标签 + JSON。
- `function_call`：继续走平台函数工具（JSON Schema）。
- `freeform_call`：走函数工具 + freeform 工具混合；优先供支持 custom/freeform 的模型使用。

## 3.2 工具规格升级
将 `ToolSpec` 从单结构升级为枚举（示意）：

```rust
pub enum ToolSpec {
    Function {
        name: String,
        description: String,
        input_schema: serde_json::Value,
    },
    Freeform {
        name: String,
        description: String,
        format: FreeformFormat,
    },
}

pub struct FreeformFormat {
    pub r#type: String,     // grammar
    pub syntax: String,     // lark
    pub definition: String, // grammar body
}
```

这样可直接承载 `apply_patch` 的语法定义，不需要把长 grammar 塞进通用系统提示词。

## 3.3 调用载荷升级
工具调用结构升级为“参数双通道”：

```rust
pub enum ToolPayload {
    Json(serde_json::Value),
    Raw(String),
}

pub struct ToolCall {
    pub id: Option<String>,
    pub name: String,
    pub payload: ToolPayload,
}
```

- `Function` 工具使用 `Json`。
- `Freeform` 工具使用 `Raw`。

## 3.4 执行器升级
执行入口从 `execute_builtin_tool(context, name, &Value)` 扩展为按 `ToolPayload` 分发：
- `execute_builtin_function_tool(...)`
- `execute_builtin_freeform_tool(...)`

保证工具实现不再被“必须 JSON”约束。

---

## 4. 提示词改造策略（不新建整套，做分片增强）

## 4.1 结论
- **不建议新建一整套 prompts 目录**。
- 采用现有模板增量改造：新增 `[[TOOL_CALL_MODE_FREEFORM_CALL]]` 分支即可。

## 4.2 改造点
1. `prompts/zh/system/tools_protocol.txt`、`prompts/en/system/tools_protocol.txt`
   - 新增 `TOOL_CALL_MODE_FREEFORM_CALL` 片段。
   - 内容聚焦：
     - 何时调用 freeform 工具；
     - freeform 输入不包 JSON；
     - 严格遵循 grammar；
     - 失败后按报错最小修补重试。
2. `src/services/prompting.rs`
   - `tools_block` 渲染从二分支改为三分支。
   - `freeform_call` 模式下仍可注入工具列表，但渲染需支持 Function/Freeform 两类。

## 4.3 token 策略
- grammar 正文由工具定义携带，不在 system prompt 大段重复，减少上下文占用。
- 系统提示词仅保留“调用原则 + 错误恢复策略”。

---

## 5. 应用补丁（apply_patch）工具改造方案

## 5.1 总体原则
- 彻底移除 `edit_file` 业务能力。
- 新工具唯一入口：`apply_patch`（中文展示名建议：`应用补丁`）。
- 不保留旧别名、不做旧协议兼容。

## 5.2 工具规格
### freeform 版本（`freeform_call`）
- 名称：`apply_patch`
- 类型：`Freeform`
- format：`type=grammar`、`syntax=lark`
- definition：补丁语法（`*** Begin Patch ... *** End Patch`）

### function 版本（`function_call` 兜底）
- 名称：`apply_patch`
- 类型：`Function`
- schema：`{ input: string }`，`input` 为完整补丁文本

> 说明：同名双形态可按模式下发不同定义，便于用户按模型能力切换。

## 5.3 执行流程
1. 接收补丁文本（raw 或 `arguments.input`）。
2. 语法解析与结构校验（Begin/End、FileOp、Hunk）。
3. 路径安全检查（必须位于 workspace roots，禁止越界）。
4. 原子应用：
   - 读取原文件
   - 按补丁改写
   - 写回并校验
5. 回传结构化结果：
   - `ok`
   - `changed_files`
   - `added/updated/deleted`
   - `hunks_applied`
   - `lsp` 信息
6. 失败回传可执行错误（定位到文件/块/上下文），便于模型重试。

## 5.4 与旧 edit_file 的差异处理
- 删除 `edit_file` tool spec、执行分支、策略映射、模拟实验白名单与测试用例。
- 将原“编辑文件”权限与审计语义迁移到 `apply_patch`。
- 文档、前端可见工具说明全部改为“应用补丁”。

## 5.5 安全与稳定要求
- 禁止绝对路径越界写入。
- 对大补丁设上限（字符数、文件数、hunk 数）。
- 失败必须“可解释、可重试、无半写入”。
- 工具输出稳定、字段固定，便于前端和日志系统消费。

---

## 6. 代码改造清单（按模块）

## 6.1 模式与配置
- `src/services/llm.rs`
  - `ToolCallMode` 新增 `FreeformCall`
  - `normalize_tool_call_mode` 支持 `freeform_call`（可接受别名 `freeform`）
- `src/api/chat.rs`
  - 请求参数校验允许 `freeform_call`
  - 报错文案更新为三选一
- `src/core/config.rs`
  - 配置说明补充 `tool_call_mode=freeform_call`

## 6.2 Schema/协议
- `src/core/schemas.rs`
  - `ToolSpec` 升级为枚举结构（Function/Freeform）
- 相关 API 响应（available tools、user tools、shared tools）统一适配新结构

## 6.3 工具下发与提示词
- `src/services/prompting.rs`
  - 三模式渲染分支
  - `render_tool_spec` 支持两类工具输出
- `prompts/*/system/tools_protocol.txt`
  - 新增 `[[TOOL_CALL_MODE_FREEFORM_CALL]]` 规则段

## 6.4 模型响应解析
- `src/services/llm.rs`
  - 增强 Responses 输出解析，支持 `custom_tool_call` 聚合
- `src/orchestrator/tool_calls.rs`
  - 新增 freeform payload 解析路径（`name + raw_input`）
  - 保持现有 `<tool_call>` JSON 解析不变

## 6.5 工具执行层
- 新增模块（避免继续膨胀 `tools.rs`）：
  - `src/services/tools/apply_patch.rs`
- `src/services/tools.rs`
  - 仅接线：注册 `apply_patch`，移除 `edit_file` 分支
- `src/core/exec_policy.rs`、`src/services/sim_lab.rs`
  - 替换旧工具名判断与策略绑定

---

## 7. 测试与验收

## 7.1 单元测试
- mode 解析：`tool_call/function_call/freeform_call`
- ToolSpec 序列化：Function/Freeform
- apply_patch grammar：合法/非法/边界样例

## 7.2 集成测试
- `freeform_call + apply_patch`：新增/修改/删除/重命名
- `function_call + apply_patch(input)`：同等语义结果
- 大补丁失败回滚验证
- 多用户并发写入不同 workspace

## 7.3 回归测试
- 原 `tool_call` 生态保持可用
- 非文件工具（读取/搜索/执行命令等）不受影响
- token 统计口径保持“上下文占用量”定义不变

## 7.4 验收标准
- `edit_file` 不再出现在可用工具列表与执行入口。
- `freeform_call` 在 API 与配置中可选且可用。
- `apply_patch` 在 `freeform_call` 下具备稳定成功率与可恢复失败路径。

---

## 8. 迭代计划（建议）

### Phase 1（2-3 天）
- 三模式打通：配置/API/提示词分支。
- ToolSpec 升级为 Function/Freeform。

### Phase 2（3-4 天）
- apply_patch 工具实现（freeform + function 兜底）。
- 执行层接线与权限策略迁移。

### Phase 3（2-3 天）
- 删除 edit_file 全链路残留。
- 完成集成测试与文档更新（API文档/设计方案/系统介绍相关章节）。

### Phase 4（1-2 天）
- 灰度与稳定性收敛：错误码、重试提示、性能上限参数。

---

## 9. 风险与缓解

- 风险：不同模型对 freeform grammar 的遵循度差异大。
  - 缓解：保留 `function_call` 兜底，按模型配置切换。
- 风险：补丁解析错误导致失败率升高。
  - 缓解：错误消息标准化，提供精确定位与重试建议。
- 风险：`tools.rs` 过大进一步恶化可维护性。
  - 缓解：新业务全部放入 `src/services/tools/` 子模块，仅在 `tools.rs` 注册。

---

## 10. 最终建议
- 模式命名采用 `freeform_call`，与现有 `tool_call/function_call` 并列。
- 文件编辑能力统一升级为 `apply_patch（应用补丁）`。
- 先完成“模式+协议+执行链路”基础改造，再替换旧工具；避免一次性大爆炸改动。
- 严格以测试驱动迁移，确保 Wunder 能在接入更先进模型后，稳定完成“用 Wunder 开发 Wunder”的自举目标。
