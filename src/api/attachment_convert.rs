use crate::attachment::{convert_to_markdown, get_supported_extensions, sanitize_filename_stem};
use crate::i18n;
use axum::extract::Multipart;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

pub(crate) struct AttachmentConversion {
    pub(crate) name: String,
    pub(crate) content: String,
    pub(crate) converter: String,
    pub(crate) warnings: Vec<String>,
}

pub(crate) async fn convert_multipart_list(
    mut multipart: Multipart,
) -> Result<Vec<AttachmentConversion>, Response> {
    let mut conversions = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        let field_name = field.name().unwrap_or("");
        if field.file_name().is_none() && field_name != "file" {
            continue;
        }
        let conversion = convert_multipart_field(field).await?;
        conversions.push(conversion);
    }
    if conversions.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("error.file_not_found"),
        ));
    }
    Ok(conversions)
}

pub(crate) fn build_conversion_payload(conversions: Vec<AttachmentConversion>) -> Value {
    let mut conversions = conversions;
    if conversions.len() == 1 {
        let conversion = conversions.pop().unwrap();
        return conversion_to_json(conversion);
    }
    let items = conversions
        .into_iter()
        .map(conversion_to_json)
        .collect::<Vec<_>>();
    json!({ "items": items })
}

pub(crate) fn build_ok_conversion_payload(conversions: Vec<AttachmentConversion>) -> Value {
    match build_conversion_payload(conversions) {
        Value::Object(mut payload) => {
            payload.insert("ok".to_string(), Value::Bool(true));
            Value::Object(payload)
        }
        _ => json!({ "ok": true }),
    }
}

async fn convert_multipart_field(
    field: axum::extract::multipart::Field<'_>,
) -> Result<AttachmentConversion, Response> {
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
    let stem = Path::new(&filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("document");
    let stem = sanitize_filename_stem(stem);
    let stem = if stem.trim().is_empty() {
        "document".to_string()
    } else {
        stem
    };

    let temp_dir = create_temp_dir()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let input_path = temp_dir.join(format!("{stem}{extension}"));
    let output_path = temp_dir.join(format!("{stem}.md"));
    let result = async {
        save_multipart_field(field, &input_path).await?;
        let conversion = convert_to_markdown(&input_path, &output_path, &extension)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let content = tokio::fs::read_to_string(&output_path)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        if content.trim().is_empty() {
            return Err(error_response(
                StatusCode::BAD_REQUEST,
                i18n::t("error.empty_parse_result"),
            ));
        }
        Ok(AttachmentConversion {
            name: filename,
            content,
            converter: conversion.converter,
            warnings: conversion.warnings,
        })
    }
    .await;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    result
}

fn conversion_to_json(conversion: AttachmentConversion) -> Value {
    json!({
        "name": conversion.name,
        "content": conversion.content,
        "converter": conversion.converter,
        "warnings": conversion.warnings,
    })
}

async fn create_temp_dir() -> Result<PathBuf, std::io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_uploads");
    root.push(Uuid::new_v4().simple().to_string());
    tokio::fs::create_dir_all(&root).await?;
    Ok(root)
}

async fn save_multipart_field(
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
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    }
    Ok(())
}

fn error_response(status: StatusCode, message: String) -> Response {
    (
        status,
        axum::Json(json!({ "detail": { "message": message } })),
    )
        .into_response()
}
