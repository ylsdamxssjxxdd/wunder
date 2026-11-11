use super::{build_model_tool_success, ToolContext};
use crate::schemas::AttachmentPayload;
use crate::services::chat_media::{
    derive_image_pair_view, derive_image_view, detect_media_kind_from_path,
    process_visual_media_path, ImageCrop, ImageViewStats, MediaViewDims,
};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;

const MAX_READ_VISUAL_MEDIA_BYTES: u64 = 128 * 1024 * 1024;

/// 已读图片指纹：同一路径重复读取时据此提示“内容未变”。
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImageFingerprint {
    sha256_8: String,
    mtime_ms: i64,
    size: u64,
}

#[derive(Default)]
struct ReadImageRegistry {
    entries: std::collections::HashMap<(String, String), ImageFingerprint>,
    order: std::collections::VecDeque<(String, String)>,
}

const READ_IMAGE_REGISTRY_CAPACITY: usize = 256;
/// 超出这个体积的图像不做全文件摘要，避免为一次读图扫几十 MB。
const MAX_FINGERPRINT_BYTES: u64 = 64 * 1024 * 1024;

static READ_IMAGE_REGISTRY: std::sync::OnceLock<std::sync::Mutex<ReadImageRegistry>> =
    std::sync::OnceLock::new();

fn read_image_registry() -> &'static std::sync::Mutex<ReadImageRegistry> {
    READ_IMAGE_REGISTRY.get_or_init(|| std::sync::Mutex::new(ReadImageRegistry::default()))
}

fn file_fingerprint(path: &Path, size: u64) -> Option<ImageFingerprint> {
    if size > MAX_FINGERPRINT_BYTES {
        return None;
    }
    use sha2::Digest;
    let mtime_ms = std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64)?;
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer).ok()?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Some(ImageFingerprint {
        sha256_8: hex::encode(&digest[..4]),
        mtime_ms,
        size,
    })
}

/// 记录本次读取并返回“与上次同内容”。按会话隔离，避免串到别的会话。
fn track_fingerprint(session_id: &str, path: &str, fingerprint: &Option<ImageFingerprint>) -> bool {
    let Some(fingerprint) = fingerprint else {
        return false;
    };
    let key = (session_id.to_string(), path.to_string());
    let mut registry = read_image_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let unchanged = registry
        .entries
        .get(&key)
        .map(|previous| previous.sha256_8 == fingerprint.sha256_8)
        .unwrap_or(false);
    registry.order.retain(|existing| existing != &key);
    registry.order.push_back(key.clone());
    registry.entries.insert(key, fingerprint.clone());
    while registry.order.len() > READ_IMAGE_REGISTRY_CAPACITY {
        if let Some(evicted) = registry.order.pop_front() {
            registry.entries.remove(&evicted);
        }
    }
    unchanged
}

fn format_mtime(mtime_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(mtime_ms)
        .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| mtime_ms.to_string())
}

fn unchanged_content_notice(unchanged: bool, fingerprint: &Option<ImageFingerprint>) -> String {
    match (unchanged, fingerprint) {
        (true, Some(fingerprint)) => format!(
            " Content is identical to the previous read of this path (sha256 {}, mtime {}); the image did not change.",
            fingerprint.sha256_8,
            format_mtime(fingerprint.mtime_ms)
        ),
        _ => String::new(),
    }
}

/// 对齐 codex `image_resize_notice`：入模视图被缩小后明确告知模型，避免它
/// 以为自己看的是原始像素，需要精确细节时用 crop / scale 取局部。
fn image_resize_notice(view: &MediaViewDims) -> String {
    if !view.downscaled() {
        return String::new();
    }
    format!(
        " Image was downscaled from {}x{} to {}x{} pixels for model input; use crop or scale to inspect fine detail.",
        view.source_width, view.source_height, view.width, view.height
    )
}

pub const TOOL_READ_IMAGE: &str = "\u{8bfb}\u{56fe}\u{5de5}\u{5177}";
pub const TOOL_READ_IMAGE_ALIAS: &str = "read_image";
pub const TOOL_VIEW_IMAGE_ALIAS: &str = "view_image";

#[derive(Debug, Deserialize)]
struct ReadImageArgs {
    path: String,
    #[serde(default)]
    frame_rate: Option<f64>,
    #[serde(default)]
    frame_step: Option<usize>,
    #[serde(default)]
    crop: Option<String>,
    #[serde(default)]
    scale: Option<f64>,
    #[serde(default)]
    stats: bool,
    #[serde(default)]
    stats_only: bool,
    #[serde(default)]
    compare_with: Option<String>,
}

pub fn is_read_image_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_READ_IMAGE {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_READ_IMAGE_ALIAS | TOOL_VIEW_IMAGE_ALIAS
    )
}

pub async fn tool_read_image(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: ReadImageArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_path = payload.path.trim();
    if raw_path.is_empty() {
        return Err(anyhow!(crate::i18n::t("tool.read.no_path")));
    }

    let extra_roots = super::collect_read_roots(context);
    let resolved = super::resolve_tool_path(
        context.workspace.as_ref(),
        context.workspace_id,
        raw_path,
        &extra_roots,
    )?;

    let metadata = tokio::fs::metadata(&resolved)
        .await
        .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
    if !metadata.is_file() {
        return Err(anyhow!(crate::i18n::t("tool.read_image.not_file")));
    }
    if metadata.len() > MAX_READ_VISUAL_MEDIA_BYTES {
        return Err(anyhow!(crate::i18n::t("tool.read.too_large")));
    }

    let sample = read_visual_media_sample(&resolved, 512).await?;
    let media_kind = detect_media_kind_from_path(&resolved, &sample)
        .ok_or_else(|| anyhow!(crate::i18n::t("tool.read_image.not_image")))?;
    let crop = payload
        .crop
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ImageCrop::parse)
        .transpose()?;
    let has_view_ops =
        crop.is_some() || payload.scale.is_some() || payload.stats || payload.stats_only;

    // 图片指纹：给模型一个“这份内容是否变过”的客观依据（sha256 前 8 位 + mtime）。
    let fingerprint = if media_kind == "image" {
        file_fingerprint(&resolved, metadata.len())
    } else {
        None
    };
    let unchanged = track_fingerprint(
        context.session_id,
        &resolved.to_string_lossy(),
        &fingerprint,
    );

    let compare_with = payload
        .compare_with
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(compare_raw) = compare_with {
        if media_kind != "image" {
            return Err(anyhow!(crate::i18n::t("tool.read_image.view_image_only")));
        }
        if has_view_ops {
            return Err(anyhow!(crate::i18n::t("tool.read_image.compare_exclusive")));
        }
        let compare_resolved = super::resolve_tool_path(
            context.workspace.as_ref(),
            context.workspace_id,
            compare_raw,
            &extra_roots,
        )?;
        let compare_metadata = tokio::fs::metadata(&compare_resolved)
            .await
            .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
        if !compare_metadata.is_file() {
            return Err(anyhow!(crate::i18n::t("tool.read_image.not_file")));
        }
        let pair = derive_image_pair_view(
            context.workspace.as_ref(),
            context.workspace_id,
            &resolved,
            &compare_resolved,
        )
        .await?;
        let attachments = vec![serde_json::to_value(&pair.attachment).unwrap_or(Value::Null)];
        let mut data = json!({
            "path": raw_path,
            "resolved_path": resolved.to_string_lossy().to_string(),
            "media_kind": media_kind,
            "size_bytes": metadata.len(),
            "mode": "side_by_side",
            "compare": {
                "left_path": raw_path,
                "right_path": compare_raw,
                "left": pair.left,
                "right": pair.right,
            },
            "result": {
                "kind": "image",
                "frame_count": attachments.len(),
                "attachments": attachments,
            },
        });
        if let Some(fingerprint) = fingerprint.as_ref() {
            data["mtime_ms"] = json!(fingerprint.mtime_ms);
            data["sha256_8"] = json!(fingerprint.sha256_8);
            data["unchanged_since_last_read"] = json!(unchanged);
        }
        return Ok(build_model_tool_success(
            "read_image",
            "completed",
            format!(
                "Prepared a side-by-side comparison for model inspection: left is {}, right is {}.{}",
                raw_path,
                compare_raw,
                unchanged_content_notice(unchanged, &fingerprint)
            ),
            data,
        ));
    }

    if has_view_ops {
        if media_kind != "image" {
            return Err(anyhow!(crate::i18n::t("tool.read_image.view_image_only")));
        }
        let view = derive_image_view(
            context.workspace.as_ref(),
            context.workspace_id,
            &resolved,
            crop,
            payload.scale,
            payload.stats || payload.stats_only,
            payload.stats_only,
        )
        .await?;
        let attachments: Vec<Value> = view
            .attachment
            .iter()
            .map(|attachment| serde_json::to_value(attachment).unwrap_or(Value::Null))
            .collect();
        let mut data = json!({
            "path": raw_path,
            "resolved_path": resolved.to_string_lossy().to_string(),
            "media_kind": media_kind,
            "size_bytes": metadata.len(),
            "view": {
                "source_width": view.source_width,
                "source_height": view.source_height,
                "width": view.view_width,
                "height": view.view_height,
            },
            "result": {
                "kind": "image",
                "frame_count": attachments.len(),
                "attachments": attachments,
            },
        });
        if let Some(stats) = view.stats.as_ref() {
            data["stats"] = serde_json::to_value(stats).unwrap_or(Value::Null);
        }
        if let Some(fingerprint) = fingerprint.as_ref() {
            data["mtime_ms"] = json!(fingerprint.mtime_ms);
            data["sha256_8"] = json!(fingerprint.sha256_8);
            data["unchanged_since_last_read"] = json!(unchanged);
        }
        let message = if payload.stats_only {
            format!("Computed region statistics for {}.", raw_path)
        } else {
            format!(
                "Prepared visual media {} for model inspection.{}{}",
                raw_path,
                image_resize_notice(&MediaViewDims {
                    source_width: view.source_width,
                    source_height: view.source_height,
                    width: view.view_width,
                    height: view.view_height,
                }),
                unchanged_content_notice(unchanged, &fingerprint)
            )
        };
        return Ok(build_model_tool_success(
            "read_image",
            "completed",
            message,
            data,
        ));
    }
    let result = process_visual_media_path(
        context.workspace.as_ref(),
        context.config,
        context.workspace_id,
        &resolved,
        None,
        payload.frame_rate,
        payload.frame_step,
    )
    .await?;
    let resize_notice = result
        .view
        .as_ref()
        .map(image_resize_notice)
        .unwrap_or_default();
    let unchanged_notice = unchanged_content_notice(unchanged, &fingerprint);
    let mut data = json!({
        "path": raw_path,
        "resolved_path": resolved.to_string_lossy().to_string(),
        "media_kind": media_kind,
        "size_bytes": metadata.len(),
        "result": result,
    });
    if let Some(fingerprint) = fingerprint.as_ref() {
        data["mtime_ms"] = json!(fingerprint.mtime_ms);
        data["sha256_8"] = json!(fingerprint.sha256_8);
        data["unchanged_since_last_read"] = json!(unchanged);
    }
    Ok(build_model_tool_success(
        "read_image",
        "completed",
        format!(
            "Prepared visual media {} for model inspection.{}{}",
            raw_path, resize_notice, unchanged_notice
        ),
        data,
    ))
}

pub async fn build_followup_user_message(
    context: &ToolContext<'_>,
    result_data: &Value,
) -> Result<Option<Value>> {
    let Some(result) = parse_result_payload(result_data) else {
        return Ok(None);
    };
    let prompt = crate::i18n::t("tool.read_image.followup_prompt");
    let attachments = result
        .result
        .get("attachments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let single_attachment = attachments.len() == 1;
    let mut content = vec![json!({ "type": "text", "text": prompt })];
    let mut hinted_paths = std::collections::HashSet::new();
    for item in attachments {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let content_type = obj
            .get("content_type")
            .or_else(|| obj.get("mime_type"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if content_type.starts_with("image/") {
            let attachment = AttachmentPayload {
                name: obj
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                content: None,
                content_type: Some(content_type.clone()),
                public_path: obj
                    .get("public_path")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            };
            // 记住原图路径：持久化会省略图片像素，恢复会话时靠这段文本
            // 让模型知道该重读哪个文件，而不是只看到一条无图的指令。
            let path = attachment
                .public_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    single_attachment.then(|| result.resolved_path.to_string_lossy().to_string())
                })
                .unwrap_or_default();
            if let Some(image_url) = crate::services::chat_media::load_image_attachment_data_url(
                context.workspace.as_ref(),
                &attachment,
            )
            .await
            {
                if !path.is_empty() && hinted_paths.insert(path.clone()) {
                    content.push(json!({
                        "type": "text",
                        "text": crate::i18n::t_with_params(
                            "tool.read_image.followup_path_hint",
                            &std::collections::HashMap::from([(
                                "path".to_string(),
                                path.clone()
                            )]),
                        )
                    }));
                }
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": image_url }
                }));
            } else if result.media_kind == "image" && single_attachment {
                let bytes = tokio::fs::read(&result.resolved_path)
                    .await
                    .map_err(|_| anyhow!(crate::i18n::t("tool.read.not_found")))?;
                let data_url = crate::services::chat_media::encode_model_ready_data_url(
                    bytes,
                    content_type.clone(),
                )
                .await;
                let path = result.resolved_path.to_string_lossy().to_string();
                if hinted_paths.insert(path.clone()) {
                    content.push(json!({
                        "type": "text",
                        "text": crate::i18n::t_with_params(
                            "tool.read_image.followup_path_hint",
                            &std::collections::HashMap::from([(
                                "path".to_string(),
                                path.clone()
                            )]),
                        )
                    }));
                }
                content.push(json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));
            }
            continue;
        }
        let text = obj
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(text) = text {
            content.push(json!({
                "type": "text",
                "text": text
            }));
        }
    }
    if content.len() <= 1 {
        return Ok(None);
    }
    // 数值结论以纯文本留在图前面：像素会被持久化省略，数字不会，模型之后
    // 可以直接引用而不必重看同一帧。
    let has_image = content
        .iter()
        .any(|part| part.get("type").and_then(Value::as_str) == Some("image_url"));
    if has_image {
        if let Some(stats) = result_data.get("stats").filter(|value| value.is_object()) {
            if let Ok(stats) = serde_json::from_value::<ImageViewStats>(stats.clone()) {
                content.insert(
                    1,
                    json!({
                        "type": "text",
                        "text": format_image_stats_text(&stats),
                    }),
                );
            }
        }
    }
    Ok(Some(json!({
        "role": "user",
        "content": content
    })))
}

pub(crate) fn format_image_stats_text(stats: &ImageViewStats) -> String {
    crate::i18n::t_with_params(
        "tool.read_image.followup_stats",
        &std::collections::HashMap::from([
            ("width".to_string(), stats.width.to_string()),
            ("height".to_string(), stats.height.to_string()),
            ("mean_luma".to_string(), format!("{:.1}", stats.mean_luma)),
            (
                "background".to_string(),
                format!(
                    "{},{},{}",
                    stats.background_rgb[0], stats.background_rgb[1], stats.background_rgb[2]
                ),
            ),
            (
                "non_background".to_string(),
                format!("{:.2}", stats.non_background_ratio * 100.0),
            ),
            (
                "edge".to_string(),
                format!("{:.2}", stats.edge_ratio * 100.0),
            ),
        ]),
    )
}

async fn read_visual_media_sample(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| anyhow!(format!("{}: {err}", crate::i18n::t("tool.read.not_found"))))?;
    let mut buffer = vec![0_u8; max_bytes];
    let read = file
        .read(&mut buffer)
        .await
        .map_err(|err| anyhow!(format!("{}: {err}", crate::i18n::t("tool.read.not_found"))))?;
    buffer.truncate(read);
    Ok(buffer)
}

struct ReadImageResultPayload {
    resolved_path: PathBuf,
    media_kind: String,
    result: Value,
}

fn parse_result_payload(data: &Value) -> Option<ReadImageResultPayload> {
    let obj = data.as_object()?;
    let resolved_path = obj
        .get("resolved_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(ReadImageResultPayload {
        resolved_path: PathBuf::from(resolved_path),
        media_kind: obj
            .get("media_kind")
            .and_then(Value::as_str)
            .unwrap_or("image")
            .to_string(),
        result: obj.get("result").cloned().unwrap_or(Value::Null),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_detects_unchanged_content_per_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a.png");
        std::fs::write(&path, b"first-content").expect("write");
        let size = std::fs::metadata(&path).expect("meta").len();
        let first = file_fingerprint(&path, size).expect("fingerprint");
        assert_eq!(first.sha256_8.len(), 8);
        assert_eq!(first.size, size);

        // 首次读取不算“与上次相同”。
        assert!(!track_fingerprint("session-a", "k", &Some(first.clone())));
        // 同会话再读同一内容：命中。
        assert!(track_fingerprint("session-a", "k", &Some(first.clone())));
        // 别的会话不共享“上次”。
        assert!(!track_fingerprint("session-b", "k", &Some(first.clone())));

        std::fs::write(&path, b"second-content-changed").expect("write");
        let changed = file_fingerprint(&path, std::fs::metadata(&path).expect("meta").len())
            .expect("fingerprint");
        assert_ne!(first.sha256_8, changed.sha256_8);
        assert!(!track_fingerprint("session-a", "k", &Some(changed)));
        // 没有指纹（过大或不可读）时永不报告“未变”。
        assert!(!track_fingerprint("session-a", "k", &None));
    }

    #[test]
    fn unchanged_notice_reports_hash_and_mtime() {
        let fingerprint = Some(ImageFingerprint {
            sha256_8: "1a2b3c4d".to_string(),
            mtime_ms: 1_700_000_000_000,
            size: 2048,
        });
        let notice = unchanged_content_notice(true, &fingerprint);
        assert!(notice.contains("1a2b3c4d"), "{}", notice);
        assert!(notice.contains("did not change"));
        assert_eq!(unchanged_content_notice(false, &fingerprint), "");
        assert_eq!(unchanged_content_notice(true, &None), "");
    }

    #[test]
    fn image_resize_notice_only_when_view_is_smaller() {
        // 1080p 在 codex 高细节上限内，不缩：无提示。
        let unchanged = MediaViewDims {
            source_width: 1920,
            source_height: 1080,
            width: 1920,
            height: 1080,
        };
        assert_eq!(image_resize_notice(&unchanged), "");

        // 4K 被压到 2048x1152：明确告知模型这是缩略版。
        let downscaled = MediaViewDims {
            source_width: 3840,
            source_height: 2160,
            width: 2048,
            height: 1152,
        };
        let notice = image_resize_notice(&downscaled);
        assert!(notice.contains("3840x2160 to 2048x1152"), "{notice}");
        assert!(notice.contains("crop or scale"), "{notice}");
    }
    use crate::config::Config;
    use crate::lsp::LspManager;
    use crate::skills::SkillRegistry;
    use crate::storage::{SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use image::{DynamicImage, ImageBuffer, Rgba};
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn read_image_tool_name_supports_aliases() {
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE));
        assert!(is_read_image_tool_name(TOOL_READ_IMAGE_ALIAS));
        assert!(is_read_image_tool_name(TOOL_VIEW_IMAGE_ALIAS));
        assert!(!is_read_image_tool_name("read_file"));
    }

    #[tokio::test]
    async fn followup_user_message_uses_default_text_without_prompt_arg() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-image-tool.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace = Arc::new(WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "user",
            session_id: "session",
            workspace_id: "user",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage,
            orchestrator: None,
            monitor: None,
            workspace,
            lsp_manager,
            config: &config,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };
        let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        let resolved_path = dir.path().join("unused.png");
        std::fs::write(&resolved_path, &bytes).expect("write test image");
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        );
        let payload = json!({
            "resolved_path": resolved_path.to_string_lossy().to_string(),
            "media_kind": "image",
            "result": {
                "attachments": [
                    {
                        "name": "image.png",
                        "content": data_url,
                        "content_type": "image/png"
                    }
                ]
            }
        });

        let message = build_followup_user_message(&context, &payload)
            .await
            .expect("build followup")
            .expect("message");
        let content = message
            .get("content")
            .and_then(Value::as_array)
            .expect("content array");

        assert_eq!(content.len(), 3);
        assert_eq!(content[0].get("type").and_then(Value::as_str), Some("text"));
        let expected_prompt = crate::i18n::t("tool.read_image.followup_prompt");
        assert_eq!(
            content[0].get("text").and_then(Value::as_str),
            Some(expected_prompt.as_str())
        );
        // Persistence strips image pixels, so the followup carries the source
        // path in plain text for cross-turn re-reading. The path must stay out
        // of the image_url object: it is a non-standard field for providers.
        let resolved_text = resolved_path.to_string_lossy().to_string();
        assert_eq!(content[1].get("type").and_then(Value::as_str), Some("text"));
        assert!(content[1]
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains(&format!("[image_path:{resolved_text}]")));
        assert!(content[2]
            .get("image_url")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("path"))
            .is_none());
        assert!(content[2]
            .get("image_url")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .starts_with("data:image/png;base64,"));
    }

    fn write_png(path: &std::path::Path, color: [u8; 4], width: u32, height: u32) {
        let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(width, height, Rgba(color));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        std::fs::write(path, &bytes).expect("write png");
    }

    #[tokio::test]
    async fn read_image_reports_unchanged_content_and_side_by_side() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-image-e2e.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace = Arc::new(WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "user",
            session_id: "read-image-e2e",
            workspace_id: "user",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage,
            orchestrator: None,
            monitor: None,
            workspace,
            lsp_manager,
            config: &config,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };
        let user_root = context
            .workspace
            .resolve_path("user", "")
            .expect("user root");
        std::fs::create_dir_all(&user_root).expect("mkdir user root");
        write_png(&user_root.join("left.png"), [200, 30, 30, 255], 40, 30);
        write_png(&user_root.join("right.png"), [30, 30, 200, 255], 30, 40);

        let first = tool_read_image(&context, &json!({ "path": "left.png" }))
            .await
            .expect("first read");
        assert_eq!(first["data"]["sha256_8"].as_str().map(str::len), Some(8));
        assert_eq!(first["data"]["unchanged_since_last_read"], json!(false));
        assert!(!first["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("identical"));

        let second = tool_read_image(&context, &json!({ "path": "left.png" }))
            .await
            .expect("second read");
        assert_eq!(second["data"]["unchanged_since_last_read"], json!(true));
        assert!(second["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("identical to the previous read"));

        let paired = tool_read_image(
            &context,
            &json!({
                "path": "left.png",
                "compare_with": "right.png",
            }),
        )
        .await
        .expect("compare read");
        assert_eq!(paired["data"]["mode"], json!("side_by_side"));
        let attachments = paired["data"]["result"]["attachments"]
            .as_array()
            .expect("attachments");
        assert_eq!(attachments.len(), 1);
        assert!(paired["summary"]
            .as_str()
            .unwrap_or_default()
            .contains("side-by-side comparison"));

        // compare_with 与 crop/scale 互斥。
        assert!(tool_read_image(
            &context,
            &json!({
                "path": "left.png",
                "compare_with": "right.png",
                "crop": "0,0,10,10",
            }),
        )
        .await
        .is_err());
    }

    #[test]
    fn stats_text_carries_the_numeric_evidence() {
        let stats = ImageViewStats {
            width: 320,
            height: 200,
            mean_luma: 63.8,
            background_rgb: [0, 0, 0],
            non_background_ratio: 0.2512,
            edge_ratio: 0.0345,
        };
        let text = format_image_stats_text(&stats);
        assert!(text.contains("[image_stats]"));
        assert!(text.contains("320x200"));
        assert!(text.contains("63.8"));
        assert!(text.contains("25.12"));
        assert!(text.contains("3.45"));
    }

    #[tokio::test]
    async fn followup_inserts_stats_text_before_the_image() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("read-image-stats.db");
        let storage: Arc<dyn StorageBackend> =
            Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace = Arc::new(WorkspaceManager::new(
            dir.path().to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let lsp_manager = LspManager::new(workspace.clone());
        let config = Config::default();
        let skills = SkillRegistry::default();
        let http = reqwest::Client::new();
        let context = ToolContext {
            user_id: "user",
            session_id: "session",
            workspace_id: "user",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage,
            orchestrator: None,
            monitor: None,
            workspace,
            lsp_manager,
            config: &config,
            skills: &skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &http,
        };
        let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(2, 2, Rgba([0, 0, 0, 255]));
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("encode png");
        let resolved_path = dir.path().join("stats.png");
        std::fs::write(&resolved_path, &bytes).expect("write test image");
        let data_url = format!(
            "data:image/png;base64,{}",
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes)
        );
        let payload = json!({
            "resolved_path": resolved_path.to_string_lossy().to_string(),
            "media_kind": "image",
            "stats": {
                "width": 2, "height": 2,
                "mean_luma": 0.0,
                "background_rgb": [0, 0, 0],
                "non_background_ratio": 0.0,
                "edge_ratio": 0.0
            },
            "result": {
                "attachments": [
                    {
                        "name": "stats.png",
                        "content": data_url,
                        "content_type": "image/png"
                    }
                ]
            }
        });

        let message = build_followup_user_message(&context, &payload)
            .await
            .expect("build followup")
            .expect("message");
        let content = message["content"].as_array().expect("content array");
        assert_eq!(content.len(), 4);
        assert!(content[1]["text"]
            .as_str()
            .unwrap()
            .contains("[image_stats]"));
        assert_eq!(content[2]["type"], "text");
        assert_eq!(content[3]["type"], "image_url");
    }
}
