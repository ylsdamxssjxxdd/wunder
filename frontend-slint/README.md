# frontend-slint

这是 wunder 的默认桌面用户前端。双击 EXE 即可在同一进程启动 Rust 后端，不需要 `--native`、bridge 程序、本机 HTTP 或 WebSocket。`--native` 暂时作为无参数启动的兼容别名；`--connect`、`--bridge-smoke` 已移除。`--demo` 仅用于界面演示。

后端继续复用共享 AppState、SQLite、ThreadRuntime、工具权限与执行链路。默认数据目录是程序旁的 `WUNDER_TEMPD` 和 `WUNDER_WORK`，兼容已有桌面数据；模型供应商与外部工具自身仍按各自网络协议调用。

## 运行与预览

```powershell
cargo +1.95.0-x86_64-pc-windows-msvc run --release --manifest-path frontend-slint/Cargo.toml -j 8
slint-viewer --check frontend-slint/ui/main.slint
slint-viewer --screenshot frontend-slint/artifacts/preview.png --size 1360x820 frontend-slint/ui/main.slint
```

界面包含聊天、专家、工具、系统设置/模型配置，沿用 TS 浅色蜂巢配色；没有蜂群、独立文件页和深色切换入口。工作目录固定显示在聊天右侧顶部，包含容器编号与本地根路径。

- 聊天：加载最近 100 个会话及当前会话最近 100 条记录，支持新建、切换、草稿隔离、刷新、原生内存通道流式输出、停止和排队事件回读。消息以轻量文本块每 33 ms 合并更新，复制保留原文。单条气泡最多展示前 16 KiB/160 行，避免老系统软件渲染坐标溢出；原文缓冲最多 8 MiB，超过时提示从历史查看。发送过程中禁止重复提交和会话切换；若请求超时，请先刷新确认结果再重发。
- 智能体：读取默认智能体和已有智能体，支持复制默认配置创建智能体；消息页新建会话时使用当前选中的智能体；支持保存名称、描述、多行系统提示词和模型配置键。留空模型键继承默认值，默认专家模型在系统设置中调整。已开始线程的提示词仍由后端保持冻结。
- 工具：读取现有工具目录，提供分类切换和前缀搜索，当前仅展示，不修改工具权限。
- 工作目录：位于聊天右侧顶部，与网页端工作区一致，显示容器编号和本地路径，支持搜索、刷新、子目录、文本预览和复制。预览最多 32 KiB，二进制降级提示；加载错误直接显示。
- 设置：读取本地工作目录、语言与模型列表，支持新增/编辑模型，以及按模型类型设置默认项。编辑时配置键固定；密钥留空保留原值，已有模型的其他字段保留。本地运行时分区可保存工作目录和运行时语言，切换目录不会迁移旧文件。

`--demo` 使用有界内存演示，创建与保存不会写盘。附件上传、文件写操作、智能体删除/工具权限编辑，以及记忆、渠道和定时任务管理尚未实现。独立文件页已移除，文件能力统一收纳在聊天工作目录区域。当前任务列表投影已有会话，未展示 TS 全部调度统计。

所有现有实体页面均由 `NativeDesktop` 直接提供数据。智能体目录复用共享预设初始化、权限过滤与配置文件投影；修改模板不会覆盖已冻结的线程提示词。工具页展示当前用户可调用的工具目录。模型投影不返回密钥，配置写入串行化，保留未编辑的字段，失败时尝试恢复原配置。文件浏览限制在所选智能体工作区，拒绝父路径、绝对路径与指向外部的链接。

原生事件使用容量 128 的异步内存通道，UI 每 33 ms 最多消费 128 项并限制约 5 ms 的消费时间，只刷新一次活动文本块。原生模式开启共享任务队列，并在恢复旧任务状态后启动 dispatcher；排队请求按任务 ID 回读持久化事件；正常结束只释放订阅，不取消下一轮；显式停止清理目标与线程执行状态并保存停止标记。原生历史与会话列表各限 100 项，显示裁剪继续保留完整原文供复制。

隔离原生联调（需要 Python `psutil`，输出目录必须不存在）：

```powershell
python frontend-slint/scripts/check-native.py --ui frontend-slint/target/release/wunder-frontend-slint.exe --output target/frontend-slint/native-check
```

脚本准备独立 SQLite、工作目录和本机测试模型，检查智能体创建/编辑/归属、工具目录、模型密钥保留/默认项、工作目录设置/预览/越界拒绝，以及未知实体拒绝、启动前停止、同线程排队、订阅释放后的后端完成、长流完整性、历史回读、停止、输出期间输入/导航/停止自动滚动、退出，以及没有本机监听和 bridge 子进程。输出包含 `smoke.txt`、`stream-metrics.json` 与截图。UI 更新耗时不包含全部渲染/排版时间，Win7 真机流畅性仍需单独验证。`--native-smoke` 只接受已准备配置的隔离目录。

`builders/build-win7-slint.ps1` / `builders/build-win7-offline.ps1` 始终构建完整原生依赖，不再需要 `-NativeRuntime`；旧的仅前端离线 SDK 缺少 runtime 依赖，需补齐相应依赖包。桌面局部 `html2md` 补丁只构建 `rlib`，避免上游额外 Rust dylib 与 `panic=abort` 冲突。

2026-09-23 默认原生 x64 Release 联调通过（`target/frontend-slint/native-check-20260923-f/`）：聊天、停止与排队、智能体/工具/配置/文件页面操作及跨进程重启后回读均通过；50,999 字节、175 次 UI 刷新，最大 UI 应用耗时 1.722 ms，最大积压 6/128，峰值工作集 86,974,464 字节。无 bridge 子进程及监听端口。无参数启动、重启和提前关闭另经 `native-launch-20260923-a/` 验证。指标不包含全部布局绘制耗时，也不代表 Win7 真机验收。

## Win7 32 位

工程复用了参考项目的 Slint 实现及已验证的 Slint 1.18 software renderer、Winit 0.30.2 和 Win7 兼容 vendor patch。完整的 `config/fonts/msyh.ttc` 与 `config/fonts/msyhbd.ttc` 会在启动前从 EXE 内存注册，UI 不依赖目标机器安装字体。

有离线 SDK 时运行：

```powershell
powershell -ExecutionPolicy Bypass -File builders/build-win7-offline.ps1 -Check
powershell -ExecutionPolicy Bypass -File builders/build-win7-offline.ps1
# 旧 SDK 可使用已经缓存完整原生依赖的 Cargo 目录，仍然强制离线：
powershell -ExecutionPolicy Bypass -File builders/build-win7-offline.ps1 -CargoCacheRoot $env:USERPROFILE\.cargo
```

2026-09-23 完整原生 Win7 x86 Release 构建与 PE 门禁通过，产物约 76.6 MiB。该 32 位产物在当前 Windows 主机的隔离回归通过（`target/frontend-slint/native-win7-check-20260923-a/`）：聊天与全部已接入页面、配置重启回读，50,999 字节、184 次刷新、最大 UI 投影应用 1.010 ms、积压 30/128、峰值工作集约 70.6 MiB。无参数启动、重启及启动中关闭另经 `target/frontend-slint/native-win7-launch-20260923-a/` 验证，未创建 bridge 子进程或本机监听。**尚未在 Win7 真机执行**；编译、导入门禁及当前系统运行结果不能替代真机验证。

Release 输出：`target/frontend-slint/dist/win7-x86/wunder-frontend-slint-0.4.0-win7-x86.exe`。构建脚本会检查最终 PE 是否为 i386，并拒绝 Win7 不支持的 WinRT/API Set 导入。

## 原生冒烟检查

```powershell
wunder-frontend-slint-0.4.0-win7-x86.exe --smoke-check frontend-slint/artifacts/native
```

它会生成 `smoke.txt` 和各页面的浅色截图，并通过真实指针事件验证侧边栏单击切换，再检查输入限制、会话隔离、任务切换、有界历史、新任务、专家编辑、模型编辑和默认项切换。

无参数启动、重启和关闭回归（需要 Python `psutil`，输出目录必须尚不存在）：

```powershell
python frontend-slint/scripts/check-launch.py --ui frontend-slint/target/release/wunder-frontend-slint.exe --output target/frontend-slint/native-launch-check
```

脚本把 EXE 复制到隔离目录，验证不带参数即可启动、无子进程及监听、正常关闭、重启和启动中关闭。即使遗留配置启用了 LAN 发现，原生 UI 也不会启动本地控制监听。

## Linux AppImage 与交叉构建

Linux x86_64 与 ARM64 桌面版使用 Ubuntu 18.04/glibc 2.27 作为构建基线，AppImage 内携带
Slint 软件渲染所需的 XCB/X11/XKB 动态库，并使用 gzip SquashFS 兼容老旧发行版。
已有的 `rcho-slint-arm64-ubuntu18:latest` 作为基础镜像即可，无需重做；仓库提供的
wunder 派生镜像只补充 XCB 运行库和 SquashFS 工具：

```bash
docker build --platform linux/arm64 \
  -t wunder-slint-arm64-ubuntu18:latest \
  -f packaging/docker/Dockerfile.ubuntu18-arm64-slint .
```

ARM64 的本地构建脚本（Windows 主机）使用：

```powershell
powershell -ExecutionPolicy Bypass -File builders/build-linux-arm64-appimage.ps1
```

脚本默认使用同级 `rcho/target/electron/release/` 中已有 ARM64 AppImage 作为
type-2 runtime；也可以显式传入 `-AppImageRuntime <路径>`。Rust 1.92 和 Cargo
vendor 从同级 `Rust-builder/kylin2` 挂载，构建完成后产物位于
`target/slint/dist/linux-arm64/`。检查产物可运行：

```bash
bash frontend-slint/scripts/check-linux-arm64-appimage.sh \
  target/slint/dist/linux-arm64/wunder-slint-0.4.0-linux-arm64.AppImage
```

AppImage 启动时把 `WUNDER_TEMPD` 和 `WUNDER_WORK` 放到用户数据目录，不会写入只读的
AppImage 挂载目录；也可通过 `--temp-root`、`--workspace` 或对应环境变量覆盖。

构建入口集中在仓库根 `builders/`；`frontend-slint/scripts/` 只保留原生联调、启动回归和
AppImage 结构检查。参考 rcho 的离线 SDK 组织，ARM64 Ubuntu 18.04 主机（或 Docker 中
的 ARM64 Ubuntu 18.04 镜像）可交叉生成 Linux amd64 与 Win32 i686 原生 Slint 产物：

```bash
# ARM64 Linux -> Ubuntu 18.04 x86_64 ELF
bash build-linux-amd64-offline.sh --native

# ARM64 Linux -> type-2 x86_64 AppImage
WUNDER_APPIMAGE_RUNTIME=/path/to/x86_64-runtime.AppImage \
  bash build-linux-amd64-offline.sh --docker --appimage

# ARM64 Linux -> Win32 i686 PE
bash build-win32-arm64-offline.sh --native

# 统一入口
bash build-linux-arm64-offline.sh -t amd64 --docker
bash build-linux-arm64-offline.sh -t win32 --docker
```

Windows 主机使用对应 PowerShell 包装器：

```powershell
powershell -ExecutionPolicy Bypass -File builders/build-linux-amd64-offline.ps1 -Docker
powershell -ExecutionPolicy Bypass -File builders/build-linux-amd64-offline.ps1 -Docker -AppImage -AppImageRuntime <x86_64-AppImage>
powershell -ExecutionPolicy Bypass -File builders/build-win32-arm64-offline.ps1 -Docker
```

Linux AppImage 必须提供同架构 type-2 runtime（`WUNDER_APPIMAGE_RUNTIME` 或
`-AppImageRuntime`），不会再依赖固定版本的参考工程产物。ARM64 打包、amd64 交叉打包
都在输出目录创建唯一私有临时目录，完成后原子发布；失败会清理临时文件并保留已有版本。
构建缓存分别位于 `target/linux-arm64-ubuntu18-slint`、
`target/linux-amd64-ubuntu18-slint` 和 `target/win32-x86-arm64`，与旧桌面壳产物隔离。

CI 发布只保留三个桌面程序：Win7 32 位 EXE、Ubuntu 18.04 x86_64 AppImage 和 Ubuntu 18.04 ARM64 AppImage。
