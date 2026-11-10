# 容器字体

舰体的 Docker 镜像已安装 `fonts-noto-cjk` 等字体包。Compose 还会把本目录的
`.otf`、`.ttf`、`.ttc` 文件只读挂载到 `wunder-server`、`wunder-sandbox` 和
`extra-mcp` 的 `/usr/share/fonts/wunder`，并为 OnlyOffice 挂载自定义字体目录。
`config/fonts.conf` 把 `/usr/share/fonts/wunder/conf.d` 一并包含进来，字体名映射
（如 `仿宋_GB2312` → `FangSong`）因此在云端容器内生效。

字体本体不进 git。部署云端或本地开发时运行：

```bash
python scripts/fetch_fonts.py                 # 通用字体 + 视频字体
python scripts/fetch_fonts.py --set common    # 只下载公文/办公通用字体
python scripts/fetch_fonts.py --check         # 只校验已有文件
```

脚本按 `packaging/fonts/*.json` 里固定的 URL 与 SHA-256 下载，已存在且校验通过的
文件会直接复用。

- `common/`：公文/办公常用字体。仿宋（`simfang.ttf`）、楷体（`simkai.ttf`）、
  黑体（`simhei.ttf`）、宋体/新宋体（`simsun.ttc`）、方正小标宋
  （`FZXBSJW.ttf`）、微软雅黑（`msyh.ttc`）、等线（`Deng.ttf`）与
  Times New Roman 四个字重。字体名映射在 `conf.d/99-wunder-common.conf`。
- `video/`：视频制作技能用的 8 个 OFL 字体。云端从
  `/usr/share/fonts/wunder/video` 读取；补充包构建时用
  `packaging/fonts/fetch_video_fonts.py` 从 `config/fonts/video` 复制（没有时按
  `packaging/fonts/video-fonts.json` 的固定 URL 和 SHA-256 下载），再打进补充包。

`common/` 里是从 Windows/方正字体文件直接取用的测试字体，发布或对外分发前应换成
已获授权的副本；补充包构建时如果 `config/fonts/common` 不存在，也会按同一清单下载。

Matplotlib 的中文默认字体由 `config/matplotlibrc` 配置，舰体与沙盒通过
`MATPLOTLIBRC` 加载。普通绘图沿用默认字体即可，无需为每幅图重新探测字体或指定路径。
只有任务明确要求某个字族时，才先在执行绘图的同一 Python 解释器中验证：

```python
from matplotlib import font_manager

font_path = font_manager.findfont(
    font_manager.FontProperties(family=["SimHei"]),
    fallback_to_default=False,
)
```

`fc-list` 与 Matplotlib 使用不同的字体发现机制。Fontconfig 可列出 TTC 集合中的
多个字族，当前 Matplotlib 可能只登记首个字族。例如 `NotoSansCJK-Regular.ttc`
可被 Fontconfig 识别为 `Noto Sans CJK SC`，而 Matplotlib 只登记
`Noto Sans CJK JP`；清理缓存不会改变这一限制。应选择 Matplotlib 已登记且覆盖目标字符的
字族，或使用所需字族的单独 TTF/OTF 文件。

`findfont: Font family ... not found` 表示某个候选字族不可用；只要实际回退字体
覆盖目标字符，仍可正常生成中文图片。结合 `Glyph ... missing` 警告、实际字符覆盖与
产物检查判断是否需要修复，避免把候选字体警告误判为绘图失败。

Matplotlib 字体索引持久化在 `MPLCONFIGDIR` 中，容器重启不会自动清空。
新增字体后，仅当新 Python 进程仍无法识别新文件时，才清理该目录内的
`fontlist-v*.json` 并重新启动绘图进程；不要删除整个缓存目录或每次任务强制重扫字体。
Fontconfig 缓存与 Matplotlib 索引独立，`fc-cache` 不能代替 Matplotlib 的验证。
