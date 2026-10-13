---
title: 多模态生成
summary: `语音生成` / `声转文` / `图像生成` / `视频生成` 四个工具的前置模型、参数、产物落盘与失败码。
read_when:
  - 需要生成或识别语音、生成图片或视频
  - 工具在能力面上看不到，需要确认对应模型是否已配置
source_docs:
  - crates/wunder-runtime/src/services/tools/multimodal_generation_tool.rs
  - crates/wunder-runtime/src/services/tools/catalog.rs
  - crates/wunder-runtime/src/services/multimodal_models.rs
  - crates/wunder-runtime/src/services/llm.rs
  - crates/wunder-core/src/config.rs
updated_at: 2026-10-13
---

# 多模态生成

四个工具同族，名字常量集中在 `crates/wunder-runtime/src/services/tools/multimodal_generation_tool.rs:18-27`：

| 规范名 | 别名 | 其它别名 |
| --- | --- | --- |
| `语音生成` | `generate_speech` | — |
| `声转文` | `transcribe_speech` | — |
| `图像生成` | `generate_image` | `绘图生成`（旧规范名保留为别名） |
| `视频生成` | `generate_video` | — |

## 什么时候用

- `语音生成`：把文本念成音频文件（旁白、播报、无障碍回读）。
- `声转文`：把工作区里的音频文件转成文字（会议录音、语音消息）。
- `图像生成`：文生图，或给输入图做编辑/重绘。
- `视频生成`：文本生成短视频。

不要用来生成"临时装饰图"塞进答案：产物会落盘到工作区并占用存储。

## 前置条件：按模型类型自动开关

四件套**不是默认工具集成员**，是否出现在能力面取决于是否配了对应类型的模型（`catalog.rs:1362-1381`、`multimodal_generation_tool.rs:152-166`）：

| 工具 | 需要的模型类型（`model_type`） | 默认模型配置项 |
| --- | --- | --- |
| `语音生成` | `tts` | `llm.default_tts` |
| `声转文` | `asr`（别名 `stt`/`speech_to_text` 等归一） | `llm.default_asr` |
| `图像生成` | `image` | `llm.default_image` |
| `视频生成` | `video` | `llm.default_video` |

判定在 `crates/wunder-runtime/src/services/llm.rs:149-163`（`normalize_model_type`），可用清单与解析在 `crates/wunder-runtime/src/services/multimodal_models.rs:233-352`：指定 `model_name` 时只接受该名字且类型匹配的档案；未指定时取对应默认项；都解析不到即视为不可用。

## 产物落盘

三个生成类工具的产物写入当前工作区（`multimodal_generation_tool.rs:939-969`）：

- 传了 `path` → 按该相对路径写，缺扩展名时按响应 content-type 补。
- 没传 → `generated_media/<类型>_<UTC 时间戳>.<扩展名>`，类型前缀为 `speech` / `image` / `video`。
- 默认扩展名：语音 `wav`、图像 `png`、视频 `mp4`（由 content-type 推断，推断不出用默认）。
- 写完后标记工作区树脏，返回 `path`（对外呈现路径）与 `workspace_relative_path`（工作区相对路径）。

`声转文` 不落盘，只回文本。

## 参数

模型侧 schema 见 `catalog.rs:560-739`，字段定义见 `multimodal_generation_tool.rs:33-150`。

### 语音生成

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `text` | 是 | 待合成文本，去空白后不得为空 |
| `path` | 否 | 输出相对路径 |
| `model_name` | 否 | 指定 tts 档案 |
| `voice` | 否 | 音色 |
| `instructions` | 否 | 风格指令 |
| `response_format` | 否 | 响应格式 |
| `speed` | 否 | 语速 |
| `reference_path` | 否 | 工作区参考音频，用于音色克隆 |
| `ref_audio` | 否 | 直接给 `data:`/`http(s)` URL 或路径 |
| `ref_text` | 否 | 参考音频对应文本；**必须与 `reference_path` 或 `ref_audio` 同时给**，否则报错 |
| `model_specific_params` | 否 | 必须是 JSON 对象，其它类型报错 |

音色克隆会把参考音频读成 base64 数据 URL，空文件直接报错（`multimodal_generation_tool.rs:700-735`）。

### 声转文

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `path` 或 `source_public_path` | 二选一必填 | 指向工作区内音频，文件必须存在且是普通文件 |
| `model_name` | 否 | asr 档案 |
| `language` | 否 | 语种提示 |
| `prompt` | 否 | 转写提示 |
| `response_format` | 否 | 输出格式 |
| `temperature` | 否 | 采样温度 |
| `filename` | 否 | 上传文件名，缺省取源文件名，再缺省 `audio.wav` |
| `content_type` | 否 | 缺省 `application/octet-stream` |

返回 `data` 含 `text`、`content_type`、`source_public_path`、`raw_response`（`multimodal_generation_tool.rs:279-289`）。

### 图像生成

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `prompt` | 是 | 提示词，去空白后不得为空 |
| `path` | 否 | 输出相对路径 |
| `model_name` | 否 | image 档案 |
| `size` | 否 | 如 `1024x1024`；未给时取档案 `image_size`（`multimodal_generation_tool.rs:408-414`） |
| `output_format` | 否 | 输出格式 |
| `negative_prompt` | 否 | 负向提示 |
| `num_inference_steps` / `guidance_scale` / `seed` | 否 | 采样参数 |
| `input_path` / `input_paths` | 否 | 有输入图即切到编辑模式 |
| `mask_path` | 否 | 蒙版 |
| `reference_path` | 否 | 参考图 |
| `strength` / `true_cfg_scale` / `output_compression` / `layers` / `resolution` | 否 | 按 provider 透传 |

`mode` 由是否给了输入图决定：无输入图 `text_to_image`，有则 `image_edit`（`multimodal_generation_tool.rs:317-322`）。

**尺寸对齐**：provider 为 `vllm-omni` 时，宽与高都必须能被 16 整除，否则在调用前就返回失败并给出建议尺寸（`VLLM_OMNI_IMAGE_ALIGNMENT`，`multimodal_generation_tool.rs:31`、`416-441`）。

### 视频生成

| 字段 | 必填 | 说明 |
| --- | --- | --- |
| `prompt` | 是 | 提示词，去空白后不得为空 |
| `path` | 否 | 输出相对路径 |
| `model_name` | 否 | video 档案 |
| `size` | 否 | 分辨率 |
| `seconds` | 否 | 时长（秒） |
| `fps` / `num_frames` | 否 | 帧率 / 帧数 |
| `negative_prompt` | 否 | 负向提示 |
| `num_inference_steps` / `guidance_scale` / `guidance_scale_2` / `boundary_ratio` / `flow_shift` / `seed` | 否 | 采样参数，按 provider 透传 |
| `enable_frame_interpolation` | 否 | 补帧 |

视频生成以同步模式发起（`sync_mode = true`，`multimodal_generation_tool.rs:906`）。

## 返回

四者成功时都是统一骨架 `ok / action / state=completed / summary / data`（`multimodal_generation_tool.rs:214-229`、`279-289`、`387-405`、`919-930`）：

- `generate_speech` → `data.{text,content_type,path,workspace_relative_path,bytes,reference_path,voice_clone,ref_text,model_specific_params}`
- `transcribe_speech` → `data.{text,content_type,source_public_path,raw_response}`
- `generate_image` → `data.{prompt,content_type,path,workspace_relative_path,bytes,mode,model_name,provider,size,input_paths,mask_path,reference_path}`
- `generate_video` → `data.{prompt,content_type,path,workspace_relative_path,bytes}`

## 失败

- **`图像生成` 不抛错**：模型未配置、尺寸不合规、provider 报错都会转成结构化失败结果 `ok=false` + `error` + `data.error_meta`，并附带 `provider`/`model_name`/`mode`/`size`/`input_paths`/`failure_summary`/`error_detail_head`/`next_step_hint`/`suggested_size`（`multimodal_generation_tool.rs:454-495`）。

| code | retryable | 处置 |
| --- | --- | --- |
| `TOOL_TIMEOUT` | 是 | 缩小范围、降步数后重试 |
| `IMAGE_PROVIDER_BUSY` | 是 | 上游繁忙（429/503 类），稍后重试或降并发 |
| `IMAGE_SIZE_ALIGNMENT_INVALID` | 否 | 改用宽高都能被 16 整除的尺寸（结果里给了 `suggested_size`） |
| `IMAGE_MODEL_NOT_CONFIGURED` | 否 | 配默认图像模型，或显式传可用 `model_name` |
| `IMAGE_PROVIDER_RESPONSE_INVALID` | 否 | 上游响应格式非预期，查 `/v1/images/generations`、`/v1/images/edits` 兼容 |
| `IMAGE_REQUEST_INVALID` | 否 | 检查 `size`、输入图、蒙版、参考图 |
| `IMAGE_GENERATION_FAILED` | 否 | 兜底码，按 `failure_summary` 判断 |

- **其它三者走普通错误路径**：`text is required`、`prompt is required`、`path or source_public_path is required`、`audio source file not found`、`reference audio file is empty: <路径>`、`reference audio file not found: <路径>`、`ref_text requires reference_path or ref_audio for voice cloning`、`model_specific_params must be a JSON object`，以及 `video model is not configured` 等类型解析错误（`multimodal_generation_tool.rs:171-189`、`235-243`、`296-298`、`700-778`、`multimodal_models.rs:229`）。执行层记为 `ok=false`，错误元数据形如 `error_meta{code,hint,retryable,retry_after_ms}`（`crates/wunder-runtime/src/services/tools/tool_error.rs:4-50`）。

## 边界

- 参数面没有长度上限声明：文本、提示词受上游模型接口约束，超长由 provider 报错；不要指望平台帮你截断。
- 产物路径是工作区相对路径，读回用 [读取文件](/docs/zh-CN/tools/workspace-files/) / [读图工具](/docs/zh-CN/tools/read-image/)；音视频类不要直接读进上下文。
- 云端形态下生成与落盘同样受沙箱与工作区归属约束，写的是本用户目录。
- 生成类调用普遍慢且昂贵：一个任务只调一次，失败先按 `error_meta.hint` 改参数，不要循环重试同一请求。

## 相关页面

- 模型档案与类型：[配置参考](/docs/zh-CN/reference/config/)
- 读图（视觉输入）：[读图工具](/docs/zh-CN/tools/read-image/)
