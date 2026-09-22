# frontend-slint

这是 wunder 用户侧 Messenger 的 Slint 原型。默认启动时保留进程内演示数据；通过 `--connect` 启动时会使用桌面 bridge 的既有聊天 HTTP 接口加载会话和历史、创建会话并发送非流式消息。WebSocket 流式事件、附件和工作目录仍在后续接入。

## 运行与预览

```powershell
cargo +1.95.0-x86_64-pc-windows-msvc run --release --manifest-path frontend-slint/Cargo.toml -j 8
slint-viewer --check frontend-slint/ui/main.slint
slint-viewer --screenshot frontend-slint/artifacts/preview.png --size 1360x820 frontend-slint/ui/main.slint
```

连接正在运行的本地 desktop bridge：

```powershell
wunder-frontend-slint-0.4.0-win7-x86.exe --connect http://127.0.0.1:18123
```

`--connect` 会从该 bridge 的 `/config.json` 读取本地访问令牌；它只接受本机回环 `http://` 地址，确保连接保留在 desktop bridge 内。

原型包含聊天页、智能体页、工具页和系统设置/模型配置页，固定使用 TS 版本的浅色视觉。

- 聊天：加载最近 100 个会话及当前会话最近 100 条记录，支持新建、切换、草稿隔离、刷新和非流式发送。发送过程中禁止重复提交和会话切换；若请求超时，请先刷新确认结果再重发。
- 智能体：读取默认智能体和已有智能体，支持复制默认配置创建智能体；消息页新建会话时使用当前选中的智能体。
- 工具：读取现有工具目录并提供名称/分类前缀搜索，当前仅展示，不修改工具权限。
- 设置：读取本地工作目录、语言与模型列表，支持新增/编辑模型，以及按模型类型设置默认项。编辑时配置键固定；密钥留空保留原值，已有模型的其他字段保留。

无 `--connect` 时上述页面使用有界内存演示，创建与保存不会写盘。WebSocket 实时投影、附件、真实任务/文件列表、智能体编辑删除、工具配置及其他系统设置仍待接入。连接模式的右侧面板不显示模拟任务或文件。

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

它会生成 `smoke.txt` 和各页面的浅色截图，并验证输入限制、会话隔离、任务切换、有界历史、新任务、智能体创建、模型编辑和默认项切换。

真实 bridge 联合验证（输出目录必须尚不存在）：

```powershell
python frontend-slint/scripts/check-bridge.py --ui target/frontend-slint/dist/win7-x86/wunder-frontend-slint-0.4.0-win7-x86.exe --bridge target/release/wunder-desktop-bridge.exe --output frontend-slint/artifacts/bridge-check
```

脚本启动隔离 SQLite/工作目录及本机模型测试服务，通过原生 UI 回调验证智能体、工具、模型保存/默认项和聊天历史回读；完成后关闭测试进程，不连接外部模型。`--bridge-smoke` 会写入测试数据，仅用于该隔离环境。Windows 7 的验收目前包含 x86 Release 构建及 PE 导入门禁，仍需在 Win7 真机完成实际运行验证。
