use super::*;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

fn is_image_attachment(attachment: &AttachmentPayload, content: &str) -> bool {
    let content_type = attachment
        .content_type
        .as_deref()
        .unwrap_or("")
        .to_lowercase();
    if content_type.starts_with("image") {
        return true;
    }
    if content_type.contains("image") {
        return true;
    }
    if content.starts_with("data:image/") {
        return true;
    }
    let name = attachment.name.as_deref().unwrap_or("").to_lowercase();
    matches!(
        Path::new(&name)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or(""),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    )
}

fn data_url_regex() -> Option<&'static Regex> {
    static REGEX: OnceLock<Option<Regex>> = OnceLock::new();
    REGEX
        .get_or_init(|| {
            compile_regex(
                r"data:image/[a-zA-Z0-9+.-]+;base64,[A-Za-z0-9+/=\r\n]+",
                "data_url",
            )
        })
        .as_ref()
}

impl Orchestrator {
    pub(super) async fn build_user_message(
        &self,
        question: &str,
        attachments: Option<&[AttachmentPayload]>,
    ) -> Value {
        let Some(attachments) = attachments else {
            return json!({ "role": "user", "content": question });
        };
        if attachments.is_empty() {
            return json!({ "role": "user", "content": question });
        }
        let attachment_label = i18n::t("attachment.label");
        let attachment_separator = i18n::t("attachment.label.separator");
        let attachment_default_name = i18n::t("attachment.default_name");
        let attachment_invalid_image_notice = i18n::t("attachment.invalid_image_notice");
        let mut attachment_parts: Vec<String> = Vec::new();
        let mut image_parts: Vec<Value> = Vec::new();
        for attachment in attachments {
            let content = attachment.content.as_deref().unwrap_or("");
            let trimmed = content.trim();
            if content.trim().is_empty()
                && !attachment
                    .public_path
                    .as_deref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            {
                continue;
            }
            let name = attachment
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(&attachment_default_name);
            let display_name = Self::display_attachment_name(name);
            if is_image_attachment(attachment, trimmed) {
                if let Some(image_url) =
                    crate::services::chat_media::load_image_attachment_data_url(
                        &self.workspace,
                        attachment,
                    )
                    .await
                {
                    image_parts.push(json!({
                        "type": "image_url",
                        "image_url": { "url": image_url }
                    }));
                } else {
                    attachment_parts.push(format!(
                        "[{attachment_label}{attachment_separator}{display_name}]\n{attachment_invalid_image_notice}"
                    ));
                }
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            attachment_parts.push(format!(
                "[{attachment_label}{attachment_separator}{display_name}]\n{trimmed}"
            ));
        }
        let mut text_content = String::new();
        if !attachment_parts.is_empty() {
            text_content.push_str(&attachment_parts.join("\n\n"));
        }
        if !question.is_empty() {
            if !text_content.is_empty() {
                text_content.push_str("\n\n");
            }
            text_content.push_str(question);
        }
        if !image_parts.is_empty() {
            let text_payload = if text_content.trim().is_empty() {
                i18n::t("attachment.image_prompt")
            } else {
                text_content
            };
            let mut parts = vec![json!({ "type": "text", "text": text_payload })];
            parts.extend(image_parts);
            return json!({ "role": "user", "content": parts });
        }
        json!({ "role": "user", "content": text_content })
    }

    pub(super) fn display_attachment_name(name: &str) -> &str {
        let stem = Path::new(name)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(name);
        if stem.is_empty() {
            name
        } else {
            stem
        }
    }
}
