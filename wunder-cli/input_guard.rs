use crate::locale;
use anyhow::{anyhow, Result};
use wunder_server::orchestrator_constants::MAX_USER_INPUT_TEXT_CHARS;
use wunder_server::request_limits::measure_request_text_input_chars;
use wunder_server::schemas::AttachmentPayload;

pub(crate) fn validate_request_text_input_size(
    language: &str,
    prompt: &str,
    attachments: Option<&[AttachmentPayload]>,
) -> Result<()> {
    let actual_chars = measure_request_text_input_chars(prompt, attachments);
    if actual_chars <= MAX_USER_INPUT_TEXT_CHARS {
        return Ok(());
    }
    Err(anyhow!(format_input_too_large_message(
        language,
        actual_chars,
    )))
}

pub(crate) fn format_input_too_large_message(language: &str, actual_chars: usize) -> String {
    if locale::is_zh_language(language) {
        return format!(
            "输入内容过长，最大支持 {MAX_USER_INPUT_TEXT_CHARS} 个字符，当前约 {actual_chars} 个字符（已计入文本附件，不计图片附件）。"
        );
    }
    format!(
        "Message exceeds the maximum length of {MAX_USER_INPUT_TEXT_CHARS} characters ({actual_chars} provided; non-image attachment text is included)."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_request_text_input_size_accepts_boundary() {
        let prompt = "x".repeat(MAX_USER_INPUT_TEXT_CHARS);
        assert!(validate_request_text_input_size("en", &prompt, None).is_ok());
    }

    #[test]
    fn validate_request_text_input_size_rejects_oversized_payload_with_attachment_text() {
        let prompt = "x".repeat(MAX_USER_INPUT_TEXT_CHARS - 2);
        let attachments = vec![AttachmentPayload {
            name: Some("note.md".to_string()),
            content: Some("abcd".to_string()),
            content_type: Some("text/markdown".to_string()),
            public_path: None,
        }];
        let err = validate_request_text_input_size("en", &prompt, Some(&attachments))
            .expect_err("oversized payload");
        let message = err.to_string();
        assert!(message.contains(&MAX_USER_INPUT_TEXT_CHARS.to_string()));
        assert!(message.contains(&(MAX_USER_INPUT_TEXT_CHARS + 2).to_string()));
    }

    #[test]
    fn validate_request_text_input_size_ignores_image_payloads() {
        let prompt = "x".repeat(MAX_USER_INPUT_TEXT_CHARS);
        let attachments = vec![AttachmentPayload {
            name: Some("image.png".to_string()),
            content: Some("data:image/png;base64,AAAA".to_string()),
            content_type: Some("image/png".to_string()),
            public_path: None,
        }];
        assert!(validate_request_text_input_size("zh", &prompt, Some(&attachments)).is_ok());
    }
}
