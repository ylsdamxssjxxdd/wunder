# frontend-slint

这是 wunder 用户侧 Messenger 的 Slint 视觉原型。它暂时不连接 server、WebSocket 或数据库，所有会话、消息和工作目录条目都是进程内演示数据。

## 运行与预览

```powershell
cargo +1.95.0-x86_64-pc-windows-msvc run --release --manifest-path frontend-slint/Cargo.toml -j 8
slint-viewer --check frontend-slint/ui/main.slint
slint-viewer --screenshot frontend-slint/artifacts/preview.png --size 1360x820 frontend-slint/ui/main.slint
```

原型包含左侧导航与会话列表、中间聊天区、智能体循环折叠条、右侧任务/工作目录面板、消息编辑器、浅色/深色主题和窄窗口工作目录浮层。消息列表使用有界内存模型，便于后续接入 ThreadRuntime 和实时投影。

## Win7 32 位

工程复用了 `D:\proj\rcho\frontend-slint` 已验证的 Slint 1.18 software renderer、Winit 0.30.2 和 Win7 兼容 vendor patch。完整的 `config/fonts/msyh.ttc` 与 `config/fonts/msyhbd.ttc` 会在启动前从 EXE 内存注册，UI 不依赖目标机器安装字体。

有离线 SDK 时运行：

```powershell
powershell -ExecutionPolicy Bypass -File frontend-slint/scripts/build-offline.ps1 -Check
powershell -ExecutionPolicy Bypass -File frontend-slint/scripts/build-offline.ps1
```

Release 输出：`target/frontend-slint/dist/win7-x86/wunder-frontend-slint-0.4.0-win7-x86.exe`。构建脚本会检查最终 PE 是否为 i386，并拒绝 Win7 不支持的 WinRT/API Set 导入。

## 原生冒烟检查

```powershell
wunder-frontend-slint-0.4.0-win7-x86.exe --smoke-check frontend-slint/artifacts/native
```

它会生成 `smoke.txt`、浅色截图和深色截图，并验证输入限制、会话隔离、任务切换、有界历史、主题切换和新任务流程。
