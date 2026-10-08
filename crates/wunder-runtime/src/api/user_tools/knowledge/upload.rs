use super::*;
use crate::attachment::{convert_to_markdown, get_supported_extensions};
use tokio::io::AsyncWriteExt;
use walkdir::WalkDir;

pub(super) async fn convert_upload_to_markdown(
    upload: &UploadedKnowledgeFile,
) -> Result<(String, String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let metadata = tokio::fs::metadata(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() > MAX_KNOWLEDGE_CONTENT_BYTES as u64 {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if content.len() > MAX_KNOWLEDGE_CONTENT_BYTES {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    if content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.empty_parse_result"),
        ));
    }
    Ok((content, conversion.converter, conversion.warnings))
}

pub(super) fn resolve_knowledge_path(
    root: &Path,
    relative_path: &str,
) -> Result<PathBuf, Response> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.absolute_path_forbidden"),
        ));
    }
    let target = root.join(rel);
    let resolved = normalize_target_path(&target);
    let normalized_root = normalize_existing_path(root);
    // Windows 有时会生成 \\?\ 前缀，这里做统一化比较避免误报路径越界。
    let root_compare = normalize_path_for_compare(&normalized_root);
    let target_compare = normalize_path_for_compare(&resolved);
    if resolved != root && !target_compare.starts_with(&root_compare) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.path_out_of_bounds"),
        ));
    }
    Ok(resolved)
}

pub(super) struct UploadedKnowledgeFile {
    pub(super) filename: String,
    pub(super) extension: String,
    pub(super) stem: String,
    pub(super) temp_dir: PathBuf,
    pub(super) input_path: PathBuf,
}

pub(super) async fn save_knowledge_upload_field(
    field: axum::extract::multipart::Field<'_>,
) -> Result<UploadedKnowledgeFile, Response> {
    let filename = field.file_name().unwrap_or("upload").to_string();
    let extension = Path::new(&filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();
    if extension.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.file_extension_missing"),
        ));
    }
    let extension = format!(".{extension}");
    let supported = get_supported_extensions();
    if !supported
        .iter()
        .any(|item| item.eq_ignore_ascii_case(&extension))
    {
        let message = i18n::t_with_params(
            "error.unsupported_file_type",
            &HashMap::from([("extension".to_string(), extension.clone())]),
        );
        return Err(error_response(StatusCode::BAD_REQUEST, message));
    }
    let stem_raw = Path::new(&filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let stem = sanitize_filename_stem(stem_raw);
    let stem = if stem.trim().is_empty() {
        "document".to_string()
    } else {
        stem
    };
    let temp_dir = create_knowledge_temp_dir().await?;
    let input_path = temp_dir.join(format!("{stem}{extension}"));
    save_knowledge_upload_content(field, &input_path).await?;
    Ok(UploadedKnowledgeFile {
        filename,
        extension,
        stem,
        temp_dir,
        input_path,
    })
}

pub(super) fn build_markdown_output_path(filename: &str, stem: &str) -> String {
    let raw_path = Path::new(filename);
    let output_name = format!("{stem}.md");
    let output = match raw_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        Some(parent) => parent.join(output_name),
        None => PathBuf::from(output_name),
    };
    output.to_string_lossy().replace('\\', "/")
}

pub(super) async fn persist_knowledge_upload(
    upload: &UploadedKnowledgeFile,
    target: &Path,
) -> Result<(String, Vec<String>), Response> {
    let output_path = upload.temp_dir.join(format!("{}.md", upload.stem));
    let conversion = convert_to_markdown(&upload.input_path, &output_path, &upload.extension)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let metadata = tokio::fs::metadata(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if metadata.len() > MAX_KNOWLEDGE_CONTENT_BYTES as u64 {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    let content = tokio::fs::read_to_string(&output_path)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if content.len() > MAX_KNOWLEDGE_CONTENT_BYTES {
        return Err(error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            i18n::t("tool.read.too_large"),
        ));
    }
    if content.trim().is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.empty_parse_result"),
        ));
    }
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }
    tokio::fs::write(target, content)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok((conversion.converter, conversion.warnings))
}

async fn create_knowledge_temp_dir() -> Result<PathBuf, Response> {
    let mut root = std::env::temp_dir();
    root.push("wunder_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(root)
}

async fn save_knowledge_upload_content(
    mut field: axum::extract::multipart::Field<'_>,
    target: &Path,
) -> Result<(), Response> {
    if let Some(parent) = target.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let mut total = 0usize;
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        total = total.saturating_add(chunk.len());
        if total > MAX_KNOWLEDGE_UPLOAD_BYTES {
            let _ = tokio::fs::remove_file(target).await;
            return Err(error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                i18n::t("workspace.error.upload_too_large"),
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(())
}

pub(super) async fn cleanup_non_markdown_upload(root: &Path, filename: &str, output_name: &str) {
    if filename == output_name {
        return;
    }
    let raw_path = match resolve_knowledge_path(root, filename) {
        Ok(path) => path,
        Err(_) => return,
    };
    let is_markdown = raw_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);
    if is_markdown {
        return;
    }
    if raw_path.exists() && raw_path.is_file() {
        let _ = tokio::fs::remove_file(raw_path).await;
    }
}

pub(super) fn list_markdown_files(root: &Path) -> Vec<String> {
    if !root.exists() || !root.is_dir() {
        return Vec::new();
    }
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_map(|item| item.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()).unwrap_or("") != "md" {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(path);
        files.push(rel.to_string_lossy().replace('\\', "/"));
    }
    files.sort();
    files
}
