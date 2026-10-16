# 容器字体

舰体的 Docker 镜像已安装 `fonts-noto-cjk` 等字体包。Compose 还会把本目录的
`.otf`、`.ttf`、`.ttc` 文件只读挂载到 `wunder-server`、`wunder-sandbox` 和
`extra-mcp` 的 `/usr/share/fonts/wunder`，并为 OnlyOffice 挂载自定义字体目录。

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
