use super::{build_model_tool_success, context::ToolContext};
use crate::i18n;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) const TOOL_SESSIONS_YIELD: &str = "会话让出";
pub(crate) const TOOL_SESSIONS_YIELD_ALIAS: &str = "sessions_yield";
pub(crate) const TOOL_SESSIONS_YIELD_ALIAS_ALT: &str = "yield";

const TURN_CONTROL_META_KEY: &str = "turn_control";
const TURN_CONTROL_KIND_KEY: &str = "kind";
const TURN_CONTROL_KIND_YIELD: &str = "yield";
const TURN_CONTROL_MESSAGE_KEY: &str = "message";

#[derive(Debug, Deserialize)]
struct SessionsYieldArgs {
    #[serde(default)]
    message: Option<String>,
}

pub(crate) async fn execute_sessions_yield_tool(
    _context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: SessionsYieldArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let message = normalize_yield_message(payload.message.as_deref());
    let mut result = build_model_tool_success(
        "sessions_yield",
        "yielded",
        "Yielded the current turn and is waiting.",
        json!({
            "status": "yielded",
            "message": message,
        }),
    );
    result["meta"] = build_turn_yield_meta(&message);
    Ok(result)
}

pub(crate) fn extract_turn_yield_message(meta: Option<&Value>, data: &Value) -> Option<String> {
    let turn_control = meta?.get(TURN_CONTROL_META_KEY)?;
    let kind = turn_control
        .get(TURN_CONTROL_KIND_KEY)
        .and_then(Value::as_str)
        .map(str::trim)?;
    if !kind.eq_ignore_ascii_case(TURN_CONTROL_KIND_YIELD) {
        return None;
    }
    let message = turn_control
        .get(TURN_CONTROL_MESSAGE_KEY)
        .and_then(Value::as_str)
        .or_else(|| data.get(TURN_CONTROL_MESSAGE_KEY).and_then(Value::as_str));
    Some(normalize_yield_message(message))
}

pub(crate) fn build_turn_yield_stop_meta(message: &str) -> Value {
    json!({
        "type": TOOL_SESSIONS_YIELD_ALIAS,
        "status": "yielded",
        "message": normalize_yield_message(Some(message)),
    })
}

fn build_turn_yield_meta(message: &str) -> Value {
    json!({
        TURN_CONTROL_META_KEY: {
            TURN_CONTROL_KIND_KEY: TURN_CONTROL_KIND_YIELD,
            TURN_CONTROL_MESSAGE_KEY: normalize_yield_message(Some(message)),
        }
    })
}

fn normalize_yield_message(message: Option<&str>) -> String {
    message
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| i18n::t("response.sessions_yield_waiting"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_turn_yield_message_prefers_meta_message() {
        let meta = json!({
            "turn_control": {
                "kind": "yield",
                "message": "continue later"
            }
        });
        let data = json!({
            "status": "yielded",
            "message": "ignored"
        });
        assert_eq!(
            extract_turn_yield_message(Some(&meta), &data).as_deref(),
            Some("continue later")
        );
    }

    #[test]
    fn extract_turn_yield_message_returns_none_for_other_turn_controls() {
        let meta = json!({
            "turn_control": {
                "kind": "noop"
            }
        });
        assert_eq!(extract_turn_yield_message(Some(&meta), &json!({})), None);
    }
}
