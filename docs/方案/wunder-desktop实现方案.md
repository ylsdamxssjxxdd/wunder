# wunder-desktop实现方案

## 0. 目标与约束（对齐本次需求）

- 新增 `wunder-desktop`（Tauri GUI）与 `wunder-desktop-bridge`（无窗口桥接），Electron/AppImage 通过 bridge 壳运行；面向本地普通用户，保留 `server/cli` 现有形态不受影响。
- 核心能力复用 `src/`：提示词体系、智能体编排链路、工具系统、MCP/Skills、网关能力均沿用。
- 产物形态：Tauri 桌面窗口 + 可选 Electron AppImage（均复用本地 bridge）。
- 持久化目录默认程序同级：`./WUNDER_TEMPD`（SQLite + 配置 + 会话状态 + 用户工具配置）；Electron 版本通过 `--temp-root` 指向 `userData/WUNDER_TEMPD`。
- 智能体工作目录可由用户选择；默认 `程序所在目录/WUNDER_WORK`，Electron 版本通过 `--workspace` 指向 `userData/WUNDER_WORK`。
- 支持 MCP/Skills 配置与直接工具执行（包括执行命令和内置工具）。
- 工具调用协议支持 `function_call` 与 `tool_call` 双模式切换。
- 通讯策略遵循“WebSocket 优先，SSE 兜底”。

---

## 1. 现有代码可复用性分析（结论）

### 1.1 可直接复用的核心模块

1. **配置与状态初始化**
   - `src/core/config.rs` + `src/core/config_store.rs`：支持 base + override 合并、环境变量展开、持久化差异写回。
   - `src/core/state.rs`：`AppState::new_with_options` 已支持 `cli_default()` 轻量初始化选项（可作为 desktop 初始模板）。

2. **智能体编排链路**
   - `src/orchestrator/*`：会话、工具调度、流式事件、上下文压缩、token 统计、会话锁等核心逻辑完整。
   - `src/services/prompting.rs`：提示词模板、缓存、`tool_call/function_call` 双模式提示词拼接已具备。

3. **工具/MCP/Skills 体系**
   - `src/services/tools.rs`：内置工具、MCP 工具、Skills、知识库工具统一调度。
   - `src/services/user_tools.rs` + `src/services/skills.rs`：用户维度 MCP/Skills 持久化和加载机制可直接复用。

4. **存储与工作区**
   - `src/storage/sqlite.rs`：SQLite 初始化、WAL、busy_timeout、Schema 演进能力齐全。
   - `src/services/workspace.rs`：支持 `WUNDER_WORKSPACE_SINGLE_ROOT=1` 单根工作区模式，适配 desktop 本地单用户。

5. **WebSocket/SSE 通讯能力**
   - `src/api/chat_ws.rs`、`src/api/chat.rs`、`src/api/core_ws.rs` 已具备 WS + SSE 链路，可在 desktop 内部复用。

### 1.2 需要新增/拆分的部分

- 将 `wunder-cli/runtime.rs` 中“本地运行时目录与环境变量初始化逻辑”抽到可复用模块（供 CLI 与 Desktop 共用）。
- 新增 desktop 专属运行时配置文件（窗口状态、工作目录选择、UI 偏好等），与 wunder 主配置分层。
- 新增 desktop 壳层（Tauri）与前端入口，不侵入现有 `web`（管理员端）和线上 `frontend` 发布流程。

---

## 2. 技术栈方案（参考 cc-switch-main）

> 参考项目结论：Tauri 2 + Rust 后端 + 前端工程（Vite + TS）+ SQLite 持久化 + Commands/Services 分层。

### 2.1 推荐栈

- **桌面壳**：Tauri 2.x（Rust）
- **桌面前端**：Vue 3 + TypeScript + Vite（与现有用户前端技术保持一致，降低迁移成本）
- **桌面后端**：Rust（复用当前仓库 `src/` 核心模块）
- **本地存储**：SQLite（业务数据）+ YAML/JSON（运行时配置）
- **流式通讯**：WebSocket 优先，SSE fallback

### 2.2 分层方式（借鉴 cc-switch-main）

- **Desktop Commands 层**：处理窗口、目录选择、运行时开关、配置读写。
- **Desktop Services 层**：封装 `AppState`、配置更新、WS/SSE 服务启动与重启。
- **Core Domain 层**：直接复用 `src/`（orchestrator/tools/storage/prompting/gateway 等）。

---

## 3. 总体架构

```text
┌─────────────────────────────────────────────┐
│               Wunder Desktop UI             │
│      (Vue3 + TS, 面向本地普通用户)          │
└───────────────┬─────────────────────────────┘
                │
     Tauri Commands（配置/系统能力）
                │
┌───────────────▼─────────────────────────────┐
│         Desktop Host (Tauri + Rust)         │
│  - DesktopRuntimeBootstrap                    │
│  - DesktopConfigService                       │
│  - Internal WS/SSE Bridge                     │
└───────────────┬─────────────────────────────┘
                │ 复用
┌───────────────▼─────────────────────────────┐
│                  wunder core                 │
│ src/core + src/orchestrator + src/services   │
│ + src/storage + src/gateway + src/api(*)     │
└───────────────────────────────────────────────┘
```

说明：

- `(*)` desktop 不直接暴露完整 admin 路由，只挂载普通用户所需最小路由集合。
- 主链路仍是 wunder 现有链路，不重写 prompt/orchestrator/tool 执行内核。

---

## 4. 目录与持久化设计

## 4.1 运行目录（默认规则）

Tauri 版本以 `wunder-desktop(.exe)` 所在目录为 `APP_DIR`；Electron/AppImage 版本通过 `--temp-root`/`--workspace` 将运行目录落到 `userData`（避免只读目录问题）：

```text
APP_DIR/
  wunder-desktop(.exe)
  WUNDER_TEMPD/
    wunder_desktop.sqlite3
    config/
      wunder.override.yaml
      desktop.settings.json
    sessions/
      current_session.json
    user_tools/
      desktop_user/
        config.json
        skills/
        knowledge/
    vector_knowledge/
    logs/
  WUNDER_WORK/   # 默认工作目录（可在 UI 中改）
```

约束：

- 业务持久化不写入 `data/`。
- SQLite 固定在 `WUNDER_TEMPD/wunder_desktop.sqlite3`。
- 工作目录默认 `WUNDER_WORK`，支持用户改为任意可访问目录；Electron 版本通过 `--workspace` 指向 `userData/WUNDER_WORK`。

## 4.2 环境变量覆盖（desktop 启动时注入）

- `WUNDER_CONFIG_PATH`
- `WUNDER_CONFIG_OVERRIDE_PATH=./WUNDER_TEMPD/config/wunder.override.yaml`
- `WUNDER_USER_TOOLS_ROOT=./WUNDER_TEMPD/user_tools`
- `WUNDER_VECTOR_KNOWLEDGE_ROOT=./WUNDER_TEMPD/vector_knowledge`
- `WUNDER_PROMPTS_ROOT=<repo_or_bundle>`（指向包含 `prompts/` 子目录的根路径）
- `WUNDER_SKILL_RUNNER_PATH=<repo_or_bundle>/scripts/skill_runner.py`
- `WUNDER_WORKSPACE_SINGLE_ROOT=1`

---

## 5. Desktop 核心模块设计

## 5.1 DesktopRuntimeBootstrap

职责：

- 解析 `APP_DIR`、创建 `WUNDER_TEMPD` 与默认 `WUNDER_WORK`。
- 初始化 `ConfigStore` 并应用 desktop 默认配置。
- 创建 `AppState`（基于 `AppStateInitOptions::cli_default()`，再按 desktop 需要启用/禁用能力）。

desktop 默认配置建议：

- `server.mode = "desktop"`
- `storage.backend = "sqlite"`
- `storage.db_path = ./WUNDER_TEMPD/wunder_desktop.sqlite3`
- `workspace.root = <user_selected_or_default>`
- `channels.enabled = false`
- `agent_queue.enabled = false`
- `cron.enabled = false`
- `gateway.enabled = false`（默认关闭，保留开关）
- `sandbox.mode = "local"`

## 5.2 Internal WS/SSE Bridge

目标：前端沿用现有对话流式语义，减少重写。

- WS 主通道：`ws://127.0.0.1:<ephemeral>/wunder/chat/ws`
- SSE 兜底：沿用现有 chat stream/resume 接口。
- 启动时仅监听 `127.0.0.1`，端口随机分配；通过 Tauri command 回传前端。

安全措施：

- 仅本机回环地址监听。
- desktop 进程内生成一次性 token，前端请求必须携带。
- 路由白名单，仅开放 desktop 需要的 chat/core/tools/mcp/skills/workspace 接口。

## 5.3 Desktop Commands（Tauri）

建议命令清单：

- `desktop_get_runtime_info`：返回 `api_base/ws_base/session_token`。
- `desktop_get_settings` / `desktop_update_settings`。
- `desktop_pick_workspace_dir`：系统目录选择。
- `desktop_open_path`：打开工作目录/日志目录。
- `desktop_restart_runtime`：当工作目录、模型、工具模式改变时热重启 bridge。

## 5.4 MCP/Skills 管理

- 复用 `UserToolStore` 与 `UserToolManager`。
- Desktop UI 提供：列表、启停、增删、测试（可调用 `tool list`/`tool run` 同步校验）。
- Skills 搜索路径：
  1) 工作目录下 `skills/`
  2) 项目内置 `skills/`

## 5.5 工具调用模式切换（重点）

- 全局模式：写入 `llm.models.<model>.tool_call_mode`。
- 单次请求覆盖：通过 `config_overrides` 临时覆盖。
- UI 设计：
  - 模型设置页提供 `tool_call / function_call` 单选。
  - 聊天输入区域可临时切换“仅本轮生效”。

---

## 6. 关键流程设计

## 6.1 启动流程

1. 启动 `wunder-desktop`。
2. 初始化 `WUNDER_TEMPD` 与默认 `WUNDER_WORK`。
3. 读取 `desktop.settings.json`（若存在）确定工作目录与 UI 偏好。
4. 应用 Config 覆盖并初始化 `AppState`。
5. 启动 internal WS/SSE bridge。
6. 前端通过 Tauri command 获取 `api_base/ws_base/token`，建立 WS 连接。

## 6.2 聊天流程（WS 优先）

1. 前端发起 WS `connect/start`。
2. 后端复用 `orchestrator.stream()` 产生事件。
3. 工具调用、观察事件、最终回复按现有 event schema 下发。
4. 若 WS 异常，前端自动切换 SSE resume。

## 6.3 工作目录切换流程

1. 用户在设置中选择新目录。
2. 校验目录可读写并写入 `desktop.settings.json`。
3. 调用 `desktop_restart_runtime` 重建 `AppState`（不迁移旧数据，仅切换新工作根）。
4. 前端重连 WS 并继续可用。

## 6.4 MCP/Skills 改动生效

1. 用户更新 MCP/Skills 配置。
2. 更新 `user_tools` 持久化。
3. 清理 `UserToolManager` 技能缓存。
4. 后续会话自动按新配置生效（无需重启应用）。

---

## 7. 与现有模式的关系

- `wunder-server`：继续面向多租户/远程 API。
- `wunder-cli`：继续面向本地开发者。
- `wunder-desktop`：面向本地普通用户，单机单用户优先。

兼容原则：

- 不破坏 server/cli 路径与行为。
- 不要求 `user_id` 必须在用户管理中注册；desktop 仍支持虚拟 user_id 发起会话。
- token 统计沿用当前“上下文占用量”口径，不改统计含义。

---

## 8. 实施里程碑（节点清单）

## M0：工程骨架（1 天）

- 新建 `wunder-desktop/`（Tauri + Vue3 + TS）。
- 跑通最小窗口与 `desktop_get_runtime_info` command。

## M1：运行时落盘与配置分层（1~2 天）

- 完成 `WUNDER_TEMPD` 初始化。
- 接入 `ConfigStore` 与 desktop 默认覆盖策略。
- SQLite 路径切换到 `WUNDER_TEMPD`。

## M2：内置通信桥（2 天）

- 跑通 internal WS 主通道与 SSE fallback。
- 前端接入 runtime 动态 base_url，不再写死 localhost 固定端口。

## M3：会话与工具主链路（2~3 天）

- 跑通聊天、流式回复、工具执行、会话恢复。
- 跑通 `tool_call/function_call` 全局与单次切换。

## M4：MCP/Skills/工作目录（2 天）

- 完成 MCP/Skills 配置页。
- 完成工作目录选择与重启生效。

## M5：稳定性与打包（2 天）

- 增加异常恢复、崩溃保护、日志归档。
- 产出 `wunder-desktop` 打包脚本与安装包配置。

---

## 9. 性能与稳定性策略

- `AppState` 采用轻量初始化，避免不必要后台任务常驻。
- SQLite 继续 WAL + busy_timeout（沿用现有实现）。
- bridge 事件通道限流与背压，避免 UI 阻塞拖垮后端。
- 路由最小暴露，减少不必要处理开销与攻击面。
- 长稳运行策略：日志轮转、会话索引清理、配置原子写入。

---

## 10. 风险与应对

1. **安装目录不可写（Windows Program Files）**
   - 应对：首次启动检测写权限，提示用户切换到可写目录（便携模式目录）；仍保持默认“同级目录”优先。

2. **前端对固定 API 地址耦合**
   - 应对：统一改为 runtime command 注入动态 base_url/ws_url。

3. **WS 中断导致会话体验下降**
   - 应对：SSE resume 自动兜底；保留事件 id 断点续传。

4. **MCP/Skills 配置错误导致工具不可用**
   - 应对：提供“测试连接/测试调用”按钮与诊断结果。

---

## 11. 验收标准

- 启动后自动创建 `WUNDER_TEMPD` 与默认 `WUNDER_WORK`。
- 聊天可稳定流式返回（WS），断连可自动降级到 SSE 并恢复。
- MCP/Skills 配置可增删改启停并即时生效。
- 可直接执行命令工具与内置工具。
- `tool_call/function_call` 可在设置和请求级别切换，且调用结果正确。
- `cargo check`、`cargo clippy`、desktop 前端构建全部通过。

---

## 12. 文档联动要求（实施阶段）

本方案落地代码后，同步更新：

- `docs/设计方案.md`：新增 server/cli/desktop 三形态架构章节。
- `docs/API文档.md`：补充 desktop bridge 命令与内部通信协议说明。
- `docs/系统介绍.md`：补充本地普通用户使用路径与目录结构说明。


---

## 13. 当前落地状态（2026-02-11）

### 13.1 已完成（与本轮目标直接对应）

- `wunder-desktop` 默认启动形态已切到 **Tauri GUI**：双击程序直接打开桌面窗口；保留 `--bridge-only` 作为诊断模式。
- 新增 `wunder-desktop-bridge`（无窗口桥接）并配套 Electron 桌面壳方案，方便在旧版 Linux 发行版使用 AppImage。
- release 版本启用 `windows_subsystem = "windows"`，Windows 双击不再弹出终端窗口。
- 运行后自动创建 `WUNDER_TEMPD` 与默认 `WUNDER_WORK`，并落盘：
  - `WUNDER_TEMPD/wunder_desktop.sqlite3`
  - `WUNDER_TEMPD/config/wunder.override.yaml`
  - `WUNDER_TEMPD/config/desktop.settings.json`
- 已复用 `src/` 核心能力：提示词、智能体编排链路、工具系统、MCP/Skills、WS/SSE 流式链路。
- 桌面端 UI 已回归用户侧前端布局（不再使用桌面专属侧边栏）。
  - 聊天页复用 `ChatView`
  - 智能体应用页复用 `PortalView`
  - 设置页复用 `SettingsView`，并提供 MCP/Skills 入口与 `tool_call/function_call` 切换
- desktop 模式默认免登录：
  - 注入 `window.__WUNDER_DESKTOP_RUNTIME__`
  - 自动写入 `localStorage.access_token`
  - 路由默认进入 `/desktop/home`
- 已补齐运行时引导接口：`GET /config.json` 与 `GET /wunder/desktop/bootstrap`。
- 已提供 Tauri command：`desktop_runtime_info`（前端可直接读取 runtime 快照）。
- Tauri 相关工程资产已统一收敛到 wunder-desktop/（含 build.rs、tauri.conf.json、capabilities/、icons/）。
- 已支持 `--workspace`、`--temp-root`、`--frontend-root`、`--user`、`--bridge-only` 等运行参数。

### 13.2 协议与链路更新

- 聊天请求新增可选字段 `tool_call_mode`（`tool_call` / `function_call`）：
  - `POST /wunder/chat/sessions/{id}/messages`
  - `WS /wunder/chat/ws` 的 `start` payload
- 服务端会基于该字段生成本次请求的 `config_overrides.llm.models.<default>.tool_call_mode`，用于请求级协议切换。

### 13.3 已验证结果

- `npm run typecheck`（frontend）通过。
- `npm run build`（frontend）通过。
- `cargo check --bin wunder-server --bin wunder-cli --bin wunder-desktop` 通过。
- `cargo clippy --bin wunder-server --bin wunder-desktop -- -D warnings` 通过。
- 本地 smoke 测试通过：
  - `/config.json` 与 `/wunder/desktop/bootstrap` 返回 `mode=desktop` 和 runtime 信息；
  - `/wunder/chat/transport` 无 token 为 `401`，携带 token 可成功返回；
  - `/` 返回注入后的前端首页并包含 `__WUNDER_DESKTOP_RUNTIME__`。

### 13.4 当前范围说明

- 本轮已完成“本地普通用户双击可用”的 desktop 主链路落地（Tauri 桌面窗口 + 本地 bridge + 免登录 UI）。
- 安装器、自动升级、更多原生系统集成（托盘/通知/文件关联）可作为后续增强，不影响当前单二进制可用性。


## 14. 容器管理与系统设置落地细节（M4 补充）

### 14.1 后端设置模型

- `wunder-desktop/runtime.rs` 的 `DesktopSettings` 已扩展：
  - `container_roots: HashMap<i32, String>`
  - `language: String`
  - `remote_gateway`（仅 `enabled` 与 `server_base_url`，用于连接远端 wunder-server）
- 启动时会做归一化：
  - 容器 1 默认落到 `<app_dir>/WUNDER_WORK`；
  - 相对路径按 `app_dir` 解析为绝对路径；
  - 自动创建目录并写回 `WUNDER_TEMPD/config/desktop.settings.json`。

### 14.2 Desktop 设置接口

- 新增 `GET/PUT /wunder/desktop/settings`（`src/api/desktop.rs`）。
- `GET` 返回：
  - `workspace_root`（容器 1）
  - `container_roots`（容器到目录映射）
  - `language/supported_languages`
  - `llm`
  - `remote_gateway`（仅 `enabled` 与 `server_base_url`）
- `PUT` 更新后同步三处：
  1. `desktop.settings.json`
  2. `ConfigStore`（llm/i18n/workspace.container_roots）
  3. `WorkspaceManager.set_container_roots(...)`（即时生效）

### 14.3 前端页面与 TS 位置

- API 封装：`frontend/src/api/desktop.ts`。
- 容器管理页：`frontend/src/views/DesktopContainerSettingsView.vue`。
- 系统设置页：`frontend/src/views/DesktopSystemSettingsView.vue`。
- 路由入口：`frontend/src/router/index.ts`（`/desktop/containers` 与 `/desktop/system`）。
- 设置页入口：`frontend/src/views/SettingsView.vue`。

### 14.4 服务端接入（一期）

- 系统设置仅保留 1 个参数：`server_base_url`（服务端地址）。
- 点击“接入服务端”后，前端会立即切换到远端 API 基址并跳转到 `/login`。
- Desktop 不会自动创建账号、不会自动登录；用户按常规流程注册或登录。
- 服务端地址会持久化到 `desktop.settings.json`，并通过前端运行时覆盖机制即时生效（无需重启）。
- 远端模式下 runtime `token` 为空；登录成功后由正常鉴权流程写入 `access_token`。
- 本地 `desktop.settings` 接口继续使用 `desktop_token` 独立鉴权，确保本地设置管理始终可用。
- 当地址非法或不可达时保持本地模式，并通过 `remote_error` 返回诊断信息。
