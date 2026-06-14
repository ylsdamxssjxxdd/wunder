use crate::i18n;
use anyhow::Result;
#[cfg(windows)]
use encoding_rs::GBK;
use std::fs::File;
use std::io::Read;
use std::path::Path;

const BINARY_SAMPLE_BYTES: usize = 4096;
const CONTROL_BYTE_RATIO_THRESHOLD: f64 = 0.12;
const LOW_TEXT_SCORE_THRESHOLD: f64 = 0.55;

pub(crate) enum ReadFileGuardResult {
    Text(String),
    Omitted(BinaryFileNotice),
}

pub(crate) struct BinaryFileNotice {
    pub(crate) message: String,
    pub(crate) kind: &'static str,
    pub(crate) mime_type: Option<String>,
}

pub(crate) fn read_text_file_with_limit(
    path: &Path,
    max_bytes: usize,
) -> Result<ReadFileGuardResult> {
    let file = File::open(path)?;
    let mut buffer = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buffer)?;

    let sample_len = buffer.len().min(BINARY_SAMPLE_BYTES);
    let sample = &buffer[..sample_len];
    if let Some(mime_type) = detect_binary_image_mime(path, sample) {
        return Ok(ReadFileGuardResult::Omitted(BinaryFileNotice {
            message: i18n::t("tool.read.binary_image_use_read_image"),
            kind: "image",
            mime_type: Some(mime_type.to_string()),
        }));
    }
    if looks_like_binary(sample) {
        return Ok(ReadFileGuardResult::Omitted(BinaryFileNotice {
            message: i18n::t("tool.read.binary_omitted"),
            kind: "binary",
            mime_type: None,
        }));
    }

    Ok(ReadFileGuardResult::Text(decode_text_bytes(&buffer)))
}

// Keep binary payloads out of model context. A short textual hint is safer than lossy-decoding.
fn looks_like_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    if bytes.contains(&0) {
        return true;
    }
    if std::str::from_utf8(bytes).is_ok() {
        return false;
    }

    #[cfg(windows)]
    {
        let (decoded, _, had_errors) = GBK.decode(bytes);
        if !had_errors && text_score(&decoded) >= LOW_TEXT_SCORE_THRESHOLD {
            return false;
        }
    }

    let control_ratio = bytes
        .iter()
        .filter(|byte| is_binary_control(**byte))
        .count() as f64
        / bytes.len() as f64;
    if control_ratio >= CONTROL_BYTE_RATIO_THRESHOLD {
        return true;
    }

    text_score(&String::from_utf8_lossy(bytes)) < LOW_TEXT_SCORE_THRESHOLD
}

fn is_binary_control(byte: u8) -> bool {
    matches!(byte, 0x01..=0x08 | 0x0B | 0x0C | 0x0E..=0x1A | 0x1C..=0x1F)
}

fn text_score(text: &str) -> f64 {
    let mut total = 0usize;
    let mut text_like = 0usize;
    for ch in text.chars() {
        total += 1;
        if ch == '\u{FFFD}'
            || (!ch.is_whitespace() && !ch.is_ascii_graphic() && !ch.is_alphanumeric())
        {
            continue;
        }
        text_like += 1;
    }
    if total == 0 {
        1.0
    } else {
        text_like as f64 / total as f64
    }
}

fn decode_text_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let utf8_lossy = String::from_utf8_lossy(bytes).to_string();

    #[cfg(windows)]
    {
        let (decoded, _, had_errors) = GBK.decode(bytes);
        let gbk_text = decoded.into_owned();
        if !had_errors || text_score(&gbk_text) > text_score(&utf8_lossy) {
            return gbk_text;
        }
    }

    utf8_lossy
}

fn detect_binary_image_mime(path: &Path, bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.len() >= 6 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.len() >= 2 && bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.len() >= 4
        && (bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]))
    {
        return Some("image/tiff");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
    }
    detect_binary_image_mime_by_extension(path)
}

fn detect_binary_image_mime_by_extension(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.trim().to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn treats_utf8_text_as_text() {
        let result = looks_like_binary("hello, world\nsecond line".as_bytes());
        assert!(!result);
    }

    #[test]
    fn detects_binary_png_sample() {
        let result = detect_binary_image_mime(
            Path::new("chart.png"),
            b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR",
        );
        assert_eq!(result, Some("image/png"));
        assert!(looks_like_binary(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"));
    }

    #[test]
    fn keeps_svg_like_content_readable() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><text>x</text></svg>"#;
        assert_eq!(detect_binary_image_mime(Path::new("chart.svg"), svg), None);
        assert!(!looks_like_binary(svg));
    }
}
