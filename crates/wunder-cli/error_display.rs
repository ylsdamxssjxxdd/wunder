use serde_json::Value;

pub(crate) fn format_error_message(payload: &Value) -> Option<String> {
    let base_message = payload
        .as_str()
        .or_else(|| payload.get("message").and_then(Value::as_str))
        .or_else(|| payload.get("detail").and_then(Value::as_str))
        .or_else(|| payload.get("error").and_then(Value::as_str))
        .or_else(|| {
            payload
                .get("data")
                .and_then(Value::as_object)
                .and_then(|inner| inner.get("message"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let detail_message = payload
        .get("detail")
        .and_then(format_error_detail_message)
        .filter(|value| !value.is_empty());

    match (base_message, detail_message) {
        (Some(message), Some(detail)) if !message.contains(&detail) => {
            Some(format!("{message} ({detail})"))
        }
        (Some(message), _) => Some(message),
        (None, Some(detail)) => Some(detail),
        (None, None) => None,
    }
}

fn format_error_detail_message(detail: &Value) -> Option<String> {
    let field = detail.get("field").and_then(Value::as_str)?.trim();
    if field != "input_text" {
        return None;
    }
    let max_chars = parse_usize(detail.get("max_chars"));
    let actual_chars = parse_usize(detail.get("actual_chars"));
    match (actual_chars, max_chars) {
        (Some(actual_chars), Some(max_chars)) => {
            Some(format!("text input {actual_chars}/{max_chars} chars"))
        }
        (Some(actual_chars), None) => Some(format!("text input {actual_chars} chars")),
        (None, Some(max_chars)) => Some(format!("text input limit {max_chars} chars")),
        (None, None) => None,
    }
}

fn parse_usize(value: Option<&Value>) -> Option<usize> {
    value.and_then(|item| {
        item.as_u64()
            .and_then(|num| usize::try_from(num).ok())
            .or_else(|| item.as_i64().and_then(|num| usize::try_from(num).ok()))
            .or_else(|| {
                item.as_str()
                    .and_then(|text| text.trim().parse::<usize>().ok())
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn format_error_message_appends_structured_input_limit_detail() {
        let payload = json!({
            "code": "INVALID_REQUEST",
            "message": "Input is too long",
            "detail": {
                "field": "input_text",
                "max_chars": 100,
                "actual_chars": 123
            }
        });
        assert_eq!(
            format_error_message(&payload),
            Some("Input is too long (text input 123/100 chars)".to_string())
        );
    }

    #[test]
    fn format_error_message_falls_back_to_structured_detail() {
        let payload = json!({
            "detail": {
                "field": "input_text",
                "max_chars": 10,
                "actual_chars": 12
            }
        });
        assert_eq!(
            format_error_message(&payload),
            Some("text input 12/10 chars".to_string())
        );
    }
}
