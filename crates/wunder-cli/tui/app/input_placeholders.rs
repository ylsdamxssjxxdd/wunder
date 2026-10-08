use crate::attachments::{AttachmentKind, PreparedAttachment};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AttachmentPlaceholderMatch {
    pub index: usize,
    pub start: usize,
    pub end: usize,
}

pub(super) fn attachment_placeholder(item: &PreparedAttachment, index: usize) -> String {
    let kind = match item.kind {
        AttachmentKind::Image => "Image",
        AttachmentKind::Text => "File",
    };
    format!("[{kind} #{}]", index + 1)
}

pub(super) fn attachment_placeholders(items: &[PreparedAttachment]) -> Vec<String> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| attachment_placeholder(item, index))
        .collect()
}

pub(super) fn replace_attachment_placeholders(
    text: &str,
    previous: &[PreparedAttachment],
    current: &[PreparedAttachment],
) -> String {
    if text.is_empty() || previous.is_empty() {
        return text.to_string();
    }

    let current_by_source = current
        .iter()
        .enumerate()
        .map(|(index, item)| {
            (
                item.source.to_ascii_lowercase(),
                attachment_placeholder(item, index),
            )
        })
        .collect::<HashMap<_, _>>();

    let mut rewritten = text.to_string();
    let markers = previous
        .iter()
        .enumerate()
        .map(|(index, _)| format!("__ATTACH_PLACEHOLDER_{index}__"))
        .collect::<Vec<_>>();

    for (index, item) in previous.iter().enumerate() {
        let placeholder = attachment_placeholder(item, index);
        rewritten = rewritten.replace(placeholder.as_str(), markers[index].as_str());
    }

    for (index, item) in previous.iter().enumerate() {
        let source_key = item.source.to_ascii_lowercase();
        let replacement = current_by_source
            .get(source_key.as_str())
            .cloned()
            .unwrap_or_default();
        rewritten = rewritten.replace(markers[index].as_str(), replacement.as_str());
    }

    rewritten
}

pub(super) fn strip_attachment_placeholders(text: &str, items: &[PreparedAttachment]) -> String {
    let mut stripped = text.to_string();
    for placeholder in attachment_placeholders(items) {
        stripped = stripped.replace(placeholder.as_str(), "");
    }
    stripped
}

pub(super) fn find_attachment_placeholder_covering_cursor(
    text: &str,
    cursor: usize,
    items: &[PreparedAttachment],
    include_end_boundary: bool,
) -> Option<AttachmentPlaceholderMatch> {
    for (index, placeholder) in attachment_placeholders(items).into_iter().enumerate() {
        let mut search_from = 0usize;
        while search_from < text.len() {
            let Some(offset) = text[search_from..].find(placeholder.as_str()) else {
                break;
            };
            let start = search_from + offset;
            let end = start + placeholder.len();
            let covers = if include_end_boundary {
                cursor > start && cursor <= end
            } else {
                cursor >= start && cursor < end
            };
            if covers {
                return Some(AttachmentPlaceholderMatch { index, start, end });
            }
            search_from = end;
        }
    }
    None
}

pub(super) fn clamp_cursor_out_of_attachment_placeholder(
    text: &str,
    cursor: usize,
    items: &[PreparedAttachment],
) -> usize {
    for placeholder in attachment_placeholders(items) {
        let mut search_from = 0usize;
        while search_from < text.len() {
            let Some(offset) = text[search_from..].find(placeholder.as_str()) else {
                break;
            };
            let start = search_from + offset;
            let end = start + placeholder.len();
            if cursor > start && cursor < end {
                let left_gap = cursor.saturating_sub(start);
                let right_gap = end.saturating_sub(cursor);
                return if left_gap <= right_gap { start } else { end };
            }
            search_from = end;
        }
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::{AttachmentKind, PreparedAttachment};
    use wunder_server::schemas::AttachmentPayload;

    fn prepared(source: &str, kind: AttachmentKind) -> PreparedAttachment {
        PreparedAttachment {
            source: source.to_string(),
            payload: AttachmentPayload {
                name: Some(source.to_string()),
                content: None,
                content_type: None,
                public_path: None,
            },
            kind,
            size_bytes: 1,
            detail: None,
        }
    }

    #[test]
    fn attachment_placeholder_matches_codex_like_tokens() {
        assert_eq!(
            attachment_placeholder(&prepared("shot.png", AttachmentKind::Image), 0),
            "[Image #1]"
        );
        assert_eq!(
            attachment_placeholder(&prepared("notes.md", AttachmentKind::Text), 1),
            "[File #2]"
        );
    }

    #[test]
    fn replace_attachment_placeholders_relabels_retained_items() {
        let previous = vec![
            prepared("a.png", AttachmentKind::Image),
            prepared("b.png", AttachmentKind::Image),
        ];
        let current = vec![prepared("b.png", AttachmentKind::Image)];
        assert_eq!(
            replace_attachment_placeholders("[Image #1] [Image #2] hello", &previous, &current),
            " [Image #1] hello"
        );
    }

    #[test]
    fn strip_attachment_placeholders_removes_inline_tokens() {
        let items = vec![
            prepared("a.png", AttachmentKind::Image),
            prepared("notes.md", AttachmentKind::Text),
        ];
        assert_eq!(
            strip_attachment_placeholders("look [Image #1] and [File #2]", &items),
            "look  and "
        );
    }
}
