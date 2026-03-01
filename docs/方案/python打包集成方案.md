# Python 打包集成方案（AppImage 内置 Python，Ubuntu 20.04）

## 1. 目标与约束
- 目标：在 wunder-desktop 的 AppImage 内置完整 Python 运行时与常用三方库，保证在用户机器无 Python 的情况下，智能体功能可用。
- 平台：Ubuntu 20.04（glibc 2.31），需要确保二进制与动态库兼容。
- 依赖：运行时不再依赖系统可访问的外部依赖（无 apt 在线安装、无 pip 在线安装、无系统缺失库）。
- 交付：单一 AppImage，开箱即用。

## 2. 方案概览（推荐）
**推荐方案：AppImage + 内置 CPython + 离线 wheelhouse + venv**
- 在 Ubuntu 20.04 构建 Python 基座与离线依赖仓库（wheelhouse）。
- 将 Python 运行时与离线依赖打入 AppImage（默认安装到内置 Python 前缀，可选 venv）。
- AppRun 负责设置 PYTHONHOME / LD_LIBRARY_PATH / SSL_CERT_FILE 等环境，使智能体调用内置 Python。

## 3. 方案对比
| 方案 | 优点 | 风险 / 缺点 | 适配度 |
| --- | --- | --- | --- |
| **A. AppImage + 内置 CPython + wheelhouse（推荐）** | 完全离线、可控、兼容性高 | 构建流程复杂，需要维护依赖清单 | ⭐⭐⭐⭐⭐ |
| B. conda-pack 内置 Miniconda | 包管理方便 | 体积大、许可与依赖复杂 | ⭐⭐⭐ |
| C. PyInstaller 打包 Python + 业务脚本 | 交付简单 | 对大量依赖不友好、可维护性差 | ⭐⭐ |
| D. PyOxidizer / Nuitka | 性能好 | 构建复杂、对第三方库兼容性不稳 | ⭐⭐ |

## 4. 推荐方案详细设计

### 4.1 目录结构（AppDir）
```
AppDir/
  AppRun
  wunder.desktop
  wunder.png
  usr/
    bin/
      wunder-desktop
      python3  -> ../../opt/python/bin/python3
    lib/
      (放置必要的共享库，例如 libssl.so, libcrypto.so 等)
  opt/
    python/
      bin/python3
      lib/python3.11/...
      lib/libpython3.11.so
      .wunder-python-version
  share/
    wheelhouse/   (可选：保留离线轮子用于诊断)
```

### 4.2 Python 基座构建
**目标**：保证在 Ubuntu 20.04 可运行，并携带完整 stdlib 与必要动态库。

推荐两种方式：
1) **在 Ubuntu 20.04 容器/构建机上编译 CPython**（首选，控制力强）
- 使用官方 Python 源码编译，开启共享库：`--enable-shared`。
- 链接 OpenSSL / SQLite / libffi 等库，并在 AppDir 中一并打包。
- 编译完成后拷贝到 `AppDir/opt/python`。

2) **使用 python-build-standalone（若版本可用且合规）**
- 选择 manylinux2014 或 glibc 2.17 构建产物，以兼容 Ubuntu 20.04。
- 解压后置入 `AppDir/opt/python`。

> 关键要求：构建环境必须不高于 Ubuntu 20.04，避免引入高版本 glibc 依赖。

### 4.3 依赖库规划（“大部分常用库”）
建议按层分级，保证体积与稳定性：
- **基础层**（默认内置）
  - requests, httpx, urllib3, certifi, charset-normalizer, idna
  - pydantic, typing-extensions, python-dotenv
  - fastapi, starlette, uvicorn
  - jinja2, markupsafe
  - rich, loguru
- **数据处理层**（默认内置）
  - numpy, pandas, scipy
  - pyarrow（若体积可接受）
  - openpyxl, xlrd, xlsxwriter
- **文本/解析层**（默认内置）
  - beautifulsoup4, lxml
  - markdown, pygments
- **多媒体/图像层（可选包）**
  - pillow
  - opencv-python-headless（无 GUI 版本，依赖更少）
- **LLM/向量相关（可选包）**
  - tiktoken, sentence-transformers, faiss-cpu

**建议做法**：维护单一完整清单 `packaging/python/requirements-full.txt` 作为默认依赖，如需轻量版可自备清单并通过 `REQ_FILE` 指定。

### 4.4 离线 wheelhouse 生成
- 锁定依赖版本：维护 `packaging/python/requirements-full.txt`。
- 构建 wheelhouse（只用本地离线依赖）：
```
python -m pip download -r packaging/python/requirements-full.txt -d wheelhouse --only-binary=:all:
```
- 对于没有 wheel 的包：
  - 在 Ubuntu 20.04 容器内编译 wheel。
  - 使用 `auditwheel repair` 生成 manylinux 兼容 wheel。

### 4.5 依赖安装到内置 Python 前缀
```
python -m pip download -r packaging/python/requirements-full.txt -d wheelhouse --only-binary=:all:
PYTHONHOME=AppDir/opt/python LD_LIBRARY_PATH=AppDir/opt/python/lib \
  AppDir/opt/python/bin/python3 -m pip install --no-index --find-links wheelhouse -r packaging/python/requirements-full.txt
```
- 安装完成后清理缓存与测试文件，减少体积。

### 4.6 AppRun 脚本（关键）
AppRun 负责绑定运行环境并让智能体调用内置 Python：
```
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export APPDIR="$HERE"
PY_VER="$(cat "$APPDIR/opt/python/.wunder-python-version" 2>/dev/null || echo "3.11")"

export PYTHONHOME="$APPDIR/opt/python"
export PYTHONPATH="$APPDIR/opt/python/lib/python${PY_VER}/site-packages"
export LD_LIBRARY_PATH="$APPDIR/opt/python/lib:$APPDIR/usr/lib:${LD_LIBRARY_PATH}"
export SSL_CERT_FILE="$APPDIR/opt/python/lib/python${PY_VER}/site-packages/certifi/cacert.pem"
export PYTHONNOUSERSITE=1
export PIP_NO_INDEX=1

# 暴露给智能体调用的 python 入口
export WUNDER_PYTHON_BIN="$APPDIR/opt/python/bin/python3"

exec "$APPDIR/usr/bin/wunder-desktop" "$@"
```

### 4.7 wunder-desktop 调用方式
- 建议在桌面端/智能体配置中读取 `WUNDER_PYTHON_BIN` 变量。
- 如果未配置，则 fallback 到系统 python（兼容开发环境）。

### 4.8 依赖完整性检查
构建完成后必须执行：
- `ldd` / `lddtree` 检查 python 与关键扩展模块的动态库。
- 确保所有依赖库都在 AppDir 内（无系统缺失库）。
- `AppImage --appimage-extract` 验证内部结构。

### 4.9 体积与性能优化
- 删除 `tests/`、`__pycache__/`、`.dist-info/RECORD` 中无用条目。
- 对可执行与共享库执行 `strip`。
- 可选：为大库（如 opencv/pyarrow）拆分为可选 AppImage。

## 5. 交付清单
- `docs/python打包集成方案.md`（本文件）
- `packaging/python/requirements-full.txt`（完整依赖清单）
- `wheelhouse/`（离线轮子包）
- `docker-extra/scripts/build_embedded_python.sh`（内置 Python 构建脚本）
- `docker-extra/scripts/package_appimage_with_python.sh`（AppImage 注入脚本）
- `AppDir/` + `AppRun` + `wunder.desktop` + icon

## 6. 验收清单
- Ubuntu 20.04 无 Python 环境下可启动 AppImage。
- 智能体调用 `WUNDER_PYTHON_BIN` 成功执行并能加载常用库。
- 断网环境下功能可用，pip 不会访问网络。
- `ldd` 检查无 “not found”。

## 7. 里程碑 / 节点
1) **需求确认与依赖清单冻结**（锁定 requirements-full 依赖）
2) **Python 基座构建完成**（Ubuntu 20.04 编译或官方可用基座）
3) **离线 wheelhouse 产出**（可重复构建、无缺失 wheel）
4) **AppImage 集成完成**（AppRun + 内置 Python + 依赖库）
5) **联调与验收**（无 Python 环境、断网测试、依赖完整性检查）
6) **发布与回滚策略**（版本号、校验和、可回滚旧包）

---

**结论**：该方案可实现。关键在于严格控制构建环境（Ubuntu 20.04）与离线依赖供应链，并通过 AppRun 固化运行时环境，确保用户侧无 Python 也可稳定运行。





