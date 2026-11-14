use super::image_tool_contract::{self, ImageFailure, ImageFormat, ImageLimits};
use super::tool_error::{build_failed_tool_result, ToolErrorMeta};
use super::{build_model_tool_success, collect_read_roots, ToolContext};
use crate::config::LlmModelConfig;
use crate::llm::normalize_provider;
use crate::path_utils::{is_within_root, normalize_path_for_compare, normalize_target_path};
use crate::services::multimodal_models::{
    self, AudioTranscriptionRequest, ImageGenerationRequest, SpeechSynthesisRequest,
    VideoGenerationRequest,
};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use tokio::fs;

pub const TOOL_GENERATE_SPEECH: &str = "语音生成";
pub const TOOL_TRANSCRIBE_SPEECH: &str = "声转文";
pub const TOOL_GENERATE_IMAGE: &str = "图像生成";
pub const TOOL_GENERATE_IMAGE_LEGACY: &str = "绘图生成";
pub const TOOL_GENERATE_VIDEO: &str = "视频生成";

pub const TOOL_GENERATE_SPEECH_ALIAS: &str = "generate_speech";
pub const TOOL_TRANSCRIBE_SPEECH_ALIAS: &str = "transcribe_speech";
pub const TOOL_GENERATE_IMAGE_ALIAS: &str = "generate_image";
pub const TOOL_GENERATE_VIDEO_ALIAS: &str = "generate_video";

const GENERATED_MEDIA_DIR: &str = "generated_media";

#[derive(Debug, Deserialize)]
pub struct GenerateSpeechArgs {
    pub text: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    #[serde(default)]
    pub reference_path: Option<String>,
    #[serde(default)]
    pub ref_audio: Option<String>,
    #[serde(default)]
    pub ref_text: Option<String>,
    #[serde(default)]
    pub model_specific_params: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct TranscribeSpeechArgs {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub source_public_path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub response_format: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateVideoArgs {
    pub prompt: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub model_name: Option<String>,
    #[serde(default)]
    pub size: Option<String>,
    #[serde(default)]
    pub seconds: Option<f32>,
    #[serde(default)]
    pub fps: Option<u32>,
    #[serde(default)]
    pub num_frames: Option<u32>,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub num_inference_steps: Option<u32>,
    #[serde(default)]
    pub guidance_scale: Option<f32>,
    #[serde(default)]
    pub guidance_scale_2: Option<f32>,
    #[serde(default)]
    pub boundary_ratio: Option<f32>,
    #[serde(default)]
    pub flow_shift: Option<f32>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub enable_frame_interpolation: Option<bool>,
}

pub fn speech_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_tts_model_names(config).is_empty()
}

pub fn transcribe_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_asr_model_names(config).is_empty()
}

pub fn image_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_image_model_names(config).is_empty()
}

pub fn video_tool_available(config: &crate::config::Config) -> bool {
    !multimodal_models::list_video_model_names(config).is_empty()
}

pub async fn tool_generate_speech(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: GenerateSpeechArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let text = payload.text.trim();
    if text.is_empty() {
        return Err(anyhow!("text is required"));
    }
    let reference_path = normalize_optional_string(payload.reference_path.as_deref());
    let direct_ref_audio = normalize_optional_string(payload.ref_audio.as_deref());
    let ref_audio =
        resolve_reference_audio(context, reference_path.as_deref(), direct_ref_audio).await?;
    let ref_text = normalize_optional_string(payload.ref_text.as_deref());
    if ref_text.is_some() && ref_audio.is_none() {
        return Err(anyhow!(
            "ref_text requires reference_path or ref_audio for voice cloning"
        ));
    }
    let model_specific_params = match payload.model_specific_params {
        Some(Value::Object(map)) => Some(Value::Object(map)),
        Some(_) => return Err(anyhow!("model_specific_params must be a JSON object")),
        None => None,
    };
    let result = multimodal_models::synthesize_speech(
        context.config,
        SpeechSynthesisRequest {
            text: text.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            voice: normalize_optional_string(payload.voice.as_deref()),
            instructions: normalize_optional_string(payload.instructions.as_deref()),
            response_format: normalize_optional_string(payload.response_format.as_deref()),
            speed: payload.speed,
            ref_audio: ref_audio.clone(),
            ref_text: ref_text.clone(),
            model_specific_params: model_specific_params.clone(),
        },
    )
    .await?;
    let extension = extension_from_content_type(&result.content_type, "wav");
    let saved = persist_generated_media(
        context,
        payload.path.as_deref(),
        "speech",
        &extension,
        &result.bytes,
    )
    .await?;
    Ok(build_model_tool_success(
        "generate_speech",
        "completed",
        format!("Generated speech and saved it to {}.", saved.public_path),
        json!({
            "text": text,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
            "reference_path": reference_path,
            "voice_clone": ref_audio.is_some(),
            "ref_text": ref_text,
            "model_specific_params": model_specific_params,
        }),
    ))
}

pub async fn tool_transcribe_speech(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: TranscribeSpeechArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let source_public_path = normalize_optional_string(payload.source_public_path.as_deref())
        .or_else(|| normalize_optional_string(payload.path.as_deref()))
        .ok_or_else(|| anyhow!("path or source_public_path is required"))?;
    let resolved_path = context
        .workspace
        .resolve_path(context.workspace_id, &source_public_path)?;
    if !resolved_path.exists() || !resolved_path.is_file() {
        return Err(anyhow!("audio source file not found"));
    }
    let filename = payload
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            resolved_path
                .file_name()
                .and_then(|value| value.to_str())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "audio.wav".to_string());
    let content_type = payload
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let bytes = fs::read(&resolved_path).await?;
    let result = multimodal_models::transcribe_audio(
        context.config,
        AudioTranscriptionRequest {
            audio_bytes: bytes.into(),
            filename,
            content_type,
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            language: normalize_optional_string(payload.language.as_deref()),
            prompt: normalize_optional_string(payload.prompt.as_deref()),
            response_format: normalize_optional_string(payload.response_format.as_deref()),
            temperature: payload.temperature,
        },
    )
    .await?;
    Ok(build_model_tool_success(
        "transcribe_speech",
        "completed",
        format!("Transcribed audio from {}.", source_public_path),
        json!({
            "text": result.text,
            "content_type": result.content_type,
            "source_public_path": source_public_path,
            "raw_response": result.raw_response,
        }),
    ))
}

// ---------------------------------------------------------------------------
// 图像生成
// ---------------------------------------------------------------------------

/// 一次图像调用的上下文：失败结果要带全这些信息，调用方才知道自己错在哪。
struct ImageCallContext<'a> {
    /// 工具入口时间：成功与失败回执都要给 elapsed_ms，调用方才能建性能基线。
    started: std::time::Instant,
    model_config: Option<&'a LlmModelConfig>,
    model_name: Option<String>,
    mode: &'a str,
    size: Option<String>,
    input_paths: Vec<String>,
    mask_path: Option<String>,
    reference_path: Option<String>,
}

impl<'a> ImageCallContext<'a> {
    fn empty() -> Self {
        Self {
            started: std::time::Instant::now(),
            model_config: None,
            model_name: None,
            mode: "unknown",
            size: None,
            input_paths: Vec::new(),
            mask_path: None,
            reference_path: None,
        }
    }
}

pub async fn tool_generate_image(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let mut ctx = ImageCallContext::empty();
    let mut warnings: Vec<String> = Vec::new();

    // 1) 不需要模型就能判定的参数错误，先挡在调用上游之前。
    let prompt = args
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if prompt.is_empty() {
        let failure = ImageFailure::request(
            "prompt",
            "prompt 为空",
            "用一句话写清要生成或要改的画面，例如「把背景换成夜景」。",
        );
        return Ok(image_failure_result(&failure, &ctx, &warnings));
    }
    let (input_paths, input_warnings) = match image_tool_contract::merge_input_paths(args) {
        Ok(value) => value,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    warnings.extend(input_warnings);
    ctx.input_paths = input_paths.clone();
    ctx.mode = if input_paths.is_empty() {
        "text_to_image"
    } else {
        "image_edit"
    };

    let requested_path = normalize_optional_string(args.get("path").and_then(Value::as_str));
    let requested_format =
        normalize_optional_string(args.get("output_format").and_then(Value::as_str));
    let format = match image_tool_contract::resolve_output_format(
        requested_format.as_deref(),
        requested_path.as_deref(),
    ) {
        Ok(format) => format,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    let seed = match image_tool_contract::parse_seed(args.get("seed")) {
        Ok(seed) => seed,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    let strength = match image_tool_contract::parse_strength(args.get("strength")) {
        Ok(strength) => strength,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    let overwrite = args
        .get("overwrite")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mask_path = normalize_optional_string(args.get("mask_path").and_then(Value::as_str));
    let reference_path =
        normalize_optional_string(args.get("reference_path").and_then(Value::as_str));
    ctx.mask_path = mask_path.clone();
    ctx.reference_path = reference_path.clone();

    if mask_path.is_some() && input_paths.is_empty() {
        let failure = ImageFailure::request(
            "mask_path",
            "蒙版只用于局部重绘，本次没有给主图（input_paths）",
            "同时传 `input_paths` 指向要改的那张图；纯文生图请去掉 mask_path。",
        );
        return Ok(image_failure_result(&failure, &ctx, &warnings));
    }
    if reference_path.is_some() && input_paths.is_empty() {
        let failure = ImageFailure::request(
            "reference_path",
            "参考图只在编辑/合成时作为附加输入，单独给它没有主图可编辑",
            "同时传 `input_paths`（主图）；只想指定风格时删掉 reference_path，把风格写进 prompt。",
        );
        return Ok(image_failure_result(&failure, &ctx, &warnings));
    }
    if strength.is_some() && input_paths.is_empty() {
        warnings.push("strength 只对图生图/编辑生效，本次是文生图，已忽略该参数。".to_string());
    }

    // 2) 模型与尺寸约束：上限、对齐、短边都在本地先判，不再靠上游报错。
    let model_name_arg = args.get("model_name").and_then(Value::as_str);
    let (resolved_model_name, model_config) =
        match multimodal_models::resolve_image_model(context.config, model_name_arg) {
            Some(model) => model,
            None => {
                let failure = ImageFailure::new(
                    "IMAGE_MODEL_NOT_CONFIGURED",
                    false,
                    "图像模型（未配置）：当前没有可用的图像生成模型，本次请求没有发出去。",
                    "先在系统设置里配置默认图像模型，或为本次调用显式指定可用的 `model_name`。",
                );
                return Ok(image_failure_result(&failure, &ctx, &warnings));
            }
        };
    let provider = normalize_provider(model_config.provider.as_deref());
    let limits = ImageLimits::for_model(&model_config);
    ctx.model_config = Some(&model_config);
    ctx.model_name = Some(resolved_model_name.clone());

    let size = match image_tool_contract::decide_size(
        args.get("size").and_then(Value::as_str),
        &limits,
        !input_paths.is_empty(),
    ) {
        Ok(size) => size,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    ctx.size = size.effective.clone().or_else(|| size.requested.clone());
    warnings.extend(size.warnings);

    // 3) 输出路径：越界、目录、覆盖、与输入同名，都在生成之前拦住。
    let pre_target = match resolve_media_target(
        context.workspace.as_ref(),
        context.workspace_id,
        requested_path.as_deref(),
        format.extension(),
        "image",
        overwrite,
        false,
    ) {
        Ok(target) => target,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    if let Some(warning) = pre_target.warning.clone() {
        warnings.push(warning);
    }
    if let Some(clash) = conflicting_source(
        context.workspace.as_ref(),
        context.workspace_id,
        &pre_target,
        &input_paths,
        mask_path.as_deref(),
        reference_path.as_deref(),
    ) {
        let failure = ImageFailure::request(
            "path",
            format!(
                "与 `{}` 指向同一个文件（`{}`），写入会把源文件覆盖掉",
                clash, pre_target.relative
            ),
            "换一个输出名，例如 `temp/out.png`；源图应保持只读。",
        );
        return Ok(image_failure_result(&failure, &ctx, &warnings));
    }

    // 4) 载入输入文件，顺带校验蒙版。
    let input_images = match load_image_input_files(context, &input_paths, "input_paths").await {
        Ok(files) => files,
        Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
    };
    let mask_image =
        match load_optional_image_input_file(context, mask_path.as_deref(), "mask_path").await {
            Ok(file) => file,
            Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
        };
    let reference_image =
        match load_optional_image_input_file(context, reference_path.as_deref(), "reference_path")
            .await
        {
            Ok(file) => file,
            Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
        };
    if let (Some(mask), Some(first)) = (mask_image.as_ref(), input_images.first()) {
        if let Some(info) = image_tool_contract::inspect_mask(&mask.bytes) {
            if let Some((input_width, input_height)) =
                image_tool_contract::image_dimensions(&first.bytes)
            {
                if (info.width, info.height) != (input_width, input_height) {
                    let failure = ImageFailure::input(
                        "mask_path",
                        format!(
                            "蒙版尺寸 {}x{} 与主输入图 {}x{} 不一致，上游要求两者同尺寸",
                            info.width, info.height, input_width, input_height
                        ),
                        "用与原图同尺寸的蒙版；需要改区域大小就重新导出一张同尺寸蒙版。",
                    );
                    return Ok(image_failure_result(&failure, &ctx, &warnings));
                }
            }
            if info.scanned && info.all_black {
                warnings.push(
                    "蒙版全黑：没有任何白色重绘区域，按约定等于不做修改，上游可能返回原图或直接拒绝。把要改的区域涂白后重试。"
                        .to_string(),
                );
            }
            if info.scanned && info.all_white {
                warnings.push(
                    "蒙版全白：等于整图重绘，局部编辑会退化成整图重绘。只想改局部时把保留区域涂黑。"
                        .to_string(),
                );
            }
        }
    }

    // 5) seed：总是显式下发并回显，否则这张图以后无法复现。
    let seed_source = if seed.is_some() { "explicit" } else { "auto" };
    let effective_seed = seed.unwrap_or_else(random_image_seed);
    if seed.is_none() {
        warnings.push(format!(
            "未指定 seed，本次使用随机 seed {}；要复现这张图，下次显式传 `seed: {}`。",
            effective_seed, effective_seed
        ));
    }

    let result = match multimodal_models::generate_image(
        context.config,
        ImageGenerationRequest {
            prompt: prompt.to_string(),
            model_name: Some(resolved_model_name.clone()),
            size: size.effective.clone(),
            output_format: Some(format.as_str().to_string()),
            negative_prompt: normalize_optional_string(
                args.get("negative_prompt").and_then(Value::as_str),
            ),
            num_inference_steps: optional_u32(args, "num_inference_steps"),
            guidance_scale: optional_f32(args, "guidance_scale"),
            seed: Some(effective_seed),
            input_images,
            mask_image,
            reference_image,
            strength,
            true_cfg_scale: optional_f32(args, "true_cfg_scale"),
            output_compression: optional_u32(args, "output_compression"),
            layers: optional_u32(args, "layers"),
            resolution: optional_u32(args, "resolution"),
        },
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            let failure = classify_provider_failure(
                provider.as_str(),
                size.effective.as_deref(),
                &limits,
                &err.to_string(),
            );
            return Ok(image_failure_result(&failure, &ctx, &warnings));
        }
    };

    // 任何被缩放的输入都必须说出来：模型看到的是自己给的路径，看不到里面换了尺寸。
    if let Some(report) = result.resized_input.as_deref() {
        warnings.push(format!(
            "主图已缩放到画布 {}（输出尺寸按缩放后的图计算，避免上游再缩一遍）。",
            report
        ));
    }
    if let Some(report) = result.resized_mask.as_deref() {
        warnings.push(format!(
            "蒙版已随画布缩放到 {}；蒙版必须与主图同尺寸，否则上游会拒绝。",
            report
        ));
    }
    if let Some(report) = result.resized_reference.as_deref() {
        warnings.push(format!("参考图超出尺寸上限，已等比缩小 {}。", report));
    }
    for (index, report) in result.resized_extra_inputs.iter().enumerate() {
        warnings.push(format!(
            "第 {} 张输入图超出尺寸上限，已等比缩小 {}。",
            index + 2,
            report
        ));
    }

    let actual_format = ImageFormat::from_content_type(&result.content_type).unwrap_or(format);
    if actual_format != format {
        warnings.push(format!(
            "请求的输出格式是 {}，上游实际返回 {}；已按真实格式保存为 .{}。",
            format.as_str(),
            actual_format.as_str(),
            actual_format.extension()
        ));
    }
    let target = if actual_format == format {
        pre_target
    } else {
        match resolve_media_target(
            context.workspace.as_ref(),
            context.workspace_id,
            requested_path.as_deref(),
            actual_format.extension(),
            "image",
            overwrite,
            true,
        ) {
            Ok(target) => target,
            Err(failure) => return Ok(image_failure_result(&failure, &ctx, &warnings)),
        }
    };
    if let Err(failure) = write_media_bytes(context, &target, &result.bytes).await {
        return Ok(image_failure_result(&failure, &ctx, &warnings));
    }
    let saved = PersistedMediaFile {
        public_path: context
            .workspace
            .display_path(context.workspace_id, &target.absolute),
        workspace_relative_path: target.relative.clone(),
        size_bytes: result.bytes.len(),
    };

    // 计时包含参数校验、输入图缩放、上游请求与落盘，正好是调用方感知的墙钟时间。
    let elapsed_ms = ctx.started.elapsed().as_millis() as u64;
    let edited = ctx.mode == "image_edit";
    // 编辑模式必须说清“从哪张图改的”，否则模型会把结果当成一次新的文生图。
    let summary = if edited {
        match input_paths.as_slice() {
            [source] => format!(
                "Edited image from {} and saved it to {}.",
                source, saved.public_path
            ),
            sources => format!(
                "Edited image from {} input images and saved it to {}.",
                sources.len(),
                saved.public_path
            ),
        }
    } else {
        format!("Generated image and saved it to {}.", saved.public_path)
    };
    let reported_size = result
        .size
        .clone()
        .or_else(|| size.effective.clone())
        .or_else(|| size.requested.clone());
    Ok(build_model_tool_success(
        "generate_image",
        "completed",
        summary,
        json!({
            "prompt": prompt,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
            "mode": ctx.mode,
            "edited": edited,
            "model_name": resolved_model_name,
            "provider": provider,
            "size": reported_size,
            "size_source": size.source,
            "requested_size": size.requested,
            "output_format": actual_format.as_str(),
            "requested_output_format": format.as_str(),
            "seed": effective_seed,
            "seed_source": seed_source,
            "input_resized": result.resized_input,
            "mask_resized": result.resized_mask,
            "reference_resized": result.resized_reference,
            "extra_inputs_resized": result.resized_extra_inputs,
            "elapsed_ms": elapsed_ms,
            "input_paths": input_paths,
            "mask_path": mask_path,
            "reference_path": reference_path,
            "limits": {
                "default_size": format!("{}x{}", limits.default_size.0, limits.default_size.1),
                "max_size": limits.max_label(),
                "alignment": limits.alignment,
                "min_short_edge": limits.min_short_edge,
                "summary": limits.describe(),
            },
            "warnings": warnings,
        }),
    ))
}

fn optional_u32(args: &Value, key: &str) -> Option<u32> {
    let value = args.get(key)?;
    match value {
        Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| number.as_f64().map(|value| value.max(0.0) as u32)),
        Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn optional_f32(args: &Value, key: &str) -> Option<f32> {
    let value = args.get(key)?;
    match value {
        Value::Number(number) => number.as_f64().map(|value| value as f32),
        Value::String(text) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn random_image_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = nanos ^ (nanos >> 29) ^ u64::from(std::process::id());
    let value = mixed % (u64::from(u32::MAX) + 1);
    if value == 0 {
        1
    } else {
        value
    }
}

fn image_failure_result(
    failure: &ImageFailure,
    ctx: &ImageCallContext<'_>,
    warnings: &[String],
) -> Value {
    let provider = ctx
        .model_config
        .map(|config| normalize_provider(config.provider.as_deref()))
        .unwrap_or_else(|| "unknown".to_string());
    let mut data = json!({
        "tool": "generate_image",
        "phase": "execution",
        "provider": provider,
        "model_name": ctx.model_name,
        "mode": ctx.mode,
        "size": ctx.size,
        "input_paths": ctx.input_paths,
        "mask_path": ctx.mask_path,
        "reference_path": ctx.reference_path,
        "failure_summary": failure.message,
        "next_step_hint": failure.hint,
        "suggested_size": failure.suggested_size,
        "elapsed_ms": ctx.started.elapsed().as_millis() as u64,
        "warnings": warnings,
    });
    if let (Some(field), Some(map)) = (failure.field.as_deref(), data.as_object_mut()) {
        map.insert("field".to_string(), json!(field));
    }
    build_failed_tool_result(
        failure.message.clone(),
        data,
        ToolErrorMeta::new(
            failure.code,
            Some(failure.hint.clone()),
            failure.retryable,
            None,
        ),
        false,
    )
}

/// 上游失败的分类：只在这里决定 code / retryable / 怎么改。
fn classify_provider_failure(
    provider: &str,
    size: Option<&str>,
    limits: &ImageLimits,
    detail: &str,
) -> ImageFailure {
    let detail = summarize_image_error_detail(detail);
    let who = if provider.trim().is_empty() || provider == "unknown" {
        "上游图像服务".to_string()
    } else {
        format!("上游图像服务（{}）", provider)
    };
    let lower = detail.to_ascii_lowercase();
    let status = provider_http_status(&detail);

    if contains_timeout_hint(&detail) {
        return ImageFailure::new(
            "TOOL_TIMEOUT",
            true,
            format!("{}等待超时：{}。这是耗时问题，不是参数问题。", who, detail),
            "先确认是否已经产出图片（检查目标目录），再原样重试或降低尺寸/推理步数。",
        );
    }
    if status == Some(429) || lower.contains("too many requests") || lower.contains("busy") {
        return ImageFailure::new(
            "IMAGE_PROVIDER_BUSY",
            true,
            format!(
                "{}当前繁忙：{}。这是服务侧限流，不是参数问题。",
                who, detail
            ),
            "稍后原样重试，或降低并发与推理步数；不要为了绕过限流去改参数。",
        );
    }
    if status.is_some_and(|status| status >= 500)
        || lower.contains("internal server error")
        || lower.contains("bytearray index out of range")
    {
        return ImageFailure::new(
            "IMAGE_PROVIDER_ERROR",
            true,
            format!(
                "{}内部出错：{}。这是上游服务的临时故障，不是参数问题。",
                who, detail
            ),
            "原样重试；连续多次失败请查看上游日志，而不是反复调整 prompt 或 size。",
        );
    }
    if provider == "vllm_omni"
        && (lower.contains("divisible by 16") || lower.contains("divisible by"))
    {
        let suggestion = size
            .and_then(image_tool_contract::parse_size)
            .map(|(width, height)| {
                image_tool_contract::suggest_aligned_size(width, height, limits)
            });
        let fix = match suggestion.as_deref() {
            Some(candidate) => format!(
                "改用宽高都是 {} 的倍数、且不超过上限 {} 的尺寸，例如 `{}`、`1024x1024` 或 `1344x768`。",
                image_tool_contract::IMAGE_ALIGNMENT_UNIT,
                limits.max_label(),
                candidate
            ),
            None => format!(
                "改用宽高都是 {} 的倍数、且不超过上限 {} 的尺寸，例如 `1024x1024` 或 `1344x768`。",
                image_tool_contract::IMAGE_ALIGNMENT_UNIT,
                limits.max_label()
            ),
        };
        return ImageFailure::new(
            "IMAGE_SIZE_ALIGNMENT_INVALID",
            false,
            format!("{}拒绝了请求：{}。", who, detail),
            fix,
        )
        .with_suggested_size(suggestion);
    }
    if lower.contains("image model is not configured") {
        return ImageFailure::new(
            "IMAGE_MODEL_NOT_CONFIGURED",
            false,
            format!("图像模型（未配置）：{}。", detail),
            "先在系统设置里配置默认图像模型，或显式指定可用的 `model_name`。",
        );
    }
    if lower.contains("missing b64_json")
        || lower.contains("parse image response json")
        || lower.contains("provider response")
    {
        return ImageFailure::new(
            "IMAGE_PROVIDER_RESPONSE_INVALID",
            false,
            format!("{}返回了无法解析的响应：{}。", who, detail),
            "检查上游是否兼容 `/v1/images/generations` 或 `/v1/images/edits`，并查看其日志。",
        );
    }
    if status == Some(413) {
        return ImageFailure::new(
            "IMAGE_REQUEST_INVALID",
            false,
            format!("{}拒绝了请求：{}。请求体过大。", who, detail),
            format!(
                "降低尺寸或改用更小的输入图：当前上限是 {}，短边不小于 {}。",
                limits.max_label(),
                limits.min_short_edge
            ),
        );
    }
    if status == Some(400) && lower.contains("mask") {
        return ImageFailure::new(
            "IMAGE_INPUT_INVALID",
            false,
            format!("{}拒绝了蒙版：{}。", who, detail),
            "按约定白色=要重绘的区域、黑色=保留，且蒙版必须与主图同尺寸。",
        );
    }
    if lower.contains("invalid")
        || lower.contains("unsupported")
        || lower.contains("must be")
        || lower.contains("is required")
    {
        return ImageFailure::new(
            "IMAGE_REQUEST_INVALID",
            false,
            format!("{}认为参数不合法：{}。", who, detail),
            "按提示修正对应参数后重试；尺寸规则见 `limits`（宽高对齐、短边下限、上限）。",
        );
    }
    ImageFailure::new(
        "IMAGE_GENERATION_FAILED",
        false,
        format!("{}执行失败：{}。", who, detail),
        "先看失败详情定位是参数还是服务问题；参数类按提示修正，服务类稍后重试。",
    )
}

/// 从 `... failed: 500 Internal Server Error ...` 这类文本里取出 HTTP 状态码。
fn provider_http_status(detail: &str) -> Option<u16> {
    let tokens = detail
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|token| !token.is_empty());
    tokens
        .filter_map(|token| token.parse::<u16>().ok())
        .find(|status| (400..=599).contains(status))
}

/// 解析并校验输出位置：越界、目录、覆盖、隐藏扩展名补全都在这里。
#[derive(Debug)]
struct MediaTarget {
    relative: String,
    absolute: PathBuf,
    warning: Option<String>,
}

fn resolve_media_target(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    requested_path: Option<&str>,
    extension: &str,
    prefix: &str,
    overwrite: bool,
    rewrite_extension: bool,
) -> Result<MediaTarget, ImageFailure> {
    let root = normalize_target_path(&workspace.user_root(workspace_id));
    let mut warning = None;
    let relative = match requested_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(text) => {
            let normalized = text.replace('\\', "/");
            // 目录判断必须在补扩展名之前：`shots` 指向已有文件夹时不能被悄悄改成 `shots.png`。
            let raw_target = normalized.trim_end_matches('/');
            if probe_directory(workspace, workspace_id, raw_target) {
                return Err(ImageFailure::request(
                    "path",
                    format!("`{}` 是已存在的目录", raw_target),
                    "把 path 指向具体文件，例如 `temp/out.png`。",
                ));
            }
            let current_ext = Path::new(&normalized)
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_string();
            if current_ext.is_empty() {
                let with_extension = [normalized.trim_end_matches('/'), ".", extension].concat();
                warning = Some(format!("path 未写扩展名，已补成 `{}`。", with_extension));
                with_extension
            } else if rewrite_extension && !current_ext.eq_ignore_ascii_case(extension) {
                let replaced = Path::new(&normalized).with_extension(extension);
                let replaced = replaced.to_string_lossy().replace('\\', "/");
                warning = Some(format!("输出文件名按真实格式改成 `{}`。", replaced));
                replaced
            } else {
                normalized
            }
        }
        None => {
            let stamp = chrono::Utc::now().format("%Y%m%d_%H%M%S_%3f").to_string();
            [
                GENERATED_MEDIA_DIR,
                "/",
                prefix,
                "_",
                stamp.as_str(),
                ".",
                extension,
            ]
            .concat()
        }
    };
    let absolute = workspace
        .resolve_path(workspace_id, &relative)
        .map_err(|err| {
            ImageFailure::request(
                "path",
                format!("无法解析 `{}`：{}", relative, err),
                "改用工作区内的相对路径，例如 `temp/out.png`。",
            )
        })?;
    if !is_within_root(&root, &absolute) {
        return Err(ImageFailure::request(
            "path",
            format!(
                "`{}` 解析到工作区之外（`{}`）",
                relative,
                absolute.display()
            ),
            format!(
                "只允许写工作区内的文件，例如 `temp/out.png`；工作区根目录是 `{}`。",
                root.display()
            ),
        ));
    }
    // strip_prefix 保留原始字节切片，`temp/./out.png` 会带着 `./` 出来；
    // 逐段归一化，保证 workspace_relative_path 是唯一形态。
    let relative_normalized = match absolute.strip_prefix(&root) {
        Ok(path) => normalize_relative_components(path),
        Err(_) => {
            return Err(ImageFailure::request(
                "path",
                format!("`{}` 无法映射为工作区相对路径", relative),
                "改用工作区内的相对路径，例如 `temp/out.png`。",
            ))
        }
    };
    if relative_normalized.trim().is_empty() {
        return Err(ImageFailure::request(
            "path",
            "指向工作区根目录本身，不是文件",
            "补上文件名，例如 `out.png`。",
        ));
    }
    if absolute.is_dir() || probe_directory(workspace, workspace_id, &relative) {
        return Err(ImageFailure::request(
            "path",
            format!(
                "`{}` 是已存在的目录；工具不会把图片写进目录，也不会改写到目录旁边",
                relative_normalized
            ),
            "把 path 指向具体文件，例如 `temp/out.png`。",
        ));
    }
    if absolute.exists() && !overwrite {
        return Err(ImageFailure::request(
            "path",
            format!(
                "`{}` 已存在，为避免覆盖已有文件，本次没有写入",
                relative_normalized
            ),
            "换成新文件名；确实要覆盖就显式传 `overwrite: true`。",
        ));
    }
    Ok(MediaTarget {
        relative: relative_normalized,
        absolute,
        warning,
    })
}

/// 请求路径本身（补扩展名之前）就是目录：例如 path=shots/ 指向已有文件夹。
fn probe_directory(workspace: &WorkspaceManager, workspace_id: &str, raw: &str) -> bool {
    workspace
        .resolve_path(workspace_id, raw)
        .map(|path| path.is_dir())
        .unwrap_or(false)
}

fn normalize_relative_components(path: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::ParentDir => parts.push("..".to_string()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

/// 输出路径不能和任何输入图重合，否则生成完就把源文件写坏了。
fn conflicting_source(
    workspace: &WorkspaceManager,
    workspace_id: &str,
    target: &MediaTarget,
    input_paths: &[String],
    mask_path: Option<&str>,
    reference_path: Option<&str>,
) -> Option<String> {
    let target_key = normalize_path_for_compare(&target.absolute);
    let candidates: Vec<(&str, &str)> = input_paths
        .iter()
        .map(|path| ("input_paths", path.as_str()))
        .chain(mask_path.map(|path| ("mask_path", path)))
        .chain(reference_path.map(|path| ("reference_path", path)))
        .collect();
    for (field, raw) in candidates {
        let Ok(resolved) = workspace.resolve_path(workspace_id, raw) else {
            continue;
        };
        if normalize_path_for_compare(&resolved) == target_key {
            return Some(format!("{} 里的 `{}`", field, raw));
        }
    }
    None
}

async fn write_media_bytes(
    context: &ToolContext<'_>,
    target: &MediaTarget,
    bytes: &[u8],
) -> Result<(), ImageFailure> {
    if let Some(parent) = target.absolute.parent() {
        fs::create_dir_all(parent).await.map_err(|err| {
            ImageFailure::new(
                "IMAGE_WRITE_FAILED",
                false,
                format!("写入 {} 失败：无法创建目录（{}）。", target.relative, err),
                "检查工作区目录是否有写权限，或换一个输出路径。",
            )
        })?;
    }
    fs::write(&target.absolute, bytes).await.map_err(|err| {
        ImageFailure::new(
            "IMAGE_WRITE_FAILED",
            false,
            format!("写入 {} 失败：{}。", target.relative, err),
            "换一个有写权限的输出路径后重试。",
        )
    })?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(())
}

async fn load_image_input_files(
    context: &ToolContext<'_>,
    paths: &[String],
    field: &str,
) -> Result<Vec<multimodal_models::ImageInputFile>, ImageFailure> {
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        files.push(load_image_input_file(context, path, field).await?);
    }
    Ok(files)
}

async fn load_optional_image_input_file(
    context: &ToolContext<'_>,
    path: Option<&str>,
    field: &str,
) -> Result<Option<multimodal_models::ImageInputFile>, ImageFailure> {
    let Some(path) = normalize_optional_string(path) else {
        return Ok(None);
    };
    Ok(Some(load_image_input_file(context, &path, field).await?))
}

async fn load_image_input_file(
    context: &ToolContext<'_>,
    public_or_relative_path: &str,
    field: &str,
) -> Result<multimodal_models::ImageInputFile, ImageFailure> {
    let label = match field {
        "mask_path" => "蒙版",
        "reference_path" => "参考图",
        _ => "输入图",
    };
    let resolved = context
        .workspace
        .resolve_path(context.workspace_id, public_or_relative_path)
        .map_err(|err| {
            ImageFailure::input(
                field,
                format!("路径 `{}` 无法解析：{}", public_or_relative_path, err),
                "改用工作区内的相对路径，例如 `temp/source.png`。",
            )
        })?;
    if !resolved.exists() || !resolved.is_file() {
        return Err(ImageFailure::input(
            field,
            format!("{}文件不存在：`{}`", label, public_or_relative_path),
            "先确认文件在工作区里（可用「列出文件」核对），再把正确路径传进来。",
        ));
    }
    let bytes = fs::read(&resolved).await.map_err(|err| {
        ImageFailure::input(
            field,
            format!(
                "{}读取失败：`{}`（{}）",
                label, public_or_relative_path, err
            ),
            "检查文件权限后重试。",
        )
    })?;
    if bytes.is_empty() {
        return Err(ImageFailure::input(
            field,
            format!("{}文件是空的：`{}`", label, public_or_relative_path),
            "换成一张有内容的图片。",
        ));
    }
    let filename = resolved
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| "image.png".to_string());
    Ok(multimodal_models::image_input_file_from_bytes(
        bytes.into(),
        filename,
        None,
    ))
}

fn summarize_image_error_detail(error_text: &str) -> String {
    let trimmed = error_text.trim();
    if trimmed.is_empty() {
        return "unknown image generation error".to_string();
    }
    let lines = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return "unknown image generation error".to_string();
    }
    for needle in [
        "RuntimeError:",
        "Diffusion generation failed:",
        "Height must be divisible by 16",
        "Width must be divisible by 16",
        "image generation failed:",
    ] {
        if let Some(line) = lines
            .iter()
            .rev()
            .find(|line| line.contains(needle))
            .copied()
        {
            return line.to_string();
        }
    }
    if let Some(last) = lines.last() {
        return (*last).to_string();
    }
    trimmed.to_string()
}

fn contains_timeout_hint(message: &str) -> bool {
    let lower = message.trim().to_ascii_lowercase();
    lower.contains("timeout") || lower.contains("timed out") || lower.contains("time out")
}
async fn resolve_reference_audio(
    context: &ToolContext<'_>,
    reference_path: Option<&str>,
    direct_ref_audio: Option<String>,
) -> Result<Option<String>> {
    if let Some(ref_audio) = direct_ref_audio {
        if !is_direct_reference_audio_url(&ref_audio) {
            match resolve_reference_audio_path(context, &ref_audio) {
                Ok(resolved) => {
                    return build_reference_audio_data_url(&resolved, &ref_audio)
                        .await
                        .map(Some);
                }
                Err(err) if should_resolve_ref_audio_as_path(&ref_audio) => return Err(err),
                Err(_) => {}
            }
        }
        return Ok(Some(ref_audio));
    }
    let Some(reference_path) = reference_path else {
        return Ok(None);
    };
    let resolved = resolve_reference_audio_path(context, reference_path)?;
    build_reference_audio_data_url(&resolved, reference_path)
        .await
        .map(Some)
}

async fn build_reference_audio_data_url(resolved: &Path, reference_path: &str) -> Result<String> {
    let bytes = fs::read(&resolved).await?;
    if bytes.is_empty() {
        return Err(anyhow!("reference audio file is empty: {reference_path}"));
    }
    let mime = audio_content_type_from_path(&resolved);
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn resolve_reference_audio_path(
    context: &ToolContext<'_>,
    reference_path: &str,
) -> Result<PathBuf> {
    let read_roots = collect_read_roots(context);
    resolve_reference_audio_path_with_roots(
        context.workspace.as_ref(),
        context.workspace_id,
        reference_path,
        &read_roots,
    )
}

fn resolve_reference_audio_path_with_roots(
    workspace: &crate::workspace::WorkspaceManager,
    workspace_id: &str,
    reference_path: &str,
    read_roots: &[PathBuf],
) -> Result<PathBuf> {
    let path = normalize_reference_audio_path(reference_path);
    if path.starts_with("workspaces/") || path.starts_with("/workspaces/") {
        let resolved = workspace.resolve_path(workspace_id, &path)?;
        if resolved.exists() && resolved.is_file() {
            return Ok(resolved);
        }
        return Err(anyhow!("reference audio file not found: {reference_path}"));
    }
    if Path::new(&path).is_absolute() {
        if let Some(resolved) = resolve_existing_reference_audio_path(&path, read_roots) {
            return Ok(resolved);
        }
        return Err(anyhow!("reference audio file not found: {reference_path}"));
    }
    let resolved = workspace.resolve_path(workspace_id, &path)?;
    if resolved.exists() && resolved.is_file() {
        return Ok(resolved);
    }
    if let Some(resolved) = resolve_existing_reference_audio_path(&path, read_roots) {
        return Ok(resolved);
    }
    Err(anyhow!("reference audio file not found: {reference_path}"))
}

fn normalize_reference_audio_path(path: &str) -> String {
    let normalized = path.trim().replace('\\', "/");
    if let Some(rest) = normalized.strip_prefix("/workspaces/") {
        format!("workspaces/{rest}")
    } else {
        normalized
    }
}

fn should_resolve_ref_audio_as_path(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_direct_reference_audio_url(trimmed) {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    normalized.starts_with("/workspaces/")
        || normalized.starts_with("workspaces/")
        || normalized.starts_with("./")
        || normalized.starts_with("../")
        || normalized.contains('/')
        || Path::new(trimmed).is_absolute()
        || Path::new(trimmed)
            .extension()
            .and_then(|value| value.to_str())
            .map(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "mp3" | "wav" | "ogg" | "opus" | "aac" | "flac" | "m4a" | "webm"
                )
            })
            .unwrap_or(false)
}

fn resolve_existing_reference_audio_path(raw_path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        for root in roots {
            if is_within_root(root, path) && path.exists() && path.is_file() {
                return Some(path.to_path_buf());
            }
        }
        return None;
    }
    let relative = sanitize_reference_audio_relative_path(trimmed)?;
    for root in roots {
        let candidate = normalize_target_path(&root.join(&relative));
        if candidate.exists() && candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn sanitize_reference_audio_relative_path(raw_path: &str) -> Option<PathBuf> {
    let normalized = raw_path.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    if stripped.is_empty() {
        return None;
    }
    let path = PathBuf::from(stripped);
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return None;
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(path)
}

fn is_direct_reference_audio_url(value: &str) -> bool {
    let lower = value.trim().to_ascii_lowercase();
    lower.starts_with("data:") || lower.starts_with("http://") || lower.starts_with("https://")
}

fn audio_content_type_from_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "webm" => "audio/webm",
        _ => "application/octet-stream",
    }
}

pub async fn tool_generate_video(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: GenerateVideoArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let prompt = payload.prompt.trim();
    if prompt.is_empty() {
        return Err(anyhow!("prompt is required"));
    }
    let result = multimodal_models::generate_video(
        context.config,
        VideoGenerationRequest {
            prompt: prompt.to_string(),
            model_name: normalize_optional_string(payload.model_name.as_deref()),
            size: normalize_optional_string(payload.size.as_deref()),
            seconds: payload.seconds,
            fps: payload.fps,
            num_frames: payload.num_frames,
            negative_prompt: normalize_optional_string(payload.negative_prompt.as_deref()),
            num_inference_steps: payload.num_inference_steps,
            guidance_scale: payload.guidance_scale,
            guidance_scale_2: payload.guidance_scale_2,
            boundary_ratio: payload.boundary_ratio,
            flow_shift: payload.flow_shift,
            seed: payload.seed,
            enable_frame_interpolation: payload.enable_frame_interpolation,
            sync_mode: Some(true),
        },
    )
    .await?;
    let extension = extension_from_content_type(&result.content_type, "mp4");
    let saved = persist_generated_media(
        context,
        payload.path.as_deref(),
        "video",
        &extension,
        &result.bytes,
    )
    .await?;
    Ok(build_model_tool_success(
        "generate_video",
        "completed",
        format!("Generated video and saved it to {}.", saved.public_path),
        json!({
            "prompt": prompt,
            "content_type": result.content_type,
            "path": saved.public_path,
            "workspace_relative_path": saved.workspace_relative_path,
            "bytes": saved.size_bytes,
        }),
    ))
}
struct PersistedMediaFile {
    public_path: String,
    workspace_relative_path: String,
    size_bytes: usize,
}

/// 语音/视频产物落盘：与图像共用同一套输出路径规则（工作区内、目录拒绝、
/// 归一化相对路径），但不改它们原先“同名覆盖”的行为。
async fn persist_generated_media(
    context: &ToolContext<'_>,
    requested_path: Option<&str>,
    prefix: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<PersistedMediaFile> {
    let target = resolve_media_target(
        context.workspace.as_ref(),
        context.workspace_id,
        requested_path,
        extension,
        prefix,
        true,
        false,
    )
    .map_err(|failure| anyhow!(failure.message))?;
    write_media_bytes(context, &target, bytes)
        .await
        .map_err(|failure| anyhow!(failure.message))?;
    Ok(PersistedMediaFile {
        public_path: context
            .workspace
            .display_path(context.workspace_id, &target.absolute),
        workspace_relative_path: target.relative,
        size_bytes: bytes.len(),
    })
}

fn normalize_optional_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn extension_from_content_type(content_type: &str, fallback: &str) -> String {
    let normalized = content_type
        .split(';')
        .next()
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    match normalized.as_str() {
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        "audio/aac" => "aac",
        "audio/opus" => "opus",
        "audio/l16" => "pcm",
        "audio/wav" => "wav",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/png" => "png",
        "video/mp4" => "mp4",
        _ => fallback,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        classify_provider_failure, image_failure_result, optional_f32, optional_u32,
        provider_http_status, random_image_seed, resolve_media_target,
        resolve_reference_audio_path_with_roots, should_resolve_ref_audio_as_path,
        ImageCallContext, ImageFailure,
    };
    use crate::config::LlmModelConfig;
    use crate::path_utils::normalize_path_for_compare;
    use crate::services::tools::image_tool_contract::ImageLimits;
    use crate::storage::SqliteStorage;
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn image_model(provider: &str) -> LlmModelConfig {
        let mut config = LlmModelConfig {
            enable: Some(true),
            provider: Some(provider.to_string()),
            model: Some("demo-image".to_string()),
            model_type: Some("image".to_string()),
            ..Default::default()
        };
        config.image_default_size = Some("1024x1024".to_string());
        config.image_max_size = Some("1440x1440".to_string());
        config
    }

    #[test]
    fn transient_provider_failures_are_retryable() {
        let config = image_model("vllm_omni");
        let limits = ImageLimits::for_model(&config);
        let busy = classify_provider_failure(
            "vllm_omni",
            Some("1024x1024"),
            &limits,
            "image request failed: 429 Too Many Requests",
        );
        assert_eq!(busy.code, "IMAGE_PROVIDER_BUSY");
        assert!(busy.retryable);
        assert!(!busy.message.contains("调整图像生成参数"));

        let broken = classify_provider_failure(
            "vllm_omni",
            Some("1024x1024"),
            &limits,
            "image request failed: 500 Internal Server Error: bytearray index out of range",
        );
        assert_eq!(broken.code, "IMAGE_PROVIDER_ERROR");
        assert!(broken.retryable);

        let timeout = classify_provider_failure(
            "vllm_omni",
            Some("1024x1024"),
            &limits,
            "send image request: operation timed out",
        );
        assert_eq!(timeout.code, "TOOL_TIMEOUT");
        assert!(timeout.retryable);
    }

    #[test]
    fn alignment_failure_keeps_actionable_suggestion() {
        let config = image_model("vllm_omni");
        let limits = ImageLimits::for_model(&config);
        let failure = classify_provider_failure(
            "vllm_omni",
            Some("1920x1080"),
            &limits,
            "Diffusion generation failed: Height must be divisible by 16 (got 1080).",
        );
        assert_eq!(failure.code, "IMAGE_SIZE_ALIGNMENT_INVALID");
        assert!(!failure.retryable);
        // 建议值必须同时满足对齐与上限（`vllm_omni` 上限 1440），不能给出超限的 1920x1088。
        assert_eq!(failure.suggested_size.as_deref(), Some("1440x1088"));
        // 建议只出现在 hint 里，正文不再重复同一句。
        assert!(failure.hint.contains("1440x1088"));
        assert!(!failure.message.contains("1440x1088"));
        assert!(failure.message.contains("divisible by 16"));
    }

    #[test]
    fn failure_result_reports_elapsed_ms_and_keeps_hint_out_of_the_summary() {
        let ctx = ImageCallContext::empty();
        let failure = ImageFailure::request("path", "指向目录", "改成 `temp/out.png`。");
        let payload = image_failure_result(&failure, &ctx, &[]);
        assert!(payload["data"]["elapsed_ms"].as_u64().is_some());
        let summary = payload["data"]["failure_summary"]
            .as_str()
            .expect("failure_summary");
        assert_eq!(summary, "参数 `path` 不合法：指向目录。");
        assert_eq!(
            payload["data"]["next_step_hint"].as_str(),
            Some("改成 `temp/out.png`。")
        );
    }

    #[test]
    fn argument_errors_are_not_marked_retryable() {
        let config = image_model("vllm_omni");
        let limits = ImageLimits::for_model(&config);
        let failure = classify_provider_failure(
            "vllm_omni",
            Some("1024x1024"),
            &limits,
            "image request failed: 400 Bad Request: size is invalid",
        );
        assert_eq!(failure.code, "IMAGE_REQUEST_INVALID");
        assert!(!failure.retryable);
    }

    #[test]
    fn provider_status_is_extracted_from_failure_text() {
        assert_eq!(
            provider_http_status("image request failed: 503 Service Unavailable"),
            Some(503)
        );
        assert_eq!(provider_http_status("no status here"), None);
        assert_eq!(
            provider_http_status("image request failed: 400 Bad Request"),
            Some(400)
        );
    }

    #[test]
    fn numeric_args_accept_strings_and_reject_garbage() {
        let args = json!({ "steps": 12, "scale": "3.5", "bad": "x" });
        assert_eq!(optional_u32(&args, "steps"), Some(12));
        assert_eq!(optional_u32(&args, "bad"), None);
        assert_eq!(optional_f32(&args, "scale"), Some(3.5));
        assert_eq!(optional_f32(&args, "bad"), None);
    }

    #[test]
    fn random_seed_is_a_positive_u32() {
        let seed = random_image_seed();
        assert!(seed > 0);
        assert!(seed <= u64::from(u32::MAX));
    }

    fn workspace_fixture() -> (tempfile::TempDir, Arc<crate::workspace::WorkspaceManager>) {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let storage = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = Arc::new(crate::workspace::WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage,
            0,
            &HashMap::new(),
        ));
        (temp, workspace)
    }

    #[test]
    fn media_target_normalizes_relative_paths() {
        let (_temp, workspace) = workspace_fixture();
        let target = resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("temp/./out.png"),
            "png",
            "image",
            false,
            false,
        )
        .expect("target");
        assert_eq!(target.relative, "temp/out.png");
        assert!(target.warning.is_none());
    }

    #[test]
    fn media_target_appends_missing_extension_with_a_warning() {
        let (_temp, workspace) = workspace_fixture();
        let target = resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("temp/out"),
            "png",
            "image",
            false,
            false,
        )
        .expect("target");
        assert_eq!(target.relative, "temp/out.png");
        assert!(target.warning.is_some());
    }

    #[test]
    fn media_target_rejects_directories_and_outside_paths() {
        let (_temp, workspace) = workspace_fixture();
        let root = workspace.user_root("alice__c__1");
        fs::create_dir_all(root.join("shots")).expect("mkdir");
        let directory = resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("shots"),
            "png",
            "image",
            false,
            false,
        )
        .expect_err("directory must be rejected");
        assert_eq!(directory.code, "IMAGE_REQUEST_INVALID");
        assert!(directory.message.contains("目录"), "{}", directory.message);

        let outside = resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("../escape.png"),
            "png",
            "image",
            false,
            false,
        )
        .expect_err("outside must be rejected");
        assert!(
            outside.message.contains("工作区之外"),
            "{}",
            outside.message
        );
    }

    #[test]
    fn media_target_requires_overwrite_for_existing_files() {
        let (_temp, workspace) = workspace_fixture();
        let root = workspace.user_root("alice__c__1");
        fs::create_dir_all(root.join("temp")).expect("mkdir");
        fs::write(root.join("temp/out.png"), b"old").expect("write");
        let blocked = resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("temp/out.png"),
            "png",
            "image",
            false,
            false,
        )
        .expect_err("existing file must be rejected");
        assert!(blocked.message.contains("已存在"), "{}", blocked.message);
        // 怎么改只在 hint 里说一次，正文不重复。
        assert!(blocked.hint.contains("overwrite"), "{}", blocked.hint);
        assert!(resolve_media_target(
            workspace.as_ref(),
            "alice__c__1",
            Some("temp/out.png"),
            "png",
            "image",
            true,
            false,
        )
        .is_ok());
    }

    #[test]
    fn reference_audio_path_resolves_public_workspace_path() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let storage = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage,
            0,
            &HashMap::new(),
        );
        let reference = workspace
            .resolve_path("alice__c__1", "refs/voice.wav")
            .expect("resolve target");
        fs::create_dir_all(reference.parent().expect("parent")).expect("mkdir");
        fs::write(&reference, b"demo").expect("write reference audio");

        let resolved = resolve_reference_audio_path_with_roots(
            &workspace,
            "alice__c__1",
            "/workspaces/alice__c__1/refs/voice.wav",
            &[],
        )
        .expect("resolved");

        assert_eq!(
            normalize_path_for_compare(&resolved),
            normalize_path_for_compare(&reference)
        );
    }

    #[test]
    fn reference_audio_path_resolves_existing_read_root_relative_file() {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspace");
        let read_root = temp.path().join("workdir");
        let reference = read_root.join("refs").join("voice.wav");
        fs::create_dir_all(reference.parent().expect("parent")).expect("mkdir");
        fs::write(&reference, b"demo").expect("write reference audio");
        let storage = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        let workspace = crate::workspace::WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage,
            0,
            &HashMap::new(),
        );

        let resolved = resolve_reference_audio_path_with_roots(
            &workspace,
            "alice__c__1",
            "refs/voice.wav",
            &[read_root],
        )
        .expect("resolved");

        assert_eq!(
            normalize_path_for_compare(&resolved),
            normalize_path_for_compare(&reference)
        );
    }

    #[test]
    fn ref_audio_path_detection_keeps_urls_as_direct_inputs() {
        assert!(should_resolve_ref_audio_as_path("refs/voice.wav"));
        assert!(should_resolve_ref_audio_as_path("/workspaces/u/voice.wav"));
        assert!(!should_resolve_ref_audio_as_path(
            "data:audio/wav;base64,AAAA"
        ));
        assert!(!should_resolve_ref_audio_as_path(
            "https://example.test/voice.wav"
        ));
    }
}
