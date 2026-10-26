# 轻量路线 · 渲染与交付

渲染器：`{{SKILL_ROOT}}/scripts/render_lite.py`。只依赖补充包三件套：Python 3.8（stdlib 即可）、Chrome（headless，CDP 驱动）、FFmpeg。无 Node、无 pip 安装、离线可用。

## 基本用法

```bash
python {{SKILL_ROOT}}/scripts/render_lite.py <composition.html> -o <输出.mp4> \
  --chrome <chrome路径> --ffmpeg <ffmpeg路径>
```

- 画布/fps/时长默认读页面里的 `window.__video`，命令行 `--width --height --fps --duration` 可覆盖。
- 输出目录不存在会自动创建；成功后 stdout 打印输出绝对路径，进度打在 stderr。

## 环境路径

- 蛹/舵机补充包（导入到 WUNDER_HOME 后）：
  - Windows：`opt/browser/chrome-win/chrome.exe`、`opt/ffmpeg/bin/ffmpeg.exe`、`opt/python/python.exe`
  - Linux amd64：`opt/browser/chrome-linux64/chrome`；arm64：`opt/browser/chrome-linux/chrome`
- 也可用环境变量 `WUNDER_CHROME_BIN` / `WUNDER_FFMPEG_BIN` 代替 `--chrome/--ffmpeg`。
- 目标机 Chrome 版本 >= 90 即可（CDP `Page.captureScreenshot`、`Emulation.setDeviceMetricsOverride`）。

## 常用参数

| 参数 | 作用 |
|---|---|
| `--duration S` | 覆盖页面时长（秒） |
| `--fps N` | 覆盖帧率，默认 30 |
| `--scale 2` | 2 倍超采样截图，文字更锐（渲染时间近似翻倍） |
| `--crf N` | 画质，默认 18（越小越清晰，文件越大） |
| `--audio 旁白.mp3` | 渲染完混入音轨（`-shortest` 截齐） |

> 透明视频（alpha 通道）是 Remotion 路线的能力，见 `render-delivery.md`；轻量路线的 headless 截图链路不保留 alpha。

## 工作流建议

1. 先低清小样：`--width 640 --height 360 --fps 15` 快速出片看节奏；
2. 确认后全尺寸渲染；长视频（>60s）先渲染前 5 秒（把 `--duration` 改小）抽查；
3. 音画分开：画面渲染与 `--audio` 混音是两步，替换旁白不用重渲画面。

## 故障排查

| 现象 | 原因与处理 |
|---|---|
| `page does not expose window.seekTo` | 页面没实现契约，按 `references/lite-authoring.md` 补 `seekTo` |
| 首帧黑/字体闪烁后正常 | 字体或图片未就绪；在 `seekTo` 里返回 `document.fonts.ready` / `img.decode()` 的 Promise |
| 动画一顿一顿、时机错乱 | 页面用了墙钟动画（setTimeout/CSS transition），改成 t 的纯函数 |
| 截图尺寸不对 | HTML 里 `html,body` 尺寸必须等于 `__video` 声明的尺寸 |
| `chrome devtools endpoint did not come up` | chrome 路径不对或目标机缺依赖；先在目标机手动 `chrome.exe --version` 验证 |
| ffmpeg 报 `pipe: Invalid data` | 页面 JS 抛异常导致截图失败，渲染器会报出页面异常文本，按提示修页面 |
| win7 ia32 上渲染很慢 | 正常：逐帧截图约 2–8 帧/秒；降分辨率/帧率出小样，终片交给夜间或高性能机器 |

## 性能预期

1080p30 约 2–8 帧/秒（取决于画面复杂度与机器），即 1 分钟成片约需 4–15 分钟渲染。720p 预览快约 3 倍。渲染是 CPU 密集 + 单线程逐帧，长时间渲染时注意补充包机器的散热与电源策略。
