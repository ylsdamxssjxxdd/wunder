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

/// OpenAI function schemas require an object at the root and do not accept a
/// root composition keyword.  Action tools are normally expressed as a strict
/// `oneOf` of closed objects, which is the right surface for providers that
/// support it.  For OpenAI only, project that union into one closed object:
/// `action` becomes an enum and every action field is listed once.  The
/// runtime continues to validate the original action-specific union before
/// dispatch, so this compatibility projection cannot widen execution.
///
/// Return `true` only when every branch is an object with a literal action
/// discriminator. Generic unions retain the conservative legacy treatment.
fn lift_action_union_for_openai(map: &mut Map<String, Value>) -> bool {
    let Some(branches) = map.get("oneOf").and_then(Value::as_array) else {
        return false;
    };
    if branches.is_empty() {
        return false;
    }

    let mut fields = Map::new();
    let mut actions = Vec::new();
    for branch in branches {
        let Some(properties) = branch.get("properties").and_then(Value::as_object) else {
            return false;
        };
        let Some(action_schema) = properties.get("action") else {
            return false;
        };
        let mut branch_actions = action_values(action_schema);
        if branch_actions.is_empty() {
            return false;
        }
        actions.append(&mut branch_actions);
        for (name, schema) in properties {
            if name == "action" {
                continue;
            }
            merge_openai_action_property(&mut fields, name, schema);
        }
    }

    actions.sort();
    actions.dedup();
    if actions.is_empty() {
        return false;
    }
    fields.insert(
        "action".to_string(),
        json!({"type": SCHEMA_TYPE_STRING, "enum": actions}),
    );
    map.insert("properties".to_string(), Value::Object(fields));
    map.insert("required".to_string(), json!(["action"]));
    map.insert("additionalProperties".to_string(), Value::Bool(false));
    true
}

fn action_values(schema: &Value) -> Vec<String> {
    if let Some(value) = schema.get("const").and_then(Value::as_str) {
        return vec![value.to_string()];
    }
    schema
        .get("enum")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn merge_openai_action_property(fields: &mut Map<String, Value>, name: &str, schema: &Value) {
    let Some(existing) = fields.get_mut(name) else {
        fields.insert(name.to_string(), schema.clone());
        return;
    };
    if *existing == *schema {
        return;
    }
    let mut alternatives = match existing {
        Value::Object(existing_map) if existing_map.len() == 1 => existing_map
            .get("anyOf")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| vec![existing.clone()]),
        _ => vec![existing.clone()],
    };
    if !alternatives.iter().any(|candidate| candidate == schema) {
        alternatives.push(schema.clone());
    }
    *existing = json!({"anyOf": alternatives});
}

fn sanitize_schema_map(map: &mut Map<String, Value>) {
    // `$ref` is a complete schema in its own right. Inferring a `type` for a
    // reference-only node corrupts references to arrays or objects (for
    // example, it used to add `type: string` to `{"$ref":"#/$defs/bbox"}`).
    // Preserve it verbatim, while still sanitizing explicitly composed sibling
    // schemas when a provider uses the JSON Schema 2019-09/2020-12 form.
    let is_reference = map.contains_key("$ref") || map.contains_key("$dynamicRef");
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

    // A composition also defines its own accepted types. Adding the default
    // string type to `anyOf: [string, array]` excludes the array branch; adding
    // it to `allOf: [$ref]` can contradict the referenced object entirely.
    // Preserve untyped compositions while still normalizing their children.
    let is_untyped_composition = !map.contains_key("type")
        && ["oneOf", "anyOf", "allOf"]
            .iter()
            .any(|key| map.contains_key(*key));
    if is_reference || is_untyped_composition {
        return;
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

fn normalize_tool_input_schema_inner(
    schema: Option<&Value>,
    strip_openai_top_level_forbidden: bool,
) -> Value {
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
        if strip_openai_top_level_forbidden {
            // OpenAI tool schemas require top-level object parameters and reject
            // these composition/negation keywords at the root level. Preserve
            // action fields by projecting a strict action union first.
            let _ = lift_action_union_for_openai(map);
            for forbidden in ["oneOf", "anyOf", "allOf", "enum", "not"] {
                map.remove(forbidden);
            }
        }
    }
    normalized
}

pub fn normalize_tool_input_schema(schema: Option<&Value>) -> Value {
    normalize_tool_input_schema_inner(schema, false)
}

pub fn normalize_tool_input_schema_for_openai(schema: Option<&Value>) -> Value {
    normalize_tool_input_schema_inner(schema, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_tool_input_schema_adds_missing_array_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array"
                }
            },
            "required": ["items"]
        });
        let normalized = normalize_tool_input_schema(Some(&schema));
        assert_eq!(
            normalized["properties"]["items"]["items"]["type"],
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

    #[test]
    fn normalize_tool_input_schema_preserves_top_level_keywords_by_default() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["alpha", "beta"]
                }
            },
            "anyOf": [
                { "required": ["mode"] }
            ],
            "allOf": [
                { "properties": { "value": { "type": "string" } } }
            ],
            "oneOf": [
                { "required": ["mode"] }
            ],
            "enum": ["sample"],
            "not": { "type": "null" }
        });

        let normalized = normalize_tool_input_schema(Some(&schema));

        assert!(normalized.get("oneOf").is_some());
        assert!(normalized.get("anyOf").is_some());
        assert!(normalized.get("allOf").is_some());
        assert!(normalized.get("enum").is_some());
        assert!(normalized.get("not").is_some());
        assert_eq!(
            normalized["properties"]["mode"]["enum"],
            json!(["alpha", "beta"])
        );
    }

    #[test]
    fn normalize_tool_input_schema_for_openai_strips_forbidden_top_level_keywords() {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["alpha", "beta"]
                }
            },
            "anyOf": [
                { "required": ["mode"] }
            ],
            "allOf": [
                { "properties": { "value": { "type": "string" } } }
            ],
            "oneOf": [
                { "required": ["mode"] }
            ],
            "enum": ["sample"],
            "not": { "type": "null" }
        });

        let normalized = normalize_tool_input_schema_for_openai(Some(&schema));

        assert!(normalized.get("oneOf").is_none());
        assert!(normalized.get("anyOf").is_none());
        assert!(normalized.get("allOf").is_none());
        assert!(normalized.get("enum").is_none());
        assert!(normalized.get("not").is_none());
        assert_eq!(
            normalized["properties"]["mode"]["enum"],
            json!(["alpha", "beta"])
        );
    }

    #[test]
    fn normalize_tool_input_schema_for_openai_projects_action_union_fields() {
        let schema = json!({
            "type": "object",
            "oneOf": [
                {
                    "type": "object",
                    "properties": {
                        "action": {"const": "get"},
                        "goal_id": {"type": "string"}
                    },
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "action": {"const": "set"},
                        "enabled": {"type": "boolean"}
                    },
                    "required": ["action", "enabled"],
                    "additionalProperties": false
                }
            ]
        });

        let normalized = normalize_tool_input_schema_for_openai(Some(&schema));

        assert!(normalized.get("oneOf").is_none());
        assert_eq!(
            normalized["properties"]["action"]["enum"],
            json!(["get", "set"])
        );
        assert_eq!(normalized["properties"]["goal_id"]["type"], "string");
        assert_eq!(normalized["properties"]["enabled"]["type"], "boolean");
        assert_eq!(normalized["required"], json!(["action"]));
        assert_eq!(normalized["additionalProperties"], false);
    }

    #[test]
    fn sanitize_json_schema_replaces_boolean_subschemas() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "enabled": true,
                "disabled": false
            }
        });

        sanitize_json_schema_in_place(&mut schema);

        assert_eq!(schema["properties"]["enabled"]["type"], json!("string"));
        assert_eq!(schema["properties"]["disabled"]["type"], json!("string"));
    }

    #[test]
    fn sanitize_json_schema_normalizes_type_arrays_to_first_supported_type() {
        let mut schema = json!({
            "type": ["null", "INTEGER"],
            "minimum": 0
        });

        sanitize_json_schema_in_place(&mut schema);

        assert_eq!(schema["type"], json!("integer"));
    }

    #[test]
    fn normalize_tool_input_schema_preserves_ref_without_inferred_type() {
        let schema = json!({
            "type": "object",
            "$defs": {
                "bbox": {"type": "array", "items": {"type": "integer"}}
            },
            "properties": {
                "bbox": {"$ref": "#/$defs/bbox"}
            }
        });

        let normalized = normalize_tool_input_schema(Some(&schema));

        assert_eq!(
            normalized["properties"]["bbox"]["$ref"],
            json!("#/$defs/bbox")
        );
        assert!(normalized["properties"]["bbox"].get("type").is_none());
        assert_eq!(normalized["$defs"]["bbox"]["type"], json!("array"));
    }

    #[test]
    fn normalize_tool_input_schema_preserves_untyped_compositions() {
        let schema = json!({
            "type": "object",
            "$defs": {
                "choice": {"anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]},
                "step": {"oneOf": [
                    {"type": "object", "properties": {"kind": {"const": "wait"}}},
                    {"type": "object", "properties": {"kind": {"const": "click"}}}
                ]}
            },
            "properties": {
                "value": {"$ref": "#/$defs/choice"},
                "step": {"allOf": [{"$ref": "#/$defs/step"}]}
            }
        });

        for normalized in [
            normalize_tool_input_schema(Some(&schema)),
            normalize_tool_input_schema_for_openai(Some(&schema)),
        ] {
            assert!(normalized["$defs"]["choice"].get("type").is_none());
            assert!(normalized["$defs"]["step"].get("type").is_none());
            assert!(normalized["properties"]["step"].get("type").is_none());
            assert_eq!(normalized["$defs"]["choice"]["anyOf"][1]["type"], "array");
            assert_eq!(normalized["$defs"]["step"]["oneOf"][0]["type"], "object");
        }
    }

    #[test]
    fn normalize_tool_input_schema_defaults_missing_schema_to_object() {
        let normalized = normalize_tool_input_schema(None);

        assert_eq!(normalized["type"], json!("object"));
        assert!(normalized["properties"].is_object());
    }
}
