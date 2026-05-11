use crate::config::Config;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub const TOKEN_KIND_FILE: &str = "file";
pub const TOKEN_KIND_CALLBACK: &str = "callback";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlyOfficeAccessToken {
    pub kind: String,
    pub user_id: String,
    pub workspace_id: String,
    pub path: String,
    pub exp: u64,
}

#[derive(Debug, Clone)]
pub struct OnlyOfficeResolvedConfig {
    pub api_url: String,
    pub public_base_url: String,
    pub jwt_secret: Option<String>,
    pub token_ttl_s: u64,
    pub request_timeout_s: u64,
    pub max_download_bytes: usize,
}

pub fn resolve_config(
    config: &Config,
    request_base_url: Option<&str>,
) -> Option<OnlyOfficeResolvedConfig> {
    let office = &config.onlyoffice;
    if !office.enabled {
        return None;
    }
    let api_url = office.api_url()?;
    let public_base_url = office
        .public_base_url()
        .or_else(|| request_base_url.map(str::to_string))?;
    Some(OnlyOfficeResolvedConfig {
        api_url,
        public_base_url: public_base_url.trim_end_matches('/').to_string(),
        jwt_secret: office.jwt_secret(),
        token_ttl_s: office.token_ttl_s(),
        request_timeout_s: office.request_timeout_s(),
        max_download_bytes: office.max_download_bytes(),
    })
}

pub fn is_supported_extension(extension: &str) -> bool {
    matches!(
        extension.trim().to_ascii_lowercase().as_str(),
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx"
    )
}

pub fn document_type(extension: &str) -> Option<&'static str> {
    match extension.trim().to_ascii_lowercase().as_str() {
        "doc" | "docx" => Some("word"),
        "xls" | "xlsx" => Some("cell"),
        "ppt" | "pptx" => Some("slide"),
        _ => None,
    }
}

pub fn file_type(extension: &str) -> Option<&'static str> {
    match extension.trim().to_ascii_lowercase().as_str() {
        "doc" => Some("doc"),
        "docx" => Some("docx"),
        "xls" => Some("xls"),
        "xlsx" => Some("xlsx"),
        "ppt" => Some("ppt"),
        "pptx" => Some("pptx"),
        _ => None,
    }
}

pub fn content_type(extension: &str) -> &'static str {
    match extension.trim().to_ascii_lowercase().as_str() {
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
}

pub fn sign_access_token(
    secret: &str,
    kind: &str,
    user_id: &str,
    workspace_id: &str,
    path: &str,
    ttl_s: u64,
) -> anyhow::Result<String> {
    let payload = OnlyOfficeAccessToken {
        kind: kind.to_string(),
        user_id: user_id.to_string(),
        workspace_id: workspace_id.to_string(),
        path: path.to_string(),
        exp: now_s().saturating_add(ttl_s),
    };
    sign_json(secret, &serde_json::to_value(payload)?)
}

pub fn verify_access_token(
    secret: &str,
    token: &str,
    expected_kind: &str,
) -> anyhow::Result<OnlyOfficeAccessToken> {
    let payload = verify_json(secret, token)?;
    let parsed: OnlyOfficeAccessToken = serde_json::from_value(payload)?;
    if parsed.kind != expected_kind {
        return Err(anyhow::anyhow!("invalid token kind"));
    }
    if parsed.exp <= now_s() {
        return Err(anyhow::anyhow!("token expired"));
    }
    if parsed.user_id.trim().is_empty()
        || parsed.workspace_id.trim().is_empty()
        || parsed.path.trim().is_empty()
    {
        return Err(anyhow::anyhow!("invalid token payload"));
    }
    Ok(parsed)
}

pub fn sign_editor_config(secret: &str, payload: &Value) -> anyhow::Result<String> {
    sign_json(secret, payload)
}

pub fn public_file_url(base_url: &str, token: &str) -> String {
    format!(
        "{}/wunder/workspace/onlyoffice/file?token={}",
        base_url.trim_end_matches('/'),
        percent_encode(token)
    )
}

pub fn public_callback_url(base_url: &str, token: &str) -> String {
    format!(
        "{}/wunder/workspace/onlyoffice/callback?token={}",
        base_url.trim_end_matches('/'),
        percent_encode(token)
    )
}

pub fn build_document_key(
    workspace_id: &str,
    path: &str,
    size: u64,
    updated_epoch_ms: u128,
) -> String {
    let raw = format!("{workspace_id}:{path}:{size}:{updated_epoch_ms}");
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)
}

pub fn build_editor_config(
    resolved: &OnlyOfficeResolvedConfig,
    user_id: &str,
    workspace_id: &str,
    path: &str,
    filename: &str,
    extension: &str,
    size: u64,
    updated_epoch_ms: u128,
    language: &str,
) -> anyhow::Result<Value> {
    let Some(secret) = resolved
        .jwt_secret
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Err(anyhow::anyhow!("OnlyOffice JWT secret is required"));
    };
    let file_token = sign_access_token(
        secret,
        TOKEN_KIND_FILE,
        user_id,
        workspace_id,
        path,
        resolved.token_ttl_s,
    )?;
    let callback_token = sign_access_token(
        secret,
        TOKEN_KIND_CALLBACK,
        user_id,
        workspace_id,
        path,
        resolved.token_ttl_s,
    )?;
    let file_type = file_type(extension).ok_or_else(|| anyhow::anyhow!("unsupported file type"))?;
    let document_type =
        document_type(extension).ok_or_else(|| anyhow::anyhow!("unsupported file type"))?;
    let mut editor_config = json!({
        "document": {
            "fileType": file_type,
            "key": build_document_key(workspace_id, path, size, updated_epoch_ms),
            "title": filename,
            "url": public_file_url(&resolved.public_base_url, &file_token),
            "permissions": {
                "download": true,
                "edit": true,
                "print": true
            }
        },
        "documentType": document_type,
        "editorConfig": {
            "callbackUrl": public_callback_url(&resolved.public_base_url, &callback_token),
            "lang": normalize_language(language),
            "mode": "edit",
            "user": {
                "id": user_id,
                "name": user_id
            }
        },
        "height": "100%",
        "type": "desktop",
        "width": "100%"
    });
    let token = sign_editor_config(secret, &editor_config)?;
    if let Some(map) = editor_config.as_object_mut() {
        map.insert("token".to_string(), Value::String(token));
    }
    Ok(editor_config)
}

fn sign_json(secret: &str, payload: &Value) -> anyhow::Result<String> {
    let header = json!({ "alg": "HS256", "typ": "JWT" });
    let header_segment = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let payload_segment = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload)?);
    let signing_input = format!("{header_segment}.{payload_segment}");
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(signing_input.as_bytes());
    let signature = mac.finalize().into_bytes();
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn verify_json(secret: &str, token: &str) -> anyhow::Result<Value> {
    let mut parts = token.trim().split('.');
    let Some(header_segment) = parts.next() else {
        return Err(anyhow::anyhow!("invalid token"));
    };
    let Some(payload_segment) = parts.next() else {
        return Err(anyhow::anyhow!("invalid token"));
    };
    let Some(signature_segment) = parts.next() else {
        return Err(anyhow::anyhow!("invalid token"));
    };
    if parts.next().is_some() {
        return Err(anyhow::anyhow!("invalid token"));
    }
    let signing_input = format!("{header_segment}.{payload_segment}");
    let signature = URL_SAFE_NO_PAD.decode(signature_segment)?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(signing_input.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| anyhow::anyhow!("invalid token signature"))?;
    let header_bytes = URL_SAFE_NO_PAD.decode(header_segment)?;
    let header: Value = serde_json::from_slice(&header_bytes)?;
    if !header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .eq_ignore_ascii_case("HS256")
    {
        return Err(anyhow::anyhow!("unsupported token algorithm"));
    }
    let payload_bytes = URL_SAFE_NO_PAD.decode(payload_segment)?;
    Ok(serde_json::from_slice(&payload_bytes)?)
}

fn normalize_language(language: &str) -> &'static str {
    if language.trim().to_ascii_lowercase().starts_with("zh") {
        "zh-CN"
    } else {
        "en"
    }
}

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{
        build_document_key, content_type, document_type, is_supported_extension, sign_access_token,
        verify_access_token, TOKEN_KIND_FILE,
    };

    #[test]
    fn maps_supported_office_extensions() {
        assert!(is_supported_extension("docx"));
        assert!(is_supported_extension("XLSX"));
        assert_eq!(document_type("pptx"), Some("slide"));
        assert_eq!(
            content_type("docx"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        assert!(!is_supported_extension("pdf"));
    }

    #[test]
    fn access_token_roundtrips() {
        let token = sign_access_token("secret", TOKEN_KIND_FILE, "u", "u__c__1", "docs/a.docx", 60)
            .expect("token");
        let parsed = verify_access_token("secret", &token, TOKEN_KIND_FILE).expect("parse token");
        assert_eq!(parsed.path, "docs/a.docx");
        assert_eq!(parsed.workspace_id, "u__c__1");
    }

    #[test]
    fn document_key_changes_when_file_changes() {
        let first = build_document_key("u__c__1", "a.docx", 10, 1000);
        let second = build_document_key("u__c__1", "a.docx", 11, 1000);
        assert_ne!(first, second);
    }
}
