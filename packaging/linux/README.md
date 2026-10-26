# Linux 补充包打包说明

本目录用于构建 `wunder-desktop` Linux 版配套的补充包（压缩包名 `wunder-supplement-…`），提供：

- `opt/python`：给智能体、本地脚本与 Python 工具调用使用的内置 Python
- `opt/git`：给 `git clone`、`git status`、补丁应用与仓库操作使用的内置 Git
- `opt/rg`：给 `search_content` 与命令行检索使用的内置 ripgrep
- `opt/ffmpeg`：给音视频抽帧、抽音轨与转码使用的内置静态 ffmpeg
- `opt/browser`：Linux amd64 内置 Google Chrome for Testing 122.0.6261.111；Linux arm64 内置 Playwright Chromium 120.0.6099.28，供 `browser_*` 工具离线使用

补充包设计目标与 Win7 版一致：**统一安装到 `WUNDER_HOME/opt`，蛹与舵机共用一份运行环境**。

- 推荐在蛹设置中选择“导入补充包”，默认写入 `~/.wunder/opt/`，导入后蛹新启动的工具立即生效。
- 手动安装时，关闭使用该补充包的进程，将压缩包解压到 `${WUNDER_HOME:-$HOME/.wunder}`，使其下直接出现 `opt/python`、`opt/git`、`opt/rg`、`opt/ffmpeg`，再启动蛹或舵机。
- AppImage 同样使用这个可写用户目录；安装目录与 AppImage 同级旧 `opt/` 仅保留读取回退，新导入始终写入共享 home，不自动搬迁旧文件。

## 安装器打包

先构建蛹（AppImage）和舵机（ELF），再准备补充包 `.tar.gz`，然后执行：

```bash
bash build.sh pack -a linux-amd64
bash build.sh pack -a linux-arm64
```

Windows 上也可以执行 `build.bat pack -a linux-amd64`；组包器是 Python 3.6+ 脚本，不需要 Docker、Rust 工具链或网络。Windows 上优先复用离线 SDK 的 `Rust-builder/win7/offline/tools/python/python.exe`（Python 3.8.10 embeddable），宿主未安装 Python 也能离线组包；Linux 宿主使用系统 `python3`。输入默认从 `target/slint/dist/linux-<arch>`、`target/cli/dist/linux-<arch>`、`dist/linux-<arch>`、`target/installer/payload/linux-<arch>` 和 `temp_dir/linux-supplement/dist` 查找，可用 `--desktop`、`--cli`、`--supplement` 覆盖。产物是单文件 `target/installer/dist/linux-<arch>/wunder-<版本>-linux-<arch>.run`，同时生成 `.sha256` 与 `.manifest.json`。

运行 `.run` 后：

- 默认安装到 `~/.wunder`，安装前的交互提示可以直接输入其他绝对路径；非交互场景使用 `--home=PATH`。
- 补充组件默认全部不勾选；`--components=python,git` 或 `--all` 可指定，`--yes` 跳过确认。
- 安装根会写入 `.wunder-home`。`bin/wunder-desktop` 启动器会导出 `WUNDER_HOME=<安装根>`；直接运行 AppImage 或 `bin/wunder-cli` 时，运行时也会通过标记文件识别安装根。显式设置的非空 `WUNDER_HOME` 优先。`bin/wunder` 是 `wunder-cli` 的兼容符号链接。
- 安装器会在 `~/.local/bin` 创建 `wunder-cli`、`wunder-desktop` 和 `desktop` 三个命令链接；若 `~/.profile` 未包含该目录，会追加一段带标记的 `PATH` 设置。新开终端即可直接运行，舵机以启动目录作为工作区（`-C` 可覆盖）。
- 默认注册名称为“蛹”的应用菜单项，并在 XDG 桌面目录（系统语言为中文时通常是 `~/桌面`，英文为 `~/Desktop`）创建可执行的 `wunder.desktop` 启动图标；`--no-desktop-entry` 和 `--no-desktop-icon` 可分别跳过。重装会先清理上一次安装的入口，卸载时一并移除。
- 配置、会话、工作区、日志和 `opt/` 都在安装根下。选中已存在的组件会先清空对应 `opt/<component>`，用户数据不删除。
- 卸载执行 `<安装根>/uninstall.sh`，保留用户数据；它只移除指向本安装根的命令链接、安装器追加的 `PATH` 标记块、应用菜单项、桌面图标、`bin` 和本次选择的组件。

补充包在组包时会从旧归档的 `./opt/...` 规范化为 `opt/...`，保留权限、相对符号链接和同组件内的硬链接；新构建的补充包也已经直接使用 `opt/...`。

## 版本选择

- Python：`3.8.18`（python-build-standalone `20240107` install_only）
  - 与 Win7 补充包同为 Python 3.8，保证智能体生成的 Python 代码在三个平台行为一致。
  - gnu 构建基线为 glibc 2.17，低于 Ubuntu 18.04（glibc 2.27）的发布基线。
  - `20240107` 是 python-build-standalone 最后一个带 CPython 3.8 的版本。
- Git：`2.46.2` 源码编译
  - 与 Win7 版 MinGit 同版本。在 Ubuntu 18.04 容器内编译，
    `NO_GETTEXT/NO_TCLTK/NO_PERL/NO_PYTHON` 精简内嵌树，保留 https clone 能力。
- ripgrep：`14.1.1`
  - amd64 使用静态 musl 构建；arm64 官方未发布 musl 资产，使用 gnu 构建。
- ffmpeg：`4.4.1`（eugeneware/ffmpeg-static `b4.4.1`）
  - 与 Win7 补充包同源同版本，保证三端音视频处理行为一致（对齐 Windows）。
   - 上游为自包含静态单文件构建，Ubuntu 18.04（glibc 2.27）与 arm64 包可直接运行；
     当前仅内置 `ffmpeg`（不含 `ffprobe`），与 Win7 包保持一致。

## Python 依赖

默认安装 `packaging/python/requirements-linux-common.txt`，与
`requirements-win7-common.txt` 保持同一套版本锁定，唯一例外是 `h5py`
（`2.10.0` 没有 aarch64 wheel，Linux 侧使用 `3.10.0`）。全部走二进制 wheel
（`--only-binary=:all:`），已逐一核对 cp38 + amd64/aarch64 + glibc 2.17 兼容。

## 入口命令

基础组件在 Ubuntu 18.04 容器内构建。amd64 直接构建，
arm64 使用 `--platform linux/arm64`（本地需 binfmt/QEMU，CI 由
`docker/setup-qemu-action` 提供）：

```bash
docker build -f packaging/docker/Dockerfile.ubuntu18-supplement \
  -t wunder-supplement-ubuntu1804-amd64 .

docker run --rm --platform linux/amd64 \
  -v "$(pwd):/app" -w /app \
  -e ARCH=amd64 \
  -e WUNDER_OUTPUT_DIR=/app/temp_dir/linux-supplement/dist \
  wunder-supplement-ubuntu1804-amd64 \
  bash packaging/linux/scripts/build_linux_supplement.sh
```

可用的环境变量：

- `ARCH`：`x86_64|amd64|aarch64|arm64`（默认取宿主机架构）
- `WUNDER_OUTPUT_DIR`：产物输出目录
- `WUNDER_SUPPLEMENT_BUILD_ROOT`：构建根目录（默认 `temp_dir/linux-supplement`）
- `WUNDER_SUPPLEMENT_PYPI_INDEX`：pip 索引（默认清华 Tuna；CI 传官方源）
- `WUNDER_SUPPLEMENT_REQUIREMENTS`：requirements 覆盖

脚本自带 sha256 校验（python-build-standalone 与 ripgrep 取官方 `.sha256`
sidecar，Git 源码包与 ffmpeg 单文件构建的哈希固定在 manifest 中；ffmpeg
上游未发布 `.sha256` sidecar），下载缓存在构建根目录的
`downloads/` 下，重复执行直接复用。

## 默认输出

- 压缩包：`temp_dir/linux-supplement/dist/wunder-supplement-linux-{amd64,arm64}.tar.gz`

压缩包内部目录结构：

```text
opt/
  python/
  git/
  rg/
  ffmpeg/
  browser/                 # amd64: chrome-linux64/chrome；arm64: chrome-linux/chrome
README-linux-supplement.txt
wunder-linux-supplement.json
```

## CI 发布

`.github/workflows/desktop-supplements.yml` 负责三个补充包（Win7 ia32、
Linux amd64、Linux arm64）的构建与发布：

- 缓存键由打包脚本、manifest、requirements、Dockerfile 与字体/matplotlibrc
  的哈希组成；输入不变时直接命中缓存，不再重新构建。
- 触发：`packaging/**` 变更的 push，或手动 `workflow_dispatch`
  （可勾选“忽略缓存强制重建”）。
- 发布：固定 release 标签 `supplement`，三个包原位覆盖上传，下载地址稳定。
