# wunder-cli实现方案

## 1. 目标与定位

`wunder-cli` 是 wunder 的本地单用户运行形态，目标是在任意项目目录快速获得与 `/wunder` 同源的编排、工具与流式体验。

核心目标：

- 只交付一个可执行文件：`wunder-cli`。
- 运行时状态全部落在启动目录下 `WUNDER_TEMP/`。
- 智能体工作目录就是 CLI 启动目录。
- 核心复用 `src/`（orchestrator/tools/mcp/skills/prompting/gateway）。
- 保留 wunder 特色：`tool_call` 与 `function_call` 双协议切换。
- CLI 交互风格与命令组织对齐 `codex-main`（默认交互、子命令化管理、可 JSONL 输出）。

---

## 2. 总体原则

- **不复制核心代码**：CLI 直接引用 `wunder_server` 导出的核心模块。
- **目录物理隔离**：CLI 代码独立放在仓库根目录 `wunder-cli/`，便于维护边界。
- **运行时逻辑复用**：模型回合、工具执行、MCP/skills 解析全部复用现有实现。
- **单用户语义优先**：CLI 默认用户 `cli_user`，不依赖注册用户体系。
- **轻量运行**：关闭 server 端后台循环，降低常驻开销。

---

## 3. 目录与构建结构

### 3.1 代码目录

```text
wunder-cli/
  main.rs                        # 命令调度与主流程
  args.rs                        # clap 参数模型
  runtime.rs                     # 运行时初始化（WUNDER_TEMP、环境覆盖、轻量状态）
  render.rs                      # 流式事件渲染（text/jsonl）

src/
  ...                            # 继续复用现有核心能力
```

### 3.2 Cargo 注册

在根 `Cargo.toml` 新增：

```toml
[[bin]]
name = "wunder-cli"
path = "wunder-cli/main.rs"
```

说明：CLI 与 server 共享同一 crate，避免 workspace 多 crate 的额外复杂度。

---

## 4. 运行时持久化模型（WUNDER_TEMP）

在 CLI 启动目录（`launch_dir`）固定使用：

```text
./WUNDER_TEMP/
  wunder_cli.sqlite3
  config/
    wunder.override.yaml
  sessions/
    current_session.json
  user_tools/
    cli_user/
      config.json
      skills/
      knowledge/
  vector_knowledge/
  logs/
```

规则：

- 长期状态禁止落到 `data/`。
- `WUNDER_TEMP` 目录可整体迁移，实现本地“可拷贝会话环境”。
- CLI 每次启动都会确保目录存在。

---

## 5. 配置分层与环境覆盖

## 5.1 分层优先级（低 -> 高）

1. 基础配置：`config/wunder.yaml`（可由 `--config` 指定）
2. 本地覆盖：`WUNDER_TEMP/config/wunder.override.yaml`
3. CLI flag（当前进程）
4. 请求级 `config_overrides`（单次请求）

## 5.2 CLI 初始化默认覆盖

- `storage.backend = sqlite`
- `storage.db_path = ./WUNDER_TEMP/wunder_cli.sqlite3`
- `workspace.root = <launch_dir>`
- `server.mode = cli`
- `channels.enabled = false`
- `gateway.enabled = false`
- `agent_queue.enabled = false`
- `cron.enabled = false`
- `sandbox.mode = local`

## 5.3 路径类能力环境变量化

CLI 初始化时设置：

- `WUNDER_CONFIG_PATH`
- `WUNDER_CONFIG_OVERRIDE_PATH`
- `WUNDER_I18N_MESSAGES_PATH`
- `WUNDER_PROMPTS_ROOT`
- `WUNDER_SKILL_RUNNER_PATH`
- `WUNDER_USER_TOOLS_ROOT`
- `WUNDER_VECTOR_KNOWLEDGE_ROOT`
- `WUNDER_WORKSPACE_SINGLE_ROOT=1`

用途：确保从任意目录启动时，提示词、多语言、skills 执行器、用户工具目录都可正确定位。

---

## 6. 核心复用与必要改造点

## 6.1 直接复用

- `Orchestrator::run/stream`
- `services/tools::execute_tool`
- `services/mcp` 能力模型
- `services/skills` 的 SKILL 扫描与执行
- `ConfigStore` 覆盖持久化
- `StorageBackend` + SQLite 实现

## 6.2 必要改造（已规划/落地）

1. `AppState` 增加初始化选项（server/cli 两种模式）
   - CLI 默认关闭：team runner、agent runtime、cron、gateway maintenance。

2. `WorkspaceManager` 支持单根模式
   - 通过 `WUNDER_WORKSPACE_SINGLE_ROOT=1` 让工作区根目录直接等于 `workspace.root`。

3. `UserToolStore` 支持自定义根目录
   - 用 `WUNDER_USER_TOOLS_ROOT` 替换固定 `data/user_tools`。

4. `vector_knowledge` 支持自定义根目录
   - 用 `WUNDER_VECTOR_KNOWLEDGE_ROOT` 替换固定 `vector_knowledge/`。

5. `prompting` 支持提示词根目录
   - 用 `WUNDER_PROMPTS_ROOT` 确保 CLI 非仓库目录运行可加载提示词。

6. `skills` 支持自定义 skill runner 路径
   - 用 `WUNDER_SKILL_RUNNER_PATH` 指向仓库脚本。

---

## 7. 命令面设计（对齐 codex-main 风格）

## 7.1 全局参数

- `--model <name>`
- `--tool-call-mode <tool_call|function_call>`
- `--session <id>`
- `--json`
- `--lang <lang>`
- `--config <path>`
- `--temp-root <path>`
- `--user <id>`
- `--no-stream`

## 7.2 子命令

- `ask`：一次性提问
- `chat`：交互会话
- `tool run|list`：工具直调/列表
- `exec`：命令执行快捷入口（映射 `执行命令` 工具）
- `mcp list|add|remove|enable|disable`
- `skills list|enable|disable`
- `config show|set-tool-call-mode`
- `doctor`

## 7.3 默认行为

- 无子命令 + 有 `PROMPT`：执行一次任务。
- 无子命令 + 终端输入：进入交互模式。
- 无子命令 + 管道输入：读 stdin 执行一次任务。

这与 codex 的“默认进入主交互，子命令化扩展能力”保持一致。

---

## 8. 交互与事件输出

流式模式消费 `StreamEvent` 并渲染：

- `llm_output_delta`：增量输出
- `progress`：阶段提示
- `tool_call` / `tool_result`：工具行为可见
- `final`：最终回复
- `error`：错误输出

输出模式：

- 文本模式：面向人读
- JSONL 模式：面向脚本与流水线

---

## 9. tool_call / function_call 切换

## 9.1 设计

- 临时切换：`--tool-call-mode`
- 持久切换：`config set-tool-call-mode`

## 9.2 生效机制

- CLI flag：通过请求级 `config_overrides` 覆盖目标模型 `tool_call_mode`
- 配置命令：写入 override，后续默认生效

---

## 10. 单用户语义与兼容约束

- 默认 `user_id = cli_user`（可通过 `--user` 覆盖）。
- 不要求该用户在用户管理中注册。
- 会话持久化只维护当前用户视角。
- `/wunder` 多租户语义保持不变，CLI 仅做本地单用户投影。

---

## 11. 性能与稳定性策略

- CLI 模式不启动 server 常驻后台循环，减少 CPU/内存常驻占用。
- SQLite 继续使用 WAL + busy_timeout，保证本地并发写稳态。
- 会话与日志状态放入 `WUNDER_TEMP`，便于长期运行维护与迁移。
- `doctor` 用于快速定位模型配置、提示词路径、runner 路径异常。

---

## 12. 分阶段实施计划

### M1（已启动）

- 搭建 `wunder-cli/` 目录与 `[[bin]]`。
- 打通 `ask/chat/tool/exec/mcp/skills/config/doctor` 基础命令。
- 完成 `WUNDER_TEMP` 持久化与单根工作区模式。

### M2

- 强化交互体验（更接近 codex 的提示与状态输出）。
- 补齐命令层的错误提示与诊断信息。
- 增加更多 mcp/skills 管理动作（如导入/测试）。

### M3

- 增加 CLI 端集成测试（会话、工具、配置、MCP/skills）。
- 做稳定性回归（长会话、重启恢复、并发工具调用）。

---

## 13. 验收标准

- `cargo check` / `cargo clippy` 全量通过。
- `wunder-cli doctor` 正常输出运行时诊断。
- `wunder-cli tool list` 可展示工具能力。
- `wunder-cli` 默认可进入交互会话。
- `WUNDER_TEMP` 目录完整自动创建。
- `config set-tool-call-mode` 可持久化并可被 `config show` 读取。

---

## 14. 文档联动要求

当 CLI 功能继续扩展时同步维护：

- `docs/设计方案.md`：补充 CLI 架构章节
- `docs/API文档.md`：补充 CLI 命令面说明
- `docs/系统介绍.md`：补充 server + cli 双形态说明

当前文档采用 UTF-8 编码，后续新增内容统一保持 UTF-8，避免出现中文乱码。

## 用户原始任务
我现在想给项目增添wunder-cli模块，这样开发者/用户可以通过wunder-cli快速轻量的在本地使用类似wunder的能力，核心复用src中的。工作流程这样设计，产物只要一个wunder-cli，本地运行，在运行同级目录WUNDER_TEMP目录中存放sqlite数据库以及相关持久化配置文件。智能体的工作目录就是运行wunder-cli的那个目录。支持mcp skills的配置，可以直接执行命令或调用内置的工具。cli的实现直接参考C:\Users\32138\Desktop\参考项目\codex-main，整个wunder其实也是借鉴这个项目的。但是要有自己的特色，例如工具调用方式可以在function_call和tool_call两者间切换。提示词和智能体工作链路以及网关系统都沿用wunder的。wunder-server是面向多租户的，wunder-cli设计的时候只要面向一个用户即可，相关行为和设置要适配。请你仔细分析代码，给出wunder-cli实现方案.md到docs/方案中,注意明确好细节和节点。
