use crate::i18n;
use anyhow::{anyhow, Result};
use encoding_rs::Encoding;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub converter: String,
    pub warnings: Vec<String>,
}

const DEFAULT_SUPPORTED_EXTENSIONS: &[&str] = &[
    ".txt",
    ".md",
    ".markdown",
    ".html",
    ".htm",
    ".py",
    ".c",
    ".cpp",
    ".cc",
    ".h",
    ".hpp",
    ".json",
    ".js",
    ".ts",
    ".css",
    ".ini",
    ".cfg",
    ".log",
    ".doc",
    ".docx",
    ".odt",
    ".pptx",
    ".odp",
    ".xlsx",
    ".ods",
    ".wps",
    ".et",
    ".dps",
];

const DOC2MD_DEFAULT_REFERENCE_ROOT: &str = r"C:\Users\32138\Desktop\eva\thirdparty\doc2md";

pub fn get_supported_extensions() -> Vec<String> {
    static CACHE: OnceLock<Vec<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if let Some(list) = load_doc2md_extensions() {
                return list;
            }
            let mut exts: Vec<String> = DEFAULT_SUPPORTED_EXTENSIONS
                .iter()
                .map(|ext| ext.to_string())
                .collect();
            exts.sort();
            exts
        })
        .clone()
}

pub fn sanitize_filename_stem(name: &str) -> String {
    let cleaned = filename_safe_regex().replace_all(name.trim(), "_");
    let cleaned = cleaned.trim_matches(['.', ' '].as_ref()).to_string();
    cleaned.replace("..", "_")
}

pub async fn convert_to_markdown(
    input_path: &Path,
    output_path: &Path,
    extension: &str,
) -> Result<ConversionResult> {
    let mut warnings = Vec::new();
    if let Some(binary) = resolve_doc2md_binary() {
        match run_doc2md(&binary, input_path, output_path).await {
            Ok(_) => {
                return Ok(ConversionResult {
                    converter: "doc2md".to_string(),
                    warnings,
                });
            }
            Err(detail) => warnings.push(detail),
        }
    }

    let (markdown, converter) = convert_with_fallback(input_path, extension)?;
    if markdown.trim().is_empty() {
        return Err(anyhow!(i18n::t("error.converter_empty_result")));
    }
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(output_path, markdown).await?;
    Ok(ConversionResult {
        converter,
        warnings,
    })
}

fn load_doc2md_extensions() -> Option<Vec<String>> {
    let root = std::env::var("DOC2MD_REFERENCE_ROOT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DOC2MD_DEFAULT_REFERENCE_ROOT));
    let readme = root.join("README.md");
    if !readme.exists() {
        return None;
    }
    let content = std::fs::read_to_string(readme).ok()?;
    let mut exts: HashSet<String> = HashSet::new();
    for cap in doc2md_ext_regex().captures_iter(&content) {
        if let Some(ext) = cap.get(1) {
            exts.insert(format!(".{}", ext.as_str().to_lowercase()));
        }
    }
    if exts.is_empty() {
        return None;
    }
    let mut output: Vec<String> = exts.into_iter().collect();
    output.sort();
    Some(output)
}

fn resolve_doc2md_binary() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_root = root.join("scripts").join("doc2md");
    let mut candidates = Vec::new();
    if cfg!(windows) {
        candidates.push("doc2md-win-x86_64.exe");
    } else if cfg!(target_os = "linux") {
        let arch = std::env::consts::ARCH;
        if arch == "aarch64" || arch == "arm64" {
            candidates.push("doc2md-linux-arm64");
        }
        candidates.push("doc2md-linux-x86_64");
    }
    for name in candidates {
        let path = bin_root.join(name);
        if path.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = path.metadata() {
                    let mut perms = metadata.permissions();
                    let mode = perms.mode();
                    if mode & 0o111 == 0 {
                        perms.set_mode(mode | 0o111);
                        let _ = std::fs::set_permissions(&path, perms);
                    }
                }
            }
            return Some(path);
        }
    }
    None
}

async fn run_doc2md(binary: &Path, input: &Path, output: &Path) -> Result<(), String> {
    let result = Command::new(binary)
        .arg("-o")
        .arg(output)
        .arg(input)
        .output()
        .await
        .map_err(|err| err.to_string())?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            stdout.trim()
        } else {
            detail
        };
        if detail.is_empty() {
            return Err(i18n::t("error.converter_doc2md_failed"));
        }
        return Err(detail.to_string());
    }
    if !output.exists() {
        return Err(i18n::t("error.converter_doc2md_no_output"));
    }
    Ok(())
}

fn convert_with_fallback(path: &Path, extension: &str) -> Result<(String, String)> {
    let ext = extension.to_lowercase();
    if matches!(ext.as_str(), ".md" | ".markdown" | ".txt" | ".log") {
        return Ok((read_text(path)?, "text".to_string()));
    }
    if matches!(ext.as_str(), ".html" | ".htm") {
        return Ok((convert_html(&read_text(path)?), "html".to_string()));
    }
    let language = match ext.as_str() {
        ".py" => "python",
        ".c" => "c",
        ".cpp" | ".cc" => "cpp",
        ".h" => "c",
        ".hpp" => "cpp",
        ".json" => "json",
        ".js" => "javascript",
        ".ts" => "typescript",
        ".css" => "css",
        ".ini" | ".cfg" => "",
        _ => "",
    };
    if !language.is_empty() || matches!(ext.as_str(), ".ini" | ".cfg") {
        return Ok((
            wrap_code_block(&read_text(path)?, language),
            "code".to_string(),
        ));
    }
    let message = i18n::t_with_params(
        "error.converter_python_converter_not_found",
        &HashMap::from([("ext".to_string(), extension.to_string())]),
    );
    Err(anyhow!(message))
}

fn read_text(path: &Path) -> Result<String> {
    let data =
        std::fs::read(path).map_err(|_| anyhow!(i18n::t("error.converter_read_text_failed")))?;
    for label in ["utf-8", "utf-8-sig", "gb18030", "latin-1"] {
        if let Some(encoding) = Encoding::for_label(label.as_bytes()) {
            let (decoded, _, _) = encoding.decode(&data);
            let text = decoded.to_string();
            if !text.is_empty() {
                return Ok(text);
            }
        }
    }
    Ok(String::from_utf8_lossy(&data).to_string())
}

fn wrap_code_block(text: &str, language: &str) -> String {
    let body = text.trim_end();
    format!("```{language}\n{body}\n```")
}

fn convert_html(text: &str) -> String {
    html_tag_regex().replace_all(text, "").to_string()
}

fn doc2md_ext_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\*\.([a-z0-9]+)").expect("invalid doc2md regex"))
}

fn filename_safe_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r#"[\\/:*?"<>|]+"#).expect("invalid filename regex"))
}

fn html_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"<[^>]+>").expect("invalid html regex"))
}
