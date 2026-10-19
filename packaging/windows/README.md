# Windows 7 补充包打包说明

本目录用于构建给 `wunder-desktop` Windows 7 试包配套的补充包（压缩包名 `wunder-supplement-…`），提供：

- `opt/python`：给智能体、本地脚本与 Python 工具调用使用的内置 Python
- `opt/git`：给 `git clone`、`git status`、补丁应用与仓库操作使用的内置 Git
- `opt/rg`：给 `search_content` 与命令行检索使用的内置 ripgrep
- `opt/ffmpeg`：随包附带的静态 ffmpeg（`opt/ffmpeg/bin` 已由桌面运行时自动接入 `PATH`）
- `opt/browser`：（可选）用户自备的 Chrome 109 win32 浏览器，供 `browser_*` 工具开箱使用，详见「内置浏览器（可选）」

补充包设计目标：**统一安装到 `WUNDER_HOME/opt`，蛹与舵机共用一份运行环境**。

当前约定：

- Win7 `setup.exe` 安装包默认不再内置 Python、Git 与 ripgrep。
- 如需 Python/Git/ripgrep，请在蛹设置中导入 `wunder-supplement-win7-*.zip`，或手动解压到 `WUNDER_HOME`（默认用户主目录下的 `.wunder`）。
- 两端自动识别共享 home 中的 `opt/python`、`opt/git`、`opt/rg` 与 `opt/ffmpeg`；蛹对安装目录旧 `opt/` 保留读取回退，新导入不再写入安装目录。

## 版本选择

- Python：`3.8.10 embeddable package`
  - Python 官方从 `3.9` 开始要求 Windows `8.1+`，因此 Win7 路线固定使用 `3.8`。
  - 当前脚本使用官方可下载的 embeddable 压缩包，便于安装到共享用户目录。
- Playwright 驱动：`playwright==1.16+` 的官方 `win32` wheel 会带 x64 的 `driver/node.exe`。Win7 包固定使用 `playwright==1.15.2`，它的 `win32` wheel 原生携带 i386 Node 12.20.1；构建时会将该文件的 PE 架构作为硬门禁，防止升级依赖后重新混入 x64 驱动。
- Git：`MinGit 2.46.2`
  - Git for Windows FAQ 将 `2.46.2` 标为最后支持 Windows `7 / 8 / 8.1` 的版本。
  - `MinGit` 适合嵌入第三方桌面应用，体积比完整安装包更小。

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
  -RgArchivePath C:\cache\ripgrep-14.1.1-i686-pc-windows-msvc.zip
```

## 默认输出目录

- 下载缓存：`temp_dir/win7-gnu-lab/win7-supplement/downloads/`
- 展开目录：`temp_dir/win7-gnu-lab/win7-supplement/stage/package-root/`
- 默认压缩包：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder-supplement-win7-ia32-common.zip`

压缩包内部目录结构是：

```text
opt/
  python/
  git/
  rg/
  ffmpeg/
  browser/        （可选，投喂 Chrome 109 压缩包后才会出现）
README-win7-supplement.txt
wunder-win7-supplement.json
```

## 内置浏览器（可选）

Win7 最高只支持到 Chrome 109（`109.0.5414.x`，最后一个支持 Windows 7 的 Chrome 版本）。补充包支持把用户自备的 Chrome 109 win32 浏览器目录打进包内，导入后蛹的 `browser_*` 工具开箱可用。

- 布局：浏览器解到补充包的 `opt/browser/`，默认要求存在 `opt/browser/chrome-win/chrome.exe`；`opt/browser/chrome.exe` 也是运行时认可的回退探测位置。若投喂的压缩包顶层只有一层包裹目录（如 `chrome-win/` 或 `chrome-win32/`），会保留其结构；若解开后找不到 `chrome-win/chrome.exe`，脚本会在解包结果中递归定位 `chrome.exe` 并重排到 `chrome-win/chrome.exe`，仍找不到则报错退出。ia32 档还会校验 `chrome.exe` 的 PE 架构必须是 32 位。
- 投喂方式（二选一）：
  - 命令行参数（优先级更高）：
    ```powershell
    powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 `
      -Arch ia32 `
      -BrowserArchivePath <本地目录>\chrome-109-win32.zip
    ```
  - 或在 `packaging/windows/scripts/win7-supplement-manifest.json` 的顶层 `browser.archivePath` 填入压缩包路径。
  - `-BrowserArchivePath` 既接受 `.zip` 压缩包，也接受已解开的浏览器目录。不提供任何路径时行为与之前一致：包内不含浏览器，不报错。
- 获取途径：Chrome 官方旧版离线安装包（win32 的 109.0.5414.x）或可信镜像；解出安装包内的 `chrome.7z`/`chrome-win` 目录即可投喂。浏览器二进制是否允许再分发由投喂者自行确认，本仓库不替使用者做合规结论。
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
   - `opt/browser`（若打包时投喂了浏览器）
4. 重新启动蛹或舵机。

两端启动时会自动：

- 从共享 home 探测补充包，不受程序安装目录和启动目录影响
- 将共享 home 下的 `opt/python`、`opt/python/Scripts`、`opt/git/cmd`、`opt/git/bin`、`opt/rg`、`opt/rg/bin`、`opt/ffmpeg/bin` 接入 `PATH`，重复导入不会重复累加

这样桥接层与智能体工具调用就能优先命中内置 Python / Git / rg。

## 兼容性提示

- Python 3.8 embeddable package 在 Win7 上建议配合 `KB2533623` 与 Universal CRT 更新使用。
- `opt/ffmpeg` 仅内置 `ffmpeg`（eugeneware/ffmpeg-static `b4.4.1`），不含 `ffprobe`；
  依赖 `WUNDER_FFPROBE_BIN`/`ffprobe` 的音视频探测接口仍需系统或用户侧提供 ffprobe。
- `common` 档位（当前唯一档位）会额外内置 `pip / setuptools / wheel`，并按 `packaging/python/requirements-win7-common.txt` 预装办公文档、数据库、轻量视频、地图/雷达与绘图库。
- 默认 Python 包索引为清华 Tuna：`https://pypi.tuna.tsinghua.edu.cn/simple`；如需切回官方源，可使用 `-PythonPackageIndexUrl https://pypi.org/simple`。
- 两个档位都保持 Win7 友好，优先使用二进制 wheel，避免现场编译依赖。
