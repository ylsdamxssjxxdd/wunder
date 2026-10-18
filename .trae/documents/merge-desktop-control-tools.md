# 合并「桌面控制器」与「桌面监视器」为单一「桌面控制」工具

## Context

当前桌面能力被拆成两个模型可见工具：`桌面控制器`（注入鼠标/键盘动作后截图）与 `桌面监视器`（等待后截图，只观察不操作）。两者共用同一套截图、归一化坐标、followup 多模态回灌与开关/权限逻辑，差异只在于「是否注入动作 + 覆盖层事件类型」，对模型而言是两个高度耦合的工具，选择成本高、描述重复。

目标：合一为单一工具 `桌面控制`（别名 `desktop_control`），把监视器「等待后观察」能力收敛为该工具的一个 `wait` 动作，彻底移除 `桌面监视器`。属于原型阶段的结构清理，按「从后端深层彻底重构」处理：改名、改 schema、改配置字段、改前端映射，不留兼容别名。

## 关键设计决策

- **工具名**：规范名 `桌面控制`，别名 `desktop_control`（短别名 `desktop`）。移除 `桌面控制器`/`controller`/`桌面监视器`/`monitor` 及其别名常量。
- **动作收敛**：把原 `delay` 动作升级为 `wait`，吸收监视器语义——等待 `delay_ms`（并接受 `wait_ms`/`wait` 写法，夹到 30000ms），可选 `note` 原样回显，屏幕上走倒计时覆盖层，随后截图 + followup。动作数保持 17。`delay`/`sleep`/`observe`/`screenshot` 作为动作别名归一。
- **schema**：单一 `action` 枚举含 17 项（其中 `wait` 取代 `delay`）；参数保留 `bbox`(wait 动作仍要求，用于覆盖层定位中心，可传中心二元组)/`action`/`key`/`text`/`delay_ms`/`duration_ms`/`scroll_steps`/`to_bbox`，新增 `note`。移除监视器 spec 的 `wait_ms` 必填语义（`wait` 动作不再强制等待时长，缺省 0）。
- **覆盖层事件**：统一改名 `desktop_control_hint` / `desktop_control_hint_done` / `desktop_control_countdown` / `desktop_control_countdown_done`。桌面壳 bridge 方法名（`showControllerHint` 等）属外部契约，保持不变，只改事件名映射。
- **配置字段**：`tools.desktop_controller` → `tools.desktop_control`，结构体 `DesktopControllerConfig` → `DesktopControlConfig`，default 函数同步改名。
- **i18n**：`tool.desktop_controller.*` / `tool.spec.desktop_controller.*` → `tool.desktop_control.*` / `tool.spec.desktop_control.*`；删除 `tool.desktop_monitor.*` 与 `tool.spec.desktop_monitor.*`，`wait` 相关错误/提示并入 `desktop_control` 命名空间。

## 后端改动（crates）

1. [desktop_control_impl.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/tools/desktop_control_impl.rs)（核心重写）
   - 常量改为 `TOOL_DESKTOP_CONTROL = "桌面控制"`、`TOOL_DESKTOP_CONTROL_ALIAS = "desktop_control"`、`TOOL_DESKTOP_CONTROL_ALIAS_SHORT = "desktop"`。
   - `DesktopAction` 用 `Wait` 取代 `Delay`，`from_raw` 接受 `wait|delay|sleep|observe|screenshot`。
   - 合并 `tool_desktop_monitor` 进 `tool_desktop_control`：`Wait` 分支做等待（cap `MAX_MONITOR_WAIT_MS`）+ 倒计时覆盖层 + `note` 回显；非 `wait` 动作沿用现有 hint/hint_done 覆盖层。
   - `DesktopControllerArgs` 增 `note`；`parse_desktop_controller_args` 增 bbox 定位回退（wait 动作可只给中心点）。
   - 删除 `parse_monitor_wait_ms`、`is_desktop_controller_tool_name`/`is_desktop_monitor_tool_name` 拆分，收敛为单一 `is_desktop_control_tool_name`；`tool_result` 的 `tool` 字段与 `followup_prompt` 统一走 `desktop_control`。
   - 覆盖层事件名按上文改名。保留 `build_followup_user_message`。
   - 同步更新文件内单测（`parse_*`、`remember_*`、followup）。
2. [desktop_control.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/tools/desktop_control.rs)（feature 关闭分支）：镜像常量/函数签名，删除监视器分支与其 `disabled_result` 参数差异，保留 feature-gate 失败结果。
3. [catalog.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/tools/catalog.rs)：两个 `ToolSpec` 合并为一个；`builtin_aliases` 删除监视器/控制器 4 条、新增 `desktop_control`/`desktop` → `桌面控制`；`preferred_english_alias` 与 `runtime_tool_display_name`（`preferred_english_alias` 附近约 L1501）改为单条目；文件内 `desktop_controller_schema_*` 测试更新为新 schema。
4. [dispatch.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/tools/dispatch.rs)：两个匹配分支合并为一个 `TOOL_DESKTOP_CONTROL => tool_desktop_control(...)`。
5. [exec_policy.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/core/exec_policy.rs#L64-L72)：`resolve_exec_policy_tool_kind` 中 controller/monitor 两常量合并为一个 `desktop_control`（仍归类 `Control`）。
6. [execute_support.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/orchestrator/execute_support.rs#L1167-L1176) 及 [tests.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/orchestrator/execute_support/tests.rs#L699-L775)：`approval_kind_for_tool` 与 approval summary 相关断言合并为单工具名。
7. [execute.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/orchestrator/execute.rs)：`is_desktop_control_tool_name` + `build_desktop_followup_user_message` 调用点不变（函数名不变，仅内部收敛）。
8. [user_agents.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/api/user_agents.rs#L1610-L1650)：`filter_allowed_tools` 的 `desktop_control_tool_names` 集合改为单条 `resolve_tool_name("desktop_control")`；相关测试同步。
9. [random_sim.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/virtual_llm/random_sim.rs#L78-L90)：固定参数表两条监视器/控制器合并为一条 `desktop_control`（`action=wait, delay_ms=0`）；测试同步。
10. 配置：[config.rs](file:///d:/proj/wunder/crates/wunder-core/src/config.rs#L885)、[wunder.yaml](file:///d:/proj/wunder/config/wunder.yaml#L327)、[wunder-cli/config/wunder.yaml](file:///d:/proj/wunder/crates/wunder-cli/config/wunder.yaml#L89)：字段与 default 函数改名，yaml 键 `desktop_control:`。
11. [wunder-desktop/runtime.rs](file:///d:/proj/wunder/crates/wunder-desktop/runtime.rs#L1352-L1366)：`apply_desktop_defaults` 改字段名，`ensure_desktop_builtin_tool` 的 `"桌面控制器"`+`"桌面监视器"` 合并为 `"桌面控制"`；测试同步。
12. [wunder-cli/tool_presentation.rs](file:///d:/proj/wunder/crates/wunder-cli/tool_presentation.rs#L690)：显示名映射合并为 `("desktop_control", "桌面控制")`；测试同步。
13. 不改 [desktop_runtime_recovery.rs](file:///d:/proj/wunder/crates/wunder-runtime/src/services/desktop_runtime_recovery.rs) 的 `cancel_desktop_monitor_sessions`——其 `monitor record` 是会话运行监视记录，与桌面监视器工具无关。

## 前端改动

- 舰桥/蜂巢图标与视觉映射：
  - [tool-visuals.js](file:///d:/proj/wunder/web/shared/tool-visuals.js#L5-L6) 两条合并为一条 `桌面控制`/`desktop_control`（图标沿用鼠标类）。
  - [abilityVisuals.ts](file:///d:/proj/wunder/frontend/src/utils/abilityVisuals.ts#L51-L52) 同上。
  - [frontend-slint/tool_icons.rs](file:///d:/proj/wunder/frontend-slint/src/tool_icons.rs#L24-L36)（及 L272-L281）两组关键字合并为一条。
- 覆盖层事件：`frontend/src/stores/chatProjectionOnlyEvents.ts`、`chatRuntimeState.ts`、`chatPersist.ts` 中的四个事件名替换为新名；`chatTypes.ts`/`views/messenger/model.ts` 的 bridge 方法名保留，只调整注释（如涉及）。
- Agent 设置快照：[agentSettingsSnapshot.ts](file:///d:/proj/wunder/frontend/src/utils/agentSettingsSnapshot.ts#L16-L36) 将 `resolveDesktopToolKind` 合并为单一桌面控制判定，`canonicalizeAgentToolName` 输出 `桌面控制`；同步更新 [agent-settings-snapshot-regression.test.ts](file:///d:/proj/wunder/frontend/scripts/regression/agent-settings-snapshot-regression.test.ts#L16-L28)（含旧名归一迁移断言）。

## 配置 / i18n

- [i18n.messages.json](file:///d:/proj/wunder/config/i18n.messages.json#L1318-L1385)：键重命名 + 合并；`tool.desktop_control.*` 覆盖原两套文案（含 `wait` 动作键），spec 键 `tool.spec.desktop_control.*`。

## 文档与生成物

- 源文档：`docs/使用说明书/{zh-CN,en}/tools/desktop-control.md`（标题改「桌面控制」，删监视器小节、动作表合并为 17 项含 `wait`）、`{zh-CN,en}/tools/index.md`（工具表合并一行）、`{zh-CN,en}/concepts/core-tools.md`（「桌面额外默认」列表合并）、`zh-CN/start/quickstart.md`（若提及）。
- 其它：`README.md`/`README-English.md` 工具表合并一行；`docs/API文档.md`、`docs/技术说明书/05-工具、技能、MCP与外部能力.md`、`docs/设计文档/05-工具、技能与 MCP 系统设计.md` 中相关描述同步。`docs/归档/*` 不动。
- 生成物重跑：`python scripts/build_docs_site.py`（重写 `web/docs`）、`python scripts/build_help_manual.py`（重写 `frontend-slint/assets/help/manual.{zh,en}.json`）。
- 功能日志：`python scripts/update_feature_log.py --type 重构 --scope 工具 ...`（并补一条 `移除` 记录桌面监视器）。

## 验证

1. `cargo check -p wunder-runtime -p wunder-core -p wunder-desktop -p wunder-cli`（含 `--features desktop-control` 与不带两种编译，确认 feature-gate 分支与主分支都能过）。
2. `cargo test -p wunder-runtime`（exec_policy / execute_support / catalog / user_agents / random_sim 相关单测）、`cargo test -p wunder-desktop`、`cargo test -p wunder-cli`。
3. 前端回归：`cd frontend && npm run test:regression`（或仓库既有脚本）跑 agent-settings-snapshot-regression 等。
4. 文档生成后 `git diff --stat web/docs frontend-slint/assets/help` 确认仅有桌面控制改动，无残留 `桌面监视器` 字符串（仓库范围 `rg` 复查一次）。
5. 桌面模式手测（可选）：蛹/桌面模式启动，模型对单一 `桌面控制` 工具分别调用 `left_click` 与 `wait` 动作，确认截图 followup 与倒计时/提示覆盖层事件正常。
