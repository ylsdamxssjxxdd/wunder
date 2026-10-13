---
title: 桌面控制
summary: `桌面控制器` 与 `桌面监视器` 在蜂窝桌面模式执行本机鼠标键盘动作与截图观察，返回统一骨架并追加视觉输入。
read_when:
  - 需要让智能体操作本地桌面应用（点击、输入、滚动、拖拽）
  - 需要截图观察桌面当前状态或动作后的变化
source_docs:
  - crates/wunder-runtime/src/services/tools/desktop_control_impl.rs
  - crates/wunder-runtime/src/services/tools/desktop_control.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/default_tool_profile.rs
  - crates/wunder-runtime/src/orchestrator/execute.rs
  - crates/wunder-core/src/config.rs
updated_at: 2026-10-13
---

# 桌面控制

| 规范名 | 别名 |
| --- | --- |
| `桌面控制器` | `desktop_controller`、`controller` |
| `桌面监视器` | `desktop_monitor`、`monitor` |

名字常量与别名见 `crates/wunder-runtime/src/services/tools/desktop_control_impl.rs:19-24`、`catalog.rs:1245-1258`。

## 什么时候用

- 目标只有一个**图形界面入口**、没有 CLI/API 可用时，才用 `桌面控制器` 点与输。
- 需要观察桌面当前状态、或确认上一步动作有没有生效，用 `桌面监视器`。
- 能用命令、文件或浏览器工具解决的，不要走桌面控制：它更慢、更易错、且直接操作用户的真实屏幕。

## 前置条件（三条都得满足）

1. **编译特性**：`desktop-control` 是 `wunder-runtime` 的可选 Cargo 特性且不在 default（`crates/wunder-runtime/Cargo.toml:11-13`）。未编译时两个工具存在但调用即返回 `ok=false`、`code=TOOL_DESKTOP_CONTROL_FEATURE_DISABLED`（`desktop_control.rs:78-99`）。
2. **桌面模式 + 显式开关**：`server.mode = desktop` **且** `tools.desktop_controller.enabled = true`（该开关默认 `false`）（`desktop_control_impl.rs:216-219`、`crates/wunder-core/src/config.rs:1256-1268`）。
3. **模型具备 vision**：`读图工具`、`桌面控制器`、`桌面监视器` 在模型不支持视觉时被过滤掉（`catalog.rs:1401-1418`）。

形态差异：蜂窝（桌面模式）把这三者列入**默认额外启用**集（`crates/wunder-runtime/src/services/default_tool_profile.rs:33-35`）；舰体/蜂巢/舵机形态下桌面工具不可用。

## 参数

### 桌面控制器

模型侧 schema（`catalog.rs:1042-1102`，实现解析 `desktop_control_impl.rs:538-581`）：

| 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `bbox` | 整数数组 | 是 | 4 元（`[x1,y1,x2,y2]`）或 2 元（中心点）；也接受同形态的 JSON 字符串；坐标是**归一化坐标** |
| `action` | string | 是 | 见下表 |
| `key` | string | 否 | `press_key` 用 |
| `text` | string | 否 | `type_text` 用 |
| `delay_ms` | integer | 否 | ≥0，默认 0（`delay` 用） |
| `duration_ms` | integer | 否 | ≥0；`drag_drop` 未给时按 400ms 平滑拖动 |
| `scroll_steps` | integer | 否 | ≥1，默认 1 |
| `to_bbox` | 整数数组 | 否 | 与 `bbox` 同形态，`drag_drop` 终点 |

动作枚举 17 个（`catalog.rs:1056-1074`，映射 `desktop_control_impl.rs:81-106`）：

`left_click`、`left_double_click`、`right_click`、`middle_click`、`left_hold`、`right_hold`、`middle_hold`、`left_release`、`right_release`、`middle_release`、`scroll_down`、`scroll_up`、`press_key`、`type_text`、`delay`、`move_mouse`、`drag_drop`。

实现层还接受宽松别名：`keyboard`→`press_key`、`send_text`→`type_text`、`sleep`→`delay`、`move`→`move_mouse`、`drag`→`drag_drop`（连字符与空格自动归一）。未知动作报"未知动作"。

**坐标口径**：`bbox` 用的是归一化网格，默认 `1000 x 1000`（`tools.desktop_controller.norm_width` / `norm_height`，`config.rs:1658-1664`）。运行时取 bbox 中心、夹到归一网格内，再线性映射到真实屏幕像素（`desktop_control_impl.rs:227-238`、`697-708`）。

`description` 不在模型侧 schema 中：缺省用动作名本身填充，仅用于前端覆盖层提示（`desktop_control_impl.rs:550-556`；对应测试 `desktop_controller_schema_no_longer_requires_description_for_model_side`）。

### 桌面监视器

| 字段 | 类型 | 必填 | 约束 |
| --- | --- | --- | --- |
| `wait_ms` | integer | 是 | 0–30000；实现另接受 `wait` / `delay_ms`，超过 30000 被夹到 30000（`desktop_control_impl.rs:26`、`583-594`） |
| `note` | string | 否 | 观察备注，原样回显在 `data.note` |

## 返回

统一骨架 `ok/action/state=completed/summary/data`（`desktop_control_impl.rs:374-396`、`428-447`）。

`桌面控制器` 的 `data`：`action`、`description`、`center_norm`、`center_screen`、`normalized_width`、`normalized_height`、`screen_width`、`screen_height`、`screenshot_path`、`previous_screenshot_path`、`screenshot_download_url`、`screenshot_bytes`、`elapsed_ms`、`followup_prompt`。

`桌面监视器` 的 `data`：`wait_ms`、上述截图与尺寸字段、`followup_prompt`、`note`。

**每次动作后都会截一张图**：控制器不返回动作前的屏幕状态，动作结果由紧随其后的视觉输入给出。

## 视觉追加：结果之后还有一条多模态输入

`桌面控制器` / `桌面监视器` 成功返回后，编排器会用结果里的截图路径组装一条**用户角色的多模态消息**追加给模型：文本提示 +（若有）动作前截图 + 当前截图（`crates/wunder-runtime/src/orchestrator/execute.rs:1281-1293`、`desktop_control_impl.rs:450-486`）。单张截图进上下文上限 8 MiB，超出即失败（`MAX_SCREENSHOT_BYTES`，`desktop_control_impl.rs:27`、`518-522`）。

也就是说：模型看到的动作结果本身不含图像，画面是通过这条追加的多模态用户消息进来的。规划任务时按"一次动作 = 一次工具调用 + 一次看图"估算成本。

## 覆盖层事件

仅当事件流可用时，动作执行前会发 `desktop_controller_hint`（含坐标与描述，提示约 2000ms + 120ms 抖动），完成后发 `desktop_controller_hint_done`；监视器发 `desktop_monitor_countdown` / `desktop_monitor_countdown_done`（`desktop_control_impl.rs:28-31`、`132-147`、`241-258`、`349-367`、`403-418`）。这给用户在真实屏幕上看到"AI 正在点这里"的红点提示。

## 失败

- 特性未编译：`TOOL_DESKTOP_CONTROL_FEATURE_DISABLED`（`desktop_control.rs:88-96`）。
- 非桌面模式或未开启开关：返回"桌面控制已禁用"错误（`desktop_control_impl.rs:682-687`）。
- 参数缺失：`bbox` 必填、`action` 必填、`wait_ms` 必填，缺失各自报错；`bbox` 形态不对报非法 bbox。
- 截图失败：捕获超时默认 5000ms（下限 100ms），失败向上报错（`desktop_control_impl.rs:710-719`、`config.rs:1666-1672`）。
- 结构化失败带 `error_meta{code,hint,retryable,retry_after_ms}`（`crates/wunder-runtime/src/services/tools/tool_error.rs:4-50`）。

## 边界

- **单会话帧缓存有界**：每会话保留上一帧路径，缓存条目上限 2048，超限时先剔除已不存在的文件再淘汰（`desktop_control_impl.rs:32`、`149-188`）。
- 截图目录按帧保留：每次捕获后清理旧帧，只留最近 `max_frames` 张（默认 3），避免临时截图无限堆积（`desktop_control_impl.rs:710-731`、`784-811`、`config.rs:1666-1668`）。
- 截图会另存一份到用户容器目录；持久化失败只记警告，不影响工具结果（`desktop_control_impl.rs:734-782`）。
- 动作是**真实输入注入**，没有回滚：`left_hold` 与 `left_release` 必须成对使用，否则鼠标状态可能残留；`drag_drop` 是按下—平滑移动—松开三步。
- 屏幕坐标随分辨率变化，归一化坐标只保证同一次观察与动作之间一致；换屏/改分辨率后要重新 `桌面监视器` 观察。
- 桌面能力遵循最小权限：需要用户显式开关，不做静默常驻截屏，也不把系统能力暴露给远端。

## 相关页面

- 读本地图片/视频帧：[读图工具](/docs/zh-CN/tools/read-image/)
- 网页内的交互：[浏览器工具族](/docs/zh-CN/tools/browser/)
