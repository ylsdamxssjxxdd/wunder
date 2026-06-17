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
- 如果系统结构、外部接口或重要协议有变化，要及时更新 `docs/API文档.md`、相关设计文档或技术说明书。
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

- Rust edition 明确使用 2021；工具链最低版本以根 `Cargo.toml` 的 `rust-version` 为准，当前为 1.92。
- 不要构建debug版本
- format! 中可以内联变量时使用 `{var}`，避免额外参数。
- 能合并的 if 语句请合并（clippy::collapsible_if）。
- 能用方法引用时优先用方法引用，减少多余闭包（clippy::redundant_closure_for_method_calls）。
- 测试中优先对完整对象做 `assert_eq!`，避免逐字段对比。
- Rust 代码变更完成后记得运行 `cargo check` 要消除所有错误和告警，必要时运行 `cargo clippy`。
- 为了避免cpu资源耗尽，请用 8 线程编译/测试等操作就行。

# 开发标准总则

- 做任何改动前，先判断它属于接入层、运行时内核、投影层、控制面、存储层、用户侧前端、管理端或分发壳层。目录落点优先按职责边界决定，不按“调用方便”决定。
- 默认复用现有框架、协议、状态模型和设计 token；只有在现有边界无法表达新能力时，才新增模块或抽象。
- 面向百万行级代码演进：新增能力尽量小文件、小模块、清晰公开面；超过 2000 行的文件只做维护性修复，新功能拆到新文件后接回入口。
- 保持“三形态同核”：server、desktop、cli 可以有接入差异，但线程、工具、存储、实时事件和权限语义必须共享同一套核心。
- 区分 durable state、实时投影、进程内临时态和 UI 派生态。不要让前端、管理端或渠道适配层成为后端真相来源。
- 所有外部可见行为变更必须同步考虑：API 文档、设计文档、技术说明书、使用说明书、功能迭代记录和回归测试。
- 性能是默认要求：避免无界队列、无界缓存、无分页查询、全量重算、深层 watch、大对象复制和长时间持锁。
- 日志和测试数据不得包含具体业务名称、真实身份、真实密钥、真实路径或暴露场景意图的示例值。
- 性能优化必须遵守 `docs/性能标准.md`：任何优化都要保留同环境基线、优化后对比和回退结论；Docker 文件操作类性能测试必须使用真实挂载语义，不得迁到临时目录或内存盘规避真实 IO。

## 改动前设计检查

1. 入口是谁：用户侧前端、管理端、desktop、cli、外部渠道、gateway 还是内部任务？
2. 权威状态在哪里：数据库、stream events、线程运行时、mission 运行时、workspace 文件还是本地 UI？
3. 是否影响线程 system prompt 冻结、长期记忆注入、工具权限、审批或多租户隔离？
4. 是否需要 PostgreSQL 与 SQLite 双实现、双验收，或只属于某个运行形态？
5. 是否需要 WebSocket snapshot、delta、replay、补水、断线重连和幂等处理？
6. 是否会让大文件继续膨胀？如果会，先拆模块再接入。
7. 是否需要新增或调整测试、API 文档、设计文档、使用说明书和功能迭代记录？

# 项目结构提示（以当前仓库结构为准，发现变化及时同步更新）

- 目录落点优先按“职责边界”而不是“调用方便”决定，避免跨层耦合。
- 当前 Rust 后端是 Cargo workspace，不再是根目录 `src/` 主工程。新增后端代码优先落到 `crates/wunder-runtime/`、`crates/wunder-server/`、`crates/wunder-cli/`、`crates/wunder-desktop/` 对应边界内。

## 顶层目录职责（当前仓库）

- `crates/`：Rust workspace 子 crate；当前包含 `wunder-core`、`wunder-runtime`、`wunder-server`、`wunder-cli`、`wunder-desktop`。
- `crates/wunder-core/`：低依赖稳定基础能力，适合放跨运行形态、低业务耦合的公共逻辑。
- `crates/wunder-runtime/`：wunder 后端核心运行时，承载 API、orchestrator、services、storage、gateway、channels、ops、sandbox 等主要能力。
- `crates/wunder-server/`：server 入口、middleware、静态资源挂载、启动装配与服务端协议壳。
- `crates/wunder-cli/`：CLI/TUI 运行形态源码，复用 runtime 核心，不另造平行运行时。
- `crates/wunder-desktop/`：Tauri 桌面运行时、本地 bridge、能力声明、打包配置与脚本。
- `frontend/`：用户侧前端（Vue3 + TypeScript）；主要代码在 `frontend/src/`，按 `api/`、`components/`、`realtime/`、`router/`、`stores/`、`views/`、`styles/` 等分层。
- `web/`：管理端/调试端前端（原生 HTML + JS 模块）；`modules/` 放业务模块，`styles/` 放样式，`shared/` 放共享前端工具，`docs/` 与 `simple-chat/` 放独立页面，`third/` 放第三方资源。
- `desktop/electron/`：Electron 桌面壳；`src/` 放主进程/预加载脚本，`resources/`、`scripts/`、`build/` 放打包资源与构建脚本。
- `config/`：运行配置与内置资源；`prompts/`、`knowledge/`、`skills/`、`fonts/`、`preset_worker_cards/` 等统一放这里，不放仓库根目录。
- `docs/`：API 文档、设计文档、技术说明书、使用说明书、功能迭代、经验教训等。
- `scripts/`：仓库级脚本；包含 `update_feature_log.py`、`build_docs_site.py`、回归、压测、备份脚本等。
- `tests/`：非 Rust 测试夹具与脚本；Rust 集成测试优先放到对应 crate 的 `tests/`。
- `extra_mcp/`：额外 MCP 运行时与工具脚本。
- `packaging/`：分发与部署资源；当前按 `docker/`、`python/`、`windows/` 组织。
- `patches/`：依赖补丁与兼容性补丁资源。
- `images/`：共享图片资源。
- `.cargo/`、`.github/`：构建工具链配置与仓库自动化配置。
- `target/`、`node_modules/`、`temp_dir/`、`crates/*/target/`、`desktop/electron/node_modules/`：构建或临时产物目录，不放业务源码与长期资料。

## 后端分层标准

- `crates/wunder-server/src/main.rs` 只负责启动装配、middleware、静态资源挂载、CORS、panic guard、语言与鉴权 guard 等服务外壳逻辑；不要把业务规则写进 server 入口。
- `crates/wunder-runtime/src/api/` 是 HTTP/WS 路由层。API 文件只做鉴权衔接、参数校验、协议转换、响应 shaping 和调用服务；复杂业务必须下沉到 services、orchestrator 或 storage。
- `crates/wunder-runtime/src/core/state.rs` 是 `AppState` 装配锚点，长期维持 `kernel / projection / control` 三层：`kernel` 放智能体执行与调度，`projection` 放用户世界和实时视图，`control` 放 presence、gateway、channels、cron、审批、命令会话等治理能力。
- `crates/wunder-runtime/src/orchestrator/` 是智能体执行主链路，承载 turn、prompt、context、memory、llm、tool calls、tool execution、parallel execution、retry、stream persist、result normalize 等核心语义。模型执行相关状态机不得散落到 API、前端或渠道层。
- `crates/wunder-runtime/src/services/runtime/thread/` 管理线程生命周期、lease、排队和 dispatch；任何推进线程的能力都必须尊重 ThreadRuntime，不要绕过它直接调用 orchestrator 制造并发写入。
- `crates/wunder-runtime/src/services/runtime/mission/` 管理 mission 和多智能体协作运行时；蜂群协作、任务分发和汇总不得混入普通聊天线程状态机。
- `crates/wunder-runtime/src/services/` 放领域服务和业务编排。服务可以组合 storage、workspace、runtime、projection，但不要承担 HTTP 请求解析或 UI 特判。
- `crates/wunder-runtime/src/storage/` 放 PostgreSQL / SQLite 统一抽象与实现。新增持久化能力优先扩展 `StorageBackend` 或明确的存储模块，并同时考虑 server PostgreSQL 与 desktop SQLite。
- `crates/wunder-runtime/src/channels/` 放外部渠道适配、队列、出站、附件、日志和限流；渠道层只做协议映射和投递治理，不定义线程认知语义。
- `crates/wunder-runtime/src/gateway/` 放 gateway 协议、节点调用和管理面消息；不要把 gateway 逻辑塞进 channels、tools 或普通 API。
- `crates/wunder-runtime/src/services/tools/`、`services/skills.rs`、`services/mcp.rs` 放工具、技能与 MCP 能力。工具描述要清晰、输入 schema 要严格、返回要精简，工具执行必须尊重审批、权限、超时和结果裁剪。
- `crates/wunder-runtime/src/ops/` 放监控、吞吐、性能与 benchmark；高风险链路必须能通过 tracing、monitor 或 benchmark 观察。
- `crates/wunder-runtime/src/sandbox/` 放沙盒服务接入与隔离逻辑，不要把沙盒特例扩散到通用工具实现。

## 后端设计细则

- API 命名和 payload 使用稳定字段；新增字段优先兼容旧客户端，删除字段必须确认原型阶段是否可直接清理，并同步文档。
- HTTP 适合配置、查询、低频命令；线程流、任务状态、投影同步和协作状态优先使用 WebSocket。实时协议必须包含可恢复的 snapshot、delta、replay 或等价补水机制。
- 流式事件必须有稳定 id、序号、会话或任务归属和事件类型；消费者要能去重、乱序缓冲、断线补水和幂等应用。
- 线程 system prompt 在线程首次确定后冻结；长期记忆只允许在线程初始化时注入一次，后续轮次不得改写 system prompt。
- 区分子智能体工具与蜂群工具：子智能体是主智能体临时创建的新工作单元，默认不阻塞；蜂群工具是调用已有智能体协作，默认阻塞并汇总。
- 多租户、用户、单位、工具权限和 workspace 归属必须在服务层或存储层校验，不能只靠前端隐藏入口。
- async 代码中不要直接执行阻塞 IO 或长 CPU 任务；使用已有 blocking/long_task 设施或专用任务池，并加超时、取消和日志。
- 数据库访问要分页、限量、建索引并避免 N+1 查询。列表接口必须明确排序键和稳定分页策略。
- 缓存必须有容量、过期或失效策略。实时缓冲、事件缓存、调试事件和工具结果都不能无界增长。
- 错误返回使用统一错误结构和合适 HTTP status；内部错误记录 tracing，外部响应避免泄漏路径、密钥、SQL、prompt 和敏感上下文。
- 配置读取通过 `ConfigStore` 或既有配置模块，避免散落环境变量解析。确需环境变量覆盖时，要集中在启动装配或配置解析边界。
- AppState 新增服务时，必须明确它属于 `kernel`、`projection`、`control` 还是横向基础设施，并检查 server、desktop、cli 默认启停矩阵。
- 新增外部依赖前，先确认是否能用已有 workspace dependency；新增依赖要考虑 Windows、离线分发、体积、许可和编译时间。

## 存储与数据模型标准

- server 默认使用 PostgreSQL，desktop 默认使用 SQLite。新表、新字段、新查询必须考虑两种后端的 SQL 差异、事务语义和索引能力。
- 业务层不得拼接散落 SQL；持久化细节应收口在 storage 或明确的 repository 风格模块。
- 写入 durable state 时要考虑幂等键、唯一约束、更新时间和审计字段；实时事件写入要能支持 replay。
- 大文本、大二进制、工具产物和 workspace 文件不要直接塞进热路径列表查询；使用引用、摘要、分块或文件存储。
- 删除能力要明确是软删除、归档还是物理删除；用户可见数据删除要同步投影和缓存失效。
- 桌面端 SQLite 路径、备份、迁移和并发访问要保守设计，避免长事务阻塞 UI 或本地 bridge。

## 用户侧前端设计标准

- 用户侧前端是“温暖的蜂巢”：界面应轻、稳、亲和、可持续工作；不是营销页，不做过度装饰，不用大面积空洞 hero，不用会拖慢旧浏览器的 `backdrop-filter`。
- 用户侧前端是后端投影消费者，不是线程真相来源。允许 optimistic UI，但最终必须接受后端 stream events 回压并收敛。
- 页面级文件只做布局、路由衔接和组合编排；可复用逻辑放 `frontend/src/views/messenger/*.ts`、`stores/`、`realtime/`、`utils/` 或组件内部，不继续向单个大 `.vue` 或 controller 文件堆复杂逻辑。
- `frontend/src/realtime/` 负责实时协议归一、事件 reducer、乱序处理、去重、replay、invariant 校验和 debug shadow；页面 watcher 不直接解释复杂 WS 事件。
- `frontend/src/stores/` 负责跨组件状态、缓存、发送动作、恢复动作和派生态。store action 要可测试、可幂等，避免依赖 DOM。
- `frontend/src/api/` 负责请求封装和 payload 适配。组件不直接散写 `fetch`/`axios` URL，不在组件里重复拼接鉴权、语言或错误处理。
- `frontend/src/components/` 组件默认只负责呈现和局部交互。业务组件要用 props/emits 暴露边界，避免直接修改无关 store。
- `frontend/src/views/messenger/sections/` 放消息器大区块，`frontend/src/components/messenger/` 放聊天域可复用组件，`frontend/src/components/chat/` 放聊天通用组件。
- 新增样式优先复用 `frontend/src/styles/base.css`、`styles/theme/`、`styles/chat/`、`styles/pages/` 和 `messenger.css` 中的变量与模式；不要在组件里散落大量一次性颜色。
- 视觉上保持蜂巢风格统一：温暖、清晰、克制、信息密度适中。按钮、列表、面板、对话框、右侧 dock、消息气泡和状态提示要像同一套产品，不要混用互相冲突的圆角、阴影和色彩语言。
- 固定格式 UI 要有稳定尺寸约束，例如 grid tracks、`minmax(0, 1fr)`、固定工具栏高度、头像尺寸、按钮尺寸、列表行高和面板宽度，避免 hover、加载文案或流式内容造成布局跳动。
- 长消息、长工具结果、长列表和历史记录必须考虑虚拟窗口、懒加载、折叠、裁剪和增量渲染。不要在 computed 中反复全量扫描大型历史。
- `setup` 阶段禁止先引用后初始化；涉及网络、WS、SSE 的 `watch` 默认不在 `setup` 阶段 `immediate` 启动。
- 组件卸载时必须取消订阅、请求、定时器、observer、object URL 和异步回调落地。异步回调写响应式状态前要确认作用域仍有效。
- 路由切换必须幂等，同路径同查询不重复写入；不要用强制改 `:key` 重建大面板来掩盖状态同步问题。
- 页面级卸载不得全局关闭共享连接；共享连接生命周期由 realtime/store 管理。
- 不要用纯前端特判掩盖后端状态不一致。发现协议缺口时，优先补后端事件、snapshot 或状态字段。
- 表单、弹窗、菜单和工具栏要支持键盘可达、焦点可见、错误提示明确、加载态和禁用态完整。
- 移动端和窄容器要按主工作流降级：优先保证消息、输入、会话切换和关键状态可用；次级面板用 overlay 或懒加载。
- 需要图片、头像、图标或预览资源时，优先使用已有 assets、主题变量和图标体系。不要引入沉重资源影响首屏。

## 管理端前端标准

- 管理端位于 `web/`，采用原生 HTML + JS 模块。新增业务逻辑放 `web/modules/`，样式放 `web/styles/`，共享小工具放 `web/shared/`。
- `web/app.js` 只做启动、导航和模块装配，不继续堆业务细节。
- 管理端是治理和调试界面，设计应密集、清晰、可扫描；不要做营销化布局或用户侧情绪化装饰。
- 管理端操作默认高风险，必须有清晰状态、错误反馈、确认边界和权限校验。前端确认不能替代后端鉴权。
- 管理端表格、日志和监控面板必须分页、筛选或限量，避免一次性渲染大量 DOM。

## Desktop 与 CLI 标准

- Desktop 本地能力优先放 `crates/wunder-desktop/`，Electron 仅作为 `desktop/electron/` 分发壳和系统集成壳，不承载核心业务语义。
- CLI/TUI 能力优先放 `crates/wunder-cli/`，复用 runtime 的 AppState、配置、工具和存储语义，不另起一套执行链路。
- Desktop 和 CLI 默认本地 SQLite，必须考虑离线、启动速度、路径权限、配置迁移和本地文件安全。
- 本地 bridge、LAN overlay、系统能力调用要最小权限、显式能力声明、可观测错误，不把系统能力默默暴露给远端。

## 常见开发落点规则

- 新增 HTTP/WS 接口：优先在 `crates/wunder-runtime/src/api/` 新增或扩展对应领域文件，业务逻辑放 `crates/wunder-runtime/src/services/`、`orchestrator/` 或 `storage/`；涉及外部可见行为时同步更新 `docs/API文档.md`。
- 新增模型编排、工具执行链路、上下文压缩、回合状态管理：优先放 `crates/wunder-runtime/src/orchestrator/`。
- 新增线程调度、任务排队、lease、dispatch：优先放 `crates/wunder-runtime/src/services/runtime/thread/` 或 `runtime/mission/`。
- 新增实时投影、watch、replay、presence：优先放 `crates/wunder-runtime/src/services/stream_events*`、`beeroom_realtime*`、`presence/` 或对应 WS API。
- 新增渠道接入或渠道收发链路：优先放 `crates/wunder-runtime/src/channels/`，路由暴露由 `api/` 接住。
- 新增 gateway 能力：优先放 `crates/wunder-runtime/src/gateway/`，不要和普通渠道或工具混淆。
- 新增存储读写或数据库适配：优先放 `crates/wunder-runtime/src/storage/`，并同时考虑 PostgreSQL 与 SQLite。
- 新增工具、技能、浏览器、运行时能力：优先拆到 `crates/wunder-runtime/src/services/tools/`、`services/browser/`、`services/runtime/`、`services/abilities/` 等现有子目录；公共注册或汇总逻辑再接回对应 `mod.rs` 或聚合文件。
- 新增用户端页面能力：优先按职责放到 `frontend/src/views/`、`frontend/src/components/`、`frontend/src/stores/`、`frontend/src/api/`、`frontend/src/realtime/`；消息器主链路相关改动优先落到 `frontend/src/views/messenger/` 及其配套组件。
- 管理端页面改动：业务逻辑放 `web/modules/`，样式放 `web/styles/`，独立文档或演示页放 `web/docs/` 或 `web/simple-chat/`。
- CLI/TUI 改动：优先放 `crates/wunder-cli/`。
- 桌面端改动：Tauri 相关放 `crates/wunder-desktop/`；Electron 相关放 `desktop/electron/src/`、`desktop/electron/resources/`、`desktop/electron/scripts/`。
- 内置提示词、知识、技能、字体、预设卡片等资源统一放 `config/` 对应子目录，不要放回仓库根目录。

## 测试与验收标准

- 后端 Rust 改动完成后，至少运行相关 crate 的 `cargo check -j 8`；触及共享逻辑、存储、运行时或工具执行时，继续运行定向 `cargo test -j 8` 或 `cargo clippy -j 8`。
- 前端 TypeScript/Vue 改动完成后，至少运行 `npm run typecheck` 或相关回归脚本；触及构建、路由、样式主链或依赖时运行 `npm run build:check`。
- 实时消息、聊天运行时、watch/replay、断线恢复、发送保护等改动，要优先运行 `frontend` 中对应 `test:chat-*`、`test:chat-realtime` 或 Playwright e2e。
- 管理端原生 JS 改动至少做浏览器手工验证或已有脚本验证；涉及 API 结构时同步验证后端响应。
- 存储改动必须覆盖 PostgreSQL 和 SQLite 的行为差异；无法同时实测时，要在最终说明中明确未覆盖的后端。
- 修改 `docs/使用说明书` 后，必须手动执行 `python scripts/build_docs_site.py`。
- 每次任务完成后必须通过脚本更新 `docs/功能迭代.md`，不要手写破坏分类结构。命令使用 `python scripts/update_feature_log.py --type <类型> --scope <范围> "<摘要>"`，类型仅限仓库指南中列出的固定值。
- 性能优化或回归结论应同步沉淀到 `docs/性能基线/`，报告文件名按 `YYYY-MM-DD-<scope>-<topic>.md` 命名，必要时附原始 JSON/trace 摘要到 `docs/性能基线/assets/`。
