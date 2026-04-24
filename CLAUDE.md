# 仓库指南

- wunder (心舰) 是一个面向组织或用户的智能体调度系统，wunder拥有三种运行形态：server（服务，云端）、cli（命令行，本地）、desktop（桌面，本地），三种形态可各自独立运行或分发。server是项目核心，支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，并内置工具链、知识库与长期记忆能力。cli与desktop基于server构建。个人用户主要使用desktop，也是wunder主推的应用。
- 项目会拥有百万行级别的代码，请做好设计，但是不要提前过度抽象
- 核心理念是对开发者来说一切都是接口，对大模型来说一切皆工具
- wunder 要有可供多人并发访问的良好性能
- 注意保持优雅的项目结构和模块组成，始终要考虑系统的运行效率，速度要快，内存占用要低
- 每次完成任务，将实现内容写入 `docs/功能迭代.md` 的分类区块，使用 `python scripts/update_feature_log.py --type <类型> --scope <范围> ...`；类型仅限：新增/变更/修复/性能/文档/重构/安全/工程/测试/移除/弃用。
- 更新 `docs/使用说明书` 后，必须手动执行 `python scripts/build_docs_site.py`，确保管理员侧与用户侧帮助文档使用同一份最新内容；使用说明书内的图示资源统一放在 `docs/使用说明书/assets/`。
- 不要尝试创建git分支或提交，这些交给用户，你git diff时可能会遇到出现了不是你修改的内容，没关系那是用户自己改的不用管他
- 当前开发处于原型阶段，老旧的代码和不合适的字段或架构直接移除保持项目干净
- 文件统一使用 UTF-8 编码保存，避免 ANSI/GBK 混用导致中文乱码
- wunder 网页端系统使用postgres作为数据库，桌面端使用sqlite3，设计时请考虑性能和稳定性
- 区分用户侧前端（frontend目录，vue3实现） 管理员侧前端（web目录，html实现）
- frontend中有大量的依赖库，搜索时会返回大量内容，一定要做好限制，不要直接搜索frontend的根目录
- 前端不要使用backdrop-filter样式，这在老的浏览器中会很卡
- 会话轮次拆分为“用户轮次/模型轮次”：用户每发送一条消息记 1 轮用户轮次；模型每执行一次动作（模型调用、工具调用或最终回复）记 1 轮模型轮次；一次会话可包含多轮用户轮次，每轮用户轮次可包含多轮模型轮次。
- 你在开发的过程中，其他开发者也会在修改文件，如果你遇到了不是你修改的文件，不必理会
- 后端的开发，明确好功能需求，并且一定做好测试，保证上线可用，但是注意不要改一下测试一下，而是尽量改完后统一编译测试，提高效率
- 超过2000行代码的文件，视为维护状态，新增的功能请用新的文件，做好模块化设计，便于后期维护和协同开发
- 用户侧前端风格一定要保持统一，注意美观和协调
- 代码关键的地方，逻辑强的地方要有英文注释，方便后期维护
- 反复出现的问题，解决后记得把经验写为一条记录到docs\经验教训.md里，避免下次再犯同样的错误
- 不要在测试/代码或者文档记录中出现具体名称/业务的示例值，避免暴露任何场景意图

# 系统设计准则

- 前端是温暖的蜂巢，后端是庞大的飞船
- 如果系统结构或重要的部分有变化要及时更新docs/API/。
- 智能体线程的系统提示词一旦在该线程首次确定后保持冻结；长期记忆只允许在线程初始化时注入一次，后续轮次不得再次改写线程 system prompt，避免破坏大模型api的提示词缓存。
- 重要：做之前做好详细的规划，充分考虑边界情况和错误处理，避免技术债
- 各种通讯的实现以WebSocket为主
- 简单，快速，直观，易用，稳定，轻量

# 智能体设计准则

- 以精巧/高效/稳定为核心，不要过度抽象
- 设计工具时，其本身的描述要详细，减少歧义，方便模型调用。返回的结果要精简明确节省上下文。
- 小心不要将wunder的子智能体工具（主智能体创建新的智能体临时工作，不阻塞）与蜂群工具（母蜂利用已存在的智能体，阻塞）搞混
- 智能体的主线程是当前它的一等现实状态，新的任务都要落到主线程来
- 被智能体蜂群工具唤起的工蜂默认新建线程，并作为主线程工作。这是为了它的上下文干净

# Rust 开发提示

- 不要构建debug版本
- format! 中可以内联变量时使用 `{var}`，避免额外参数。
- 能合并的 if 语句请合并（clippy::collapsible_if）。
- 能用方法引用时优先用方法引用，减少多余闭包（clippy::redundant_closure_for_method_calls）。
- 测试中优先对完整对象做 `assert_eq!`，避免逐字段对比。
- Rust 代码变更完成后记得运行 `cargo check` 要消除所有错误和告警，必要时运行 `cargo clippy`。
- 为了避免cpu资源耗尽，请用 8 线程编译/测试等操作就行。

# 项目结构提示（以当前仓库结构为准，发现变化及时同步更新）

- 目录落点优先按“职责边界”而不是“调用方便”决定，避免跨层耦合。

## 顶层目录职责（当前仓库）

- `src/`：Rust 后端主工程，承载 server 核心能力，也提供 CLI/desktop 共用的核心逻辑。
- `frontend/`：用户侧前端（Vue3 + TypeScript）；主要代码在 `frontend/src/`，按 `api/`、`components/`、`realtime/`、`router/`、`stores/`、`views/` 等分层。
- `web/`：管理端/调试端前端（原生 HTML + JS 模块）；`modules/` 放业务模块，`styles/` 放样式，`docs/` 与 `simple-chat/` 放独立页面，`third/` 放第三方资源。
- `wunder-cli/`：CLI 运行形态源码；当前是扁平 Rust 文件布局，`tui/` 放终端界面相关实现。
- `desktop/tauri/`：Tauri 桌面运行时、本地桥接、能力声明、打包配置与脚本。
- `desktop/electron/`：Electron 桌面壳；`src/` 放主进程/预加载脚本，`resources/`、`scripts/`、`build/` 放打包资源与构建脚本。
- `config/`：运行配置与内置资源；当前 `prompts/`、`knowledge/`、`skills/`、`fonts/`、`preset_worker_cards/` 等都在这里，不在仓库根目录。
- `docs/`：API 文档、设计文档、技术说明书、使用说明书、功能迭代、经验教训等。
- `scripts/`：仓库级脚本；包含 `update_feature_log.py`、`build_docs_site.py`、回归/压测/备份脚本等。
- `tests/`：Rust 集成/回归测试，以及少量 Python/HTML/MJS 测试夹具。
- `extra_mcp/`：额外 MCP 运行时与工具脚本。
- `packaging/`：分发与部署资源；当前按 `docker/`、`python/`、`windows/` 组织。
- `patches/`：依赖补丁与兼容性补丁资源（如 `tokio-xmpp/`、`win7/`）。
- `images/`：共享图片资源。
- `.cargo/`、`.github/`：构建工具链配置与仓库自动化配置。
- `target/`、`node_modules/`、`temp_dir/`、`desktop/tauri/target/`、`desktop/electron/node_modules/`：构建/临时产物目录，不放业务源码与长期资料。

## `src/` 后端分层建议

- `src/main.rs`、`src/lib.rs`：服务入口与公共导出；`src/request_limits.rs` 放请求尺寸/配额类公共限制。
- `src/api/`：HTTP/WS 路由层，当前以扁平文件按领域拆分，如 `admin*.rs`、`chat*.rs`、`beeroom*.rs`、`user_*.rs`、`*_ws.rs`；保持薄层，负责协议编排，不堆核心业务。
- `src/services/`：核心业务实现层；除少量公共 `.rs` 文件外，已按 `abilities/`、`bridge/`、`browser/`、`cron/`、`directory/`、`presence/`、`projection/`、`runtime/`、`sim_lab/`、`swarm/`、`tools/` 等目录拆分。
- `src/orchestrator/`：模型调度与回合执行主链路；包含上下文压缩、预检、流式事件、工具调用、并行工具执行、线程运行态、重试治理等。
- `src/channels/`：外部渠道适配与运行时；当前包含飞书、企业微信/微信、WhatsApp Cloud、XMPP、QQBot 等渠道，以及队列、出站、附件、日志、限流等基础设施。
- `src/storage/`：存储抽象与实现；当前集中在 `postgres.rs`、`sqlite.rs`、`bridge.rs`，不要把 SQL/持久化细节散落到业务层。
- `src/core/`：基础设施能力；包括配置、状态、鉴权、审批策略、Schema、路径与文件工具、Python 运行时等。
- `src/ops/`：监控、性能、吞吐、benchmark 相关实现。
- `src/sandbox/`：沙盒服务接入。
- `src/bin/`：本地仿真/压测二进制入口，如 `backend_sim.rs`、`swarm_sim.rs`、`swarm_flow_sim.rs`。
- `src/gateway/`、`src/lsp/`：当前是独立边界的入口模块；后续扩展时继续保持独立，不要把网关/LSP 逻辑塞回其它层。

## 常见开发落点规则

- 新增 HTTP/WS 接口：优先在 `src/api/` 新增对应领域文件或扩展现有领域文件，并把业务逻辑放到 `src/services/`；涉及外部可见行为时同步更新 `docs/API文档.md`。
- 新增模型编排、工具执行链路、上下文压缩、回合状态管理：优先放 `src/orchestrator/`，不要误塞进 `src/services/`。
- 新增渠道接入或渠道收发链路：优先放 `src/channels/`，路由暴露再由 `src/api/` 接住。
- 新增存储读写或数据库适配：优先放 `src/storage/`，不要在 service/api 中散写 SQL。
- 新增工具、技能、浏览器、运行时能力：优先拆到 `src/services/tools/`、`src/services/browser/`、`src/services/runtime/`、`src/services/abilities/` 等现有子目录；公共注册/汇总逻辑再接回对应 `mod.rs` 或聚合文件。
- 新增用户端页面能力：优先按职责放到 `frontend/src/views/`、`frontend/src/components/`、`frontend/src/stores/`、`frontend/src/api/`、`frontend/src/realtime/`；消息器主链路相关改动优先落到 `frontend/src/views/messenger/` 及其配套组件，不要继续把复杂逻辑堆进单个大视图文件。
- 管理端页面改动：业务逻辑放 `web/modules/`，样式放 `web/styles/`，独立文档/演示页放 `web/docs/` 或 `web/simple-chat/`；避免把大段耦合逻辑继续堆进 `web/app.js`。
- CLI/TUI 改动：优先在 `wunder-cli/` 根级 Rust 文件或 `wunder-cli/tui/` 内分模块实现，不要另起一套平行目录。
- 桌面端改动：Tauri 相关放 `desktop/tauri/`；Electron 相关放 `desktop/electron/src/`、`desktop/electron/resources/`、`desktop/electron/scripts/`。
- 内置提示词、知识、技能、字体、预设卡片等资源统一放 `config/` 对应子目录，不要再放回仓库根目录。
- 每次任务完成后必须通过脚本更新 `docs/功能迭代.md`，不要手写破坏分类结构。

