use serde_json::{json, Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ToolErrorMeta {
    pub(crate) code: String,
    pub(crate) hint: Option<String>,
    pub(crate) retryable: bool,
    pub(crate) retry_after_ms: Option<u64>,
}

impl ToolErrorMeta {
    pub(crate) fn new(
        code: impl Into<String>,
        hint: Option<String>,
        retryable: bool,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self {
            code: code.into(),
            hint,
            retryable,
            retry_after_ms,
        }
    }

    pub(crate) fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "hint": self.hint,
            "retryable": self.retryable,
            "retry_after_ms": self.retry_after_ms,
        })
    }
}

pub(crate) fn build_failed_tool_result(
    error: impl Into<String>,
    data: Value,
    meta: ToolErrorMeta,
    sandbox: bool,
) -> Value {
    json!({
        "ok": false,
        "data": ensure_object_payload(data),
        "error": error.into(),
        "error_meta": meta.to_json(),
        "sandbox": sandbox,
    })
}

pub(crate) fn with_error_meta(data: Value, meta: ToolErrorMeta) -> Value {
    let mut payload = ensure_object_payload(data);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("error_meta".to_string(), meta.to_json());
    }
    payload
}

fn ensure_object_payload(data: Value) -> Value {
    if data.is_object() {
        return data;
    }
    let mut map = Map::new();
    map.insert("result".to_string(), data);
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_error_meta_wraps_non_object_payload() {
        let payload = with_error_meta(
            Value::String("bad".to_string()),
            ToolErrorMeta::new("TOOL_ERROR", None, false, None),
        );
        let obj = payload.as_object().expect("payload should be object");
        assert_eq!(obj.get("result"), Some(&Value::String("bad".to_string())));
        assert!(obj.get("error_meta").is_some());
    }
}
