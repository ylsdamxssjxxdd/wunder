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
    let name = format!("{name_prefix}-{}.png", timestamp());
    std::fs::write(directory.join(&name), &png)
        .map_err(|error| format!("无法保存图片：{error}"))?;
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    );
    Ok((name, data_url))
}

pub fn add(app: &slint::Weak<MainWindow>, name: String, data_url: String, kind: &str) {
    let Some(app) = app.upgrade() else { return };
    let mut attachments = app.get_pending_attachments().iter().collect::<Vec<_>>();
    if attachments.len() >= MAX_PENDING_ATTACHMENTS {
        app.set_status(format!("最多可附加 {MAX_PENDING_ATTACHMENTS} 张图片，请先移除一张").into());
        return;
    }
    attachments.push(ChatAttachment {
        name: name.into(),
        mime_type: "image/png".into(),
        data_url: data_url.into(),
    });
    app.set_pending_attachments(ModelRc::new(VecModel::from(attachments)));
    app.set_status(format!("{kind}已放入输入区，发送即可交给模型").into());
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

fn timestamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or_default()
}
