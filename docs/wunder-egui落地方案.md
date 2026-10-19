# wunder-egui 落地方案

> 状态：实验实现推进中，原生聊天与蜂巢视觉对齐已落地，替换验收未完成（2026-10-19）  
> 目标：在保留 `frontend-slint/` 作为当前默认蛹前端的前提下，建立一套可独立运行、可比较、可渐进接管的 egui 前端，并验证同一套 Rust UI 是否可以编译为 WASM，承担蜂巢的用户工作台页面。

## 1. 结论先行

egui 适合做蛹的第二套前端，也具备编译为 WASM 运行在浏览器中的工程条件，但不能把原生蛹的 `wunder-desktop::NativeDesktop` 直接编进 WASM。推荐把实现拆成三个稳定边界：

1. `crates/wunder-egui` 负责 egui 绘制、交互、主题、状态投影和平台无关的视图模型。
2. 原生适配器在后台线程持有 `NativeDesktop`，复用现有本地 SQLite、线程租约、`NativeStream`、`watch_chat`、终端和设置 façade。
3. WASM 适配器只消费舰体现有的 HTTP/WebSocket 契约：通过分页读模型加载会话，通过单条 `/wunder/chat/ws` 多路复用连接、观察、发送、取消和断线恢复。

因此：

- **蛹替代 Slint：可行，但应先做并行前端和功能分级验收。** egui 先接管聊天工作台，再覆盖设置、工作区、终端和系统能力；在性能、输入法、辅助功能和发行兼容性达标前，不删除 Slint。
- **网页替代蜂巢用户工作台：可行，但属于远程适配器，不是原生适配器的复用。** egui WASM 可以替代用户侧工作台的 Vue 页面；舰桥治理界面、服务端静态托管和管理功能不应被这次迁移一并改写。
- **Win7：按“尽量兼容、不能保证则降级”的要求处理。** 原生首选 Glow/OpenGL 路径，避免把 D3D12、WebGPU 或现代系统 API 设为蛹的唯一启动条件。最终是否支持 32 位 Win7，只以真实目标机和完整发行包冒烟为准。

## 2. 参考工程中应借鉴的做法

参考工程的价值在于工程边界和验收方式，而不是复制其品牌、图标或界面资产。应吸收以下经验：

- **共享 UI crate，应用壳很薄。** UI crate 只持有会话状态和命令投影；桌面壳负责窗口、日志、文件系统和平台回调，Web 壳负责浏览器文件、下载和存储。
- **命令和状态是唯一入口。** 菜单、快捷键、按钮、远程控制和测试都发同一种命令；不要在 egui 回调中直接写 SQLite 或直接调用模型。
- **主题令牌集中管理。** 颜色、间距、圆角、边框、字体和动效由主题模块提供，组件不散落硬编码颜色。
- **WASM 是正式目标。** 独立的 WebRunner 启动器、`Trunk.toml`、浏览器能力注入、localStorage 偏好、文件异步读取和 WebGL2 回退都应从第一阶段建立，而不是等原生 UI 完成后再移植。
- **测试和视觉回归属于 UI 工程。** 使用 egui 交互测试、固定 fixture、截图差异和性能探针，避免只以“能编译”判断完成。
- **分层有自动检查。** `wunder-core` 和 `wunder-runtime` 不依赖 egui；只有 UI 层依赖 `egui/eframe`，防止引擎被桌面或浏览器渲染链污染。

不能照搬的部分：参考工程的文档编辑器模型、命令目录、专有视觉资产和文件格式实现都与 wunder 无关；本方案只借鉴其 crate 分层、端口抽象、主题令牌、WASM 壳和验收门槛。

## 3. 目标与非目标

### 3.1 本轮目标

- 建立 `crates/wunder-egui`，可用同一套视图模型运行 native 和 WASM 两个目标。
- 在原生模式下复用 `wunder-desktop::NativeDesktop`，不新增本机 HTTP/WebSocket，不复制调度、鉴权、存储和线程锁语义。
- 在 WASM 模式下复用舰体已有聊天 WebSocket、分页历史和快照/补水语义，不另造一套聊天协议。
- 覆盖蛹最重要的用户路径：工作区/线程列表、线程打开、历史消息、流式输出、工具时间线、停止、重连、草稿和主题。
- 形成可量化的 Slint 对比结果：首帧、稳态帧耗时、流式延迟、内存峰值、中文输入、滚动和长线程表现。
- 保持浅色蜂巢基调，同时吸收参考工程的密集、清晰、可扫描布局；不做营销化大留白。

### 3.2 非目标

- 不把 `wunder-desktop`、SQLite、Tokio 多线程 runtime、终端 PTY、系统路径和本地模型资源编译进 WASM。
- 不在 UI crate 内重写 orchestrator、ThreadRuntime、工具执行、权限、审批、线程租约或长期记忆。
- 不以 egui 迁移为理由继续维护 Electron/Tauri 壳。
- 不在第一阶段追求 Slint 的全部系统集成（托盘、宠物窗口、Win32 原生子窗口、屏幕捕获、音频设备等）。这些能力按平台能力矩阵逐项接入。
- 不把 egui WASM 当作舰桥治理界面的默认替代品；舰桥仍是治理和调试界面。

## 4. 目标架构

### 4.1 依赖方向

```text
wunder-core
    ↓
wunder-runtime ────────┐
    ↓                   │
wunder-desktop          │  原生适配器（仅 native）
    ↓                   │
crates/wunder-egui ←────┘  共享视图、状态、主题、交互
    ↑
    └── WASM 远程适配器：HTTP + /wunder/chat/ws（不依赖 wunder-desktop）
```

`wunder-egui` 可以依赖 `wunder-desktop` 的 native feature，但 `wunder-core`、`wunder-runtime` 和存储契约不能反向依赖 egui。WASM 构建必须通过 `cfg(target_arch = "wasm32")` 和 feature 解析依赖图，确保没有 `rusqlite`、`tokio::runtime::Runtime`、`winpty` 或本地路径模块。接入 workspace 时只新增 member，暂不加入当前 `default-members`，也保持仓库的 Rust 2021/既有 MSRV，不跟随参考工程直接升级到 edition 2024。

### 4.2 crate 目录建议

```text
crates/wunder-egui/
├─ Cargo.toml
├─ Trunk.toml
├─ index.html
├─ assets/
│  ├─ icons/                 # 优先 Painter/path 绘制的原创图标
│  └─ fonts/                 # 仅放已确认可再分发的字体；默认使用系统回退
├─ src/
│  ├─ lib.rs                 # crate 根、feature 和公共启动入口
│  ├─ app.rs                 # eframe::App 壳与每帧调度
│  ├─ command.rs             # UI 命令，不执行后端工作
│  ├─ model/
│  │  ├─ mod.rs
│  │  ├─ session.rs          # 线程列表和选中态
│  │  ├─ timeline.rs         # 轮次、模型轮次、工具卡、子智能体卡
│  │  ├─ stream.rs           # 增量合并、去重、终态收敛
│  │  └─ projection.rs       # Native/Remote 结果转为共享 ViewModel
│  ├─ backend/
│  │  ├─ mod.rs              # BackendPort、BackendCommand、BackendEvent
│  │  ├─ native.rs           # worker + NativeDesktop，cfg 非 wasm32
│  │  └─ remote.rs           # browser fetch/WebSocket，cfg wasm32
│  ├─ ui/
│  │  ├─ shell.rs            # 二栏壳体、侧栏、主区与浮层状态
│  │  ├─ sidebar.rs          # 工作区与线程树
│  │  ├─ chat.rs             # 气泡、流式尾块、空态、错误态
│  │  ├─ composer.rs         # 输入、附件、发送/停止、草稿
│  │  ├─ workflow.rs         # 工具调用和文件 patch 的紧凑卡片
│  │  ├─ settings.rs         # 设置和模型选择的共享表单
│  │  ├─ terminal.rs         # native 终端能力；web 显示不可用原因
│  │  ├─ markdown.rs         # 最低必要 Markdown，纯文本可靠降级
│  │  ├─ theme.rs             # 设计令牌、浅色/深色、缩放和字体
│  │  └─ icons.rs             # 原创矢量图标和状态图标
│  ├─ platform/
│  │  ├─ native.rs           # 文件、剪贴板、窗口和可选系统能力
│  │  └─ web.rs              # picker、download、localStorage、WebSocket
│  ├─ perf.rs                # 帧、事件、布局、首帧探针
│  └─ fixtures.rs            # 无网络演示数据和测试 fixture
├─ src/bin/
│  ├─ wunder-egui.rs         # native 薄壳（可在稳定后拆成独立 package）
│  └─ wunder-egui-web.rs     # wasm 薄壳；非 wasm 目标只打印构建提示
├─ examples/
│  ├─ ui_shot.rs             # 固定 fixture 截图
│  └─ perf_probe.rs          # 帧与长线程探针
└─ tests/
   ├─ projection.rs
   ├─ stream_reducer.rs
   └─ interaction.rs
```

第一阶段可以只实现 `lib.rs`、`app.rs`、`model`、`backend`、`shell/sidebar/chat/composer/theme` 和两个 probe；设置、终端、宠物和系统集成按后续阶段增加，避免一开始复制 `frontend-slint` 的全部模块。

### 4.3 Feature 与依赖

Cargo 依赖应保持 workspace 统一版本，`egui` 和 `eframe` 始终锁定同一 minor 版本。当前实现已锁定 `0.32.3`，原生使用 Glow，WASM 使用 WGPU；`fixtures` 截图依赖仅按需启用。下例保留架构候选说明，实际依赖以 `crates/wunder-egui/Cargo.toml` 和锁文件为准，不据此升级：

```toml
[features]
default = ["native"]
native = ["dep:eframe", "dep:wunder-desktop"]
web = ["dep:eframe", "dep:wasm-bindgen", "dep:wasm-bindgen-futures", "dep:web-sys", "dep:js-sys"]
test-fixtures = []

[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
eframe = { version = "0.36", default-features = false, features = ["glow"] }

[target.'cfg(target_arch = "wasm32")'.dependencies]
eframe = { version = "0.36", default-features = false, features = ["wgpu"] }
```

`0.36` 只是与参考工程同代的起始候选，不能在未验证 Rust 1.92 MSRV、32 位 Windows 链接和 WASM 产物前视为最终版本。若该版本抬高仓库 MSRV 或无法走 Win7 Glow，应锁定最后一个通过三目标构建的 minor 版本，不为 UI 单独抬高整个 workspace 的 Rust 版本。`egui_kittest`、截图和性能工具放在 dev-dependencies，不能进入生产 WASM 依赖图。具体依赖版本以当前锁文件和三目标构建结果为准，方案中的版本号不是升级指令。

### 4.4 BackendPort：UI 与运行时的唯一边界

共享 UI 不应直接保存 `NativeDesktop` 或浏览器 WebSocket。建议以命令和事件为中心：

```rust
trait BackendPort {
    fn submit(&mut self, command: BackendCommand) -> Result<(), BackendError>;
    fn poll(&mut self, budget: usize) -> SmallVec<[BackendEvent; 32]>;
    fn wake(&self); // 请求下一帧；不在 UI 线程等待
}
```

`BackendCommand` 至少包括 `ListSessions`、`OpenSession`、`LoadOlder`、`SendMessage`、`Cancel`、`RenameSession`、`ArchiveSession`、`SaveDraft`、`SetModel` 和 `SetReasoningEffort`。`BackendEvent` 至少包括 `Sessions`、`HistoryReset`、`TurnsChanged`、`StreamDelta`、`WorkflowChanged`、`Queued`、`Terminal`、`Failed`、`ContextUsage` 和 `Disconnected`。事件必须携带线程 ID、轮次/项目稳定 ID、游标或版本号，reducer 以稳定 ID 去重，不能依赖到达顺序猜测终态。

#### NativePort

- worker 线程独占 `NativeDesktop`，UI 线程只向有界命令队列发送请求。
- `list_sessions`、`get_session_turns`、`chat_process_page`、`watch_chat` 等同步 façade 调用都在 worker 执行。
- `NativeStream` 的现有 128 容量保持不变；worker 将多个 delta 合并为一帧事件后再送给 UI。丢弃 UI 订阅只停止观察，不隐式取消已接受的后台任务。
- 线程打开、租约、取消、终端和本地文件权限继续由 `wunder-desktop` 和引擎执行；egui 不复制检查。
- UI 线程断帧或窗口拖动时，后台仍继续排空 stream 并收敛 durable 状态；恢复后通过稳定游标补水。

#### RemotePort（WASM）

- 启动先请求会话列表和当前线程的有限历史页；历史继续使用 `transcript`/thread view 的分页读模型，不能一次拉全量日志。
- 每个浏览器标签最多保持一条 `/wunder/chat/ws`，复用现有 `connect`、`watch`、`start`、`cancel`、ping/pong 和游标恢复语义。
- 断线时显示明确的 reconnecting 状态；重连先带 `after_change_seq` 或服务端要求的快照游标补水，再应用实时 delta，不能用“重新播放完整回复”的打字动画代替流式。
- 认证沿用现有 cookie/协议票据和服务端鉴权，不把长期密钥塞进 URL、localStorage 或 WebSocket 查询参数。
- 浏览器文件选择通过异步 picker 进入有界 inbox；下载通过浏览器 download；偏好仅存 localStorage。没有本地 SQLite、PTY、工作区绝对路径和系统托盘。

## 5. 共享状态与流式投影

### 5.1 单向数据流

```text
用户输入/快捷键
        ↓
BackendCommand（有界）
        ↓
NativePort worker 或 RemotePort async task
        ↓
BackendEvent（带稳定 ID/游标）
        ↓
纯 Rust reducer / Projection
        ↓
egui 当前帧的可见投影
```

`egui::Context` 只用于请求重绘、读取输入和绘制，不能作为业务事件总线。所有异步事件先进入后端适配器的有界队列，再由每帧预算内的 reducer 应用。

### 5.2 线程和时间线

- 线程是第一隔离键；任何事件必须先匹配当前线程，不允许旧线程的晚到事件污染新线程。
- 用户轮次、模型轮次、工具调用和子智能体卡使用后端投影中的稳定 ID；不从正文、时间戳或到达顺序推断身份。
- 一次用户轮次显示为用户气泡 + 模型回复 + 可折叠工作过程。工作过程只渲染有界摘要、状态、耗时、token 和 patch 预览，原始大 payload 不进入 UI 状态。
- 完成的消息块保持不可变；只有活动尾块合并新的 delta。复制操作读取完整原始内容或后端补水结果，不把展示截断误当作数据丢失。
- 长线程初始只加载最近 50 个用户轮次；单个工作过程页最多 100 项，沿用后端游标加载更早内容。单轮打开只取有限尾部，查看详情再翻页。

### 5.3 增量合并和预算

初始预算建议如下，实测后可以收紧，但不能取消上限：

| 项目 | 初始上限 | 处理策略 |
| --- | ---: | --- |
| NativeStream 事件通道 | 128 | 保持现有契约，生产端异步等待容量 |
| UI 待应用事件 | 256 | 每帧最多应用一批，超时则留到下一帧 |
| 单个 delta 进入 UI 的字节数 | 64 KiB | 更大内容切块，保持顺序 |
| 每帧流式刷新间隔 | 16–33 ms | 底部跟随时请求重绘；上滚时降低刷新频率 |
| 初始用户轮次 | 50 | 更早历史按游标加载 |
| 工作过程页 | 100 项 | 向前/向后分页，不做全量展开 |
| 可见消息缓存 | 由 viewport + 预算控制 | 完成块稳定，淘汰不可见的装饰性布局缓存 |
| 单次文件选择 | 文件数、总字节数双限额 | 超限逐项报错，不能无限堆积 inbox |

事件队列满时不能静默丢字。可以丢弃重复的 repaint 请求或合并同一稳定 ID 的相邻 delta，但必须保留文本顺序、终态、错误和游标；无法合并时暂停读取并回压后端。

### 5.4 虚拟化与 Markdown

egui 的 immediate mode 不等于必须重建全部历史。聊天区使用按消息块组织的可见窗口：

- 维护消息块高度估计和滚动锚点；只绘制 viewport 附近的块。
- 活动尾块允许重新计算布局，已完成块的高度和布局缓存保持稳定。
- 上滚查看历史时禁止自动跳底；用户回到底部后才恢复跟随。
- Markdown 只实现段落、换行、列表、代码块和安全链接的最低集合；图片、表格、复杂 HTML、公式和高亮降级为可读文本或占位提示。
- 解析在 worker/reducer 侧按块完成；UI 线程只拿已准备好的轻量 token/行模型。原始消息全文始终可复制和按需重新解析。

## 6. 视觉和交互方案

### 6.1 风格方向

采用蜂巢当前用户工作台的浅色暖灰基调：`#FBFAF8` 页面底、`#F6F5F3` 侧栏、白色输入面、低对比边框和赤陶强调色 `#C96443`。界面清晰、安静、可扫描，突出线程状态和模型工作过程，不堆装饰性插画。视觉令牌以 `frontend/src/styles/pages/messenger-shell.css`、`messenger-polish.css` 和 `frontend-slint/ui/design_tokens.slint` 为准。

建议布局：

```text
┌──────────── 240px 侧栏 ────────────┬────────────── 聊天主区 ──────────────┐
│ 工作区 + 线程树                    │ 居中 1000px 时间线                  │
│ ───── 9px 工作目录分隔 ─────       │ 用户气泡 / 助手正文 / 工具时间线       │
│ 工作目录文件树                     │                                      │
│ 用户设置行                         ├──────── Composer：输入 · 发送/停止 ───┤
└────────────────────────────────────┴──────────────────────────────────────┘
```

主区不设置通用顶栏；线程标题和状态通过侧栏行、时间线状态与 Composer 工具行表达。窄窗口时按“主时间线优先”收起侧栏和次要统计；侧栏状态和错误不能只靠颜色表达。工具卡默认折叠，运行中突出状态、进度和取消入口，完成后保留可扫描摘要。

### 6.2 设计令牌

目标由 `theme.rs` 统一定义浅色、深色、跟随系统三套语义令牌；当前绘制实现为浅色主题，其余属于后续工作。令牌覆盖：背景、面板、边框、主文本、次文本、禁用文本、悬浮、按下、选中、强调、成功、警告、错误、流式状态、代码块和工具卡。组件只引用令牌，不直接写 RGB。

主题还负责：

- 中文和拉丁文字体回退顺序、字号层级和行高；默认不捆绑未确认许可的字体。
- 系统缩放和手动 UI 缩放；聊天内容缩放与界面缩放分开。
- 动效开关和 reduced-motion；流式输出不以动画掩盖真实事件。
- 高对比和键盘焦点边界；不能只用色相区分运行、成功和失败。

图标优先用 `Painter` 绘制简单原创路径，复杂图标使用明确可再分发的资产并登记来源。不得复制参考工程或其他产品的商标、图标、截图、字体和主题资源。

### 6.3 输入法、快捷键和可访问性

首期必须验证中文 IME 组合输入、粘贴、选区、Enter/Shift+Enter、Ctrl/Cmd+Enter、Escape 停止、上下文菜单和焦点恢复。发送按钮、状态、工具卡展开、连接错误和后台任务都提供可读文本与 tooltip；接入 AccessKit 前先以普通键盘和屏幕阅读器 smoke 检查验证 egui 版本的基础能力，避免为 Win7 引入不必要的依赖。

## 7. 原生蛹方案

### 7.1 启动和生命周期

- `wunder-egui` native bin 只做参数解析、日志、`eframe::run_native` 和 `NativeBackend` 创建。
- 首帧先显示静态壳和本地最近线程摘要；SQLite、线程观察、云状态和资源索引在 worker 后台就绪。
- 退出时先停止 UI 订阅，再让 `NativeDesktop` 按现有生命周期释放线程租约、终端和 runtime；关闭窗口不能把运行中的任务当作取消。
- 保留 `--demo`、`--smoke-check` 和独立 fixture 模式，便于无网络截图和回归。

### 7.2 原生能力矩阵

| 能力 | 首版 | 说明 |
| --- | --- | --- |
| 本地 SQLite、线程租约 | 必须 | 只通过 NativeBackend，不能在 UI 中直读数据库 |
| 聊天、历史、工具时间线、停止 | 必须 | 复用 `NativeStream`、`watch_chat` 和分页 façade |
| 本地工作区文件 | 必须 | 路径围栏和权限由后端负责，UI 只显示相对路径 |
| 终端 PTY/管道 | 第二阶段 | 复用现有 terminal façade；没有后端时明确显示不可用 |
| 系统拖放、剪贴板、文件对话框 | 第二阶段 | 注入 platform service，不能让 egui 组件直接调用 Win32 |
| 宠物窗口、托盘、屏幕捕获、音频 | 第三阶段 | 单独 capability，不阻塞聊天工作台验收 |

### 7.3 Win7 与旧硬件

- 原生首选 `eframe` Glow/OpenGL renderer，避免把 WGPU/D3D12 当作唯一路径；现代 GPU 路径可以作为可选 feature。
- Win7 发布固定使用 `i686-win7-windows-gnu` 与 `-Z build-std=std,panic_abort`，而不是将主机 x64/MSVC 构建复制到旧系统。该 target 让 Rust 标准库和 `getrandom` 选用 Win7 路径；发布脚本在 strip 前检查 PE 必须为 i386，并拒绝 `bcryptprimitives.dll`、`ProcessPrng`、`api-ms-win-*`、`combase.dll`、`PathCchStripPrefix`、`WaitOnAddress`、`WakeByAddress*`、`GetSystemTimePreciseAsFileTime`、DPI 与线程命名等已知 Win8+/Win10+ 导入。
- `build.bat -Egui -a win7-x86` 输出 `target/wunder-egui/dist/win7-x86/` 和可审计的 `imports.txt`；`build.bat -Egui` 只用于当前主机快速验收。缺少锁定依赖时必须补充 Win7 SDK 的独立 `cargo-vendor-egui` 快照并让离线解析通过，不能回退在线解析或分发系统 DLL。
- 锁定并审计 `winit/glutin/eframe` 的 Windows 7 支持和 Rust 1.92 MSRV；依赖升级必须重新做 32 位链接检查。
- 不启用需要 Windows 10+ 的系统 API 作为启动硬依赖；系统能力失败时使用普通窗口、软件回退或隐藏对应入口。
- 对 32 位 Windows、旧 OpenGL 驱动和无硬件加速机器分别记录启动、首帧、文本输入、窗口缩放和退出结果。无法运行时要在发行说明中明确“不支持”，不能用现代 Windows 结果代替 Win7 结论。
- 软件渲染回退只作为最后的可用性路径；如果帧时间无法达到可接受范围，保留 Slint 作为旧系统默认前端。

## 8. WASM 蜂巢候选方案

### 8.1 浏览器壳

`wunder-egui-web` 使用 `eframe::WebRunner` 挂载 `<canvas>`。默认走 WGPU，检测不到 WebGPU 时回退 WebGL2；通过 URL 或启动配置提供一个强制 WebGL2 的诊断开关。浏览器壳只负责：

- 读取当前登录态和服务端地址；
- 建立一条聊天 WebSocket；
- 把异步文件 picker、download、localStorage 和浏览器可见性事件注入 RemoteBackend；
- 显示连接失败和浏览器能力错误；
- 在页面刷新前保存主题、缩放、侧栏折叠和草稿等偏好。

不要把服务端 URL、长期凭据或用户隐私写进静态 bundle。静态资源应由现有蜂巢发布流程或独立静态服务器只读托管，API 仍由舰体鉴权。

### 8.2 Web 与原生差异

| 功能 | Native 蛹 | WASM 蜂巢候选 |
| --- | --- | --- |
| 执行位置 | 本地引擎、SQLite | 舰体远程运行时 |
| 线程打开 | 本地租约 | 服务端用户权限和线程状态 |
| 工作区 | 本地目录和本地工具 | 云端工作区，通过现有 API |
| 终端 | PTY/管道可用 | 不提供本机终端；显示远程终端能力时另走已有服务契约 |
| 文件 | 系统对话框、拖放、剪贴板 | 浏览器 picker、上传、下载 |
| 认证 | 本地配置/云会话 | 现有网页 cookie/票据和 WebSocket 协议 |
| 数据持久化 | SQLite、配置文件 | 服务端数据 + localStorage 偏好 |
| 离线 | 可继续使用本地线程和工具 | 只保留草稿和 UI 偏好，不能伪造在线运行态 |

### 8.3 替代蜂巢的边界

先以“用户侧聊天工作台”作为替换候选，覆盖登录后工作区、线程、聊天、工具过程、附件、重连和设置。以下内容继续由现有前端承担，除非另立项目：舰桥治理、租户/用户管理、智能体发布、网关管理、审计和复杂管理表格。这样可以验证 egui WASM 的实际收益，而不会把一次 UI 试验变成全站重写。

## 9. 性能设计与目标

性能是默认验收项，不在最后阶段补救。

### 9.1 UI 线程规则

- UI 线程不做网络读取、SQLite、文件扫描、终端 IO、完整 Markdown 解析或大对象复制。
- 每帧只应用有界事件批次；空闲帧不轮询忙等，只有待处理事件、动画或输入时请求 repaint。
- 不按 token 重建消息列表；按帧合并 delta，只更新活动尾块。
- 不为每次状态更新复制完整历史；reducer 以稳定 ID 增量替换，完成块使用 `Arc`/共享字符串或不可变快照。
- 所有事件、附件、分页、渲染缓存、重连重试和日志都有上限；上限触发时显示可理解的错误或分页入口。

### 9.2 初始预算

以下是启动门槛，最终数值应在本机和目标机测量后写入性能报告：

| 指标 | 目标 |
| --- | --- |
| Native 首帧可见 | 正常机器 1000 ms 内；后台初始化不阻塞首帧 |
| Native UI 接线完成 | 1000 ms 内，失败必须显式显示 |
| 空闲帧 | 无持续忙等；无输入时不维持高频 repaint |
| 流式跟手 | 16–33 ms 合并一批；滚动和输入不被阻塞 |
| 普通历史滚动 | 60 FPS 优先，旧硬件允许以 30 FPS 为下限 |
| 长线程 | 打开和滚动只读取可见/分页窗口，不随总 item 数线性复制 |
| WASM 首屏 | 记录下载、编译、canvas ready、首个可交互帧四段时间；不得把 wasm 下载时间伪装成 UI 帧耗时 |
| WASM 体积 | 先建立 release 基线，后续版本不得无说明地增长超过 20%；字体和调试符号不进入生产包 |

### 9.3 可观测性

`perf.rs` 记录启动阶段、每帧 UI/布局/绘制耗时、事件队列长度、delta 合并字节数、可见消息数、重连次数、历史页大小和 WASM backend（WebGPU/WebGL2）。默认不记录正文、密钥、真实用户标识或绝对路径。`perf_probe` 输出机器可读 JSON，截图和报告只使用 fixture 数据。

## 10. 分阶段实施

### P0：可编译骨架和视觉试验

1. 新增 `crates/wunder-egui`、workspace 依赖和 feature 分层。
2. 完成 `BackendPort`、共享 ViewModel、主题令牌、静态 fixture 和最小 shell。
3. native `eframe` 窗口与 WASM `WebRunner` 均能启动；WASM 不链接 `wunder-desktop`。
4. 加入 `ui_shot`、`perf_probe`、projection/reducer 单测和首张基线截图。

出口：native 与 wasm `cargo check` 通过，fixture 可截图，主题和窗口缩放在桌面/浏览器都稳定。

### P1：原生聊天工作台

1. NativeBackend worker 接入线程列表、打开、有限历史、发送、取消、流式 delta 和 `watch_chat`。
2. 完成侧栏、聊天气泡、工具卡、Composer、草稿、连接/错误/排队状态。
3. 接入中文 IME、复制、滚动锚点、长消息分块和上下文占用摘要。
4. 与 Slint 进行同一 fixture 的功能和性能对比。

出口：核心聊天路径可替代 Slint 演示模式，真实本地 SQLite 冒烟通过，线程租约和取消语义无回归。

### P2：WASM 远程聊天工作台

1. RemoteBackend 完成 bootstrap、分页历史、单 WS connect/watch/start/cancel、游标恢复和终态去重。
2. 接入浏览器登录态、异步附件、下载、localStorage 偏好和 WebGPU/WebGL2 诊断信息。
3. 在固定舰体环境中做断线、刷新、重复提交、慢网和服务端重同步测试。

出口：用户侧聊天工作台可在浏览器完成一条完整线程生命周期；没有本地能力时显示明确边界，不出现假运行状态。

### P3：功能覆盖和视觉收敛

1. 按能力矩阵接入设置、模型/推理等级、工作区文件、远程线程、终端（若已有远程契约）和子智能体详情。
2. 完成深色/浅色、缩放、键盘导航、辅助功能、窄窗口和错误态。
3. 将重复的 Slint/egui 投影逻辑下沉到 `wunder-desktop` 或共享契约，禁止复制两套执行语义。

出口：每个声明支持的功能都有 fixture、交互测试、权限/错误状态和性能上限。

### P4：兼容性、性能和替换决策

1. 原生验证 x64、32 位 Windows、旧 OpenGL、Ubuntu 18.04 X11；记录不支持组合。
2. WASM 验证 WebGPU、WebGL2、窄屏、刷新恢复、浏览器存储受限和大线程。
3. 运行完整回归、截图差异、长线程压力和资源释放测试。
4. 依据决策表选择：保留双前端、egui 成为蛹默认、或先替换蜂巢用户工作台。

## 11. 测试与验收门槛

### 11.1 编译和静态检查

- `cargo fmt --check`、`cargo clippy -p wunder-egui --all-targets --all-features -- -D warnings`。
- native 默认 feature 的 debug/release 构建；确认生产包没有测试 fixture 和调试符号。
- `wasm32-unknown-unknown` 的 `cargo check`/release 构建和 `trunk build --release`。
- 依赖图检查：WASM 不出现 `wunder-desktop`、SQLite、native-only terminal、系统窗口 API。
- 分层检查：下层 crate 不依赖 egui/eframe；新增 feature 不能绕过这一约束。

### 11.2 纯逻辑回归

- 事件 ID、游标和重连重放幂等；重复 delta 不重复显示，晚到旧线程事件被拒绝。
- 用户轮次/模型轮次/工具调用/子智能体卡顺序与现有 NativeDesktop 投影一致。
- 取消、排队、失败、服务端重同步和窗口关闭都能收敛到终态。
- 分页边界、空线程、长文本、控制字符、畸形 Markdown 和超限附件不会 panic 或无界分配。

### 11.3 UI 与视觉回归

- `egui_kittest` 覆盖侧栏选择、发送/停止、流式尾块、工具卡展开、滚动锚点、主题、窄窗口和键盘焦点。
- `ui_shot` 使用合成 fixture 生成浅色/深色/窄窗截图；差异阈值和允许的字体栅格差异写入脚本，不凭肉眼口头验收。
- 中文 IME、复制粘贴、快捷键、浏览器文件选择和刷新恢复做真实手工 smoke；测试记录不含真实业务名称、路径或凭据。

### 11.4 性能与资源

- 1、100、1000 个用户轮次和大量工具项分别测打开、滚动、流式和切换线程。
- 快速 delta、慢网络、断线重连、窗口连续缩放和高频终端输出测事件队列峰值与 UI 帧时间。
- 关闭窗口、切换线程和刷新页面后检查 worker、WebSocket、租约、文件句柄、GPU 纹理和 WASM inbox 是否释放。
- 报告首帧、可交互、队列峰值、内存峰值和产物体积；不能只报告平均帧率。

## 12. 替换决策表

| 决策项 | 必须达到 |
| --- | --- |
| 蛹核心聊天替代 Slint | 核心路径功能一致；线程租约/取消/重连无回归；首帧与流式预算达标；中文输入和复制通过 |
| 蛹默认前端替换 | P1–P4 目标平台冒烟通过；终端、文件、设置等声明支持的能力有等价路径；Win7 失败组合明确降级 |
| 蜂巢用户工作台替换 | RemoteBackend 覆盖 bootstrap、分页、WS、附件、刷新/断线；WebGPU/WebGL2 均可用；浏览器存储受限时仍可工作 |
| 保留 Slint | 任一关键门槛未达标，尤其是 Win7、IME、辅助功能、长线程或首帧；两者共享后端，不继续复制业务逻辑 |

最终决策必须附：功能矩阵、机器/浏览器矩阵、性能 JSON、截图差异报告、已知不支持项和回滚方式。未完成真实 Win7/Ubuntu 18.04 验收时，只能写“待验证”或“尽量兼容”，不能宣称支持。

## 13. 风险与应对

| 风险 | 应对 |
| --- | --- |
| egui 变量高度列表滚动抖动 | 消息块化、高度估计、活动尾块单独更新；必要时按页而非逐行虚拟化 |
| WASM 误带入 native 依赖 | target-specific dependencies + CI 检查 `cargo tree --target wasm32-unknown-unknown` |
| WebSocket 断线后重复/漏事件 | 使用现有游标、稳定 ID、快照补水和 reducer 幂等，不依赖时间戳 |
| Win7 renderer/依赖不兼容 | Glow/OpenGL 首选、锁版本、真实 32 位构建；失败则保留 Slint |
| CJK 字体和 IME 差异 | 系统字体回退、可再分发字体审查、真实输入 smoke；不在 UI 中硬编码替代字体 |
| UI 与后端状态漂移 | 所有能力经 BackendPort；新增字段先更新共享 ViewModel 和契约测试 |
| 视觉迭代导致 bundle 变大 | 图标优先代码绘制、图片和字体预算、release 体积基线和 20% 回归门槛 |
| 迁移范围失控 | 先替换聊天工作台，系统能力和舰桥治理按独立能力矩阵推进 |

## 14. 第一批实施清单

1. 创建 crate skeleton、workspace member、native/web feature 和 MSRV 构建检查。
2. 定义 `BackendPort`、共享 ViewModel、事件稳定 ID、错误模型和 reducer。
3. 按蜂巢二栏 shell 做浅色主题、空态、错误态和 fixture 截图；深色与其他主题在默认视觉通过后再扩展。
4. 写 NativeBackend worker，接入 `NativeDesktop::list_sessions`、`get_session_turns`、`send_chat`、`watch_chat` 和 `cancel_chat`。
5. 实现按帧 delta 合并、活动尾块、分页历史和滚动锚点。
6. 建立 `egui_kittest`、`ui_shot`、`perf_probe` 与 native smoke 的 CI 命令。
7. 再实现 RemoteBackend 和 WebRunner；先通过 mock/fixture 验证协议状态机，再连接真实舰体。
8. 每完成一个阶段，用 `python scripts/update_feature_log.py --type <类型> --scope <范围> ...` 登记，并在本方案中更新出口结果。

## 15. 当前实现与蜂巢视觉验收

### 15.1 已落地的呈现基线

本轮从蜂巢实际页面捕获聊天、展开工作过程、条目详情和 11 个设置分类，以固定测试数据和计算样式作为对照。UI 使用同一套原生/WASM 绘制代码；布局为左侧工作区、线程和工作目录加主聊天区，没有第三栏或通用顶部工具条。

| 项目 | 实测基线与当前绘制约束 |
| --- | --- |
| 比较视口 | 1440×900 逻辑像素，固定演示数据 |
| 侧栏 | 240px，工作区行 34px、线程行 28px、分隔条 9px、账户按钮 32px |
| 阅读列 | 消息和输入区最大 1000px；活动线程有独占的 18px 轮次刻度通道 |
| 消息 | 行内左右 24px；用户气泡最多占可用行宽 70%，圆角 12px；助手全宽，无头像、姓名和时间行 |
| 工作过程 | 正文之前默认收起，入口 40px；展开节点行 26px；内部滚动限高 |
| 正文与尾栏 | 14px 正文、23.8px 行高；最低必要 Markdown；22px 尾栏含轮次胶囊和悬停复制 |
| 输入区 | 默认总高 128px；16px 圆角白色输入卡；输入高度 44–168px；卡外工具行 30px；发送/停止共用 28px 动作槽 |
| 设置窗口 | 960×680px；标题栏 44px；搜索导航宽 260px；内容独立滚动，Escape 关闭 |

颜色和图标来源为蜂巢现有令牌与 Font Awesome 资源，中文字体复用当前蛹已有字库。原生和浏览器各自栅格化文字，仍需对照字重、基线、换行和字体宽度。当前属于按真实截图校准主要几何，尚未达到“完全像素一致”或自动图像差阈值验收结论。窄窗口、不同 DPI、深色主题、长线程和持续流式负载继续按第 11 节验收。

### 15.2 演示与截图入口

在仓库根目录打开固定聊天数据或指定设置分类：

```powershell
cargo run -p wunder-egui --bin wunder-egui -- --demo
cargo run -p wunder-egui --bin wunder-egui -- --demo-settings=general
```

`--demo` 与 `--demo-settings` 都不初始化 `NativeDesktop`、SQLite 或网络。设置分类可选 `general`、`account`、`tools`、`agent`、`companion`、`cron`、`memory`、`channels`、`devices`、`prompts`、`archived`。账户行同样可打开设置；不设置截图环境变量时，可以手动检查过程展开、文本选择和浮层交互。

启用 `fixtures` 并设置截图出口，可在稳定帧后保存 PNG，随后自动关闭窗口：

```powershell
New-Item -ItemType Directory -Force temp_dir/egui-visual | Out-Null
$env:WUNDER_EGUI_CAPTURE_TO = 'temp_dir/egui-visual/egui-chat.png'
try {
    cargo run -p wunder-egui --bin wunder-egui --features fixtures -- --demo
} finally {
    Remove-Item Env:WUNDER_EGUI_CAPTURE_TO -ErrorAction SilentlyContinue
}
```

设置截图将命令参数替换为 `--demo-settings=general`，并使用对应输出文件名。截图依赖可用的图形桌面和 OpenGL；按实际 framebuffer 像素保存。比较时应统一逻辑视口、缩放、状态和测试数据，不能把不同 DPI 产物直接当作像素差。截图依赖不进入默认构建，也不进入 WASM 生产依赖图。

### 15.3 真实能力与未接线部分

| 范围 | 当前状态 |
| --- | --- |
| 原生运行时 | 默认启动通过有界后台 worker 调用 `NativeDesktop`；线程列表、打开、有限历史、流式文本、工具摘要和停止已有命令/事件通路 |
| 共享聊天视图 | 已实现二栏壳体、时间线、折叠过程、基础 Markdown、完整原文复制和输入区；发送键包含输入法确认保护 |
| 设置浮窗 | 已呈现 11 个分类的页面、导航、搜索、关闭及部分临时选项；尚未接入设置 façade 的读取、校验、保存和持久化 |
| 线程与工作目录 | 原生侧栏已通过有界后台调用 `NativeDesktop` 完成新建、重命名、归档和本地目录的受限读取、新建、刷新、系统打开；线程悬停操作带、右键菜单和路径校验对齐 Slint。文件重命名、删除、引用和预览仍待接入。 |
| Composer 菜单与模型 | 加号菜单按 Slint 的命令、预设问题、审批模式、语音、截图、终端顺序呈现，并提供级联选项；命令/预设可写入草稿。语音、截图和独立终端窗口尚无 egui 平台适配器，明确显示未接入。模型浮层读取 LLM 目录（最多 128 项），通过 façade 保存线程模型与推理力度；真实上下文占用由线程观察事件提供。 |
| WASM | 已建立 `WebRunner` 和平台依赖隔离，可通过 WASM 检查；本地线程、工作目录和模型命令明确报告未连接，HTTP/WebSocket、认证、分页和补水仍待 P2 实现。 |
| 替换结论 | Slint 仍为默认蛹前端；当前不具备直接替换蛹或蜂巢的验收证据。Win7 支持与性能达标均未确认 |

原生真实聊天使用无演示参数的默认入口，不应把整个原生页面都称为 fixture。设置预览和固定截图模式则应清楚区分临时呈现与后端状态，不能把界面中的选项或演示字段当作真实配置已保存。

### 15.4 本轮验证与下一步

本轮已通过原生 `cargo test -p wunder-egui --lib --features native`（16 项）、WASM 目标 `cargo check -p wunder-egui --no-default-features --features web --target wasm32-unknown-unknown` 和 `./build.bat -Egui`。回归覆盖线程隔离、流式文本合并、工具参数与有界详情、只读提示词预览的迟到响应及 UTF-8 截断、模型保存不改变聊天状态、运行时上下文投影、侧栏路径校验、中文 Markdown、设置分类布局和 IME/发送快捷键。测试只验证所列边界；真实中文输入法、浏览器与旧系统仍需实机检查。

`./build.bat -Egui` 已成功生成 `target/wunder-egui/release/wunder-egui.exe`，默认功能构建的固定演示启动冒烟通过。本轮另对聊天收起、过程展开、工具详情和 11 个设置分类生成了 14 张原生实拍；对照文件位于本地忽略目录 `temp_dir/egui-visual/`，其中 `hive-contact-sheet.jpg` 为真实 Vue 页面总览，`egui-contact-sheet.jpg` 为原生总览，`compare-chat.jpg` 等图片左侧为蜂巢、右侧为 egui。该目录是人工核对产物，尚非自动图像回归门禁。

Win7 兼容修复已接入 `builders/build-win7-egui.ps1` 和 `build.bat -Egui -a win7-x86`：使用 32 位 Win7 GNU target 重编标准库，并在发布阶段执行与 Slint 相同的 PE 导入门禁。此前主机 egui 产物直接导入 `bcryptprimitives.dll!ProcessPrng`、`api-ms-win-core-synch-l1-2-0.dll!WaitOnAddress` 等 Win7 不可用接口；不能通过复制 DLL 解决。`arboard` 的 `PathCchStripPrefix` 也会静态引入 Win8 的 `api-ms-win-core-path-l1-1-0.dll`，因此已由仓库内补丁改为等价的 UTF-16 前缀处理，保留文件拖放剪贴板路径。2026-10-19 的完整离线 release 已通过 i386 与禁止导入门禁，并生成 `target/wunder-egui/dist/win7-x86/wunder-egui-0.1.0-win7-x86.exe` 及 `imports.txt`；Win7 真机图形、中文输入、HTTPS 与聊天冒烟仍需完成，故暂不宣称 Win7 已支持。

下一步优先补齐真实设置 façade、新建线程与工作目录读取，再实现 WASM 远程适配器。同时建立固定截图的自动差异阈值，补测长线程、流式响应、事件队列峰值、内存和首帧。达到第 12 节门槛后，再决定是否切换默认前端。
