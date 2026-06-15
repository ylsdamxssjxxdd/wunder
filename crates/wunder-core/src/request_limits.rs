use crate::schemas::AttachmentPayload;

pub fn measure_request_text_input_chars(
    question: &str,
    attachments: Option<&[AttachmentPayload]>,
) -> usize {
    let mut total = question.chars().count();
    for attachment in attachments.unwrap_or(&[]) {
        let content = attachment.content.as_deref().unwrap_or("").trim();
        if content.is_empty() || request_attachment_is_image(attachment, content) {
            continue;
        }
        total = total.saturating_add(content.chars().count());
    }
    total
}

pub fn request_attachment_is_image(attachment: &AttachmentPayload, content: &str) -> bool {
    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if content_type.starts_with("image") || content_type.contains("image") {
        return true;
    }
    if content.starts_with("data:image/") {
        return true;
    }
    let name = attachment
        .name
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        std::path::Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or(""),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_request_text_input_chars_ignores_image_attachments() {
        let attachments = vec![
            AttachmentPayload {
                name: Some("item.png".to_string()),
                content: Some("data:image/png;base64,xxxx".to_string()),
                content_type: Some("image/png".to_string()),
                public_path: None,
            },
            AttachmentPayload {
                name: Some("note.txt".to_string()),
                content: Some("text".to_string()),
                content_type: Some("text/plain".to_string()),
                public_path: None,
            },
        ];

        assert_eq!(
            measure_request_text_input_chars("abc", Some(&attachments)),
            "abctext".chars().count()
        );
    }
}
