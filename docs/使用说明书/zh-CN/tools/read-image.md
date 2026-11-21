---
title: 读图工具
summary: `read_image` 的路径、取帧（帧号/间隔/联系表）与对比参数，以及静态图、GIF、视频三类素材的进上下文方式与返回字段。
updated_at: 2026-11-20
---

# 读图工具

读图工具按路径把本地素材送进当前对话：截图、照片、设计稿，以及 GIF 和本地视频。

## 参数

参数是文件路径：

```json
{
  "path": "screenshots/demo.png"
}
```

GIF 可以按帧号取帧，帧号从 0 开始，与联系表每格标注的数字一致：

```json
{
  "path": "captures/pelican.gif",
  "frame": 12
}
```

一次要看几帧就传数组，最多 8 帧：

```json
{
  "path": "captures/pelican.gif",
  "frames": [0, 6, 12]
}
```

按间隔抽帧仍然可用，视频则指定每秒取几帧：

```json
{
  "path": "captures/demo.gif",
  "frame_step": 2
}
```

```json
{
  "path": "captures/demo.mp4",
  "frame_rate": 1
}
```

只想先看一眼整段动作，就用 `sheet`：GIF 和视频都适用，一次回一张最多 4 列、16 格的联系表。长的动图可以用 `frame_start` 翻页，从后半段开始抽。

```json
{
  "path": "captures/demo.mp4",
  "sheet": true
}
```

## 素材处理

- 静态图片原样作为图片输入进上下文；调用时给路径，看的意图由当前对话补上。
- GIF 不会整段丢给模型：不带取帧参数时返回一张带帧号的联系表（不再只给首帧），给了 `frame` / `frames` 就取指定帧，给了 `frame_step` 就按间隔抽帧。
- 视频会被整理成一串图片帧；如果视频里带音轨，还会附上转写出来的文字。
- `sheet: true` 时不再逐帧回图，而是把整段均匀抽成一张联系表；视频会先把抽帧收敛到 16 帧以内，格子按行列排布，画面等比缩放、不拉伸。
- 联系表的格子宽高比跟随真实帧比例（16:9 仍是 16:9），不会为了凑格子把画面裁掉一截；每格左下角写着它的 0-based 帧号。
- 一张联系表只用一个缩放系数：采样帧尺寸不一致时（视频或个别动图），小尺寸帧不会被单独放大去撑满格子，格子里画面大小不同只反映源帧本身的大小。`sheet.frame_sizes` 给出每格的源帧尺寸，`sheet.mixed_frame_sizes` 为 `true` 时 `warnings` 也会写明。
- `frame: N` 单独使用时返回第 N 帧的 PNG（原分辨率、不经过联系表），结果里的 `frame_scope` 写明帧号、时间戳与总帧数；再叠加 `crop` / `scale` / `stats` 才进入单帧细看流程。

## 对比两张图

要看"这一版比上一版好在哪"，把另一张图放进 `compare_with`：

```json
{
  "path": "renders/v2.png",
  "compare_with": "renders/v1.png"
}
```

默认返回一张左右并排图。加 `compare_mode: "diff"` 则返回三联图（左图、红色差异热力图、右图），并给出 SSIM、平均绝对差与被改动像素占比：

```json
{
  "path": "renders/v2.png",
  "compare_with": "renders/v1.png",
  "compare_mode": "diff"
}
```

GIF 之间也能比：主图用 `frame` 取帧，对照图是动图时用 `compare_frame` 取帧。

## 返回值

返回里会说明素材的类型和大小，但那只表示素材已经准备好，不是模型的分析结论。

联系表会额外给出 `sheet.columns`、`sheet.rows`、`sheet.cell_width`、`sheet.cell_height`、`sheet.frame_indices` 和 `sheet.frame_sizes`：`frame_indices` 按行列顺序列出每格对应的源帧序号，`frame_sizes` 是对齐的源帧像素尺寸，视频的 `warnings` 里还写明第 k 格大约对应第几秒。

按帧号取帧时会给出 `frame_scope`：总帧数、总时长，以及每帧的帧号、时间戳与落盘路径；只取一帧时 `mode` 为 `frame`，附件就是这一帧的 PNG。差异对比会给出 `compare.metrics`（`ssim`、`mean_abs_diff`、`changed_ratio`）。

## 错误与处理

返回里可以核对路径是否解析到真实文件、素材类型是否正确、字节大小是否符合预期。路径写错或素材类型不支持，会直接报错。

裁切（`crop` / `crop_xyxy`）落在图片边界外时，超出的部分会自动收敛到边界并在结果里注明实际区域；只有裁切起点完全在图片之外才报错。
