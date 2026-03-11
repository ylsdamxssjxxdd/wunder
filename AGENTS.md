# 仓库指南

- wunder 是一个面向组织或用户的智能体调度系统，wunder拥有三种运行形态：server（服务，云端）、cli（命令行，本地）、desktop（桌面，本地），三种形态可各自独立运行或分发。server是项目核心，支持多租户、用户与单位管理、智能体应用构建与发布、网关统一接入与调度，并内置工具链、知识库与长期记忆能力。cli与desktop基于server构建。个人用户主要使用desktop，也是wunder主推的应用。
- 核心理念是对开发者来说一切都是接口，对大模型来说一切皆工具
- 基于 Rust Axum 最终暴露出 /wunder端口 需要用户id和用户问题，可流式返回中间过程和最终回复
- wunder内置了一组必要的工具链用于基本操作，例如读文件，写文件，编辑文件，搜索内容，列出文件，替换内容，执行命令，ptc等工具
- wunder可供多人并发访问，系统会为不同id用户开辟一块空间作为工作区，这些工作区将持久化
- 注意保持优雅的项目结构和模块组成
- 始终要考虑系统的运行效率，速度要快，内存占用要低
- 每次完成任务，将实现内容写入 `docs/功能迭代.md` 的分类区块，使用 `python scripts/update_feature_log.py --type <类型> --scope <范围> ...`；类型仅限：新增/变更/修复/性能/文档/重构/安全/工程/测试/移除/弃用。
- 如果系统结构或重要的部分有变化要及时更新docs/设计方案.md和docs/API文档.md和docs/系统介绍.md等文档。
- 不要尝试创建git分支或提交，这些交给用户
- 不要主动使用git除非必须，你git diff时可能会遇到出现了不是你修改的内容，没关系那是用户自己改的不用管他
- 当前开发处于原型阶段，数据库等不需要考虑对之前的兼容性，老旧的代码直接删除即可
- 注意系统中对token的统计，记录的是token占用量即实际的上下文占用而不是总的消耗量。
- 确保系统能流畅稳定运行10年以上
- 文件统一使用 UTF-8 编码保存，避免 ANSI/GBK 混用导致中文乱码
- 注意data目录是临时的不要在里面存东西
- wunder 网页端系统使用postgres作为数据库，桌面端使用sqlite3，设计时请考虑性能和稳定性
- 区分用户侧前端（frontend目录，vue3实现） 管理员侧前端（web目录，html实现）
- 智能体线程的创建用户（可以是任意虚构的名称）和实际注册用户（用户管理页面控制）注意区分开，确保/wunder接口可以顺畅调用，传入的用户id不需要是已注册的用户
- frontend中有大量的依赖库，搜索时会返回大量内容，一定要做好限制，不要直接搜索frontend的根目录
- 前端不要使用backdrop-filter样式，这在老的浏览器中会很卡
- 用户侧前端维护了浅/深两套主题，在修改界面时注意整体考虑页面搭配，避免颜色不适配的情况
- 会话轮次拆分为“用户轮次/模型轮次”：用户每发送一条消息记 1 轮用户轮次；模型每执行一次动作（模型调用、工具调用或最终回复）记 1 轮模型轮次；一次会话可包含多轮用户轮次，每轮用户轮次可包含多轮模型轮次。
- 各种通讯的实现以WebSocket为先，SSE作为兜底
- 你在开发的过程中，其他开发者也会在修改文件，如果你遇到了不是你修改的文件，不必理会
- 后端的开发，明确好功能需求，并且一定做好测试，保证上线可用
- 超过2000行代码的文件，视为维护状态，新增的功能请用新的文件，做好模块化设计，便于后期维护和协同开发
- 注意 src\services\tools.rs frontend\src\views\MessengerView.vue 代码行数太多了，请不要在往里面添内容了，后续的开放中逐步将功能拆解出来。
- 用户侧前端风格一定要保持统一，注意美观和协调
- 代码关键的地方，逻辑强的地方要有英文注释，方便后期维护

### 前端稳定性（Vue）

- `setup` 阶段禁止出现“先引用后初始化”的时序错误：凡是会被 `computed/watch/watchEffect`（尤其 `immediate: true`）调用的函数，必须使用函数声明（`function fn(){}`）或定义在调用点之前，避免 TDZ（`Cannot access 'x' before initialization`）导致整棵组件更新链路崩溃。
- 涉及网络请求、WS/SSE 订阅、重渲染调度的 `watch`，默认不使用 `immediate` 直接启动；需在 `onMounted` 后通过 `mounted/disposed` 双标记控制启动与停止，卸载后严禁再写响应式状态。
- 组件卸载时优先“取消订阅/取消请求/清定时器/断开观察器”，不要对 Vue 管理的节点做破坏性 DOM 操作（如对挂载容器直接 `innerHTML = ''`），避免触发 `patchElement` 空节点异常。
- 路由切换必须幂等：同路径同查询参数不重复 `router.push/replace`；同一交互帧内避免多次路由写入，统一走去抖或 token 化调度，防止组件树并发重排。
- 禁止为规避状态同步问题而滥用 `:key` 强制重建大面板（例如聊天主体/蜂群工作台）；优先修复状态流与副作用边界，减少 remount 竞态。
- 共享连接（如模块级 WS multiplexer）不得在单个页面组件卸载时全局 `close`；页面级仅取消本次请求/订阅，由连接层按空闲策略回收。
- 异步回调落地前必须检查作用域有效性（如 `if (disposed) return`），包括 `Promise.then/catch/finally`、`requestAnimationFrame`、`setTimeout`、`ResizeObserver` 回调。

## Rust 开发提示

- 不要构建debug版本
- format! 中可以内联变量时使用 `{var}`，避免额外参数。
- 能合并的 if 语句请合并（clippy::collapsible_if）。
- 能用方法引用时优先用方法引用，减少多余闭包（clippy::redundant_closure_for_method_calls）。
- 测试中优先对完整对象做 `assert_eq!`，避免逐字段对比。
- Rust 代码变更完成后记得运行 `cargo check` 要消除所有错误和告警，必要时运行 `cargo clippy`。

## 项目结构提示（可能滞后，你可以根据你看到的最新情况来更新）

- 目录落点优先按“职责边界”而不是“调用方便”决定，避免跨层耦合。

### 1) 顶层目录职责（以当前仓库为准）

- `src/`：Rust 后端主工程（server 核心）。
- `frontend/`：用户侧前端（Vue3，浅色/深色双主题）。
- `web/`：管理端/调试端前端（原生 HTML + JS 模块）。
- `wunder-cli/`：CLI 运行形态（含 TUI）。
- `wunder-desktop/`：Tauri 桌面形态与本地桥接。
- `wunder-desktop-electron/`：Electron 桌面壳与打包资源。
- `config/`：基础配置与 i18n/font/matplotlib 等运行配置。
- `docs/`：设计、API、系统介绍、功能迭代等文档。
- `scripts/`：仓库级脚本（含 `update_feature_log.py`）。
- `tests/`：后端集成/回归测试。
- `prompts/`：系统提示词模板（`zh/`、`en/`）。
- `knowledge/`：知识库内容。
- `extra_mcp/`：额外 MCP 运行时与工具脚本。
- `docker-extra/`、`packaging/`：构建与分发附加资源。
- `images/`、`fonts/`：图标与字体资源。
- `target/`、`frontend/node_modules/`、`temp_dir/`：构建/临时产物目录，不放业务源码与长期资料。

### 2) `src/` 后端分层建议

- `src/api/`：HTTP/WS 路由与请求/响应编排，保持薄层，避免堆业务逻辑。
- `src/services/`：核心业务实现（chat、agents、tools、skills、cron、workspace 等）。
- `src/orchestrator/`：模型调度、工具执行编排、上下文与流式事件控制。
- `src/channels/`：外部渠道适配（企微/飞书/WhatsApp/XMPP/QQ 等）。
- `src/storage/`：存储抽象与实现（Postgres/SQLite）。
- `src/core/`：配置、状态、鉴权、通用工具函数与基础能力。
- `src/sandbox/`：沙盒服务与运行时接入。
- `src/ops/`：监控、性能、吞吐、评估等运维能力。
- `src/bin/`：仿真/演练入口（`*_sim`）。
- `src/gateway/`、`src/lsp/`：网关与 LSP 相关入口模块（按现有结构扩展）。

### 3) 常见开发落点规则

- 新增接口：优先改 `src/api/*` + `src/services/*`，并同步更新 `docs/API文档.md`。
- 新增工具：优先在 `src/services/tools/` 新建文件并接入注册链路；不要继续向 `src/services/tools.rs` 堆功能。
- 新增用户端页面能力：优先拆分到 `frontend/src/views/messenger/` 与 `frontend/src/components/messenger/`；不要继续向 `frontend/src/views/MessengerView.vue` 堆功能。
- 管理端页面改动：放在 `web/modules/` 与 `web/styles/`，避免在 `web/app.js` 写大段耦合逻辑。
- 桌面端资源/打包改动：优先放 `wunder-desktop-electron/resources/`、`wunder-desktop-electron/scripts/` 或 `wunder-desktop/` 对应模块。
- 每次任务完成后必须通过脚本更新 `docs/功能迭代.md`，不要手写破坏分类结构。

### 4) 协作与维护约束

- 单文件超过 2000 行视为维护态，新增功能请拆新文件并在原处保留最小入口。
- 搜索前端代码时限制范围到 `frontend/src` 下具体子目录，不要直接扫 `frontend/` 根目录。
- 运行时/临时数据不要沉淀在业务源码目录，示例与文档放 `docs/`，脚本放 `scripts/`，知识内容放 `knowledge/`。
