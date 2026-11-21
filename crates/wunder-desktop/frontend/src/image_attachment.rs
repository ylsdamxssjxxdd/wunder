use crate::{ChatAttachment, MainWindow};
use base64::Engine;
use slint::{Model, ModelRc, VecModel};

const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const MAX_PENDING_ATTACHMENTS: usize = 4;

pub fn save(
    width: u32,
    height: u32,
    rgba: &[u8],
    name_prefix: &str,
) -> Result<(String, String), String> {
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
    Ok((name, data_url))
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
pub fn push(app: &slint::Weak<MainWindow>, name: String, data_url: String) -> bool {
    let Some(app) = app.upgrade() else {
        return false;
    };
    let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
    if attachments.len() >= MAX_PENDING_ATTACHMENTS {
        app.set_status(format!("最多可附加 {MAX_PENDING_ATTACHMENTS} 张图片，请先移除一张").into());
        return false;
    }
    attachments.push(ChatAttachment {
        name: name.into(),
        mime_type: "image/png".into(),
        data_url: data_url.into(),
    });
    app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
    true
}

pub fn add(app: &slint::Weak<MainWindow>, name: String, data_url: String, kind: &str) {
    if !push(app, name, data_url) {
        return;
    }
    if let Some(app) = app.upgrade() {
        app.set_status(format!("{kind}已放入输入区，发送即可交给模型").into());
    }
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
