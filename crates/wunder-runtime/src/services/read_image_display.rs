//! 读图条目在工作过程里要说清的事实：看了哪张图的哪一部分、用什么精度
//! 看的、数值结论是什么。
//!
//! 这些事实只由结果数据与调用参数决定，所以统一在这里生成；桌面版、命令行
//! 与云端展示层只做排版。此前各端只读 `size_bytes` 与 `frame_count`，默认
//! 读图的条目因此长得一模一样，看不出这次和上次到底差别在哪。

use serde_json::Value;

/// 一次读图的展示行：画面与源尺寸、体积、实际查看尺寸、裁切区、缩放、
/// 统计结论、帧与时长、并排对比、指纹、派生输出。
pub fn display_lines(data: &Value, args: &Value, is_zh: bool) -> Vec<String> {
    let data = unwrap_data(data);
    let args = parse_args(args);
    let mut lines = Vec::new();

    let kind = media_kind(data);
    let source = source_dims(data);
    let view = view_dims(data);
    let crop = crop_arg(&args);
    let crop_dims = crop.as_ref().and_then(|(_, dims)| *dims);

    if let Some(line) = media_line(&kind, source, is_zh) {
        lines.push(line);
    }
    if let Some(line) = size_line(data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = downscale_line(crop_dims.or(source), view, is_zh) {
        lines.push(line);
    }
    if let Some((crop, _)) = crop {
        lines.push(label_value("裁切", "crop", &crop, is_zh));
    } else if let Some((width, height)) = stats_dims(data) {
        // 历史行可能没有调用参数，但统计区域仍说明这次只看了一块区域。
        if source.is_some_and(|dims| dims != (width, height)) {
            lines.push(label_value(
                "裁切区域",
                "crop region",
                &px(width, height),
                is_zh,
            ));
        }
    }
    if let Some(scale) = number(&args, &["scale"]) {
        lines.push(label_value(
            "缩放",
            "scale",
            &format!("{}×", trim_float(scale)),
            is_zh,
        ));
    }
    if let Some(line) = stats_line(data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = frames_line(&kind, data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = duration_line(data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = compare_line(data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = unchanged_line(data, is_zh) {
        lines.push(line);
    }
    if let Some(line) = derived_output_line(data, is_zh) {
        lines.push(line);
    }
    lines
}

/// 折叠行的一句话摘要：这次的画面与「读了哪一部分」，没有就不编。
pub fn summary(data: &Value, args: &Value, is_zh: bool) -> String {
    let data = unwrap_data(data);
    let args = parse_args(args);
    let source = source_dims(data);
    let kind = media_kind(data);
    let Some(media) = media_line(&kind, source, is_zh) else {
        return String::new();
    };
    let mut parts = vec![media];
    match crop_arg(&args) {
        Some((crop, _)) => parts.push(label_value("裁切", "crop", &crop, is_zh)),
        None => {
            if let Some((width, height)) = stats_dims(data) {
                if source.is_some_and(|dims| dims != (width, height)) {
                    parts.push(label_value(
                        "裁切区域",
                        "crop region",
                        &px(width, height),
                        is_zh,
                    ));
                }
            }
        }
    }
    if let Some(scale) = number(&args, &["scale"]) {
        parts.push(label_value(
            "缩放",
            "scale",
            &format!("{}×", trim_float(scale)),
            is_zh,
        ));
    }
    if data.get("unchanged_since_last_read") == Some(&Value::Bool(true)) {
        parts.push(if is_zh { "内容未变" } else { "unchanged" }.to_string());
    }
    parts.join(" · ")
}

fn label_value(zh: &str, en: &str, value: &str, is_zh: bool) -> String {
    format!("{} {value}", if is_zh { zh } else { en })
}

/// 结果里嵌了一层 `data` 的持久化行（事件与存储各占一种形态）。
fn unwrap_data(value: &Value) -> &Value {
    let mut current = value;
    for _ in 0..4 {
        if current.get("path").is_some() || current.get("media_kind").is_some() {
            break;
        }
        let Some(nested) = current.get("data").filter(|nested| nested.is_object()) else {
            break;
        };
        current = nested;
    }
    current
}

fn parse_args(args: &Value) -> Value {
    if let Some(raw) = args.as_str() {
        if raw.len() <= 16 * 1024 {
            if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
                return parsed;
            }
        }
        return Value::Null;
    }
    args.clone()
}

fn string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn number(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_f64))
        .filter(|number| number.is_finite())
}

fn dims(value: &Value, prefix: &str) -> Option<(u32, u32)> {
    if !value.is_object() {
        return None;
    }
    let width = value
        .get(format!("{}width", prefix))
        .and_then(Value::as_u64)?;
    let height = value
        .get(format!("{}height", prefix))
        .and_then(Value::as_u64)?;
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

fn media_kind(data: &Value) -> String {
    string(data, &["media_kind"])
        .or_else(|| {
            data.get("result")
                .and_then(|result| string(result, &["kind"]))
        })
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn source_dims(data: &Value) -> Option<(u32, u32)> {
    let view = data.get("view").filter(|value| value.is_object());
    let result = data.get("result").filter(|value| value.is_object());
    view.and_then(|view| dims(view, "source_"))
        .or_else(|| result.and_then(|result| dims(result, "source_")))
        .or_else(|| {
            result
                .and_then(|result| result.get("view"))
                .and_then(|view| dims(view, "source_"))
        })
}

fn view_dims(data: &Value) -> Option<(u32, u32)> {
    let view = data.get("view").filter(|value| value.is_object());
    let result = data.get("result").filter(|value| value.is_object());
    view.and_then(|view| dims(view, ""))
        .or_else(|| result.and_then(|result| dims(result, "")))
        .or_else(|| {
            result
                .and_then(|result| result.get("view"))
                .and_then(|view| dims(view, ""))
        })
}

fn stats_dims(data: &Value) -> Option<(u32, u32)> {
    dims(data.get("stats").unwrap_or(&Value::Null), "")
}

/// 裁切尺寸是判断「有没有再缩小」的基准；两种写法都归一化成宽高。
fn parse_crop_dims(crop: &str) -> Option<(u32, u32)> {
    let values = parse_crop_values(crop)?;
    let width = values[2];
    let height = values[3];
    (width > 0 && height > 0).then_some((width, height))
}

fn parse_crop_xyxy_dims(crop: &str) -> Option<(u32, u32)> {
    let values = parse_crop_values(crop)?;
    let width = values[2].checked_sub(values[0])?;
    let height = values[3].checked_sub(values[1])?;
    (width > 0 && height > 0).then_some((width, height))
}

fn parse_crop_values(crop: &str) -> Option<[u32; 4]> {
    let mut parts = crop.split(',').map(str::trim);
    let values = [
        parts.next()?.parse::<u32>().ok()?,
        parts.next()?.parse::<u32>().ok()?,
        parts.next()?.parse::<u32>().ok()?,
        parts.next()?.parse::<u32>().ok()?,
    ];
    (parts.next().is_none()).then_some(values)
}

fn crop_arg(args: &Value) -> Option<(String, Option<(u32, u32)>)> {
    if let Some(crop) = string(args, &["crop"]) {
        let dims = parse_crop_dims(&crop);
        return Some((crop, dims));
    }
    if let Some(crop) = string(args, &["crop_xyxy"]) {
        let dims = parse_crop_xyxy_dims(&crop);
        return Some((format!("xyxy:{crop}"), dims));
    }
    None
}

fn media_line(kind: &str, source: Option<(u32, u32)>, is_zh: bool) -> Option<String> {
    let label = match (kind, is_zh) {
        ("image", true) => "图像",
        ("image", false) => "image",
        ("video", true) => "视频",
        ("video", false) => "video",
        ("gif", _) => "GIF",
        ("audio", true) => "音频",
        ("audio", false) => "audio",
        ("", _) => return None,
        (other, _) => other,
    };
    Some(match source {
        Some((width, height)) => format!("{} {}", label, px(width, height)),
        None => label.to_string(),
    })
}

fn size_line(data: &Value, is_zh: bool) -> Option<String> {
    let bytes = format_bytes(number(data, &["size_bytes"])?);
    let format = attachment(data, "content_type")
        .or_else(|| string(data, &["mime_type"]))
        .map(|mime| short_format(&mime))
        .filter(|format| !format.is_empty());
    Some(match (format, is_zh) {
        (Some(format), true) => format!("体积 {bytes} · {format}"),
        (Some(format), false) => format!("size {bytes} · {format}"),
        (None, true) => format!("体积 {bytes}"),
        (None, false) => format!("size {bytes}"),
    })
}

/// 模型实际看到的像素与基准尺寸不同才值得说：这解释了细节为什么被压掉。
fn downscale_line(
    reference: Option<(u32, u32)>,
    view: Option<(u32, u32)>,
    is_zh: bool,
) -> Option<String> {
    let (reference_width, reference_height) = reference?;
    let (width, height) = view?;
    if (width, height) == (reference_width, reference_height) {
        return None;
    }
    let scale = f64::from(width) / f64::from(reference_width.max(1));
    let verb = match (scale < 1.0, is_zh) {
        (true, true) => "缩小",
        (true, false) => "downscaled",
        (false, true) => "放大",
        (false, false) => "upscaled",
    };
    let percent = (scale * 100.0).round().max(1.0);
    let label = if is_zh { "查看" } else { "view" };
    Some(format!(
        "{} {} · {} {}%",
        label,
        px(width, height),
        verb,
        percent.round() as i64
    ))
}

fn stats_line(data: &Value, is_zh: bool) -> Option<String> {
    let stats = data.get("stats").filter(|stats| stats.is_object())?;
    let mut parts = Vec::new();
    if let Some(luma) = number(stats, &["mean_luma"]) {
        parts.push(label_value("亮度", "luma", &fixed1(luma), is_zh));
    }
    if let Some(background) = stats.get("background_rgb").and_then(Value::as_array) {
        if background.len() == 3 {
            let rgb = background
                .iter()
                .map(|channel| channel.as_u64().unwrap_or(0).min(255) as u8)
                .collect::<Vec<_>>();
            let hex = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
            parts.push(label_value("背景", "background", &hex, is_zh));
        }
    }
    let ratio = |key: &str| number(stats, &[key]).map(|value| value * 100.0);
    if let Some(non_background) = ratio("non_background_ratio") {
        parts.push(label_value(
            "非背景",
            "non-bg",
            &format!("{}%", fixed1(non_background)),
            is_zh,
        ));
    }
    if let Some(edge) = ratio("edge_ratio") {
        parts.push(label_value(
            "边缘",
            "edges",
            &format!("{}%", fixed1(edge)),
            is_zh,
        ));
    }
    if parts.is_empty() {
        return None;
    }
    let label = if is_zh { "统计" } else { "stats" };
    Some(format!("{} {}", label, parts.join(" · ")))
}

/// 只有动图/视频才有「取了几帧」这一说；普通图片的 1 张不算帧数。
fn frames_line(kind: &str, data: &Value, is_zh: bool) -> Option<String> {
    let result = data.get("result").filter(|value| value.is_object())?;
    let frames = number(result, &["frame_count"])?;
    if frames <= 0.0 {
        return None;
    }
    let total = number(result, &["total_frame_count"]).filter(|total| *total > frames);
    if !matches!(kind, "video" | "gif") && total.is_none() {
        return None;
    }
    let rate = number(result, &["applied_frame_rate"])
        .or_else(|| number(result, &["requested_frame_rate"]));
    let count = trim_float(frames);
    let mut parts = vec![label_value("帧", "frames", &count, is_zh)];
    if let Some(total) = total {
        parts.push(if is_zh {
            format!("共 {} 帧", trim_float(total))
        } else {
            format!("of {}", trim_float(total))
        });
    }
    if let Some(rate) = rate {
        parts.push(if is_zh {
            format!("每秒 {} 帧", trim_float(rate))
        } else {
            format!("{} fps", trim_float(rate))
        });
    }
    Some(parts.join(" · "))
}

fn duration_line(data: &Value, is_zh: bool) -> Option<String> {
    let result = data.get("result").filter(|value| value.is_object())?;
    let seconds = number(result, &["duration_ms"])? / 1000.0;
    let has_audio = result.get("has_audio") == Some(&Value::Bool(true));
    let mut line = label_value("时长", "duration", &format!("{}s", fixed1(seconds)), is_zh);
    if has_audio {
        line.push_str(if is_zh {
            " · 含音轨"
        } else {
            " · with audio"
        });
    }
    Some(line)
}

fn compare_line(data: &Value, is_zh: bool) -> Option<String> {
    let compare = data.get("compare").filter(|value| value.is_object())?;
    let right = string(compare, &["right_path"])?;
    Some(if is_zh {
        format!("并排对比 · 右 {right}")
    } else {
        format!("side by side · right {right}")
    })
}

fn unchanged_line(data: &Value, is_zh: bool) -> Option<String> {
    if data.get("unchanged_since_last_read") != Some(&Value::Bool(true)) {
        return None;
    }
    let fingerprint = string(data, &["sha256_8"]);
    Some(match (is_zh, fingerprint) {
        (true, Some(fingerprint)) => format!("与上次读取相同 · 指纹 {fingerprint}"),
        (true, None) => "与上次读取相同".to_string(),
        (false, Some(fingerprint)) => format!("unchanged since last read · {fingerprint}"),
        (false, None) => "unchanged since last read".to_string(),
    })
}

/// 裁切/缩放会落一份派生图；它就是模型真正看到的像素来源。
fn derived_output_line(data: &Value, is_zh: bool) -> Option<String> {
    let result = data.get("result").filter(|value| value.is_object())?;
    let attachments = result.get("attachments").and_then(Value::as_array)?;
    let source_name = string(data, &["path"])
        .map(|path| path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string());
    let mut names = attachments
        .iter()
        .filter_map(|attachment| {
            string(attachment, &["name"]).or_else(|| {
                string(attachment, &["public_path"])
                    .map(|path| path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string())
            })
        })
        .filter(|name| Some(name) != source_name.as_ref())
        .take(2)
        .collect::<Vec<_>>();
    names.dedup();
    if names.is_empty() {
        return None;
    }
    Some(label_value("输出", "output", &names.join(" · "), is_zh))
}

fn attachment(data: &Value, key: &str) -> Option<String> {
    data.get("result")
        .and_then(|result| result.get("attachments"))
        .and_then(Value::as_array)
        .and_then(|attachments| attachments.first())
        .and_then(|attachment| string(attachment, &[key]))
}

/// `image/png` → `PNG`：体积旁边的格式比 MIME 全名更好认。
fn short_format(mime: &str) -> String {
    mime.rsplit(['/', '+'])
        .next()
        .unwrap_or(mime)
        .to_ascii_uppercase()
}

/// 与读文件的行数并排看，整数比 `0.5` 这样的尾数更好读。
fn trim_float(value: f64) -> String {
    if (value - value.round()).abs() < f64::EPSILON {
        format!("{}", value.round() as i64)
    } else {
        format!("{}", value)
    }
}

fn px(width: u32, height: u32) -> String {
    format!("{}×{}", width, height)
}

/// 一位小数，且总能读成 `42.0` 而不是 `42`：统计口径比较时这一点很要紧。
fn fixed1(value: f64) -> String {
    let scaled = (value * 10.0).round() as i64;
    let mut text = (scaled / 10).to_string();
    text.push('.');
    text.push_str(&(scaled % 10).abs().to_string());
    text
}

fn format_bytes(bytes: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if bytes >= MB {
        format!("{} MB", fixed1(bytes / MB))
    } else if bytes >= KB {
        format!("{} KB", (bytes / KB).round() as i64)
    } else {
        format!("{} B", bytes.round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_image_read_states_the_frame_and_size() {
        let data = json!({
            "path": "shot.png",
            "media_kind": "image",
            "size_bytes": 320_512,
            "result": {
                "kind": "image",
                "frame_count": 1,
                "attachments": [{"name": "shot.png", "content_type": "image/png"}],
                "view": {"source_width": 1536, "source_height": 1024, "width": 1536, "height": 1024}
            }
        });
        let lines = display_lines(&data, &json!({"path": "shot.png"}), true);
        assert_eq!(lines[0], "图像 1536×1024");
        assert_eq!(lines[1], "体积 313 KB · PNG");
        // 原图未缩放、单帧图片：不编造「查看」与「帧」两行。
        assert!(!lines.iter().any(|line| line.contains("查看")));
        assert!(!lines.iter().any(|line| line.starts_with("帧")));
        // 默认读图的附件就是源文件本身，不该再报一次「输出」。
        assert!(!lines.iter().any(|line| line.starts_with("输出")));
        assert_eq!(summary(&data, &json!({}), true), "图像 1536×1024");
    }

    #[test]
    fn crop_and_scale_and_stats_name_the_read_region() {
        let data = json!({
            "path": "shot.png",
            "media_kind": "image",
            "size_bytes": 1024,
            "view": {"source_width": 1536, "source_height": 1024, "width": 400, "height": 300},
            "stats": {"width": 400, "height": 300, "mean_luma": 128.34,
                "background_rgb": [30, 30, 30], "non_background_ratio": 0.42, "edge_ratio": 0.0321},
            "result": {"kind": "image", "frame_count": 1,
                "attachments": [{"name": "shot_view.png", "content_type": "image/png"}]}
        });
        let args = json!({"path": "shot.png", "crop": "0,0,400,300", "scale": 0.5, "stats": true});
        let lines = display_lines(&data, &args, true);
        assert!(lines.iter().any(|line| line == "裁切 0,0,400,300"));
        assert!(lines.iter().any(|line| line == "缩放 0.5×"));
        // 裁切区就是查看区：没有额外缩放，就不该报「缩小 26%」。
        assert!(!lines.iter().any(|line| line.contains("查看")));
        assert!(lines.iter().any(|line| line.contains("亮度 128.3")
            && line.contains("背景 #1e1e1e")
            && line.contains("非背景 42.0%")));
        assert!(lines.iter().any(|line| line == "输出 shot_view.png"));
        assert_eq!(
            summary(&data, &args, true),
            "图像 1536×1024 · 裁切 0,0,400,300 · 缩放 0.5×"
        );
    }

    #[test]
    fn a_cropped_region_that_was_downscaled_reports_the_view_ratio() {
        let data = json!({
            "path": "shot.png",
            "media_kind": "image",
            "size_bytes": 1024,
            "view": {"source_width": 1536, "source_height": 1024, "width": 200, "height": 150},
            "result": {"kind": "image", "frame_count": 1}
        });
        let args = json!({"path": "shot.png", "crop": "0,0,400,300"});
        let lines = display_lines(&data, &args, true);
        assert!(lines.iter().any(|line| line == "查看 200×150 · 缩小 50%"));

        let corners = json!({"path": "shot.png", "crop_xyxy": "0,0,400,300"});
        let lines = display_lines(&data, &corners, true);
        assert!(lines.iter().any(|line| line == "查看 200×150 · 缩小 50%"));
        assert!(lines.iter().any(|line| line == "裁切 xyxy:0,0,400,300"));
        assert!(summary(&data, &corners, true).contains("裁切 xyxy:0,0,400,300"));
    }

    #[test]
    fn video_read_states_frames_rate_and_duration() {
        let data = json!({
            "path": "clip.mp4",
            "media_kind": "video",
            "size_bytes": 2 * 1024 * 1024,
            "result": {"kind": "video", "frame_count": 12, "total_frame_count": 120,
                "applied_frame_rate": 2.0, "duration_ms": 4000, "has_audio": true,
                "attachments": [{"name": "clip_001.png", "content_type": "image/png"}]}
        });
        let lines = display_lines(&data, &json!({"path": "clip.mp4"}), false);
        assert!(lines
            .iter()
            .any(|line| line == "frames 12 · of 120 · 2 fps"));
        assert!(lines
            .iter()
            .any(|line| line == "duration 4.0s · with audio"));
    }

    #[test]
    fn comparison_and_unchanged_reads_are_named() {
        let data = json!({
            "path": "after.png",
            "media_kind": "image",
            "size_bytes": 2048,
            "mode": "side_by_side",
            "compare": {"left_path": "before.png", "right_path": "after.png"},
            "sha256_8": "1a2b3c4d",
            "unchanged_since_last_read": true,
            "result": {"kind": "image", "frame_count": 1}
        });
        let lines = display_lines(
            &data,
            &json!({"path": "after.png", "compare_with": "before.png"}),
            true,
        );
        assert!(lines.iter().any(|line| line == "并排对比 · 右 after.png"));
        assert!(lines.iter().any(|line| line.contains("与上次读取相同")));
        assert!(summary(&data, &json!({}), true).contains("内容未变"));
    }
}
