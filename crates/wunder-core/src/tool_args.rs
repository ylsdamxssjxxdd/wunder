use serde_json::{json, Map, Value};

pub struct ToolArgsRecovery {
    pub value: Value,
    pub repair: Option<Value>,
}

pub struct ToolCallPayloadSanitization {
    pub value: Value,
    pub repair: Option<Value>,
}

pub fn recover_tool_args_value(args: &Value) -> Value {
    recover_tool_args_value_with_meta(args).value
}

/// Recover a malformed tool payload and, when the active tool schema is known,
/// restore JSON literals that an upstream transport incorrectly turned into
/// strings.  Values are recovered only when the selected schema branch expects
/// a non-string JSON type, so ordinary text such as IDs and user input remains
/// untouched.
pub fn recover_tool_args_value_with_schema_and_meta(
    args: &Value,
    schema: &Value,
) -> ToolArgsRecovery {
    let recovered = recover_tool_args_value_with_meta(args);
    let mut schema_repair_count = 0usize;
    let mut schema_node_visits = 0usize;
    let value = recover_schema_stringified_values(
        recovered.value,
        schema,
        schema,
        &mut schema_repair_count,
        &mut schema_node_visits,
        0,
    );
    let repair = merge_tool_args_repairs(
        recovered.repair,
        (schema_repair_count > 0).then(|| {
            json!({
                "kind": "tool_args",
                "source": "schema",
                "strategy": "schema_type_recovery",
                "count": schema_repair_count,
            })
        }),
    );
    ToolArgsRecovery { value, repair }
}

/// Schema-aware convenience form of
/// [`recover_tool_args_value_with_schema_and_meta`].
pub fn recover_tool_args_value_with_schema(args: &Value, schema: &Value) -> Value {
    recover_tool_args_value_with_schema_and_meta(args, schema).value
}

/// Recover only field values proven by the supplied schema to be non-string
/// JSON. Callers that already ran raw-wrapper recovery can use this form to
/// retain their original repair metadata.
pub fn recover_tool_args_value_against_schema_with_meta(
    args: &Value,
    schema: &Value,
) -> ToolArgsRecovery {
    let mut schema_repair_count = 0usize;
    let mut schema_node_visits = 0usize;
    let value = recover_schema_stringified_values(
        args.clone(),
        schema,
        schema,
        &mut schema_repair_count,
        &mut schema_node_visits,
        0,
    );
    ToolArgsRecovery {
        value,
        repair: (schema_repair_count > 0).then(|| {
            json!({
                "kind": "tool_args",
                "source": "schema",
                "strategy": "schema_type_recovery",
                "count": schema_repair_count,
            })
        }),
    }
}

fn merge_tool_args_repairs(existing: Option<Value>, next: Option<Value>) -> Option<Value> {
    match (existing, next) {
        (None, None) => None,
        (Some(repair), None) | (None, Some(repair)) => Some(repair),
        (Some(first), Some(second)) => Some(json!({
            "kind": "tool_args",
            "repairs": [first, second],
        })),
    }
}

/// A depth limit turns a hostile recursive `$ref` graph into a no-op at the
/// boundary instead of consuming an unbounded amount of stack.
const MAX_SCHEMA_RECOVERY_DEPTH: usize = 64;
const MAX_SCHEMA_RECOVERY_NODES: usize = 4096;

fn recover_schema_stringified_values(
    value: Value,
    schema: &Value,
    root_schema: &Value,
    recovered_count: &mut usize,
    node_visits: &mut usize,
    depth: usize,
) -> Value {
    recover_schema_stringified_values_inner(
        value,
        schema,
        root_schema,
        recovered_count,
        node_visits,
        depth,
        true,
    )
}

fn recover_schema_stringified_values_inner(
    value: Value,
    schema: &Value,
    root_schema: &Value,
    recovered_count: &mut usize,
    node_visits: &mut usize,
    depth: usize,
    apply_direct_type_recovery: bool,
) -> Value {
    if !consume_schema_recovery_visit(node_visits, depth) {
        return value;
    }

    // Recover a direct scalar before visiting `$ref` / `allOf`. Type evidence
    // follows both, so a string is converted only if every applicable schema
    // excludes string. Their recursive pass then handles nested fields without
    // re-applying an individual scalar branch prematurely.
    let mut value = if apply_direct_type_recovery {
        recover_value_for_declared_type(value, schema, root_schema, recovered_count, node_visits)
    } else {
        value
    };

    // `$ref` can have sibling constraints in modern JSON Schema.  The direct
    // recovery above considers both; this pass only projects nested members.
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(target) = resolve_local_schema_reference(root_schema, reference) {
            value = recover_schema_stringified_values_inner(
                value,
                target,
                root_schema,
                recovered_count,
                node_visits,
                depth + 1,
                false,
            );
        }
    }

    // Like `$ref`, allOf branches are simultaneous constraints.  Do not let a
    // single branch coerce a value when a sibling still permits string.
    if let Some(all_of) = schema.get("allOf").and_then(Value::as_array) {
        for branch in all_of {
            value = recover_schema_stringified_values_inner(
                value,
                branch,
                root_schema,
                recovered_count,
                node_visits,
                depth + 1,
                false,
            );
        }
    }

    // `oneOf` / `anyOf` can safely contribute a type only after one branch is
    // unambiguously selected from the original structured discriminator.
    for keyword in ["oneOf", "anyOf"] {
        let Some(branches) = schema.get(keyword).and_then(Value::as_array) else {
            continue;
        };
        let Some(branch) = select_schema_branch(&value, branches, root_schema, node_visits) else {
            continue;
        };
        value = recover_schema_stringified_values_inner(
            value,
            branch,
            root_schema,
            recovered_count,
            node_visits,
            depth + 1,
            true,
        );
    }

    match &mut value {
        Value::Object(object) => {
            if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
                for (key, field_schema) in properties {
                    let Some(field_value) = object.remove(key) else {
                        continue;
                    };
                    object.insert(
                        key.clone(),
                        recover_schema_stringified_values_inner(
                            field_value,
                            field_schema,
                            root_schema,
                            recovered_count,
                            node_visits,
                            depth + 1,
                            true,
                        ),
                    );
                }
            }
            if let Some(additional_schema) = schema
                .get("additionalProperties")
                .filter(|candidate| candidate.is_object())
            {
                let known_properties = schema.get("properties").and_then(Value::as_object);
                for (key, field_value) in object.iter_mut() {
                    if known_properties.is_some_and(|properties| properties.contains_key(key)) {
                        continue;
                    }
                    *field_value = recover_schema_stringified_values_inner(
                        std::mem::take(field_value),
                        additional_schema,
                        root_schema,
                        recovered_count,
                        node_visits,
                        depth + 1,
                        true,
                    );
                }
            }
        }
        Value::Array(items) => {
            let prefix_len =
                if let Some(prefix_items) = schema.get("prefixItems").and_then(Value::as_array) {
                    for (index, item_schema) in prefix_items.iter().enumerate() {
                        let Some(item) = items.get_mut(index) else {
                            break;
                        };
                        *item = recover_schema_stringified_values_inner(
                            std::mem::take(item),
                            item_schema,
                            root_schema,
                            recovered_count,
                            node_visits,
                            depth + 1,
                            true,
                        );
                    }
                    prefix_items.len().min(items.len())
                } else {
                    0
                };
            if let Some(item_schema) = schema.get("items").filter(|schema| schema.is_object()) {
                for item in items.iter_mut().skip(prefix_len) {
                    *item = recover_schema_stringified_values_inner(
                        std::mem::take(item),
                        item_schema,
                        root_schema,
                        recovered_count,
                        node_visits,
                        depth + 1,
                        true,
                    );
                }
            }
        }
        _ => {}
    }

    value
}

fn consume_schema_recovery_visit(node_visits: &mut usize, depth: usize) -> bool {
    if depth >= MAX_SCHEMA_RECOVERY_DEPTH || *node_visits >= MAX_SCHEMA_RECOVERY_NODES {
        return false;
    }
    *node_visits = node_visits.saturating_add(1);
    true
}

fn resolve_local_schema_reference<'a>(
    root_schema: &'a Value,
    reference: &str,
) -> Option<&'a Value> {
    reference
        .strip_prefix('#')
        .filter(|pointer| pointer.starts_with('/'))
        .and_then(|pointer| root_schema.pointer(pointer))
}

fn recover_value_for_declared_type(
    value: Value,
    schema: &Value,
    root_schema: &Value,
    recovered_count: &mut usize,
    node_visits: &mut usize,
) -> Value {
    let Value::String(text) = &value else {
        return value;
    };
    let expected_types = declared_non_string_types(schema, root_schema, node_visits);
    if expected_types.is_empty() || schema_allows_string(schema, root_schema, node_visits) {
        return value;
    }
    let Ok(parsed) = serde_json::from_str::<Value>(text.trim()) else {
        return value;
    };
    if value_matches_non_string_type(&parsed, &expected_types) {
        *recovered_count = recovered_count.saturating_add(1);
        parsed
    } else {
        value
    }
}

fn declared_non_string_types(
    schema: &Value,
    root_schema: &Value,
    node_visits: &mut usize,
) -> Vec<&'static str> {
    let mut types = Vec::new();
    collect_declared_types(schema, root_schema, &mut types, node_visits, 0);
    types.retain(|kind| *kind != "string");
    types.sort_unstable();
    types.dedup();
    types
}

fn schema_allows_string(schema: &Value, root_schema: &Value, node_visits: &mut usize) -> bool {
    let mut types = Vec::new();
    collect_declared_types(schema, root_schema, &mut types, node_visits, 0);
    types.contains(&"string")
}

fn collect_declared_types(
    schema: &Value,
    root_schema: &Value,
    output: &mut Vec<&'static str>,
    node_visits: &mut usize,
    depth: usize,
) {
    if !consume_schema_recovery_visit(node_visits, depth) {
        return;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        if let Some(target) = resolve_local_schema_reference(root_schema, reference) {
            collect_declared_types(target, root_schema, output, node_visits, depth + 1);
        }
    }
    match schema.get("type") {
        Some(Value::String(kind)) => push_json_type(kind, output),
        Some(Value::Array(kinds)) => {
            for kind in kinds.iter().filter_map(Value::as_str) {
                push_json_type(kind, output);
            }
        }
        _ => {}
    }
    // allOf is an intersection. Include all of its declared constraints so a
    // string-friendly branch prevents scalar recovery.
    if let Some(branches) = schema.get("allOf").and_then(Value::as_array) {
        for branch in branches {
            collect_declared_types(branch, root_schema, output, node_visits, depth + 1);
        }
    }
    // These object keywords express an object-shaped value even when `type`
    // was omitted by a provider schema.
    if schema.get("properties").is_some()
        || schema
            .get("additionalProperties")
            .is_some_and(Value::is_object)
    {
        push_json_type("object", output);
    }
    if schema.get("items").is_some() || schema.get("prefixItems").is_some() {
        push_json_type("array", output);
    }
}

fn push_json_type(kind: &str, output: &mut Vec<&'static str>) {
    let normalized = match kind {
        "null" => "null",
        "boolean" => "boolean",
        "number" => "number",
        "integer" => "integer",
        "array" => "array",
        "object" => "object",
        "string" => "string",
        _ => return,
    };
    output.push(normalized);
}

fn value_matches_non_string_type(value: &Value, expected_types: &[&str]) -> bool {
    expected_types.iter().any(|kind| match *kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    })
}

fn select_schema_branch<'a>(
    value: &Value,
    branches: &'a [Value],
    root_schema: &Value,
    node_visits: &mut usize,
) -> Option<&'a Value> {
    let mut selected: Option<(i32, &Value)> = None;
    let mut tied = false;
    for branch in branches {
        let Some(score) = schema_branch_score(value, branch, root_schema, node_visits, 0) else {
            continue;
        };
        if value.is_string() {
            let mut types = Vec::new();
            collect_declared_types(branch, root_schema, &mut types, node_visits, 0);
            // A matching text branch means the input can already be valid.
            // Array length constraints must not outscore it and reinterpret
            // literal JSON text as an array. Unknown branch types likewise
            // do not prove that recovering a non-string value is safe.
            if types.is_empty() || types.contains(&"string") {
                return None;
            }
        }
        match selected {
            None => selected = Some((score, branch)),
            Some((current, _)) if score > current => {
                selected = Some((score, branch));
                tied = false;
            }
            Some((current, _)) if score == current => tied = true,
            _ => {}
        }
    }
    // A score tie does not prove which branch owns the value.  Keep the raw
    // string instead of picking an arbitrary conversion, including for
    // `anyOf`: a schema such as `[string, integer]` must preserve `"42"`.
    let selected = selected?;
    (!tied).then_some(selected.1)
}

fn schema_branch_score(
    value: &Value,
    schema: &Value,
    root_schema: &Value,
    node_visits: &mut usize,
    depth: usize,
) -> Option<i32> {
    if !consume_schema_recovery_visit(node_visits, depth) {
        return None;
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = resolve_local_schema_reference(root_schema, reference)?;
        return schema_branch_score(value, target, root_schema, node_visits, depth + 1);
    }
    // Branch selection may need to inspect constraints such as `minItems`
    // before the value itself is recovered. Use a parsed JSON literal only
    // when this branch explicitly describes a non-string shape; a `string`
    // branch keeps the original text, so `[string, integer]` remains
    // ambiguous for `"42"`.
    let parsed_candidate = value
        .as_str()
        .and_then(|raw| serde_json::from_str::<Value>(raw.trim()).ok());
    let candidate = parsed_candidate
        .as_ref()
        .filter(|parsed| schema_expects_non_string_shape(schema, parsed))
        .unwrap_or(value);
    let mut score = 0i32;
    if let Some(kind) = schema.get("type").and_then(Value::as_str) {
        if !value_matches_schema_type_or_recoverable(candidate, kind) {
            return None;
        }
        score += 4;
    }
    if let Some(constant) = schema.get("const") {
        if candidate != constant {
            return None;
        }
        score += 1_000;
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.iter().any(|entry| entry == candidate) {
            return None;
        }
        score += 500;
    }
    if let Some(items) = candidate.as_array() {
        if let Some(min_items) = schema.get("minItems").and_then(Value::as_u64) {
            if items.len() < min_items as usize {
                return None;
            }
            score += 2;
        }
        if let Some(max_items) = schema.get("maxItems").and_then(Value::as_u64) {
            if items.len() > max_items as usize {
                return None;
            }
            score += 2;
        }
        if let Some(prefix_items) = schema.get("prefixItems").and_then(Value::as_array) {
            if items.len() < prefix_items.len() {
                return None;
            }
            for (item, item_schema) in items.iter().zip(prefix_items) {
                let item_score =
                    schema_branch_score(item, item_schema, root_schema, node_visits, depth + 1)?;
                score += 1 + item_score.min(100);
            }
        }
        return Some(score);
    }
    let Some(object) = candidate.as_object() else {
        return Some(score);
    };
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for required_key in required.iter().filter_map(Value::as_str) {
            if object.contains_key(required_key) {
                score += 10;
            } else {
                return None;
            }
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, field_schema) in properties {
            let Some(field_value) = object.get(key) else {
                continue;
            };
            let field_score = schema_branch_score(
                field_value,
                field_schema,
                root_schema,
                node_visits,
                depth + 1,
            )?;
            score += 1 + field_score.min(100);
        }
    }
    Some(score)
}

fn schema_expects_non_string_shape(schema: &Value, parsed: &Value) -> bool {
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => false,
        Some(kind) => value_matches_schema_type(parsed, kind),
        None => {
            (parsed.is_array()
                && (schema.get("items").is_some()
                    || schema.get("prefixItems").is_some()
                    || schema.get("minItems").is_some()
                    || schema.get("maxItems").is_some()))
                || (parsed.is_object()
                    && (schema.get("properties").is_some()
                        || schema.get("required").is_some()
                        || schema.get("additionalProperties").is_some()))
        }
    }
}

fn value_matches_schema_type_or_recoverable(value: &Value, kind: &str) -> bool {
    value_matches_schema_type(value, kind)
        || value
            .as_str()
            .and_then(|raw| serde_json::from_str::<Value>(raw.trim()).ok())
            .is_some_and(|parsed| value_matches_schema_type(&parsed, kind))
}

fn value_matches_schema_type(value: &Value, kind: &str) -> bool {
    match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => true,
    }
}

pub fn recover_tool_args_value_with_meta(args: &Value) -> ToolArgsRecovery {
    match args {
        Value::Object(map) => {
            let Some(raw) = map.get("raw").and_then(Value::as_str).map(str::trim) else {
                return ToolArgsRecovery {
                    value: args.clone(),
                    repair: None,
                };
            };
            if raw.is_empty() {
                return ToolArgsRecovery {
                    value: args.clone(),
                    repair: None,
                };
            }

            if let Some(Value::Object(mut parsed)) = strict_parse_json_value(raw) {
                for (key, value) in map {
                    if key == "raw" {
                        continue;
                    }
                    parsed.entry(key.clone()).or_insert_with(|| value.clone());
                }
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: None,
                };
            }

            if let Some(Value::Object(mut parsed)) = parse_json_value_lossy(raw) {
                for (key, value) in map {
                    if key == "raw" {
                        continue;
                    }
                    parsed.entry(key.clone()).or_insert_with(|| value.clone());
                }
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: Some(build_repair_meta(
                        "tool_args",
                        "raw",
                        "lossy_json_string_repair",
                    )),
                };
            }

            ToolArgsRecovery {
                value: args.clone(),
                repair: None,
            }
        }
        Value::String(raw) => {
            let trimmed = raw.trim();
            if let Some(Value::Object(parsed)) = strict_parse_json_value(trimmed) {
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: None,
                };
            }
            if let Some(Value::Object(parsed)) = parse_json_value_lossy(trimmed) {
                return ToolArgsRecovery {
                    value: Value::Object(parsed),
                    repair: Some(build_repair_meta(
                        "tool_args",
                        "string",
                        "lossy_json_string_repair",
                    )),
                };
            }
            ToolArgsRecovery {
                value: args.clone(),
                repair: None,
            }
        }
        _ => ToolArgsRecovery {
            value: args.clone(),
            repair: None,
        },
    }
}

#[cfg(test)]
pub fn normalize_tool_arguments_value(arguments: &str) -> Value {
    normalize_tool_arguments_json_with_meta(arguments)
        .0
        .parse::<Value>()
        .ok()
        .unwrap_or_else(|| json!({ "raw": arguments }))
}

pub fn normalize_tool_arguments_json(arguments: &str) -> String {
    normalize_tool_arguments_json_with_meta(arguments).0
}

pub fn normalize_tool_arguments_json_with_meta(arguments: &str) -> (String, Option<Value>) {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return ("{}".to_string(), None);
    }

    if let Some(Value::Object(_)) = strict_parse_json_value(trimmed) {
        return (trimmed.to_string(), None);
    }

    if let Some(value) = strict_parse_json_value(trimmed) {
        return (
            serde_json::to_string(&json!({ "value": value }))
                .unwrap_or_else(|_| "{\"value\":null}".to_string()),
            Some(build_repair_meta(
                "tool_arguments",
                "arguments",
                "non_object_arguments_wrapped",
            )),
        );
    }

    if let Some(value) = parse_json_value_lossy(trimmed) {
        return match value {
            Value::Object(map) => (
                serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "lossy_json_string_repair",
                )),
            ),
            other => (
                serde_json::to_string(&json!({ "value": other }))
                    .unwrap_or_else(|_| "{\"value\":null}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "non_object_arguments_wrapped",
                )),
            ),
        };
    }

    if let Some(prefixed) = strip_empty_object_prefix(trimmed) {
        if let Some(Value::Object(map)) = strict_parse_json_value(prefixed) {
            return (
                serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "empty_object_prefix_removed",
                )),
            );
        }
        if let Some(Value::Object(map)) = parse_json_value_lossy(prefixed) {
            return (
                serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string()),
                Some(build_repair_meta(
                    "tool_arguments",
                    "arguments",
                    "empty_object_prefix_removed",
                )),
            );
        }
    }

    (
        serde_json::to_string(&json!({ "raw": arguments }))
            .unwrap_or_else(|_| "{\"raw\":\"\"}".to_string()),
        Some(build_repair_meta(
            "tool_arguments",
            "arguments",
            "raw_arguments_wrapped",
        )),
    )
}

pub fn sanitize_tool_call_payload(payload: &Value) -> Value {
    sanitize_tool_call_payload_with_meta(payload).value
}

pub fn sanitize_tool_call_payload_with_meta(payload: &Value) -> ToolCallPayloadSanitization {
    let mut repaired_count = 0usize;
    let value = sanitize_tool_call_payload_inner(payload, &mut repaired_count);
    let repair = (repaired_count > 0).then(|| {
        json!({
            "kind": "tool_call_payload",
            "source": "function.arguments",
            "strategy": "sanitize_before_request",
            "count": repaired_count,
        })
    });
    ToolCallPayloadSanitization { value, repair }
}

fn sanitize_tool_call_payload_inner(payload: &Value, repaired_count: &mut usize) -> Value {
    match payload {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| sanitize_tool_call_payload_inner(item, repaired_count))
                .collect(),
        ),
        Value::Object(map) => {
            let mut sanitized = Map::with_capacity(map.len());
            for (key, value) in map {
                if key == "arguments" {
                    let normalized = match value {
                        Value::String(text) => {
                            let (normalized, repair) =
                                normalize_tool_arguments_json_with_meta(text);
                            if repair.is_some() {
                                *repaired_count = repaired_count.saturating_add(1);
                            }
                            Value::String(normalized)
                        }
                        other => sanitize_tool_call_payload_inner(other, repaired_count),
                    };
                    sanitized.insert(key.clone(), normalized);
                    continue;
                }
                sanitized.insert(
                    key.clone(),
                    sanitize_tool_call_payload_inner(value, repaired_count),
                );
            }
            Value::Object(sanitized)
        }
        _ => payload.clone(),
    }
}

fn strict_parse_json_value(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw).ok()
}

fn parse_json_value_lossy(raw: &str) -> Option<Value> {
    let repaired = repair_json_string_syntax(raw);
    (repaired != raw)
        .then(|| serde_json::from_str::<Value>(&repaired).ok())
        .flatten()
}

fn strip_empty_object_prefix(raw: &str) -> Option<&str> {
    let stripped = raw.strip_prefix("{}")?.trim_start();
    stripped.starts_with('{').then_some(stripped)
}

fn build_repair_meta(kind: &str, source: &str, strategy: &str) -> Value {
    json!({
        "kind": kind,
        "source": source,
        "strategy": strategy,
    })
}

fn repair_json_string_syntax(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut output = String::with_capacity(raw.len() + 16);
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in chars.iter().copied().enumerate() {
        if !in_string {
            if ch == '"' {
                in_string = true;
            }
            output.push(ch);
            continue;
        }

        if escaped {
            output.push(ch);
            if ch == '"'
                && matches!(
                    next_non_whitespace_char(&chars, index + 1),
                    Some(',') | Some('}')
                )
            {
                output.push('"');
                in_string = false;
            }
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                output.push(ch);
                escaped = true;
            }
            '"' => {
                output.push(ch);
                in_string = false;
            }
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            other if other.is_control() => {
                let code = other as u32;
                output.push_str(&format!("\\u{code:04x}"));
            }
            _ => output.push(ch),
        }
    }

    output
}

fn next_non_whitespace_char(chars: &[char], start: usize) -> Option<char> {
    chars
        .iter()
        .copied()
        .skip(start)
        .find(|ch| !ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_tool_arguments_json_with_meta, normalize_tool_arguments_value,
        recover_tool_args_value, recover_tool_args_value_against_schema_with_meta,
        recover_tool_args_value_with_meta, sanitize_tool_call_payload,
        sanitize_tool_call_payload_with_meta,
    };
    use serde_json::{json, Value};

    #[test]
    fn recover_tool_args_value_repairs_multiline_raw_json() {
        let args = json!({
            "raw": "{\"content\": \"python3 -c \\\"\nprint('sample')\n\\\", \"timeout_s\": 30, \"workdir\": \".\"}"
        });

        assert_eq!(
            recover_tool_args_value(&args),
            json!({
                "content": "python3 -c \"\nprint('sample')\n\"",
                "timeout_s": 30,
                "workdir": "."
            })
        );
    }

    #[test]
    fn recover_tool_args_value_with_meta_marks_repaired_raw_payload() {
        let args = json!({
            "raw": "{\"content\": \"python3 -c \\\"\nprint('sample')\n\\\", \"timeout_s\": 30}"
        });

        let repaired = recover_tool_args_value_with_meta(&args);
        assert_eq!(
            repaired.repair,
            Some(json!({
                "kind": "tool_args",
                "source": "raw",
                "strategy": "lossy_json_string_repair"
            }))
        );
    }

    #[test]
    fn normalize_tool_arguments_value_repairs_literal_newlines() {
        let value = normalize_tool_arguments_value(
            "{\"content\": \"python3 -c \\\"\nprint('sample')\n\\\"\", \"workdir\": \".\"}",
        );

        assert_eq!(
            value,
            json!({
                "content": "python3 -c \"\nprint('sample')\n\"",
                "workdir": "."
            })
        );
    }

    #[test]
    fn normalize_tool_arguments_json_with_meta_marks_wrapped_raw_arguments() {
        let (_, repair) = normalize_tool_arguments_json_with_meta("python3 -c \"print('sample')\"");
        assert_eq!(
            repair,
            Some(json!({
                "kind": "tool_arguments",
                "source": "arguments",
                "strategy": "raw_arguments_wrapped"
            }))
        );
    }

    #[test]
    fn normalize_tool_arguments_json_with_meta_repairs_empty_object_prefix() {
        let (normalized, repair) = normalize_tool_arguments_json_with_meta(
            "{}{\"filename\":\"sample.py\",\"content\":\"print(1)\"}",
        );
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).expect("prefixed args should parse"),
            json!({
                "filename": "sample.py",
                "content": "print(1)"
            })
        );
        assert_eq!(
            repair,
            Some(json!({
                "kind": "tool_arguments",
                "source": "arguments",
                "strategy": "empty_object_prefix_removed"
            }))
        );
    }

    #[test]
    fn normalize_tool_arguments_json_with_meta_wraps_strict_scalar_values() {
        let (normalized, repair) = normalize_tool_arguments_json_with_meta("[\"alpha\", 2]");
        assert_eq!(
            serde_json::from_str::<Value>(&normalized).expect("wrapped args should parse"),
            json!({
                "value": ["alpha", 2]
            })
        );
        assert_eq!(
            repair,
            Some(json!({
                "kind": "tool_arguments",
                "source": "arguments",
                "strategy": "non_object_arguments_wrapped"
            }))
        );
    }

    #[test]
    fn recover_tool_args_value_keeps_raw_fields_when_merging_outer_fields() {
        let args = json!({
            "raw": "{\"path\":\"from_raw.txt\",\"mode\":\"read\"}",
            "path": "explicit.txt",
            "limit": 3
        });

        let recovered = recover_tool_args_value(&args);

        assert_eq!(
            recovered,
            json!({
                "path": "from_raw.txt",
                "mode": "read",
                "limit": 3
            })
        );
    }

    #[test]
    fn schema_recovery_selects_goal_action_and_preserves_string_identifiers() {
        let schema = json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {"action": {"const": "get"}},
                    "required": ["action"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "action": {"const": "complete"},
                        "goal_id": {"type": "string"},
                        "revision": {"type": "integer", "minimum": 1}
                    },
                    "required": ["action", "goal_id", "revision"],
                    "additionalProperties": false
                }
            ]
        });
        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({"action": "complete", "goal_id": "0007", "revision": "2"}),
            &schema,
        );

        assert_eq!(
            recovered.value,
            json!({"action": "complete", "goal_id": "0007", "revision": 2})
        );
        assert_eq!(
            recovered.repair,
            Some(json!({
                "kind": "tool_args",
                "source": "schema",
                "strategy": "schema_type_recovery",
                "count": 1,
            }))
        );
    }

    #[test]
    fn schema_recovery_restores_desktop_bbox_through_ref_and_any_of() {
        let schema = json!({
            "$defs": {
                "coord": {"type": "integer"},
                "bbox": {
                    "anyOf": [
                        {"type": "array", "minItems": 2, "maxItems": 2, "items": {"$ref": "#/$defs/coord"}},
                        {"type": "array", "minItems": 4, "maxItems": 4, "items": {"$ref": "#/$defs/coord"}}
                    ]
                }
            },
            "oneOf": [{
                "type": "object",
                "properties": {
                    "action": {"const": "left_click"},
                    "bbox": {"$ref": "#/$defs/bbox"}
                },
                "required": ["action", "bbox"],
                "additionalProperties": false
            }]
        });
        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({"action": "left_click", "bbox": "[10,20,30,40]"}),
            &schema,
        );

        assert_eq!(
            recovered.value,
            json!({"action": "left_click", "bbox": [10, 20, 30, 40]})
        );
        assert_eq!(recovered.repair.unwrap()["count"], 1);
    }

    #[test]
    fn schema_recovery_handles_schedule_object_boolean_and_all_of() {
        let schema = json!({
            "$defs": {
                "schedule": {
                    "allOf": [
                        {"type": "object"},
                        {
                            "properties": {
                                "kind": {"type": "string"},
                                "every_ms": {"type": "integer"}
                            },
                            "additionalProperties": false
                        }
                    ]
                }
            },
            "oneOf": [{
                "type": "object",
                "properties": {
                    "action": {"const": "add"},
                    "schedule": {"$ref": "#/$defs/schedule"},
                    "enabled": {"type": "boolean"},
                    "message": {"type": "string"}
                },
                "required": ["action", "schedule", "enabled", "message"],
                "additionalProperties": false
            }]
        });
        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({
                "action": "add",
                "schedule": "{\"kind\":\"every\",\"every_ms\":\"5000\"}",
                "enabled": "false",
                "message": "007"
            }),
            &schema,
        );

        assert_eq!(
            recovered.value,
            json!({
                "action": "add",
                "schedule": {"kind": "every", "every_ms": 5000},
                "enabled": false,
                "message": "007"
            })
        );
        assert_eq!(recovered.repair.unwrap()["count"], 3);
    }

    #[test]
    fn schema_recovery_keeps_ambiguous_union_string_values() {
        let schema = json!({
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "integer"}
                    ]
                }
            }
        });

        let recovered =
            recover_tool_args_value_against_schema_with_meta(&json!({"value": "42"}), &schema);

        assert_eq!(recovered.value, json!({"value": "42"}));
        assert_eq!(recovered.repair, None);
    }

    #[test]
    fn schema_recovery_keeps_json_text_allowed_by_constrained_union() {
        for keyword in ["anyOf", "oneOf"] {
            let mut value_schema = json!({});
            value_schema[keyword] = json!([
                {"$ref": "#/$defs/text"},
                {"type": "array", "minItems": 1, "maxItems": 100, "items": {"type": "string"}}
            ]);
            let schema = json!({
                "$defs": {"text": {"type": "string"}},
                "type": "object",
                "properties": {"value": value_schema}
            });
            let args = json!({"value": "[\"sample\"]"});
            let recovered = recover_tool_args_value_against_schema_with_meta(&args, &schema);

            assert_eq!(recovered.value, args, "{keyword} must retain valid text");
            assert_eq!(recovered.repair, None);
        }
    }

    #[test]
    fn schema_recovery_applies_additional_properties_only_to_unknown_fields() {
        let schema = json!({
            "type": "object",
            "properties": {
                "label": {"type": "string"}
            },
            "additionalProperties": {"type": "integer"}
        });

        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({"label": "007", "attempt": "2"}),
            &schema,
        );

        assert_eq!(recovered.value, json!({"label": "007", "attempt": 2}));
        assert_eq!(recovered.repair.unwrap()["count"], 1);
    }

    #[test]
    fn schema_recovery_keeps_invalid_and_empty_integer_strings() {
        let schema = json!({
            "type": "object",
            "properties": {
                "empty": {"type": "integer"},
                "decimal": {"type": "integer"}
            }
        });

        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({"empty": "", "decimal": "2.5"}),
            &schema,
        );

        assert_eq!(recovered.value, json!({"empty": "", "decimal": "2.5"}));
        assert_eq!(recovered.repair, None);
    }

    #[test]
    fn schema_recovery_bounds_recursive_ref_cycles() {
        let schema = json!({
            "$defs": {
                "loop": {"$ref": "#/$defs/loop"}
            },
            "$ref": "#/$defs/loop"
        });

        let recovered =
            recover_tool_args_value_against_schema_with_meta(&json!({"value": "[1,2,3]"}), &schema);

        assert_eq!(recovered.value, json!({"value": "[1,2,3]"}));
        assert_eq!(recovered.repair, None);
    }

    #[test]
    fn schema_recovery_does_not_apply_items_twice_after_prefix_items() {
        let schema = json!({
            "type": "object",
            "properties": {
                "values": {
                    "type": "array",
                    "prefixItems": [{"type": "integer"}],
                    "items": {"type": "string"}
                }
            }
        });

        let recovered = recover_tool_args_value_against_schema_with_meta(
            &json!({"values": "[\"1\",\"007\"]"}),
            &schema,
        );

        assert_eq!(recovered.value, json!({"values": [1, "007"]}));
        assert_eq!(recovered.repair.unwrap()["count"], 2);
    }

    #[test]
    fn sanitize_tool_call_payload_counts_each_repaired_arguments_field() {
        let payload = json!([
            {
                "function": {
                    "name": "one",
                    "arguments": "plain text"
                }
            },
            {
                "function": {
                    "name": "two",
                    "arguments": "[\"alpha\"]"
                }
            }
        ]);

        let sanitized = sanitize_tool_call_payload_with_meta(&payload);

        assert_eq!(
            sanitized.repair,
            Some(json!({
                "kind": "tool_call_payload",
                "source": "function.arguments",
                "strategy": "sanitize_before_request",
                "count": 2
            }))
        );
        assert_eq!(
            sanitized.value[0]["function"]["arguments"],
            json!("{\"raw\":\"plain text\"}")
        );
        assert_eq!(
            sanitized.value[1]["function"]["arguments"],
            json!("{\"value\":[\"alpha\"]}")
        );
    }

    #[test]
    fn sanitize_tool_call_payload_normalizes_nested_arguments() {
        let payload = json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\": \"python3 -c \\\"\nprint('sample')\n\\\"\"}"
                }
            }
        ]);

        let sanitized = sanitize_tool_call_payload(&payload);
        assert_eq!(
            sanitized,
            json!([
                {
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "execute_command",
                        "arguments": "{\"content\":\"python3 -c \\\"\\nprint('sample')\\n\\\"\"}"
                    }
                }
            ])
        );
    }

    #[test]
    fn sanitize_tool_call_payload_with_meta_counts_repairs() {
        let payload = json!([
            {
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "execute_command",
                    "arguments": "{\"content\": \"python3 -c \\\"\nprint('sample')\n\\\"\"}"
                }
            }
        ]);

        let sanitized = sanitize_tool_call_payload_with_meta(&payload);
        assert_eq!(
            sanitized.repair,
            Some(json!({
                "kind": "tool_call_payload",
                "source": "function.arguments",
                "strategy": "sanitize_before_request",
                "count": 1
            }))
        );
    }
}
