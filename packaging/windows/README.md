# Windows 7 补充包打包说明

本目录用于构建给 `wunder-desktop` Windows 7 试包配套的 `wunder补充包`，提供：

- `opt/python`：给智能体、本地脚本与 Python 工具调用使用的内置 Python
- `opt/git`：给 `git clone`、`git status`、补丁应用与仓库操作使用的内置 Git

补充包设计目标：**直接解压到桌面安装目录即可生效**。

当前约定：

- Win7 `setup.exe` 安装包默认不再内置 Python 与 Git。
- 如需 Python/Git，请单独分发并解压 `wunder补充包-win7-*.zip` 到安装目录根部。
- Electron 运行时会自动识别安装目录中的 `opt/python` 与 `opt/git`。

## 版本选择

- Python：`3.8.10 embeddable package`
  - Python 官方从 `3.9` 开始要求 Windows `8.1+`，因此 Win7 路线固定使用 `3.8`。
  - 当前脚本使用官方可下载的 embeddable 压缩包，便于直接解压进安装目录。
- Git：`MinGit 2.46.2`
  - Git for Windows FAQ 将 `2.46.2` 标为最后支持 Windows `7 / 8 / 8.1` 的版本。
  - `MinGit` 适合嵌入第三方桌面应用，体积比完整安装包更小。

## 入口命令

在仓库根目录执行：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 -Arch x64
```

如果希望补充包内直接包含一组 Win7 友好的常用 Python 依赖（含绘图）：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 -Arch x64 -PythonProfile common
```

如需重新下载官方压缩包：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 -Arch x64 -RefreshDownloads
```

如果外网波动导致官方包下载不稳定，也可以先手工准备两个压缩包，再直接喂给脚本：

```powershell
powershell -ExecutionPolicy Bypass -File packaging/windows/scripts/build_win7_desktop_supplement.ps1 `
  -Arch x64 `
  -PythonArchivePath C:\cache\python-3.8.10-embed-amd64.zip `
  -GitArchivePath C:\cache\MinGit-2.46.2-64-bit.zip
```

## 默认输出目录

- 下载缓存：`temp_dir/win7-gnu-lab/win7-supplement/downloads/`
- 展开目录：`temp_dir/win7-gnu-lab/win7-supplement/stage/package-root/`
- 最终压缩包：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-x64.zip`
- `common` 档位压缩包：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-x64-common.zip`

压缩包内部目录结构是：

```text
opt/
  python/
  git/
README-win7-supplement.txt
wunder-win7-supplement.json
```

## 使用方式

1. 关闭已运行的 Wunder Desktop。
2. 将 `wunder补充包-win7-x64.zip` 解压到桌面安装目录根部。
3. 解压后确认安装目录下出现：
   - `opt/python`
   - `opt/git`
4. 重新启动桌面端。

Electron Win7 包启动时会自动：

- 把安装目录写入 `WUNDER_DESKTOP_APP_DIR`
- 将 `opt/python`、`opt/python/Scripts`、`opt/git/cmd`、`opt/git/bin` 追加到 `PATH` 前部

这样桥接层与智能体工具调用就能优先命中内置 Python / Git。

## 兼容性提示

- Python 3.8 embeddable package 在 Win7 上建议配合 `KB2533623` 与 Universal CRT 更新使用。
- 默认 `minimal` 档位提供的是 **基础 Python + 基础 Git**，尽量控制体积。
- `common` 档位会额外内置 `pip / setuptools / wheel`，并预装一组精简常用依赖：`requests`、`numpy`、`pandas`、`openpyxl`、`matplotlib`、`Pillow`、`tabulate`。
- 两个档位都保持 Win7 友好，优先使用二进制 wheel，避免现场编译依赖。
