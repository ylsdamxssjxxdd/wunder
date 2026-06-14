use super::{build_model_tool_success, ToolContext};
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;
use tokio::time::{sleep, Duration};

const MAX_SLEEP_SECONDS: f64 = 3600.0;

pub const TOOL_SLEEP_WAIT: &str = "\u{4f11}\u{7720}\u{7b49}\u{5f85}";
pub const TOOL_SLEEP_ALIAS: &str = "sleep";
pub const TOOL_SLEEP_WAIT_ALIAS: &str = "sleep_wait";
pub const TOOL_SLEEP_PAUSE_ALIAS: &str = "pause";

#[derive(Debug, Deserialize)]
struct SleepArgs {
    #[serde(default)]
    seconds: Option<f64>,
    #[serde(default)]
    duration_s: Option<f64>,
    #[serde(default)]
    wait_s: Option<f64>,
    #[serde(default)]
    reason: Option<String>,
}

pub fn is_sleep_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned == TOOL_SLEEP_WAIT {
        return true;
    }
    matches!(
        cleaned.to_ascii_lowercase().as_str(),
        TOOL_SLEEP_ALIAS | TOOL_SLEEP_WAIT_ALIAS | TOOL_SLEEP_PAUSE_ALIAS
    )
}

pub fn extract_sleep_seconds(args: &Value) -> Option<f64> {
    let obj = args.as_object()?;
    for key in ["seconds", "duration_s", "wait_s"] {
        let Some(value) = obj.get(key) else {
            continue;
        };
        let seconds = if let Some(number) = value.as_f64() {
            Some(number)
        } else {
            value
                .as_str()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .and_then(|text| text.parse::<f64>().ok())
        };
        let Some(seconds) = seconds else {
            continue;
        };
        if seconds.is_finite() && seconds > 0.0 {
            return Some(seconds);
        }
    }
    None
}

pub async fn tool_sleep_wait(_context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SleepArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let seconds = resolve_sleep_seconds(&payload)?;
    let started_at = Instant::now();
    sleep(Duration::from_secs_f64(seconds)).await;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    Ok(build_model_tool_success(
        "sleep",
        "completed",
        format!("Slept for {seconds} seconds."),
        json!({
            "requested_seconds": seconds,
            "elapsed_ms": elapsed_ms,
            "reason": normalize_reason(payload.reason.as_deref()),
        }),
    ))
}

fn resolve_sleep_seconds(payload: &SleepArgs) -> Result<f64> {
    let seconds = payload
        .seconds
        .or(payload.duration_s)
        .or(payload.wait_s)
        .unwrap_or(0.0);
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err(anyhow!(crate::i18n::t("tool.sleep.invalid_seconds")));
    }
    if seconds > MAX_SLEEP_SECONDS {
        return Err(anyhow!(crate::i18n::t("tool.sleep.seconds_too_large")));
    }
    Ok(seconds)
}

fn normalize_reason(reason: Option<&str>) -> Option<String> {
    reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleep_tool_name_supports_aliases() {
        assert!(is_sleep_tool_name(TOOL_SLEEP_WAIT));
        assert!(is_sleep_tool_name(TOOL_SLEEP_ALIAS));
        assert!(is_sleep_tool_name(TOOL_SLEEP_WAIT_ALIAS));
        assert!(is_sleep_tool_name(TOOL_SLEEP_PAUSE_ALIAS));
        assert!(!is_sleep_tool_name("wait"));
    }

    #[test]
    fn extract_sleep_seconds_accepts_duration_aliases() {
        assert_eq!(extract_sleep_seconds(&json!({ "seconds": 2.5 })), Some(2.5));
        assert_eq!(
            extract_sleep_seconds(&json!({ "duration_s": "3" })),
            Some(3.0)
        );
        assert_eq!(extract_sleep_seconds(&json!({ "wait_s": 4 })), Some(4.0));
    }
}
