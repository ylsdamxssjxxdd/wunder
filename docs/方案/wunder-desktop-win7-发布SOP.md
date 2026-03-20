# wunder-desktop Win7 发布 SOP

## 1. 目标

- Win7 版本默认统一走 **Electron 22 + x64 GNU bridge + common 补充运行时**。
- 默认最终交付物为 **NSIS 安装包 + wunder补充包 zip** 两个文件；安装包本体不再内置 Python/Git。

## 2. 默认发布命令

在仓库根目录执行：

```powershell
npm run setup:desktop:win7:gnu:x64
npm run build:desktop:win7:gnu:x64
```

如果工具链已经初始化过，日常重建直接执行：

```powershell
npm run build:desktop:win7:gnu:x64:fast
```

- 上述默认入口会产出 `setup.exe + wunder补充包-win7-x64-common.zip`。
- Win7 `common` 补充包默认通过清华 Tuna 简单索引拉取 `packaging/python/requirements-win7-common.txt` 中的依赖；如需切回官方源，可显式传 `-SupplementPythonPackageIndexUrl https://pypi.org/simple`。

## 3. 产物

- 最终安装包：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder-desktop-win7-0.1.0-x64-setup.exe`
- 同目录补充包：`temp_dir/win7-gnu-lab/electron-win7-x64/dist/wunder补充包-win7-x64-common.zip`

### CI / 自动发布

- Nightly 自动发布链路已接入 Win7 兼容版构建。
- 自动发布产物名统一为：`Wunder-Desktop-win7-compat-x64-<date>-setup.exe`。
- 该资产在自动发布说明中会明确标注为 **Windows 7 兼容性安装包**，并说明其默认不内置 `Python + Git`。
- 若补充包也一并上传，则命名为：`Wunder-Desktop-win7-compat-x64-<date>-supplement-common.zip`，用于用户按需手工解压到安装目录。

默认对外分发时，安装包与补充包建议一起提供；补充包按需使用，不再嵌入安装包。

## 4. 安装包内置内容

- Win7 GNU bridge
- 前端静态资源与桌面壳
- `README-win7-supplement.txt`（提示补充包解压方式）

## 5. 发布前最小验证

安装后至少验证以下项目：

```powershell
where python
where git
```

期望结果：

- 应用可正常启动
- 安装目录根部存在 `README-win7-supplement.txt`
- 若已解压补充包，则安装目录下存在 `opt/python` 与 `opt/git`
- 若已解压补充包，则 `python` / `git` 命中安装目录下对应路径

如需额外验证绘图能力，可执行：

```powershell
python -c "import matplotlib; matplotlib.use('Agg'); import matplotlib.pyplot as plt; plt.plot([1,2,3],[1,4,9]); plt.savefig('probe.png')"
```

## 6. 例外情况

- 若只是排查 Python/Git 运行时问题，可单独使用 `wunder补充包-win7-x64-common.zip` 并解压到安装目录
- 若只是排查 bridge 或构建链问题，可参考 `docs/方案/wunder-desktop-win7-gnu实验方案.md`
- `ia32` / Tauri / WebView2 路线当前都不作为默认 Win7 发布方案

## 7. 当前约定

- 后续 Win7 版本构建优先使用本 SOP
- 后续 Win7 版本发布优先使用“安装包 + 补充包”分离交付
- 除非是专门做兼容性排障，否则不再新增平行的 Win7 构建方案文档
