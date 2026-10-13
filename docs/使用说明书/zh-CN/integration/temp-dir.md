---
title: 临时目录与文档转换
summary: 临时上传、下载和中转都走临时目录；文档、音频、视频在进入会话前先做一次转换或媒体处理。
---

# 临时目录与文档转换

`temp_dir` 是中转层，不是正式工作区。别把它当长期存储。

## 临时目录与工作区的分工

- 只是临时上传一个文件给系统处理 → 临时目录。
- 要给外部发一个能点开的下载链接 → 临时目录。
- 要把 doc / pdf / ppt / xlsx 之类转成 Markdown → 先走文档转换。
- 要长期沉淀的产物 → 放工作区，别放临时目录。

## 最短链路：常用接口

- 文档转 Markdown：`POST /wunder/doc2md/convert`
- 转换（需鉴权）：`POST /wunder/attachments/convert`
- 会话里的文档附件转换：`POST /wunder/chat/attachments/convert`
- 会话里的音频 / 视频处理：`POST /wunder/chat/attachments/media/process`
- 下载：`GET /wunder/temp_dir/download`
- 上传：`POST /wunder/temp_dir/upload`
- 列表：`GET /wunder/temp_dir/list`
- 删除：`POST /wunder/temp_dir/remove`

## 四种转换怎么分

| 接口 | 用在哪 | 出来的东西 |
| --- | --- | --- |
| `POST /wunder/doc2md/convert` | 公共文档转换 | Markdown 内容 |
| `POST /wunder/attachments/convert` | 调试面板、鉴权联调 | 和上面一样，但要鉴权 |
| `POST /wunder/chat/attachments/convert` | 聊天输入区的文档附件 | 供会话装配的文本附件 |
| `POST /wunder/chat/attachments/media/process` | 聊天输入区的音频 / 视频 | 音频转写结果，或视频拆出的图片帧加音轨 |

两点补充：图片一般不用走转换，直接当会话附件发即可；视频不会直接发给模型，会先拆成图片序列和音轨，重新抽帧依赖原始文件的公共路径。

## 为什么拿到的是下载链接

很多外部客户端看不懂 wunder 内部的工作区路径。所以发出去的内容里，内部路径会被改写成可点击的临时下载链接，外部才打得开。聊天媒体处理时，源文件通常先落在工作区公共路径，真正发给外部下载时仍可能再换成临时下载链接。

## 关键设置与常见坑

- 临时目录是给中转和分发的，不适合长期放业务文件。
- 转换前先分清是公共能力、调试面板还是聊天输入区。
- 音频 / 视频走媒体处理，别把原始视频直接当模型输入。
- 外部渠道能点开文件，靠的就是这个下载链接。

## 延伸阅读

- [工作区 API](/docs/zh-CN/integration/workspace-api/)
- [渠道 Webhook](/docs/zh-CN/integration/channel-webhook/)
- [数据与存储](/docs/zh-CN/ops/data-and-storage/)
