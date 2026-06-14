use crate::config::Config;
use serde::Serialize;

const DRAWIO_EXTENSIONS: &[&str] = &["dio", "drawio"];
const DRAWIO_XML_EXTENSION_SUFFIX: &str = ".drawio.xml";

#[derive(Debug, Clone, Serialize)]
pub struct DrawioResolvedConfig {
    pub editor_url: String,
    pub max_file_bytes: usize,
}

pub fn resolve_config(config: &Config) -> Option<DrawioResolvedConfig> {
    let drawio = &config.drawio;
    if !drawio.enabled() {
        return None;
    }
    let editor_url = drawio.editor_url()?;
    Some(DrawioResolvedConfig {
        editor_url,
        max_file_bytes: drawio.max_file_bytes(),
    })
}

pub fn is_supported_filename(filename: &str) -> bool {
    let normalized = filename.trim().to_ascii_lowercase();
    if normalized.ends_with(DRAWIO_XML_EXTENSION_SUFFIX) {
        return true;
    }
    let Some(extension) = normalized.rsplit('.').next() else {
        return false;
    };
    DRAWIO_EXTENSIONS.contains(&extension)
}

pub fn editor_url_with_params(editor_url: &str, language: &str) -> String {
    let separator = if editor_url.contains('?') { '&' } else { '?' };
    let lang = normalize_language(language);
    format!("{editor_url}{separator}embed=1&proto=json&spin=1&noSaveBtn=1&lang={lang}")
}

fn normalize_language(language: &str) -> &'static str {
    match language.trim().to_ascii_lowercase().as_str() {
        "zh-cn" | "zh-hans" | "zh-hans-cn" | "zh" => "zh",
        "en" | "en-us" => "en",
        _ => "zh",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_drawio_file_names() {
        assert!(is_supported_filename("diagram.drawio"));
        assert!(is_supported_filename("diagram.dio"));
        assert!(is_supported_filename("diagram.drawio.xml"));
        assert!(!is_supported_filename("diagram.xml"));
    }

    #[test]
    fn appends_embed_query_params() {
        assert_eq!(
            editor_url_with_params("http://drawio.example", "en-US"),
            "http://drawio.example?embed=1&proto=json&spin=1&noSaveBtn=1&lang=en"
        );
        assert_eq!(
            editor_url_with_params("http://drawio.example/?offline=1", "zh-CN"),
            "http://drawio.example/?offline=1&embed=1&proto=json&spin=1&noSaveBtn=1&lang=zh"
        );
    }
}
