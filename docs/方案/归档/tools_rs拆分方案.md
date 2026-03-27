# tools.rs 拆分方案

## 背景与目标
- `src/services/tools.rs` 已约 8000 行，包含工具目录、执行入口、业务实现与大量工具细节。
- 文件已标注为维护模式（maintenance mode），继续堆积会放大耦合与回归风险。
- 目标：单文件控制在 2000 行以内；按领域拆分到独立模块；保持现有外部 API 不变；提高可维护性与可测试性。

## 现有功能块概览（按行号范围）
> 行号基于 2026-03-03 的 `tools.rs` 版本。

| 区块 | 大致行号 | 主要内容 |
| --- | --- | --- |
| 入口/上下文 | 1-167 | ToolEventEmitter/ToolContext/ToolRoots + build_tool_roots |
| 内置工具规格 | 168-624 | builtin_tool_specs_with_language + schema 定义 |
| 别名/工具汇总 | 624-946 | builtin_aliases/resolve_tool_name/collect_* |
| 内置工具路由 | 968-1025 | execute_builtin_tool 分发 |
| 记忆管理 | 1027-1195 | MemoryManager 工具 |
| 定时任务压缩 | 1197-1267 | cron 返回结构裁剪 |
| 计划面板 | 1269-1344 | plan 工具 |
| 询问面板 | 1346-1477 | question_panel 工具 |
| 用户世界 | 1480-1970 | user_world + 文件引用处理 |
| 节点调用 | 1972-2121 | node invoke 工具 |
| 子智能体与会话 | 2123-4163 | sessions_* + spawn_session_run |
| 蜂群 | 2244-3443 | agent_swarm + 运行态/等待 |
| 工具可用性/覆盖 | 4218-4392 | allowed tools + overrides |
| 用户工具 | 4394-4550 | user tool（skill/mcp/knowledge） |
| 知识库 | 4550-4734 | knowledge + vector knowledge |
| 工具辅助 | 4737-4865 | 解析/限流/分块等工具函数 |
| 命令/ptc | 4865-5657 | execute_command + ptc + 输出解码 |
| 文件系统 | 5659-6264 | list/search/read/write |
| 技能工具 | 6266-6400 | skill_call + tree |
| LSP | 6403-6804 | lsp 工具 |
| A2A/MCP | 6806-7455 | a2a + mcp |
| tests | 7457-7582 | 本地测试 |

## 目标模块拆分（建议）
目录：`src/services/tools/`

| 新模块 | 主要职责 | 预计行数 | 关键导出 |
| --- | --- | --- | --- |
| context.rs | ToolEventEmitter/ToolContext/ToolRoots | 200-300 | ToolContext/ToolRoots/build_tool_roots |
| catalog.rs | builtin specs/aliases/collect_* | 700-1000 | builtin_tool_specs/builtin_aliases/collect_prompt_tool_specs |
| dispatch.rs | execute_tool/execute_builtin_tool 统一分发 | 200-400 | execute_tool/execute_builtin_tool |
| memory_tool.rs | 记忆管理工具 | 150-250 | execute_memory_manager_tool |
| cron_tool.rs | cron 结果裁剪 | 80-150 | compact_cron_tool_result |
| plan_tool.rs | 计划面板 | 120-200 | execute_plan_tool |
| question_panel.rs | 询问面板 | 120-200 | execute_question_panel_tool |
| user_world_tool.rs | 用户世界 + 文件引用处理 | 400-600 | user_world_tool + file refs |
| node_invoke.rs | 节点调用 | 200-300 | execute_node_invoke |
| session_tools.rs | sessions_list/history/send/spawn + spawn_session_run | 700-900 | sessions_* + spawn_session_run |
| swarm_tools.rs | agent_swarm + 等待/批量派发 | 900-1200 | agent_swarm + wait_for_swarm_runs |
| tool_policy.rs | allowed tools/overrides/agent access | 200-300 | collect_user_allowed_tools/build_effective_tool_names |
| user_tool.rs | user tool（skill/mcp/knowledge）执行 | 200-350 | execute_user_tool |
| knowledge_tool.rs | knowledge + vector knowledge | 300-500 | execute_knowledge_tool/execute_vector_knowledge |
| command_tool.rs | execute_command + streaming/解码 | 500-800 | execute_command/run_command_streaming |
| ptc_tool.rs | ptc 执行 | 200-300 | execute_ptc |
| fs_tool.rs | list/search/read/write + path 处理 | 600-900 | list_files/search_content/read_files/write_file |
| skill_tool.rs | skill_call + tree | 150-250 | execute_skill_call |
| lsp_tool.rs | LSP 查询 + diagnostics | 350-500 | lsp_query/touch_lsp_file |
| a2a_tool.rs | a2a + mcp + schema | 500-700 | execute_a2a_service/a2a_observe/a2a_wait/execute_mcp_tool |
| utils.rs | 通用小工具（时间/字符串/分块等） | 150-250 | normalize_optional_string/now_ts/... |

> 说明：`tools.rs` 作为模块入口保留，但只负责 re-export、模块声明与顶层路由，控制在 500 行以内。

## 拆分依赖边界建议
- `dispatch.rs` 仅负责分发，调用各 tool 模块的 `execute_*`；不直接持有业务逻辑。
- `catalog.rs` 依赖 `a2a_tool::a2a_service_schema_with_language`，避免重复 schema 逻辑。
- `swarm_tools.rs` 依赖 `session_tools.rs` 的 `sessions_send/history/spawn`（建议 `pub(crate)`）。
- `session_tools.rs` 依赖 `tool_policy.rs`（工具白名单/覆盖）与 `utils.rs`。
- `user_tool.rs` 依赖 `knowledge_tool.rs`；`knowledge_tool.rs` 不反向依赖用户工具。
- `fs_tool.rs` 依赖 `utils.rs` 与 `context.rs`，不要跨依赖 `command_tool.rs`。
- 共享辅助函数集中在 `utils.rs`，禁止出现跨模块拷贝。

## 拆分步骤与节点（Milestones）

### 节点 M1：基础结构与公共类型
1. 新建 `context.rs`、`utils.rs`、`dispatch.rs`、`catalog.rs` 空壳。
2. 移动 ToolContext/ToolEventEmitter/ToolRoots/build_tool_roots 到 `context.rs`。
3. 移动通用工具函数到 `utils.rs`（time/string/limit/chunk）。
4. 在 `tools.rs` 中 re-export 并通过 `pub(crate)` 暴露给内部模块。
5. 编译通过（cargo check）。

### 节点 M2：目录与分发
1. 迁移 builtin specs/aliases/collect_* 到 `catalog.rs`。
2. 迁移 execute_tool/execute_builtin_tool 到 `dispatch.rs`。
3. `tools.rs` 仅保留模块声明 + re-export + execute_tool 入口。
4. 保持外部 API 不变（`services::tools::*` 导出）。

### 节点 M3：工具域拆分（一）
1. 迁移 memory/cron/plan/question_panel 到对应模块。
2. 迁移 user_world 与 file refs 到 `user_world_tool.rs`（附带相关 tests）。
3. 迁移 node_invoke 到 `node_invoke.rs`。

### 节点 M4：工具域拆分（二）
1. 迁移 sessions_* 与 spawn_session_run 到 `session_tools.rs`。
2. 迁移 swarm 相关到 `swarm_tools.rs`。
3. 迁移 tool_policy（allowed/override/access）到 `tool_policy.rs`。

### 节点 M5：工具域拆分（三）
1. 迁移 command/ptc 到 `command_tool.rs`、`ptc_tool.rs`。
2. 迁移 fs 工具到 `fs_tool.rs`。
3. 迁移 skill_call 到 `skill_tool.rs`。
4. 迁移 LSP 到 `lsp_tool.rs`。

### 节点 M6：A2A/MCP 与知识库
1. 迁移 a2a/mcp 相关到 `a2a_tool.rs`。
2. 迁移 knowledge/vector 到 `knowledge_tool.rs`。
3. 迁移 user_tool 到 `user_tool.rs`。

### 节点 M7：测试与收口
1. 将 tests 按功能拆分到对应模块或 `tools/tests.rs`。
2. `tools.rs` 总行数控制在 500-800 行内。
3. cargo check + cargo clippy，修复 clippy::too_many_arguments 等注解迁移后的问题。

## 变更影响与注意事项
- 仅结构拆分，不修改对外行为与工具协议；所有 public 函数签名保持不变。
- 避免跨模块循环依赖；必要时将共享逻辑收敛到 `utils.rs` 或 `tool_policy.rs`。
- 保持 `apply_patch_tool.rs` 独立模块不动。
- 对 `frontend` 等目录不要做全量搜索，避免性能问题。
- 更新 docs/功能迭代.md 记录本次“文档”变更。

## 验收标准
- `tools.rs` ≤ 2000 行，且只承担入口/路由职责。
- 新模块单文件 ≤ 2000 行。
- 现有测试通过，编译无 warning。
- `/wunder/tools` 与工具执行行为无回归。
