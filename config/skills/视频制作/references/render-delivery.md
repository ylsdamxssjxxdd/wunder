# 渲染与交付

只在用户明确要求导出视频、静帧或透明视频时读取本文件。

## 先确认渲染对象

在项目目录列出或确认 composition id，再渲染。基本命令：

```sh
npx remotion render <composition-id> <output-path>
```

静帧使用：

```sh
npx remotion still <composition-id> <output-path>
```

抽取少量验收帧优先于完整成片：

```sh
npx remotion render <composition-id> <frames-dir> --frames=0,90,180 --image-format=png
```

帧号必须根据实际 `fps`、总帧数和场景边界计算；不要对短视频请求不存在的帧。

## 输出选择

- 默认交付通常是 H.264 MP4；只在用户要求浏览器透明、编辑软件透明或给定平台规范时更换格式。
- 浏览器透明视频使用 VP9 WebM：

```sh
npx remotion render <composition-id> <output-path>.webm --image-format=png --pixel-format=yuva420p --codec=vp9
```

- 后期软件透明素材使用 ProRes 4444：

```sh
npx remotion render <composition-id> <output-path>.mov --image-format=png --pixel-format=yuva444p10le --codec=prores --prores-profile=4444
```

- 没有透明通道需求时，不使用 PNG 序列或高位深 ProRes，以免无必要地放大时间和文件体积。

## 完成后检查

1. 命令成功结束，输出文件存在且非零。
2. 用项目已有媒体探测工具，或 `ffprobe`（如可用）核验时长、宽高、fps、codec 与约定一致。
3. 检查首、中、末关键帧；若渲染包含音频，检查音轨存在和时长合理。必要时回到 Studio 确认同步。
4. 输出路径要在用户工作区；同时交付源项目目录和 composition id，便于再次渲染。

不要用文件扩展名判断成功，不把临时帧目录作为最终交付，也不要在渲染失败后声称已生成视频。

