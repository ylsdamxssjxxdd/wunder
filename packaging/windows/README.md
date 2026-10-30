# Windows 7 补充包打包说明

本目录用于构建给 `wunder-desktop` Windows 7 试包配套的补充包（压缩包名 `wunder-supplement-…`），提供：

- `opt/python`：给智能体、本地脚本与 Python 工具调用使用的内置 Python
- `opt/git`：给 `git clone`、`git status`、补丁应用与仓库操作使用的内置 Git
- `opt/rg`：给 `search_content` 与命令行检索使用的内置 ripgrep
- `opt/ffmpeg`：随包附带的静态 ffmpeg（`opt/ffmpeg/bin` 已由桌面运行时自动接入 `PATH`）
- `opt/powershell`：随包附带的便携 PowerShell 7.1.7（`pwsh.exe`），是 `执行命令` 工具的默认 shell；可用 `-SkipPowerShell` 跳过，详见「版本选择」
- `opt/browser`：默认内置 Chrome 109 win32 浏览器，供 `browser_*` 工具开箱使用；可用 `-SkipBrowser` 跳过，详见「内置浏览器」

补充包设计目标：**统一安装到 `WUNDER_HOME/opt`，蛹与舵机共用一份运行环境**。

当前约定：

- `build.bat pack -a win7-x86` 生成单文件 `wunder-<版本>-win7-x86-setup.exe`；安装向导把程序本体也作为可勾选项：蛹（桌面端）与舵机（命令行）默认勾选，不勾则不安装对应的 `wunder-desktop.exe`/`wunder-cli.exe`，因此可以只装补充组件。补充组件里只有 PowerShell 7 默认勾选（它是命令工具的默认 shell，缺了会退回系统 PowerShell 2.0），Python/Git/ripgrep/ffmpeg/内置浏览器默认不勾选。勾选蛹即自动创建开始菜单与桌面快捷方式（不再单列“桌面快捷方式”任务）。页面提供“全选（推荐）”一次勾选全部项，任一项取消会自动回退全选状态。
- 安装向导允许改安装目录，默认 `%USERPROFILE%\.wunder`。安装根会写入 `.wunder-home`，蛹和舵机启动时以该根作为 `WUNDER_HOME`；显式设置的非空 `WUNDER_HOME` 优先。
- 安装目录同时保存配置、会话、工作区、日志和 `opt/` 补充组件；迁移时复制整个安装根。
- 选中已存在的组件时，安装器先清空对应 `opt/<component>`，再写入新版本；`config`、`data`、`workspace`、`logs` 等用户数据不会删除。
- 勾选舵机时，安装完成后 `{安装根}\bin` 会加入当前用户 `PATH`（重复安装不重复追加，卸载时只移除这一项）。新开终端可直接运行 `wunder-cli`；`desktop` 启动蛹，`wunder-desktop` 也可直接运行。
- 开始菜单使用形态名称：蛹（桌面程序）与舵机（命令行）；桌面快捷方式随勾选“蛹”自动创建；卸载入口仍为“卸载心舰”。
- 卸载器移除 `bin`、`.wunder-home`、安装时选择的组件目录和安装器写入的用户 `PATH` 项，保留用户数据。
- 补充包仍可单独导入：在蛹设置中选择“导入补充包”，或手动解压到 `WUNDER_HOME`。两端自动识别共享 home 中的 `opt/python`、`opt/git`、`opt/rg`、`opt/ffmpeg`、`opt/powershell` 与 `opt/browser`。

## 安装后的终端命令

```bat
wunder-cli
cd D:\work\demo
wunder-cli
desktop
```

- `wunder-cli` 是舵机；`desktop` 是蛹的启动器（`wunder-desktop` 等价）。安装器通过用户级 `PATH` 暴露命令，已经打开的终端需要重开，或从系统属性重新加载环境变量。
- 舵机把启动它的目录登记为工作区；需要指定其他目录时使用 `wunder-cli -C <目录>`。工作区数据仍保存在安装根下。

## 安装器打包

先构建蛹和舵机，再准备补充包 zip，然后在仓库根目录执行：

```bat
build.bat pack -a win7-x86
```

输入默认从 `target/frontend-slint/dist/win7-x86`、`target/cli/dist/win7-x86`、`dist/win7-x86`、`target/installer/payload/win7-x86` 和补充包构建目录查找；也可以用 `-Desktop`、`-Cli`、`-Supplement` 显式指定。产物在 `target/installer/dist/win7-x86/`，同时生成 `.sha256` 和 `.manifest.json`。`-CoreOnly` 用于快速验证核心安装器。

Windows 安装包优先使用离线 SDK 的 `Rust-builder/win7/offline/tools/inno-setup/app/ISCC.exe`（Inno Setup 6.7.3，版本与 SHA256 见同目录 `VERSION.txt`）。也可以用 `-Iscc` 或 `WUNDER_ISCC` 指定已有编译器，`-NoDownload` 禁止联网；只有离线编译器缺失且允许下载时才会准备到 `temp_dir/tools/inno-setup`。SDK 根目录可用 `WUNDER_WIN7_BUILDER_ROOT` 或 `WUNDER_BUILDER_ROOT` 覆盖。

补充包的版本与下载说明见下文。

## 版本选择

- Python：`3.8.10 embeddable package`
  - Python 官方从 `3.9` 开始要求 Windows `8.1+`，因此 Win7 路线固定使用 `3.8`。
  - 当前脚本使用官方可下载的 embeddable 压缩包，便于安装到共享用户目录。
- Playwright 驱动：Win7 包固定使用已验证的 `playwright==1.40.0` 与 Chrome 109 补充包组合。官方 `win32` wheel 内仍带 x64 的 `driver/node.exe`，构建脚本会下载固定 SHA256 的 Node 16.20.2 `win-x86`，替换该文件后再做 i386 PE 门禁。Node 16 不在 Node 官方的 Windows 7 支持矩阵内，桥接会设置 `NODE_SKIP_PLATFORM_CHECK=1`，仍需在真实 ia32 Win7 上验收。
- Git：`MinGit 2.46.2`
  - Git for Windows FAQ 将 `2.46.2` 标为最后支持 Windows `7 / 8 / 8.1` 的版本。
  - `MinGit` 适合嵌入第三方桌面应用，体积比完整安装包更小（不含 Git Bash 与 POSIX 工具链，`git.exe` 位于 `cmd/`）。
  - `MinGit` 默认仍带一批智能体用不到的东西，构建的剪裁阶段会一并移除：Git Credential Manager 的 .NET/Avalonia UI（含 `SkiaSharp`、`Microsoft.Identity.*`、`msalruntime` 等，约 25 MB）、`scalar`、`tig`、`gitk`/`git gui`、vim/perl 与 Tcl/Tk 运行时、各语言文档树。剪裁后 `opt/git` 约 58 MB，保留 `git.exe`、原生依赖 DLL、`usr/bin` 下的 MSYS 运行时（shell 脚本用）和 `mingw32/libexec/git-core` 下的脚本。
  - 剪裁同时把 `etc/gitconfig` 的 `credential.helper` 从 `manager` 改为 `wincred`，并删除 MinGit 原带的、指向本机 `C:/Program Files/Git/etc/gitconfig` 的 `[include]` 段：否则装过 Git for Windows 的机器会重新启用已被删除的凭据管理器。
- capstone：`5.0.1`
  - `5.0.2` 的 `py3-none-win32` wheel 里 `capstone/lib/capstone.dll` 实际是 x86-64 镜像（上游打包错误，`win32` 与 `win_amd64` 两个 wheel 携带同一个 0x8664 DLL），32 位 Python 无法加载；`5.0.3` 起不再发布 win32 wheel。`5.0.1` 是同系列 API 且带真正的 i386 DLL。
  - 该问题暴露的是「逐个组件自己声明架构」不可靠：补充包在打包前会对 `opt/` 下所有 `exe/dll/pyd/node` 做一次全量 PE 架构门禁，出现与目标架构不符的文件直接构建失败。
- ripgrep：`14.1.0`
  - `14.1.1` 起的官方 Windows 二进制用 Rust 1.78+ 构建，静态导入 `bcryptprimitives.dll`（`ProcessPrng`）和 `api-ms-win-core-synch-l1-2-0.dll`（`WaitOnAddress`），这两个 DLL 从 Windows 8 才提供；在 32 位 Windows 7 上启动即崩溃并报 `Segmentation fault`。
  - `14.1.0` 是最后一个仍能在 Windows 7 运行的官方版本。构建脚本会校验 SHA256、PE 架构和导入表；若以后要升级到 `14.1.1+`，必须用 Rust `<= 1.77.2` 自行重编，否则门禁会直接失败。
  - 代价：`14.1.1` 才修复了正则内层字面量优化可能漏匹配的问题（ripgrep #2884），`14.1.0` 仍带该缺陷；若搜索正确性必须包含该修复，只能用 Rust `<= 1.77.2` 自行重编 `14.1.1`。
- PowerShell：`7.1.7 portable (win-x86 zip)`
  - `7.1.7` 是最后一个能在 Windows 7 上启动的 PowerShell 7：`7.2` 起基于 .NET 6，要求 Windows 8.1+；它也是最后一个提供自包含 win-x86 zip 的版本。
  - zip 自带 .NET 5 运行时、降级版 API-set shim 与 UCRT，解压即用：不写注册表、不需要管理员权限、不需要重启，这决定它（而不是需要系统级安装的 WMF 5.1）成为补充包的默认选择。
  - 目标机仍需打齐 Universal CRT（`KB2999226`）与 Visual C++ 2015-2019 运行库才能加载 .NET 5 宿主；即便缺失，执行命令工具会在启动探测失败后自动回退系统 Windows PowerShell（Win7 上是 2.0），不会出现整条命令链瘫痪。
  - 运行时按绝对路径优先选择 `opt/powershell/pwsh.exe`（可执行文件名不是 `powershell.exe`，靠 PATH 永远命中不了它），并把路径发布到 `WUNDER_POWERSHELL_EXE`；版本探测与实际启动使用同一个二进制。
  - 剪裁阶段会移除非 `en-US/zh-Hans/zh-Hant` 的语言资源目录与 `ref/` 引用程序集（约 22 MB），模型侧的工具说明会按 pwsh 7 的差异（无 `Get-WmiObject`、无 `-ComputerName` 参数等）单独下发。

## 入口命令

在仓库根目录执行：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 -Arch ia32
```

- 该默认入口会产出 `common` 档位补充包（当前唯一档位），并默认通过清华 Tuna 简单索引安装 `packaging/python/requirements-win7-common.txt` 中的依赖。

如需重新下载官方压缩包：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 -Arch ia32 -RefreshDownloads
```

如需切回官方 PyPI：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 `
  -Arch ia32 `
  -PythonPackageIndexUrl https://pypi.org/simple
```

如果外网波动导致官方包下载不稳定，也可以先手工准备两个压缩包，再直接喂给脚本：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 `
  -Arch ia32 `
  -PythonArchivePath C:\cache\python-3.8.10-embed-win32.zip `
  -GitArchivePath C:\cache\MinGit-2.46.2-32-bit.zip `
  -RgArchivePath C:\cache\ripgrep-14.1.0-i686-pc-windows-msvc.zip
```

### 剪裁与两步式构建

每次构建都会在打包前跑一次剪裁（`prune-supplement-stage.py`，PS 脚本只是它的调用方）：删除各 wheel 的 `tests`/`testdata`、与目标架构不符的 PE 文件（ia32 包里不允许出现 x64/arm64 的 exe/dll/pyd）、以及 MinGit 里智能体用不到的部分。随后依次执行：

- `git` 冒烟：用包内 `cmd\git.exe` 在临时仓库里 `init` / `commit` / `log`，验证 MSYS 运行时与 shell 脚本仍然可用。
- PE 架构门禁：扫描 `opt/` 下全部 PE 文件，出现一个不符架构的直接失败。
- 压缩打包。

有些构建环境（例如在带批量删除保护的 agent 运行时里执行）会拦截 `python.exe` 子进程的批量删除，剪裁步骤会被拒绝。这类主机用两步式构建：

```powershell
# 第一步：只做落位，跳过剪裁（末尾 PE 门禁会失败并提示下一步）
...build_win7_desktop_supplement.ps1 -Arch ia32 -SkipPrune
# 第二步：仅剪裁 + 冒烟 + 门禁 + 重新打包
...build_win7_desktop_supplement.ps1 -Arch ia32 -PruneOnly
```

`-PruneOnly` 不重新下载、不重装 Python 包，直接用现有 stage，几十秒即可产出干净的压缩包。

## 默认输出目录

- 下载缓存：`temp_dir/win7-gnu-lab/win7-supplement/downloads/`
- 展开目录：`temp_dir/win7-gnu-lab/win7-supplement/stage/package-root/`
- 默认压缩包：`target/installer/payload/win7-x86/wunder-supplement-win7-ia32-common.zip`（`build.bat pack` 直接检索该目录；显式传 `-BuildRoot` 时产物留在该构建根的 `dist/` 下）

压缩包内部目录结构是：

```text
opt/
  python/
  git/
  rg/
  ffmpeg/
  powershell/     （便携 PowerShell 7.1.7，pwsh.exe；可用 `-SkipPowerShell` 跳过）
  browser/        （默认自动准备 Chrome 109；可用 `-SkipBrowser` 跳过）
README-win7-supplement.txt
wunder-win7-supplement.json
```

## 内置浏览器

Win7 最高只支持到 Chrome 109（`109.0.5414.x`，最后一个支持 Windows 7 的 Chrome 版本）。补充包默认自动准备 Chrome 109 win32 浏览器，导入后蛹的 `browser_*` 工具开箱可用。

- 布局：浏览器解到补充包的 `opt/browser/`，默认要求存在 `opt/browser/chrome-win/chrome.exe`；`opt/browser/chrome.exe` 也是运行时认可的回退探测位置。若投喂的压缩包顶层只有一层包裹目录（如 `chrome-win/` 或 `chrome-win32/`），会保留其结构；若解开后找不到 `chrome-win/chrome.exe`，脚本会在解包结果中递归定位 `chrome.exe` 并重排到 `chrome-win/chrome.exe`，仍找不到则报错退出。ia32 档还会校验 `chrome.exe` 的 PE 架构必须是 32 位。
- 浏览器来源优先级：`-BrowserArchivePath` 显式路径、清单 `browser.archivePath`、仓库中已有的 Chrome 109 缓存；都找不到时，构建脚本自动下载清单中固定版本并校验 SHA-256，再用 7-Zip 解包。若本机没有 7-Zip，请安装 7-Zip 或显式传入已解开的浏览器目录。
- 投喂方式（二选一）：
  - 命令行参数（优先级更高）：
    ```powershell
    powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 `
      -Arch ia32 `
      -BrowserArchivePath <本地目录>\chrome-109-win32.zip
    ```
  - 或在 `packaging/windows/scripts/win7-supplement-manifest.json` 的顶层 `browser.archivePath` 填入压缩包路径。
  - `-BrowserArchivePath` 既接受 `.zip` 压缩包，也接受已解开的浏览器目录；`-SkipBrowser` 可跳过内置浏览器。
- 获取途径：Chrome 官方旧版离线安装包（win32 的 109.0.5414.x）或可信镜像；解出安装包内的 `chrome.7z`/`chrome-win` 目录即可投喂。浏览器二进制是否允许再分发由投喂者自行确认，本仓库不替使用者做合规结论。
- CI 构建：`.github/workflows/desktop-supplements.yml` 的 build-win7-ia32 已默认投喂 Chromium 109.0.5414.120 win32（Hibbiki/chromium-win32 发布页 `v109.0.5414.120-r1070088` 的 `mini_installer.sync.exe`，pin 死 URL 并强制 SHA256 校验，浏览器 pin 以字面量并入 win7 缓存键，变更即触发重建）；本地默认构建也使用同一份清单 URL/SHA-256 自动准备浏览器。
- 运行时如何被发现：在蛹设置中导入补充包后，运行时会自动探测 `WUNDER_HOME/opt/browser/chrome-win/chrome.exe`（回退 `opt/browser/chrome.exe`），`browser_*` 工具直接使用，无需手工修改 `wunder.yaml`。

## 使用方式

推荐在蛹设置中选择“导入补充包”，导入后新启动的工具立即生效。手动安装步骤：

1. 关闭使用该补充包的蛹、舵机及工具进程。
2. 将 `wunder-supplement-win7-ia32-common.zip` 解压到 `WUNDER_HOME`（默认用户主目录的 `.wunder`）。
3. 解压后确认共享 home 下出现：
   - `opt/python`
   - `opt/git`
   - `opt/rg`
   - `opt/ffmpeg`
   - `opt/powershell`（若打包时未用 `-SkipPowerShell` 跳过）
   - `opt/browser`（若打包时投喂了浏览器）
4. 重新启动蛹或舵机。

两端启动时会自动：

- 从共享 home 探测补充包，不受程序安装目录和启动目录影响
- 将共享 home 下的 `opt/python`、`opt/python/Scripts`、`opt/git/cmd`、`opt/git/bin`、`opt/rg`、`opt/rg/bin`、`opt/ffmpeg/bin`、`opt/powershell` 接入 `PATH`，重复导入不会重复累加
- 把 `opt/powershell/pwsh.exe` 的绝对路径发布为 `WUNDER_POWERSHELL_EXE`，执行命令工具探测成功后即以它为默认 shell

这样桥接层与智能体工具调用就能优先命中内置 Python / Git / rg / PowerShell。

## 兼容性提示

- Python 3.8 embeddable package 在 Win7 上建议配合 `KB2533623` 与 Universal CRT 更新使用。
- 便携 PowerShell 7 的 .NET 5 宿主依赖 Universal CRT（`KB2999226`）与 Visual C++ 2015-2019 运行库；缺失时执行命令工具自动回退系统 Windows PowerShell，命令不会失败，只是回到旧语法。
- `opt/ffmpeg` 仅内置 `ffmpeg`（eugeneware/ffmpeg-static `b4.4.1`），不含 `ffprobe`；
  依赖 `WUNDER_FFPROBE_BIN`/`ffprobe` 的音视频探测接口仍需系统或用户侧提供 ffprobe。
- `common` 档位（当前唯一档位）会额外内置 `pip / setuptools / wheel`，并按 `packaging/python/requirements-win7-common.txt` 预装办公文档、数据库、轻量视频、地图/雷达与绘图库。
- 默认 Python 包索引为清华 Tuna：`https://pypi.tuna.tsinghua.edu.cn/simple`；如需切回官方源，可使用 `-PythonPackageIndexUrl https://pypi.org/simple`。
- 两个档位都保持 Win7 友好，优先使用二进制 wheel，避免现场编译依赖。
