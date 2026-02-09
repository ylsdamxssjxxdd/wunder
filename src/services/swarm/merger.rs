use serde_json::Value;

pub fn merge_first_success(results: &[Value]) -> Option<Value> {
    results
        .iter()
        .find(|value| {
            value
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status.eq_ignore_ascii_case("success"))
        })
        .cloned()
        .or_else(|| results.first().cloned())
}
