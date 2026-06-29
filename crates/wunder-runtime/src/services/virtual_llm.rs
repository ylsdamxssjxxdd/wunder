use crate::config::{Config, LlmModelConfig, VirtualLlmLogConfig};
use crate::core::blocking;
use crate::schemas::TokenUsage;
use anyhow::{anyhow, Context, Result};
use axum::extract::Multipart;
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

pub const VIRTUAL_REPLAY_PROVIDER: &str = "virtual_replay";
const MAX_VIRTUAL_LLM_JSONL_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_TOKEN_DELAY_MS: u64 = 18;
const MAX_TOKEN_DELAY_MS: u64 = 500;
const DEFAULT_MODEL_NAME: &str = "default";
const RANDOM_REPLAY_LOG_ID: &str = "virtual_random";
const RANDOM_REPLAY_FORMAT: &str = "virtual_random";
const RANDOM_REPLY_KEYS: &[&str] = &[
    "virtual_llm.random.reply.ready",
    "virtual_llm.random.reply.processing",
    "virtual_llm.random.reply.received",
    "virtual_llm.random.reply.placeholder",
    "virtual_llm.random.reply.no_log",
];
const SIMPLE_REPLAY_FORMAT: &str = "simple_dialogue";
const WUNDER_REPLAY_FORMAT: &str = "wunder_session_export";

#[derive(Debug, Clone)]
pub struct VirtualReplayTurn {
    pub content: String,
    pub reasoning: String,
    pub usage: Option<TokenUsage>,
    pub tool_calls: Option<Value>,
    pub source_log_id: String,
    pub source_log_name: String,
    pub source_round: usize,
    pub source_model_round: Option<usize>,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VirtualLlmLogSummary {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub format: String,
    pub user_rounds: usize,
    pub size_bytes: u64,
    pub uploaded_at: String,
}

#[derive(Debug, Clone)]
pub struct VirtualLlmUpload {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone)]
struct ParsedVirtualLog {
    format: String,
    turns: Vec<VirtualReplayTurn>,
}

pub fn normalize_virtual_provider(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim();
    if raw.is_empty() {
        return String::new();
    }
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "virtual" | "virtual_llm" | "virtual_model" | "replay" | "jsonl_replay" | "mock_replay" => {
            VIRTUAL_REPLAY_PROVIDER.to_string()
        }
        other => other.to_string(),
    }
}

pub fn is_virtual_replay_provider(value: Option<&str>) -> bool {
    normalize_virtual_provider(value) == VIRTUAL_REPLAY_PROVIDER
}

pub fn resolve_virtual_model_id(config: &LlmModelConfig) -> String {
    config
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_MODEL_NAME)
        .to_string()
}

pub fn virtual_model_configured(_app_config: &Config, model: &LlmModelConfig) -> bool {
    is_virtual_replay_provider(model.provider.as_deref())
}

pub async fn list_logs(config: Config) -> Result<Vec<VirtualLlmLogSummary>> {
    blocking::run_fs("virtual_llm.list_logs", move || {
        Ok(config
            .llm
            .virtual_replay
            .enabled_logs
            .iter()
            .map(|log| VirtualLlmLogSummary {
                id: log.id.clone(),
                name: log.name.clone(),
                enabled: log.enabled,
                format: log.format.clone(),
                user_rounds: log.user_rounds,
                size_bytes: log.size_bytes,
                uploaded_at: log.uploaded_at.clone(),
            })
            .collect())
    })
    .await
}

pub async fn parse_upload_multipart(mut multipart: Multipart) -> Result<VirtualLlmUpload> {
    let mut name = String::new();
    let mut content = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .context("read virtual llm multipart field")?
    {
        let field_name = field.name().unwrap_or_default().to_string();
        if field_name == "name" {
            name = field.text().await.unwrap_or_default();
            continue;
        }
        if field_name != "file" {
            continue;
        }
        if name.trim().is_empty() {
            if let Some(file_name) = field.file_name() {
                name = file_name.to_string();
            }
        }
        while let Some(chunk) = field
            .chunk()
            .await
            .context("read virtual llm upload chunk")?
        {
            if content.len().saturating_add(chunk.len()) as u64 > MAX_VIRTUAL_LLM_JSONL_BYTES {
                return Err(anyhow!("virtual llm jsonl is too large"));
            }
            content.extend_from_slice(&chunk);
        }
    }
    if content.is_empty() {
        return Err(anyhow!("virtual llm jsonl file is required"));
    }
    let content = String::from_utf8(content).context("virtual llm jsonl must be UTF-8")?;
    let name = sanitize_log_name(&name);
    Ok(VirtualLlmUpload { name, content })
}

pub async fn store_uploaded_log(
    config: Config,
    upload: VirtualLlmUpload,
) -> Result<(Config, VirtualLlmLogSummary)> {
    blocking::run_fs("virtual_llm.store_upload", move || {
        let parsed = parse_virtual_log(&upload.content, "__pending__", &upload.name)?;
        if parsed.turns.is_empty() {
            return Err(anyhow!(
                "virtual llm jsonl contains no assistant replay turns"
            ));
        }
        let logs_root = resolve_logs_root_from_config(&config)?;
        fs::create_dir_all(&logs_root)
            .with_context(|| format!("create virtual llm root failed: {}", logs_root.display()))?;
        let hash = Sha256::digest(upload.content.as_bytes());
        let id = format!("replay_{}", hex::encode(&hash[..8]));
        let file = format!("{id}.jsonl");
        let target = logs_root.join(&file);
        fs::write(&target, upload.content.as_bytes())
            .with_context(|| format!("write virtual llm log failed: {}", target.display()))?;
        let mut updated = config;
        updated
            .llm
            .virtual_replay
            .enabled_logs
            .retain(|log| log.id != id);
        let entry = VirtualLlmLogConfig {
            id: id.clone(),
            name: upload.name,
            file,
            enabled: true,
            format: parsed.format,
            user_rounds: count_virtual_user_rounds(&parsed.turns),
            size_bytes: upload.content.len() as u64,
            uploaded_at: Utc::now().to_rfc3339(),
        };
        updated.llm.virtual_replay.enabled_logs.push(entry.clone());
        let summary = VirtualLlmLogSummary {
            id: entry.id,
            name: entry.name,
            enabled: entry.enabled,
            format: entry.format,
            user_rounds: entry.user_rounds,
            size_bytes: entry.size_bytes,
            uploaded_at: entry.uploaded_at,
        };
        Ok((updated, summary))
    })
    .await
}

pub async fn delete_log(config: Config, log_id: String) -> Result<Config> {
    blocking::run_fs("virtual_llm.delete_log", move || {
        let cleaned_id = normalize_log_id(&log_id)?;
        let logs_root = resolve_logs_root_from_config(&config)?;
        let mut updated = config;
        let mut removed_files = Vec::new();
        updated.llm.virtual_replay.enabled_logs.retain(|log| {
            if log.id == cleaned_id {
                removed_files.push(log.file.clone());
                false
            } else {
                true
            }
        });
        for file in removed_files {
            if let Ok(target) = resolve_log_file(&logs_root, &file) {
                match fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => {
                        return Err(anyhow!(
                            "remove virtual llm log failed {}: {err}",
                            target.display()
                        ));
                    }
                }
            }
        }
        Ok(updated)
    })
    .await
}

pub async fn set_log_enabled(config: Config, log_id: String, enabled: bool) -> Result<Config> {
    blocking::run_fs("virtual_llm.set_log_enabled", move || {
        let cleaned_id = normalize_log_id(&log_id)?;
        let mut updated = config;
        let mut found = false;
        for log in &mut updated.llm.virtual_replay.enabled_logs {
            if log.id == cleaned_id {
                log.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Err(anyhow!("virtual llm log not found"));
        }
        Ok(updated)
    })
    .await
}

pub async fn load_turn_for_round(
    config: Config,
    model: &LlmModelConfig,
    user_round: Option<i64>,
    model_round: Option<i64>,
) -> Result<VirtualReplayTurn> {
    let target_log_id = resolve_virtual_model_id(model);
    let round = user_round.unwrap_or(1).max(1) as usize;
    let model_round = model_round
        .filter(|value| *value > 0)
        .map(|value| value as usize);
    blocking::run_fs("virtual_llm.load_turn", move || {
        let log = config
            .llm
            .virtual_replay
            .enabled_logs
            .iter()
            .find(|log| log.enabled && log.id == target_log_id)
            .cloned();
        let Some(log) = log else {
            return Ok(random_virtual_turn(round, model_round));
        };
        let logs_root = resolve_logs_root_from_config(&config)?;
        let path = resolve_log_file(&logs_root, &log.file)?;
        let text = fs::read_to_string(&path)
            .with_context(|| format!("read virtual llm log failed: {}", path.display()))?;
        let parsed = parse_virtual_log(&text, &log.id, &log.name)?;
        if parsed.turns.is_empty() {
            return Err(anyhow!("virtual llm log contains no replay turns"));
        }
        if let Some(model_round) = model_round {
            let same_user_round = parsed
                .turns
                .iter()
                .filter(|turn| turn.source_round == round)
                .collect::<Vec<_>>();
            if let Some(turn) = same_user_round
                .iter()
                .find(|turn| turn.source_model_round == Some(model_round))
            {
                return Ok((*turn).clone());
            }
            if !same_user_round.is_empty() {
                let index = model_round.saturating_sub(1) % same_user_round.len();
                return Ok(same_user_round[index].clone());
            }
        }
        if let Some(turn) = parsed.turns.iter().find(|turn| turn.source_round == round) {
            return Ok(turn.clone());
        }
        let index = round.saturating_sub(1) % parsed.turns.len();
        parsed
            .turns
            .get(index)
            .cloned()
            .ok_or_else(|| anyhow!("virtual llm replay round not found"))
    })
    .await
}

fn random_virtual_turn(round: usize, model_round: Option<usize>) -> VirtualReplayTurn {
    let random_index = (Uuid::new_v4().as_u128() as usize) % RANDOM_REPLY_KEYS.len();
    let content = crate::i18n::t(RANDOM_REPLY_KEYS[random_index]);
    VirtualReplayTurn {
        content,
        reasoning: String::new(),
        usage: None,
        tool_calls: None,
        source_log_id: RANDOM_REPLAY_LOG_ID.to_string(),
        source_log_name: crate::i18n::t("virtual_llm.random.log_name"),
        source_round: round,
        source_model_round: model_round.or(Some(1)),
        format: RANDOM_REPLAY_FORMAT.to_string(),
    }
}

pub async fn emit_virtual_deltas<F, Fut>(
    turn: &VirtualReplayTurn,
    stream: bool,
    token_delay_ms: Option<u64>,
    mut on_delta: F,
) -> Result<()>
where
    F: FnMut(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    if !stream {
        return Ok(());
    }
    let delay_ms = token_delay_ms
        .unwrap_or(DEFAULT_TOKEN_DELAY_MS)
        .min(MAX_TOKEN_DELAY_MS);
    let content_tokens = tokenize_for_stream(&turn.content);
    for token in content_tokens {
        on_delta(token, String::new()).await?;
        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }
    let reasoning_tokens = tokenize_for_stream(&turn.reasoning);
    for token in reasoning_tokens {
        on_delta(String::new(), token).await?;
        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }
    Ok(())
}

pub fn estimate_virtual_usage(input_messages: &[Value], turn: &VirtualReplayTurn) -> TokenUsage {
    turn.usage.clone().unwrap_or_else(|| {
        let input = approx_value_tokens(&Value::Array(input_messages.to_vec()));
        let output = approx_text_tokens(&turn.content);
        let reasoning = approx_text_tokens(&turn.reasoning);
        TokenUsage {
            input,
            output,
            total: input.saturating_add(output).saturating_add(reasoning),
        }
    })
}

pub fn build_virtual_request_meta(turn: &VirtualReplayTurn) -> Value {
    json!({
        "virtual_replay": true,
        "source_log_id": turn.source_log_id,
        "source_log_name": turn.source_log_name,
        "source_round": turn.source_round,
        "source_model_round": turn.source_model_round,
        "source_format": turn.format,
    })
}

fn parse_virtual_log(text: &str, log_id: &str, log_name: &str) -> Result<ParsedVirtualLog> {
    let mut wunder_turns = Vec::new();
    let mut simple_turns = Vec::new();
    let mut pending_simple_user = false;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line)
            .with_context(|| format!("parse virtual llm jsonl line {}", index + 1))?;
        if let Some(turn) = parse_wunder_line(&value, log_id, log_name) {
            wunder_turns.push(turn);
            continue;
        }
        if let Some(role) = value.get("role").and_then(Value::as_str) {
            match role.trim().to_ascii_lowercase().as_str() {
                "user" => {
                    pending_simple_user = true;
                }
                "assistant" if pending_simple_user => {
                    if let Some(content) =
                        extract_text_field(&value, &["content", "text", "message"])
                    {
                        simple_turns.push(VirtualReplayTurn {
                            content,
                            reasoning: extract_text_field(
                                &value,
                                &["reasoning", "reasoning_content"],
                            )
                            .unwrap_or_default(),
                            usage: parse_usage(value.get("usage")),
                            tool_calls: value.get("tool_calls").cloned(),
                            source_log_id: log_id.to_string(),
                            source_log_name: log_name.to_string(),
                            source_round: simple_turns.len() + 1,
                            source_model_round: Some(1),
                            format: SIMPLE_REPLAY_FORMAT.to_string(),
                        });
                    }
                    pending_simple_user = false;
                }
                _ => {}
            }
        }
    }
    if !wunder_turns.is_empty() {
        wunder_turns.sort_by_key(|turn| (turn.source_round, turn.source_model_round.unwrap_or(1)));
        return Ok(ParsedVirtualLog {
            format: WUNDER_REPLAY_FORMAT.to_string(),
            turns: wunder_turns,
        });
    }
    Ok(ParsedVirtualLog {
        format: SIMPLE_REPLAY_FORMAT.to_string(),
        turns: simple_turns,
    })
}

fn parse_wunder_line(value: &Value, log_id: &str, log_name: &str) -> Option<VirtualReplayTurn> {
    let event = value
        .get("event")
        .or_else(|| value.get("type"))
        .and_then(Value::as_str)?
        .trim()
        .to_ascii_lowercase();
    if event != "llm_output" {
        return None;
    }
    let data = unwrap_data(value.get("data").unwrap_or(value));
    let content = extract_text_field(data, &["content", "message", "text"]).unwrap_or_default();
    let reasoning =
        extract_text_field(data, &["reasoning", "reasoning_content"]).unwrap_or_default();
    let tool_calls = data.get("tool_calls").cloned().filter(non_empty_tool_calls);
    if content.trim().is_empty() && reasoning.trim().is_empty() && tool_calls.is_none() {
        return None;
    }
    let round = data
        .get("user_round")
        .or_else(|| value.get("round"))
        .or_else(|| value.get("user_round"))
        .and_then(|item| item.as_u64().or_else(|| item.as_str()?.parse::<u64>().ok()))
        .unwrap_or(1)
        .max(1) as usize;
    Some(VirtualReplayTurn {
        content,
        reasoning,
        usage: parse_usage(data.get("usage")),
        tool_calls,
        source_log_id: log_id.to_string(),
        source_log_name: log_name.to_string(),
        source_round: round,
        source_model_round: data
            .get("model_round")
            .or_else(|| value.get("model_round"))
            .and_then(|item| item.as_u64().or_else(|| item.as_str()?.parse::<u64>().ok()))
            .filter(|value| *value > 0)
            .map(|value| value as usize),
        format: WUNDER_REPLAY_FORMAT.to_string(),
    })
}

fn count_virtual_user_rounds(turns: &[VirtualReplayTurn]) -> usize {
    let mut rounds = std::collections::BTreeSet::new();
    for turn in turns {
        rounds.insert(turn.source_round);
    }
    rounds.len()
}

fn unwrap_data(value: &Value) -> &Value {
    let mut current = value;
    for _ in 0..2 {
        let Some(map) = current.as_object() else {
            return current;
        };
        let Some(next) = map.get("data") else {
            return current;
        };
        if next.is_object() {
            current = next;
        } else {
            return current;
        }
    }
    current
}

fn extract_text_field(value: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        let Some(item) = value.get(*key) else {
            continue;
        };
        if let Some(text) = item.as_str() {
            let cleaned = text.to_string();
            if !cleaned.trim().is_empty() {
                return Some(cleaned);
            }
        }
        if let Some(parts) = item.as_array() {
            let text = parts
                .iter()
                .filter_map(extract_part_text)
                .collect::<Vec<_>>()
                .join("");
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }
    None
}

fn extract_part_text(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    let map = value.as_object()?;
    for key in ["text", "content", "value"] {
        if let Some(text) = map.get(key).and_then(Value::as_str) {
            return Some(text.to_string());
        }
    }
    None
}

fn non_empty_tool_calls(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
        Value::String(text) => !text.trim().is_empty(),
        _ => true,
    }
}

fn parse_usage(value: Option<&Value>) -> Option<TokenUsage> {
    let usage = value?;
    let input = read_usage_u64(usage, &["input", "input_tokens", "prompt_tokens"]);
    let output = read_usage_u64(usage, &["output", "output_tokens", "completion_tokens"]);
    let total = read_usage_u64(usage, &["total", "total_tokens"]);
    let resolved_total =
        total.unwrap_or_else(|| input.unwrap_or(0).saturating_add(output.unwrap_or(0)));
    if input.unwrap_or(0) == 0 && output.unwrap_or(0) == 0 && resolved_total == 0 {
        return None;
    }
    Some(TokenUsage {
        input: input.unwrap_or(0),
        output: output.unwrap_or(0),
        total: resolved_total,
    })
}

fn read_usage_u64(value: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        let Some(item) = value.get(*key) else {
            continue;
        };
        if let Some(number) = item.as_u64() {
            return Some(number);
        }
        if let Some(text) = item.as_str() {
            if let Ok(number) = text.trim().parse::<u64>() {
                return Some(number);
            }
        }
    }
    None
}

fn tokenize_for_stream(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        current.push(ch);
        if ch.is_whitespace() || is_cjk_char(ch) || ch.is_ascii_punctuation() {
            output.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        output.push(current);
    }
    output
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF
    )
}

fn approx_value_tokens(value: &Value) -> u64 {
    serde_json::to_string(value)
        .map(|text| approx_text_tokens(&text))
        .unwrap_or(0)
}

fn approx_text_tokens(text: &str) -> u64 {
    if text.trim().is_empty() {
        return 0;
    }
    let chars = text.chars().count() as u64;
    chars.saturating_add(3) / 4
}

fn sanitize_log_name(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return "virtual-replay.jsonl".to_string();
    }
    cleaned
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn normalize_log_id(raw: &str) -> Result<String> {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("virtual llm log id is required"));
    }
    if cleaned
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        Ok(cleaned.to_string())
    } else {
        Err(anyhow!("virtual llm log id is invalid"))
    }
}

fn resolve_logs_root_from_config(config: &Config) -> Result<PathBuf> {
    let raw = config.llm.virtual_replay.logs_root.trim();
    let path = if raw.is_empty() {
        PathBuf::from("./config/data/virtual_llm_logs")
    } else {
        PathBuf::from(raw)
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn resolve_log_file(root: &Path, file: &str) -> Result<PathBuf> {
    let cleaned = file.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("virtual llm log file is empty"));
    }
    let path = PathBuf::from(cleaned);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!("virtual llm log file path is invalid"));
    }
    Ok(root.join(path))
}

pub fn logs_payload(logs: Vec<VirtualLlmLogSummary>) -> Value {
    Value::Array(
        logs.into_iter()
            .map(|log| {
                let mut map = Map::new();
                map.insert("id".to_string(), Value::String(log.id));
                map.insert("name".to_string(), Value::String(log.name));
                map.insert("enabled".to_string(), Value::Bool(log.enabled));
                map.insert("format".to_string(), Value::String(log.format));
                map.insert("user_rounds".to_string(), json!(log.user_rounds));
                map.insert("size_bytes".to_string(), json!(log.size_bytes));
                map.insert("uploaded_at".to_string(), Value::String(log.uploaded_at));
                Value::Object(map)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wunder_session_export_by_user_round() {
        let text = r#"{"record_type":"event","round":1,"event":"llm_output","data":{"content":"A","usage":{"input":4,"output":1,"total":5}}}
{"record_type":"event","round":2,"event":"llm_output","data":{"content":"B"}}"#;

        let parsed = parse_virtual_log(text, "log_1", "sample").expect("parse");

        assert_eq!(parsed.format, WUNDER_REPLAY_FORMAT);
        assert_eq!(parsed.turns.len(), 2);
        assert_eq!(parsed.turns[0].source_round, 1);
        assert_eq!(parsed.turns[0].content, "A");
        let usage = parsed.turns[0].usage.as_ref().expect("usage");
        assert_eq!(usage.input, 4);
        assert_eq!(usage.output, 1);
        assert_eq!(usage.total, 5);
    }

    #[test]
    fn parses_simple_dialogue_pairs() {
        let text = r#"{"role":"user","content":"hello"}
{"role":"assistant","content":"world","reasoning":"thinking"}"#;

        let parsed = parse_virtual_log(text, "log_1", "sample").expect("parse");

        assert_eq!(parsed.format, SIMPLE_REPLAY_FORMAT);
        assert_eq!(parsed.turns.len(), 1);
        assert_eq!(parsed.turns[0].content, "world");
        assert_eq!(parsed.turns[0].reasoning, "thinking");
    }

    #[test]
    fn parses_wunder_model_round_for_replay_selection() {
        let text = r#"{"event":"llm_output","data":{"user_round":1,"model_round":2,"content":"B"}}
{"event":"llm_output","data":{"user_round":1,"model_round":1,"content":"A"}}"#;

        let parsed = parse_virtual_log(text, "log_1", "sample").expect("parse");

        assert_eq!(parsed.turns.len(), 2);
        assert_eq!(parsed.turns[0].content, "A");
        assert_eq!(parsed.turns[0].source_model_round, Some(1));
        assert_eq!(parsed.turns[1].content, "B");
        assert_eq!(parsed.turns[1].source_model_round, Some(2));
    }

    #[test]
    fn random_virtual_turn_marks_virtual_random_source() {
        let turn = random_virtual_turn(3, Some(2));

        assert!(!turn.content.trim().is_empty());
        assert_eq!(turn.source_log_id, RANDOM_REPLAY_LOG_ID);
        assert_eq!(turn.source_round, 3);
        assert_eq!(turn.source_model_round, Some(2));
        assert_eq!(turn.format, RANDOM_REPLAY_FORMAT);
        assert!(turn.tool_calls.is_none());
    }

    #[tokio::test]
    async fn load_turn_returns_random_when_no_matching_log_exists() {
        let config = Config::default();
        let model = LlmModelConfig {
            provider: Some(VIRTUAL_REPLAY_PROVIDER.to_string()),
            model: None,
            ..Default::default()
        };

        let turn = load_turn_for_round(config, &model, Some(2), Some(3))
            .await
            .expect("random virtual turn");

        assert!(!turn.content.trim().is_empty());
        assert_eq!(turn.source_log_id, RANDOM_REPLAY_LOG_ID);
        assert_eq!(
            turn.source_log_name,
            crate::i18n::t("virtual_llm.random.log_name")
        );
        assert_eq!(turn.source_round, 2);
        assert_eq!(turn.source_model_round, Some(3));
        assert_eq!(turn.format, RANDOM_REPLAY_FORMAT);
    }

    #[test]
    fn tokenizes_ascii_text_for_streaming() {
        assert_eq!(
            tokenize_for_stream("hi, ok"),
            vec!["hi,".to_string(), " ".to_string(), "ok".to_string()]
        );
    }
}
