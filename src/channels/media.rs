use crate::channels::types::{ChannelAttachment, ChannelLocation, ChannelMessage};
use crate::config::{ChannelAsrConfig, ChannelMediaConfig, ChannelTtsConfig};
use crate::schemas::AttachmentPayload;
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::multipart::{Form, Part};
use reqwest::Client;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

const DEFAULT_ASR_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_TTS_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Debug, Clone)]
pub struct MediaProcessingResult {
    pub text: String,
    pub attachments: Vec<AttachmentPayload>,
    pub meta: Value,
}

#[derive(Clone)]
pub struct MediaProcessor {
    config: ChannelMediaConfig,
    http: Client,
}

impl MediaProcessor {
    pub fn new(config: ChannelMediaConfig) -> Self {
        Self {
            config,
            http: Client::new(),
        }
    }

    pub async fn process_inbound(
        &self,
        message: &ChannelMessage,
        allow_vision: bool,
    ) -> MediaProcessingResult {
        let mut extra_lines: Vec<String> = Vec::new();
        let mut attachments: Vec<AttachmentPayload> = Vec::new();
        let mut meta = json!({});

        for attachment in &message.attachments {
            match attachment.kind.trim().to_lowercase().as_str() {
                "image" | "photo" | "picture" => {
                    if allow_vision {
                        attachments.push(AttachmentPayload {
                            name: attachment.name.clone(),
                            content: Some(attachment.url.clone()),
                            content_type: attachment.mime.clone(),
                        });
                    } else if let Some(text) = self.ocr_image(attachment).await.ok().flatten() {
                        extra_lines.push(format!("Image {}: {}", display_name(attachment), text));
                        insert_meta(&mut meta, "ocr", attachment, &text);
                    } else {
                        extra_lines.push(format!(
                            "Image {}: {}",
                            display_name(attachment),
                            attachment.url
                        ));
                    }
                }
                "audio" | "voice" => {
                    if let Some(text) = self.transcribe_audio(attachment).await.ok().flatten() {
                        extra_lines.push(format!("Audio {}: {}", display_name(attachment), text));
                        insert_meta(&mut meta, "asr", attachment, &text);
                    } else {
                        extra_lines.push(format!(
                            "Audio {}: {}",
                            display_name(attachment),
                            attachment.url
                        ));
                    }
                }
                "video" => {
                    extra_lines.push(format!(
                        "Video {}: {}",
                        display_name(attachment),
                        attachment.url
                    ));
                }
                "file" | "document" => {
                    extra_lines.push(format!(
                        "File {}: {}",
                        display_name(attachment),
                        attachment.url
                    ));
                }
                _ => {
                    extra_lines.push(format!(
                        "Attachment {}: {}",
                        display_name(attachment),
                        attachment.url
                    ));
                }
            }
        }

        if let Some(location) = message.location.as_ref() {
            if let Some(text) = self.describe_location(location).await.ok().flatten() {
                extra_lines.push(text);
            } else {
                extra_lines.push(format!("Location: {}, {}", location.lat, location.lng));
            }
        }

        let mut text = String::new();
        if !extra_lines.is_empty() {
            text.push_str(&extra_lines.join("\n"));
        }
        if let Some(body) = message
            .text
            .as_ref()
            .map(|value| value.trim())
            .filter(|v| !v.is_empty())
        {
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(body);
        }

        MediaProcessingResult {
            text,
            attachments,
            meta,
        }
    }

    pub async fn synthesize_tts(
        &self,
        text: &str,
        voice_override: Option<&str>,
    ) -> Result<Option<ChannelAttachment>> {
        let config = &self.config.tts;
        if !config.enabled {
            return Ok(None);
        }
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let max_chars = if config.max_chars == 0 {
            2000
        } else {
            config.max_chars
        };
        let clipped = if trimmed.chars().count() > max_chars {
            trimmed.chars().take(max_chars).collect::<String>()
        } else {
            trimmed.to_string()
        };
        let provider = config
            .provider
            .as_deref()
            .unwrap_or("openai")
            .trim()
            .to_ascii_lowercase();
        match provider.as_str() {
            "openai" | "openai_compatible" => {
                self.synthesize_tts_openai(config, &clipped, voice_override)
                    .await
            }
            _ => {
                self.synthesize_tts_webhook(config, &clipped, voice_override)
                    .await
            }
        }
    }

    async fn transcribe_audio(&self, attachment: &ChannelAttachment) -> Result<Option<String>> {
        let config = &self.config.asr;
        if !config.enabled {
            return Ok(None);
        }
        let provider = config
            .provider
            .as_deref()
            .unwrap_or("openai")
            .trim()
            .to_ascii_lowercase();
        match provider.as_str() {
            "openai" | "openai_compatible" => self.transcribe_openai(config, attachment).await,
            _ => self.transcribe_webhook(config, attachment).await,
        }
    }

    async fn ocr_image(&self, attachment: &ChannelAttachment) -> Result<Option<String>> {
        let config = &self.config.ocr;
        if !config.enabled {
            return Ok(None);
        }
        let endpoint = config.endpoint.as_deref().unwrap_or("").trim();
        if endpoint.is_empty() {
            return Ok(None);
        }
        let mut headers = HeaderMap::new();
        if let Some(api_key) = config
            .api_key
            .as_deref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            let value = format!("Bearer {api_key}");
            headers.insert(AUTHORIZATION, value.parse::<HeaderValue>()?);
        }
        let payload = json!({
            "url": attachment.url,
            "mime": attachment.mime,
            "size": attachment.size,
            "prompt": config.prompt,
        });
        let timeout = Duration::from_secs(config.timeout_s.max(5));
        let response = self
            .http
            .post(endpoint)
            .headers(headers)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let text = body
            .get("text")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(text)
    }

    async fn describe_location(&self, location: &ChannelLocation) -> Result<Option<String>> {
        let config = &self.config.geocode;
        if !config.enabled {
            let address = location
                .address
                .as_ref()
                .map(|value| value.trim())
                .filter(|v| !v.is_empty());
            return Ok(address.map(|value| format!("Location: {value}")));
        }
        let endpoint = config.endpoint.as_deref().unwrap_or("").trim();
        if endpoint.is_empty() {
            let address = location
                .address
                .as_ref()
                .map(|value| value.trim())
                .filter(|v| !v.is_empty());
            return Ok(address.map(|value| format!("Location: {value}")));
        }
        let mut headers = HeaderMap::new();
        if let Some(api_key) = config
            .api_key
            .as_deref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            let value = format!("Bearer {api_key}");
            headers.insert(AUTHORIZATION, value.parse::<HeaderValue>()?);
        }
        let payload = json!({
            "lat": location.lat,
            "lng": location.lng,
        });
        let timeout = Duration::from_secs(config.timeout_s.max(5));
        let response = self
            .http
            .post(endpoint)
            .headers(headers)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let address = body
            .get("address")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(address.map(|value| format!("Location: {value}")))
    }

    async fn transcribe_openai(
        &self,
        config: &ChannelAsrConfig,
        attachment: &ChannelAttachment,
    ) -> Result<Option<String>> {
        let api_key = config.api_key.as_deref().unwrap_or("").trim();
        if api_key.is_empty() {
            return Ok(None);
        }
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_ASR_BASE_URL)
            .trim()
            .trim_end_matches('/');
        let model = config.model.as_deref().unwrap_or("whisper-1").trim();
        let filename = attachment
            .name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("audio");
        let max_bytes = if config.max_bytes == 0 {
            25 * 1024 * 1024
        } else {
            config.max_bytes
        };
        let bytes = fetch_bytes(&self.http, &attachment.url, max_bytes).await?;
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {api_key}").parse::<HeaderValue>()?,
        );
        let part = Part::bytes(bytes).file_name(filename.to_string());
        let form = Form::new()
            .part("file", part)
            .text("model", model.to_string());
        let timeout = Duration::from_secs(config.timeout_s.max(10));
        let response = self
            .http
            .post(format!("{base_url}/audio/transcriptions"))
            .headers(headers)
            .timeout(timeout)
            .multipart(form)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let text = body
            .get("text")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(text)
    }

    async fn transcribe_webhook(
        &self,
        config: &ChannelAsrConfig,
        attachment: &ChannelAttachment,
    ) -> Result<Option<String>> {
        let endpoint = config.base_url.as_deref().unwrap_or("").trim();
        if endpoint.is_empty() {
            return Ok(None);
        }
        let mut headers = HeaderMap::new();
        if let Some(api_key) = config
            .api_key
            .as_deref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            headers.insert(
                AUTHORIZATION,
                format!("Bearer {api_key}").parse::<HeaderValue>()?,
            );
        }
        let payload = json!({
            "url": attachment.url,
            "mime": attachment.mime,
            "size": attachment.size,
            "name": attachment.name,
        });
        let timeout = Duration::from_secs(config.timeout_s.max(5));
        let response = self
            .http
            .post(endpoint)
            .headers(headers)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let text = body
            .get("text")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        Ok(text)
    }

    async fn synthesize_tts_openai(
        &self,
        config: &ChannelTtsConfig,
        text: &str,
        voice_override: Option<&str>,
    ) -> Result<Option<ChannelAttachment>> {
        let api_key = config.api_key.as_deref().unwrap_or("").trim();
        if api_key.is_empty() {
            return Ok(None);
        }
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_TTS_BASE_URL)
            .trim()
            .trim_end_matches('/');
        let model = config.model.as_deref().unwrap_or("gpt-4o-mini-tts").trim();
        let voice = voice_override
            .filter(|v| !v.trim().is_empty())
            .or_else(|| config.voice.as_deref().filter(|v| !v.trim().is_empty()))
            .unwrap_or("alloy");
        let format = config
            .format
            .as_deref()
            .unwrap_or("mp3")
            .trim()
            .to_lowercase();
        let payload = json!({
            "model": model,
            "input": text,
            "voice": voice,
            "response_format": format,
        });
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {api_key}").parse::<HeaderValue>()?,
        );
        let timeout = Duration::from_secs(config.timeout_s.max(10));
        let response = self
            .http
            .post(format!("{base_url}/audio/speech"))
            .headers(headers)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let bytes = response.bytes().await?;
        let data = STANDARD.encode(bytes);
        let mime = tts_mime_type(&format);
        let data_url = format!("data:{mime};base64,{data}");
        Ok(Some(ChannelAttachment {
            kind: "audio".to_string(),
            url: data_url,
            mime: Some(mime.to_string()),
            size: None,
            name: Some("tts".to_string()),
        }))
    }

    async fn synthesize_tts_webhook(
        &self,
        config: &ChannelTtsConfig,
        text: &str,
        voice_override: Option<&str>,
    ) -> Result<Option<ChannelAttachment>> {
        let endpoint = config.base_url.as_deref().unwrap_or("").trim();
        if endpoint.is_empty() {
            return Ok(None);
        }
        let mut headers = HeaderMap::new();
        if let Some(api_key) = config
            .api_key
            .as_deref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            headers.insert(
                AUTHORIZATION,
                format!("Bearer {api_key}").parse::<HeaderValue>()?,
            );
        }
        let payload = json!({
            "text": text,
            "voice": voice_override.or(config.voice.as_deref()),
            "format": config.format,
            "model": config.model,
        });
        let timeout = Duration::from_secs(config.timeout_s.max(10));
        let response = self
            .http
            .post(endpoint)
            .headers(headers)
            .timeout(timeout)
            .json(&payload)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let body: Value = response.json().await.unwrap_or(Value::Null);
        let url = body
            .get("url")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(url) = url {
            return Ok(Some(ChannelAttachment {
                kind: "audio".to_string(),
                url,
                mime: body
                    .get("mime")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string()),
                size: body.get("size").and_then(Value::as_i64),
                name: Some("tts".to_string()),
            }));
        }
        let data = body
            .get("data")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(data) = data {
            let format = body
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("mp3")
                .to_lowercase();
            let mime = tts_mime_type(&format);
            let data_url = format!("data:{mime};base64,{data}");
            return Ok(Some(ChannelAttachment {
                kind: "audio".to_string(),
                url: data_url,
                mime: Some(mime.to_string()),
                size: None,
                name: Some("tts".to_string()),
            }));
        }
        Ok(None)
    }
}

fn display_name(attachment: &ChannelAttachment) -> String {
    attachment
        .name
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("attachment")
        .to_string()
}

fn insert_meta(meta: &mut Value, key: &str, attachment: &ChannelAttachment, text: &str) {
    let Value::Object(map) = meta else {
        return;
    };
    let entry = json!({
        "name": attachment.name,
        "url": attachment.url,
        "text": text,
    });
    map.insert(key.to_string(), entry);
}

fn tts_mime_type(format: &str) -> &'static str {
    match format {
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        _ => "audio/mpeg",
    }
}

async fn fetch_bytes(http: &Client, url: &str, max_bytes: usize) -> Result<Vec<u8>> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("empty url"));
    }
    if trimmed.starts_with("data:") {
        let (_, data_part) = trimmed
            .split_once(',')
            .ok_or_else(|| anyhow!("invalid data url"))?;
        let decoded = STANDARD.decode(data_part.as_bytes())?;
        if decoded.len() > max_bytes {
            return Err(anyhow!("media exceeds max bytes"));
        }
        return Ok(decoded);
    }
    if trimmed.starts_with("file://") {
        let path = trimmed.trim_start_matches("file://");
        let bytes = tokio::fs::read(path).await?;
        if bytes.len() > max_bytes {
            return Err(anyhow!("media exceeds max bytes"));
        }
        return Ok(bytes);
    }
    if Path::new(trimmed).exists() {
        let bytes = tokio::fs::read(trimmed).await?;
        if bytes.len() > max_bytes {
            return Err(anyhow!("media exceeds max bytes"));
        }
        return Ok(bytes);
    }
    let response = http.get(trimmed).send().await?;
    if !response.status().is_success() {
        return Err(anyhow!("download failed"));
    }
    let bytes = response.bytes().await?;
    if bytes.len() > max_bytes {
        return Err(anyhow!("media exceeds max bytes"));
    }
    Ok(bytes.to_vec())
}
