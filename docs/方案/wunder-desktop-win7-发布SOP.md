# wunder-desktop Win7 发布 SOP

## 1. 目标

- Win7 版本默认统一走 **Electron 22 + x64 GNU bridge + common 补充运行时**。
- 默认最终交付物统一为 **NSIS 一体安装包**，不再优先单独分发补充包 zip。

## 2. 默认发布命令

在仓库根目录执行：

```powershell
npm run setup:desktop:win7:gnu:x64
npm run build:desktop:win7:gnu:x64:with-supplement:common
```

如果工具链已经初始化过，日常重建直接执行：

```powershell
npm run build:desktop:win7:gnu:x64:with-supplement:common
```

## 3. 产物

- 最终安装包：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder-desktop-win7-0.1.0-x64-setup.exe`
- 中间补充包：`temp_dir/win7-gnu-lab/win7-supplement/dist/wunder补充包-win7-x64-common.zip`

默认对外分发时，以安装包为准；补充包只保留给调试、手工覆盖和应急兜底使用。

## 4. 安装包内置内容

- Win7 GNU bridge
- `opt/python`
- `opt/git`
- `pip / setuptools / wheel`
- `requests / numpy / pandas / openpyxl / matplotlib / Pillow / tabulate`

## 5. 发布前最小验证

安装后至少验证以下项目：

```powershell
python --version
py --version
where python
where py
```

期望结果：

- `python` / `py` 命中安装目录下的 `opt/python`
- Python 版本为 `3.8.10`
- 应用可正常启动
- 智能体可调用 Git 与 Python

如需额外验证绘图能力，可执行：

```powershell
python -c "import matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt; plt.plot([1,2,3],[1,4,9]); plt.savefig('probe.png')"
```

## 6. 例外情况

- 若只是排查 Python/Git 运行时问题，可单独使用 `wunder补充包-win7-x64-common.zip`
- 若只是排查 bridge 或构建链问题，可参考 `docs/方案/wunder-desktop-win7-gnu实验方案.md`
- `ia32` / Tauri / WebView2 路线当前都不作为默认 Win7 发布方案

## 7. 当前约定

- 后续 Win7 版本构建优先使用本 SOP
- 后续 Win7 版本发布优先使用一体安装包
- 除非是专门做兼容性排障，否则不再新增平行的 Win7 构建方案文档
