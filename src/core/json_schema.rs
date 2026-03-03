use serde_json::{json, Map, Value};

const SCHEMA_TYPE_OBJECT: &str = "object";
const SCHEMA_TYPE_ARRAY: &str = "array";
const SCHEMA_TYPE_STRING: &str = "string";
const SCHEMA_TYPE_NUMBER: &str = "number";
const SCHEMA_TYPE_INTEGER: &str = "integer";
const SCHEMA_TYPE_BOOLEAN: &str = "boolean";

fn default_object_schema() -> Value {
    json!({
        "type": SCHEMA_TYPE_OBJECT,
        "properties": {}
    })
}

fn supported_schema_type(value: &str) -> bool {
    matches!(
        value,
        SCHEMA_TYPE_OBJECT
            | SCHEMA_TYPE_ARRAY
            | SCHEMA_TYPE_STRING
            | SCHEMA_TYPE_NUMBER
            | SCHEMA_TYPE_INTEGER
            | SCHEMA_TYPE_BOOLEAN
    )
}

fn normalize_schema_type_field(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(kind)) => {
            let lowered = kind.trim().to_ascii_lowercase();
            if supported_schema_type(lowered.as_str()) {
                Some(lowered)
            } else {
                None
            }
        }
        Some(Value::Array(kinds)) => kinds.iter().find_map(|kind| {
            kind.as_str().and_then(|text| {
                let lowered = text.trim().to_ascii_lowercase();
                if supported_schema_type(lowered.as_str()) {
                    Some(lowered)
                } else {
                    None
                }
            })
        }),
        _ => None,
    }
}

fn infer_schema_type(map: &Map<String, Value>) -> String {
    if map.contains_key("properties")
        || map.contains_key("required")
        || map.contains_key("additionalProperties")
    {
        return SCHEMA_TYPE_OBJECT.to_string();
    }
    if map.contains_key("items") || map.contains_key("prefixItems") {
        return SCHEMA_TYPE_ARRAY.to_string();
    }
    if map.contains_key("minimum")
        || map.contains_key("maximum")
        || map.contains_key("exclusiveMinimum")
        || map.contains_key("exclusiveMaximum")
        || map.contains_key("multipleOf")
    {
        return SCHEMA_TYPE_NUMBER.to_string();
    }
    if map.contains_key("enum") || map.contains_key("const") || map.contains_key("format") {
        return SCHEMA_TYPE_STRING.to_string();
    }
    SCHEMA_TYPE_STRING.to_string()
}

fn sanitize_schema_object_values(map: &mut Map<String, Value>, key: &str) {
    if let Some(Value::Object(entries)) = map.get_mut(key) {
        for value in entries.values_mut() {
            sanitize_json_schema_in_place(value);
        }
    }
}

fn schema_type_contains(value: Option<&Value>, expected: &str) -> bool {
    match value {
        Some(Value::String(kind)) => kind.trim().eq_ignore_ascii_case(expected),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .filter_map(Value::as_str)
            .any(|kind| kind.trim().eq_ignore_ascii_case(expected)),
        _ => false,
    }
}

fn ensure_object_properties(map: &mut Map<String, Value>) {
    if !matches!(map.get("properties"), Some(Value::Object(_))) {
        map.insert("properties".to_string(), Value::Object(Map::new()));
    }
}

fn sanitize_schema_map(map: &mut Map<String, Value>) {
    sanitize_schema_object_values(map, "properties");
    sanitize_schema_object_values(map, "$defs");
    sanitize_schema_object_values(map, "definitions");

    for key in ["items", "contains", "not", "if", "then", "else"] {
        if let Some(value) = map.get_mut(key) {
            sanitize_json_schema_in_place(value);
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(value) = map.get_mut(key) {
            sanitize_json_schema_in_place(value);
        }
    }

    let schema_type =
        normalize_schema_type_field(map.get("type")).unwrap_or_else(|| infer_schema_type(map));
    map.insert("type".to_string(), Value::String(schema_type.clone()));

    if schema_type == SCHEMA_TYPE_OBJECT {
        ensure_object_properties(map);
        if let Some(additional) = map.get_mut("additionalProperties") {
            if !additional.is_boolean() {
                sanitize_json_schema_in_place(additional);
            }
        }
    }

    if schema_type == SCHEMA_TYPE_ARRAY {
        let needs_default_items =
            !map.contains_key("items") || map.get("items").is_some_and(Value::is_null);
        if needs_default_items {
            map.insert(
                "items".to_string(),
                json!({
                    "type": SCHEMA_TYPE_STRING
                }),
            );
        }
        if let Some(items) = map.get_mut("items") {
            sanitize_json_schema_in_place(items);
        }
    }
}

pub fn sanitize_json_schema_in_place(value: &mut Value) {
    match value {
        Value::Bool(_) => {
            *value = json!({
                "type": SCHEMA_TYPE_STRING
            });
        }
        Value::Array(values) => {
            for item in values {
                sanitize_json_schema_in_place(item);
            }
        }
        Value::Object(map) => {
            sanitize_schema_map(map);
        }
        _ => {}
    }
}

pub fn normalize_tool_input_schema(schema: Option<&Value>) -> Value {
    let mut normalized = schema.cloned().unwrap_or_else(default_object_schema);
    sanitize_json_schema_in_place(&mut normalized);

    if !normalized.is_object() {
        return default_object_schema();
    }

    if let Value::Object(map) = &mut normalized {
        if !schema_type_contains(map.get("type"), SCHEMA_TYPE_OBJECT) {
            map.insert(
                "type".to_string(),
                Value::String(SCHEMA_TYPE_OBJECT.to_string()),
            );
        }
        ensure_object_properties(map);
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tool_input_schema_adds_missing_array_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "edits": {
                    "type": "array"
                }
            },
            "required": ["edits"]
        });
        let normalized = normalize_tool_input_schema(Some(&schema));
        assert_eq!(
            normalized["properties"]["edits"]["items"]["type"],
            Value::String(SCHEMA_TYPE_STRING.to_string())
        );
    }

    #[test]
    fn normalize_tool_input_schema_keeps_top_level_object_shape() {
        let schema = json!({
            "type": "array",
            "items": {
                "type": "string"
            }
        });
        let normalized = normalize_tool_input_schema(Some(&schema));
        assert_eq!(
            normalized["type"],
            Value::String(SCHEMA_TYPE_OBJECT.to_string())
        );
        assert!(normalized["properties"].is_object());
    }

    #[test]
    fn sanitize_json_schema_infers_array_type_when_items_exists() {
        let mut schema = json!({
            "items": {
                "type": "integer"
            }
        });
        sanitize_json_schema_in_place(&mut schema);
        assert_eq!(schema["type"], Value::String(SCHEMA_TYPE_ARRAY.to_string()));
    }
}
