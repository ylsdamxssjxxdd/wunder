//! Classify a Markdown link or image target without touching the filesystem.
//!
//! The chat shell only promotes a reference into a card when the runtime can
//! actually serve it: a workspace image becomes a picture, a workspace
//! document becomes a file card, and everything else (http, anchor, an
//! unreadable scheme) stays inline text. Final authorization still belongs to
//! `NativeDesktop`; nothing here opens a path.

/// Extensions the workspace image loader understands.
pub fn is_image(source: &str) -> bool {
    let Some(path) = trimmed(source) else {
        return false;
    };
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Extensions rendered as a file card rather than a link.
pub fn is_document(source: &str) -> bool {
    let Some(path) = trimmed(source) else {
        return false;
    };
    let extension = path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        extension.as_str(),
        "pdf"
            | "doc"
            | "docx"
            | "xls"
            | "xlsx"
            | "csv"
            | "ppt"
            | "pptx"
            | "odt"
            | "ods"
            | "txt"
            | "md"
            | "markdown"
            | "rtf"
            | "json"
            | "yaml"
            | "yml"
            | "xml"
            | "drawio"
    )
}

/// The bare path of a reference, or `None` when it cannot address the
/// workspace (absolute URLs, protocol-relative sources, fragments).
fn trimmed(source: &str) -> Option<&str> {
    let path = source.split(['?', '#']).next()?.trim();
    if path.is_empty()
        || path.starts_with('#')
        || path.starts_with("//")
        || path.contains("://") && !path.starts_with("file://")
    {
        return None;
    }
    Some(path)
}

/// The file name a card shows when the Markdown carried no label.
pub fn file_name(source: &str, fallback: &str) -> String {
    source
        .split(['?', '#'])
        .next()
        .and_then(|value| value.rsplit(['/', '\\']).next())
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_images_are_classified_by_extension() {
        assert!(is_image("shots/a.PNG"));
        assert!(is_image("shots/a.png?raw=1"));
        assert!(!is_image("notes/a.md"));
        assert!(!is_image("https://example.com/a.png"));
        assert!(!is_image("#anchor"));
    }

    #[test]
    fn workspace_documents_are_classified_by_extension() {
        assert!(is_document("report.docx"));
        assert!(is_document("./notes/plan.md#L3"));
        assert!(!is_document("app.exe"));
        assert!(!is_document("https://example.com/a.pdf"));
    }

    #[test]
    fn file_name_falls_back_and_ignores_the_query() {
        assert_eq!(file_name("a/b/c.pdf?x=1", "文件"), "c.pdf");
        assert_eq!(file_name("C:\\x\\y.docx", "文件"), "y.docx");
        assert_eq!(file_name("/", "文件"), "文件");
    }
}
