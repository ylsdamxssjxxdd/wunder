use crate::{ChatAttachment, MainWindow};
use base64::Engine;
use slint::{Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};

const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_ATTACHMENTS: usize = 4;
/// 输入区缩略图的长边上限。预览只用于辨认，不参与发送；压到这一档后
/// 几十张附件也不会把软件渲染器的纹理与内存拖垮。
const THUMB_MAX_EDGE: u32 = 240;
/// 文件选择可能挑到超大位图。解码前先限制位图边长与解码分配，避免一张
/// “压缩炸弹”为了缩略图整张解进内存。
const MAX_PREVIEW_EDGE: u32 = 8192;
const MAX_PREVIEW_ALLOC: u64 = 64 * 1024 * 1024;

/// 工作线程读出的一个附件。预览图以裸像素缓冲传递，因为 `slint::Image`
/// 只能在 UI 线程创建；事件循环里再调用 [`AttachmentDraft::into_chat_attachment`]。
pub struct AttachmentDraft {
    pub name: String,
    pub mime_type: String,
    pub data_url: String,
    pub preview: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

impl AttachmentDraft {
    pub fn into_chat_attachment(self) -> ChatAttachment {
        ChatAttachment {
            name: self.name.into(),
            mime_type: self.mime_type.into(),
            data_url: self.data_url.into(),
            preview: self.preview.map(Image::from_rgba8).unwrap_or_default(),
        }
    }
}

/// 只剩 data URL 时重建输入区卡片（队列编辑、等待消息回灌等路径）。
pub fn chat_attachment(name: &str, mime_type: &str, data_url: &str) -> ChatAttachment {
    ChatAttachment {
        name: name.into(),
        mime_type: mime_type.into(),
        data_url: data_url.into(),
        preview: thumbnail_from_data_url(mime_type, data_url)
            .map(Image::from_rgba8)
            .unwrap_or_default(),
    }
}

pub fn save(
    width: u32,
    height: u32,
    rgba: &[u8],
    name_prefix: &str,
) -> Result<AttachmentDraft, String> {
    let png = encode_png(width, height, rgba)?;
    if png.len() > MAX_PNG_BYTES {
        return Err("图片压缩后超过 8 MiB，未放入输入区".into());
    }
    let directory = std::env::temp_dir().join("wunder-screenshots");
    std::fs::create_dir_all(&directory).map_err(|error| format!("无法创建图片目录：{error}"))?;
    let prefix = name_prefix
        .trim()
        .trim_end_matches(|ch| ch == '-' || ch == '_');
    let prefix = if prefix.is_empty() { "image" } else { prefix };
    let (mut file, name) = create_numbered_file(&directory, prefix, "png")?;
    std::io::Write::write_all(&mut file, &png).map_err(|error| format!("无法保存图片：{error}"))?;
    drop(file);
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    Ok(AttachmentDraft {
        name,
        mime_type: "image/png".into(),
        data_url,
        preview: thumbnail_from_rgba(width, height, rgba),
    })
}

/// Create the next free `<prefix>_<n>.png` in the staging directory.
///
/// The composer shows this name and the runtime keeps it when the image is
/// persisted, so a screenshot reads as `screenshot_1.png` instead of a
/// millisecond timestamp the user cannot refer to later.
fn create_numbered_file(
    directory: &std::path::Path,
    prefix: &str,
    extension: &str,
) -> Result<(std::fs::File, String), String> {
    let mut index = next_index(directory, prefix);
    for _ in 0..4096 {
        let name = format!("{prefix}_{index}.{extension}");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(&name))
        {
            Ok(file) => return Ok((file, name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                index = index.saturating_add(1);
            }
            Err(error) => return Err(format!("无法保存图片：{error}")),
        }
    }
    Err("无法保存图片：没有可用的文件名".into())
}

fn next_index(directory: &std::path::Path, prefix: &str) -> u32 {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return 1;
    };
    let prefix = format!("{prefix}_");
    let mut max = 0u32;
    for entry in entries.flatten().take(4096) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let stem = rest.split('.').next().unwrap_or(rest);
        if let Ok(index) = stem.parse::<u32>() {
            max = max.max(index);
        }
    }
    max.saturating_add(1)
}

/// Append one image to the composer. Returns whether it was accepted; when the
/// pending limit rejects it the limit message has already been reported, and
/// the caller decides which status line fits its own entry point (a screenshot
/// also mentions the clipboard).
pub fn push(app: &slint::Weak<MainWindow>, draft: AttachmentDraft) -> bool {
    let Some(app) = app.upgrade() else {
        return false;
    };
    let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
    if attachments.len() >= MAX_PENDING_ATTACHMENTS {
        app.set_status(format!("最多可附加 {MAX_PENDING_ATTACHMENTS} 张图片，请先移除一张").into());
        return false;
    }
    attachments.push(draft.into_chat_attachment());
    app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
    true
}

pub fn add(app: &slint::Weak<MainWindow>, draft: AttachmentDraft, kind: &str) {
    if !push(app, draft) {
        return;
    }
    if let Some(app) = app.upgrade() {
        app.set_status(format!("{kind}已放入输入区，发送即可交给模型").into());
    }
}

/// 把一张已解码的 RGBA 位图缩成长边不超过 [`THUMB_MAX_EDGE`] 的预览。
/// 用面积平均降采样：截图上的细文字在缩略图里仍能看出块面，不会像最近邻
/// 那样碎成噪点。调用方保证 `rgba` 是标准 RGBA8 行优先布局。
pub fn thumbnail_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if width == 0 || height == 0 || rgba.len() < expected {
        return None;
    }
    let (target_width, target_height) = thumbnail_size(width, height);
    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(target_width, target_height);
    let out = buffer.make_mut_slice();
    let source_stride = (width as usize) * 4;
    for y in 0..target_height as usize {
        let source_y0 = y * height as usize / target_height as usize;
        let source_y1 = (((y + 1) * height as usize / target_height as usize).max(source_y0 + 1))
            .min(height as usize);
        for x in 0..target_width as usize {
            let source_x0 = x * width as usize / target_width as usize;
            let source_x1 = (((x + 1) * width as usize / target_width as usize).max(source_x0 + 1))
                .min(width as usize);
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for source_y in source_y0..source_y1 {
                let row = source_y * source_stride;
                for source_x in source_x0..source_x1 {
                    let pixel = row + source_x * 4;
                    sum[0] += rgba[pixel] as u32;
                    sum[1] += rgba[pixel + 1] as u32;
                    sum[2] += rgba[pixel + 2] as u32;
                    sum[3] += rgba[pixel + 3] as u32;
                    count += 1;
                }
            }
            let average = |value: u32| ((value + count / 2) / count) as u8;
            out[y * target_width as usize + x] = Rgba8Pixel {
                r: average(sum[0]),
                g: average(sum[1]),
                b: average(sum[2]),
                a: average(sum[3]),
            };
        }
    }
    Some(buffer)
}

/// 从编码后的图片字节生成预览。只处理常见位图格式；SVG 与无法解码的
/// 文件返回 `None`，由输入区降级成文件卡片。
pub fn thumbnail_from_bytes(
    mime_type: &str,
    bytes: &[u8],
) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if !mime_type.starts_with("image/") || mime_type == "image/svg+xml" {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_PREVIEW_EDGE);
    limits.max_image_height = Some(MAX_PREVIEW_EDGE);
    limits.max_alloc = Some(MAX_PREVIEW_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode().ok()?.into_rgba8();
    thumbnail_from_rgba(decoded.width(), decoded.height(), decoded.as_raw())
}

fn thumbnail_from_data_url(
    mime_type: &str,
    data_url: &str,
) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    if !mime_type.starts_with("image/") || mime_type == "image/svg+xml" {
        return None;
    }
    let (_, payload) = data_url.split_once(";base64,")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()?;
    thumbnail_from_bytes(mime_type, &bytes)
}

fn thumbnail_size(width: u32, height: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= THUMB_MAX_EDGE {
        return (width, height);
    }
    let scale = longest as f64 / THUMB_MAX_EDGE as f64;
    (
        ((width as f64) / scale).round().max(1.0) as u32,
        ((height as f64) / scale).round().max(1.0) as u32,
    )
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|error| format!("PNG 头写入失败：{error}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|error| format!("PNG 编码失败：{error}"))?;
    writer
        .finish()
        .map_err(|error| format!("PNG 收尾失败：{error}"))?;
    Ok(bytes)
}
