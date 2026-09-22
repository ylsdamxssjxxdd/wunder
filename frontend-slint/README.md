# frontend-slint

这是 wunder 的默认桌面用户前端。默认启动会查找同目录的 `wunder-desktop-bridge.exe`（开发环境也查找仓库 `target/release`），后台拉起本地运行时并连接。后端配置和 SQLite 路径沿用 bridge 默认值；界面退出时回收本次启动的 bridge。使用 `--connect` 可连接已运行的 bridge，退出界面不会关闭外部 bridge。纯界面演示改用 `--demo`。

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

界面包含聊天、专家、工具、文件、系统设置/模型配置，沿用 TS 浅色蜂巢配色；没有蜂群和深色切换入口。

- 聊天：加载最近 100 个会话及当前会话最近 100 条记录，支持新建、切换、草稿隔离、刷新、WebSocket 流式输出、停止和断线 replay。消息以轻量文本块每 33 ms 合并更新，复制保留原文。单条气泡最多展示前 16 KiB/160 行，避免老系统软件渲染坐标溢出；原文缓冲最多 8 MiB，超过时提示从历史查看。发送过程中禁止重复提交和会话切换；若请求超时，请先刷新确认结果再重发。
- 智能体：读取默认智能体和已有智能体，支持复制默认配置创建智能体；消息页新建会话时使用当前选中的智能体；支持保存名称、描述、多行系统提示词和模型配置键。留空模型键继承默认值，默认专家模型在系统设置中调整。已开始线程的提示词仍由后端保持冻结。
- 工具：读取现有工具目录，提供分类切换和前缀搜索，当前仅展示，不修改工具权限。
- 文件：与聊天右侧共用目录投影，支持子目录、上级、刷新、每页 100 项、文本预览和复制。预览最多 32 KiB，二进制降级提示；加载错误直接显示。文件读取不会占用聊天加载状态，过期请求不会覆盖当前目录。
- 设置：读取本地工作目录、语言与模型列表，支持新增/编辑模型，以及按模型类型设置默认项。编辑时配置键固定；密钥留空保留原值，已有模型的其他字段保留。本地运行时分区可保存工作目录和运行时语言，切换目录不会迁移旧文件。

`--demo` 使用有界内存演示，创建与保存不会写盘。附件上传、文件写操作、智能体删除/工具权限编辑，以及记忆、渠道和定时任务管理尚未实现。当前任务列表投影已有会话，未展示 TS 全部调度统计。

## Win7 32 位

工程复用了参考项目的 Slint 实现及已验证的 Slint 1.18 software renderer、Winit 0.30.2 和 Win7 兼容 vendor patch。完整的 `config/fonts/msyh.ttc` 与 `config/fonts/msyhbd.ttc` 会在启动前从 EXE 内存注册，UI 不依赖目标机器安装字体。

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

它会生成 `smoke.txt` 和各页面的浅色截图，并通过真实指针事件验证侧边栏单击切换，再检查输入限制、会话隔离、任务切换、有界历史、新任务、专家编辑、模型编辑和默认项切换。

真实 bridge 联合验证（输出目录必须尚不存在）：

```powershell
python frontend-slint/scripts/check-bridge.py --ui target/frontend-slint/dist/win7-x86/wunder-frontend-slint-0.4.0-win7-x86.exe --bridge target/release/wunder-desktop-bridge.exe --output frontend-slint/artifacts/bridge-check
```

脚本启动隔离 SQLite/工作目录及本机模型测试服务，通过原生 UI 回调验证专家创建/编辑、工具、模型保存/默认项、聊天历史回读、目录导航/预览、运行时设置，以及连续长文本输出的完整性、输出期间输入和导航；UI 合并耗时记录在 `ui/stream-metrics.json`；完成后关闭测试进程，不连接外部模型。`--bridge-smoke` 会写入测试数据，仅用于该隔离环境。Windows 7 的验收目前包含 x86 Release 构建及 PE 导入门禁，仍需在 Win7 真机完成实际运行验证。

Windows 启动/退出回归（需要 Python `psutil`，输出目录必须尚不存在）：

```powershell
python frontend-slint/scripts/check-launch.py --ui target/frontend-slint/dist/win7-x86/wunder-frontend-slint-0.4.0-win7-x86.exe --bridge target/release/wunder-desktop-bridge.exe --output frontend-slint/artifacts/launch-check
```

脚本复制程序到隔离目录，发送原生窗口关闭事件，验证默认启动、启动中关闭时回收自建 bridge，以及 `--connect` 退出后保留外部 bridge；不会调用外部模型。
