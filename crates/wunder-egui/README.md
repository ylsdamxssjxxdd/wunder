# wunder-egui

`wunder-egui` 是蛹的实验性 egui 前端，目标是复用现有本地引擎，按蜂巢 AI 工作台的布局和样式验证 Slint 替代方案，并为浏览器构建共享呈现层。当前默认蛹前端仍为 `frontend-slint/`。

当前已提供原生聊天适配器、共享主题与时间线、设置浮窗的视觉呈现、固定数据演示，以及 WASM 画布启动壳。设置业务和浏览器远程聊天尚未完成，不能据此替换现有蛹或蜂巢。完整边界见 [落地方案](../../docs/wunder-egui落地方案.md)。

## 原生运行

在仓库根目录运行：

```powershell
cargo run -p wunder-egui --bin wunder-egui
```

默认启动在同一进程通过后台 worker 调用 `wunder-desktop::NativeDesktop`，使用蛹既有 SQLite 和本地配置，不经过本机 HTTP bridge。已接入线程列表、打开、新建、重命名、归档、有限历史、流式文本、工具摘要、停止和本地工作目录的受限读取/创建/刷新/打开。Composer 的模型浮层可读取 LLM 目录并保存线程模型与推理力度；语音、截图和独立终端窗口仍需 egui 平台适配器。

固定数据演示不启动 `NativeDesktop`，不读取 SQLite，也不发起网络请求：

```powershell
cargo run -p wunder-egui --bin wunder-egui -- --demo
cargo run -p wunder-egui --bin wunder-egui -- --demo-settings=general
```

`--demo-workflow` 直接展开固定工作过程，`--demo-tool` 同时展开工具详情，便于核对两种展开状态。

设置演示支持 `general`、`account`、`tools`、`agent`、`companion`、`cron`、`memory`、`channels`、`devices`、`prompts`、`archived`。也可从聊天窗口左下角账户行打开设置，按 Escape 关闭。

## 蜂巢视觉基线

本轮以现有蜂巢页面的固定测试数据截图和计算样式为依据，采用二栏 AI 工作台布局。参考来源为 `frontend/src/styles/pages/messenger-shell.css`、`messenger-polish.css` 以及实际消息和设置组件。

| 项目 | 基线 |
| --- | --- |
| 比较视口 | 1440×900 逻辑像素 |
| 左侧栏 | 240px；工作区和线程树、工作目录、32px 账户行 |
| 消息与输入阅读列 | 最大 1000px，居中；活动线程右侧另留 18px 刻度通道 |
| 消息 | 用户右侧暖色浅气泡，助手全宽正文，无头像和姓名行 |
| 工作过程 | 默认收起，40px 入口，26px 节点行 |
| 输入区 | 默认 128px；白色圆角输入卡与外置工具行，发送和停止共用一个位置 |
| 设置浮窗 | 960×680px，44px 标题栏，260px 导航 |

共享令牌使用暖白画布、暖灰侧栏和赤陶强调色；图标复用蜂巢既有 Font Awesome 字体。演示窗口以 1440×900 和固定缩放打开，便于与蜂巢截图对照。其他窗口尺寸仍需单独验收。

当前已对照聊天、工作过程和设置截图收敛主要几何，二级标题按实际浏览器的 21px 字号绘制。字体栅格、字重、文本宽度及部分表单细节仍存在差异；自动像素差门槛已建立（见下节），但尚未做跨机器基线。

## 视觉回归门槛

`scripts/check_egui_visual.py` 用固定 fixture、固定视口和固定缩放启动演示窗口，截图后与基线比较，超阈值返回非零退出码。脚本只依赖标准库，PNG 编解码内置，不需要 Pillow。

```powershell
# 首次或确认视觉改动后生成基线
cargo build -p wunder-egui --bin wunder-egui --features fixtures
python scripts/check_egui_visual.py --update

# 日常回归
python scripts/check_egui_visual.py
python scripts/check_egui_visual.py --only settings-general
```

覆盖 16 个 fixture：聊天、工作过程收起态、工具详情展开态、11 类设置页、1024×700 二栏几何和 980×700 窄窗收起态。判定同时看"明显变化的像素占比"（默认 ≤0.4%）和"全图平均通道差"（默认 ≤1.0），单像素容差 12 用于吸收文字抗锯齿。超阈值时输出差异图到 `temp_dir/egui-visual/current/diff/`，并写 `report.json` 记录尺寸、占比、平均差和峰值。

基线与本次截图都按实际 framebuffer 像素保存，受系统缩放影响（本机 175% 缩放下 1440×900 逻辑视口输出 2520×1575 像素），因此基线属于本机产物，存放在 `temp_dir/egui-visual/baseline/`，不进入仓库；换机器前用 `--update` 重建。设置页搜索框有光标闪烁，实测约 0.08% 像素差异、峰值低于 100，属于已知噪声，低于门槛。

需要手工对照时，可用 `WUNDER_EGUI_VIEWPORT`（如 `1024x700`）固定演示窗口逻辑尺寸；不设该变量则使用默认 1440×900。

## 生成单张截图

`fixtures` feature 提供可选的 PNG 截图出口，默认生产构建不启用。以下命令在仓库根目录执行，生成截图后窗口自动关闭；它需要可用的图形桌面和 OpenGL 环境，不是无界面渲染器。

```powershell
New-Item -ItemType Directory -Force temp_dir/egui-visual | Out-Null
$env:WUNDER_EGUI_CAPTURE_TO = 'temp_dir/egui-visual/egui-chat.png'
try {
    cargo run -p wunder-egui --bin wunder-egui --features fixtures -- --demo
} finally {
    Remove-Item Env:WUNDER_EGUI_CAPTURE_TO -ErrorAction SilentlyContinue
}
```

设置截图使用同一出口，将运行命令替换为：

```powershell
cargo run -p wunder-egui --bin wunder-egui --features fixtures -- --demo-settings=general
```

同时把输出文件名改为对应分类，例如 `temp_dir/egui-visual/egui-settings-general.png`。截图按实际 framebuffer 像素保存；比较前确认两侧视口、缩放和数据一致。手动检查工作过程展开与条目详情时，使用不设置截图环境变量的 `--demo`。

## WASM

先安装 `wasm32-unknown-unknown` 和 Trunk，再在 `crates/wunder-egui/` 运行：

```powershell
trunk serve --release --features web
```

`index.html` 通过 Trunk 的 Rust 入口构建 `wunder-egui-web`。目前验证的是共享 UI、`WebRunner` 和目标平台依赖隔离；`RemoteBackend` 尚未接入舰体 HTTP/WebSocket、认证、分页和断线补水。浏览器显示未连接状态，不能执行真实聊天。WASM 不链接 `wunder-desktop`、SQLite 或本地终端。

## 能力与验证范围

设置的导航、搜索、关闭与部分临时选项已可交互；账号、工具和渠道等业务表单尚未通过后端 façade 读写，不能把演示值或临时选项理解为已持久化配置。模型浮层、线程操作和本地工作目录已有原生通路；附件、语音、截图、文件重命名/删除/引用和终端窗口仍待接线。尚未形成可替换 Slint 或蜂巢的功能覆盖结论，也未完成 Win7 发行包验证或性能达标报告。

仓库根目录可执行：

```powershell
cargo check -p wunder-egui --features native
cargo check -p wunder-egui --no-default-features --features web --target wasm32-unknown-unknown
cargo test -p wunder-egui --lib --features native
```

当前 9 项回归覆盖线程隔离、流式增量合并、工具参数与有界详情、只读提示词预览的迟到响应及 UTF-8 截断、中文 Markdown 换行和网格有效性、11 类设置及渠道内部布局边界、Shift+Enter 换行、中文输入法确认保护、发送后焦点及 Ctrl+Enter 偏好。真实输入法、原生图形环境和浏览器行为仍需人工检查。

## Win7 x86 发布

`build.bat -Egui` 是当前主机的快速构建，不能分发到 Win7。发布 Win7 x86 版本时使用：

```powershell
./build.bat -Egui -a win7-x86
```

该路径使用 `i686-win7-windows-gnu` 和重编的标准库，产物为
`target/wunder-egui/dist/win7-x86/wunder-egui-<version>-win7-x86.exe`。构建会在发布前
检查 PE 导入表，拒绝 `bcryptprimitives.dll`、`ProcessPrng`、`api-ms-win-*` 与
`WaitOnAddress` 等 Win7 不兼容导入，并保留 `imports.txt` 供复核。不要向系统目录或
程序目录复制 `bcryptprimitives.dll`；该 DLL 不是此问题的正确修复方式。

此构建使用相邻 Win7 SDK 中独立的 `cargo-vendor-egui` 锁定依赖快照。PE 门禁只能说明
静态导入兼容，仍需在目标机验证 OpenGL 驱动、中文输入、HTTPS 和真实聊天流程。
