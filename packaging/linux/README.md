# Linux 补充包打包说明

本目录用于构建 `wunder-desktop` Linux 版配套的补充包（压缩包名 `wunder-supplement-…`），提供：

- `opt/python`：给智能体、本地脚本与 Python 工具调用使用的内置 Python
- `opt/git`：给 `git clone`、`git status`、补丁应用与仓库操作使用的内置 Git
- `opt/rg`：给 `search_content` 与命令行检索使用的内置 ripgrep

补充包设计目标与 Win7 版一致：**解压到桌面程序同级目录即可生效**。

- 桌面安装目录（Win7 风格布局）：解压到可执行文件所在目录。
- AppImage：解压到 `.AppImage` 文件所在的目录；运行时会额外探测 `$APPIMAGE`
  同级目录中的 `opt/`，AppImage 自身的只读挂载点无需写入。

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

## Python 依赖

默认安装 `packaging/python/requirements-linux-common.txt`，与
`requirements-win7-common.txt` 保持同一套版本锁定，唯一例外是 `h5py`
（`2.10.0` 没有 aarch64 wheel，Linux 侧使用 `3.10.0`）。全部走二进制 wheel
（`--only-binary=:all:`），已逐一核对 cp38 + amd64/aarch64 + glibc 2.17 兼容。

## 入口命令

补充包构建必须在 Ubuntu 18.04 容器内执行（保证 glibc 基线）。amd64 直接构建，
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
sidecar，Git 源码包哈希固定在 manifest 中），下载缓存在构建根目录的
`downloads/` 下，重复执行直接复用。

## 默认输出

- 压缩包：`temp_dir/linux-supplement/dist/wunder-supplement-linux-{amd64,arm64}.tar.gz`

压缩包内部目录结构：

```text
opt/
  python/
  git/
  rg/
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
