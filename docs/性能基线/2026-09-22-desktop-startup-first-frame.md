# Desktop 启动首帧基线

## 目标

验证桌面窗口先显示轻量启动页，首帧后至少等待 10ms，再启动本地 bridge 和完整前端应用；同时记录窗口首帧与工作台会话就绪的差异。

## 环境与入口

- Windows 工作区 `D:\proj\wunder`，Electron 38.7.0，Node 24.14.0。
- Electron 开发壳，前端分别使用构建后的 `dist-desktop` 目录；每组 1 次预热、3 次测量。
- 命令：`powershell -ExecutionPolicy Bypass -File desktop/electron/scripts/benchmark-startup.ps1 -Runs 3 -WarmupRuns 1 -FrontendRoot <dist-desktop> -DataRoot <data-root>`。
- 运行期间机器存在 Rust 编译负载，结果用于验证时序和回归趋势，不作为硬件无关承诺。

## 结果（中位数，毫秒）

| 指标 | 修改前同口径基线 | 修改后最终采样 | 变化 |
| --- | ---: | ---: | ---: |
| Electron 主进程模块加载 | 72.2 | 5.2 | -67.0 |
| `app.whenReady` | 117.9 | 60.3 | -57.6 |
| `window_ready_to_show` | 374.8 | 323.7 | -51.1 |
| 首帧后启动门槛 `post_first_frame` | 未记录 | 360.9 | 新增观测 |
| 前端首帧后释放 `frontend_post_first_frame` | 未记录 | 756.9 | 新增观测 |
| bridge 就绪 | 323.6 | 515.1 | +191.5 |
| 会话对话可用 | 884.4 | 1282.0 | +397.6 |

## 结论

通过首帧顺序验收：启动页先绘制，Electron 的 bridge 启动和 Tauri 的 `desktop_startup_ready` 都在首帧后至少 10ms 执行；前端大型依赖通过动态入口加载。首帧时间有改善，但会话可用时间变长，不能宣称完整流程“秒开”或整体性能提升。后续应在无并发编译、固定缓存和相同数据目录下重复 3 组采样，再决定是否继续压缩 bridge 初始化。

## 验证命令

- `node --test desktop/electron/src/startupPolicy.test.js desktop/electron/src/startupFrame.test.js desktop/electron/src/windowVisibilityGuard.test.js desktop/electron/src/desktopCompatibility.test.js`
- `node frontend/scripts/regression/desktop-startup-browser.test.cjs temp_dir/desktop-startup-frontend-final`
- `npm run build:check -- --outDir ../temp_dir/desktop-startup-web-check`
- `cargo +1.95.0-x86_64-pc-windows-msvc check --release -j 8 -p wunder-desktop --features desktop --bin wunder-desktop`（通过；使用精简 Tauri 检查配置）
- `cargo +1.95.0-x86_64-pc-windows-msvc test --release -j 8 -p wunder-desktop --features desktop --bin wunder-desktop startup::tests -- --test-threads=8`（1 通过）
