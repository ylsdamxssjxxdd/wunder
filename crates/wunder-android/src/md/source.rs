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
    let extension = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp")
}

/// Extensions rendered as a file card rather than a link.
pub fn is_document(source: &str) -> bool {
    let Some(path) = trimmed(source) else {
        return false;
    };
    let extension = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
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

/// Drop a protocol-looking prefix the model invented in front of a real path.
///
/// The prompt shows the reference shape as `![description](<path>)`, and some
/// models write that placeholder literally as a scheme — `reference_path:/data/…
/// /shot.png`. The renderer then asks the workspace for `reference_path:/…`,
/// which is neither an absolute path nor a workspace entry, so the card fails
/// with "图片加载失败" even though the file is right there. `reference_path` is
/// not a real URL scheme, so treating it as one only ever breaks a reference.
/// Any genuine scheme (`http`, `data`, `file`, …) is left untouched.
pub fn normalize_destination(source: &str) -> String {
    strip_foreign_drive(strip_reference_prefix(source.trim())).to_string()
}

fn strip_reference_prefix(trimmed: &str) -> &str {
    for prefix in ["reference_path:", "reference-path:"] {
        if let Some(rest) = trimmed.get(..prefix.len()) {
            if rest.eq_ignore_ascii_case(prefix) {
                return trimmed[prefix.len()..].trim();
            }
        }
    }
    trimmed
}

/// Drop a Windows drive letter, which Android can never read.
///
/// The prompt that teaches the forward-slash rule shows a Windows path, and
/// some models copy the drive itself onto Android: `F:/data/app/shot.png` for
/// a file that really lives at `/data/app/shot.png`. A drive prefix is not an
/// absolute path here, so the loader treats the whole string as a workspace
/// entry, joins it under the workspace root and fails — the card then reads
/// "图片加载失败" while the file is right there. Thread prompts are frozen once
/// written, so old threads keep producing the shape; dropping the impossible
/// prefix is the only place that can heal them.
fn strip_foreign_drive(path: &str) -> &str {
    let bytes = path.as_bytes();
    let drive_absolute = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && bytes[2] == b'/';
    if drive_absolute {
        &path[2..]
    } else {
        path
    }
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

    /// A model that writes the prompt's placeholder as a scheme must still get
    /// a working card: the prefix is dropped, a real scheme is not.
    #[test]
    fn invented_reference_prefixes_are_stripped() {
        assert_eq!(
            normalize_destination("reference_path:/data/app/workspace/a.png"),
            "/data/app/workspace/a.png"
        );
        assert_eq!(
            normalize_destination(" Reference_Path:/tmp/a gif.gif "),
            "/tmp/a gif.gif"
        );
        assert_eq!(
            normalize_destination("https://example.com/a.png"),
            "https://example.com/a.png"
        );
        assert_eq!(normalize_destination("a.png"), "a.png");
        assert!(is_image(&normalize_destination(
            "reference_path:/data/app/workspace/a.gif"
        )));
    }

    /// A Windows drive copied onto Android can never address a file; the
    /// absolute path behind it still can.
    #[test]
    fn foreign_drive_prefixes_are_dropped() {
        assert_eq!(
            normalize_destination("F:/data/app/workspace/a.png"),
            "/data/app/workspace/a.png"
        );
        assert_eq!(
            normalize_destination("reference_path:C:/data/app/a.gif"),
            "/data/app/a.gif"
        );
        assert_eq!(normalize_destination("/data/app/a.png"), "/data/app/a.png");
    }
}
