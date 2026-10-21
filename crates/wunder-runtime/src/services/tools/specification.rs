//! Stable full/compact/tiny tool specifications and dynamic schema limits.
use crate::schemas::ToolSpec;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::{self, Write};

pub const TOOL_CATALOG_VERSION: &str = "tools-v2";
pub const MAX_DYNAMIC_DESCRIPTION_BYTES: usize = 8 * 1024;
pub const MAX_DYNAMIC_SCHEMA_BYTES: usize = 64 * 1024;
const MAX_SCHEMA_DEPTH: usize = 24;
const MAX_SCHEMA_NODES: usize = 4096;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSchemaReference {
    pub schema_ref: String,
    pub catalog_version: String,
    pub schema_hash: String,
}

pub fn schema_reference(_tool_id: &str, schema: &Value) -> ToolSchemaReference {
    let schema_hash = canonical_hash(schema);
    ToolSchemaReference {
        schema_ref: format!("tool-schema:{schema_hash}"),
        catalog_version: TOOL_CATALOG_VERSION.into(),
        schema_hash,
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    Minimal,
    Development,
    Research,
    Media,
    Desktop,
    Runtime,
    Dynamic,
    Authorized,
}

impl ToolProfile {
    /// Parse the stable profile names accepted by runtime configuration and
    /// request overrides. Unknown values are rejected by returning `None` so
    /// a typo cannot silently broaden a restricted tool set.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "minimal" | "default" => Some(Self::Minimal),
            "development" | "develop" | "dev" => Some(Self::Development),
            "research" | "researcher" => Some(Self::Research),
            "media" | "multimodal" => Some(Self::Media),
            "desktop" | "local" => Some(Self::Desktop),
            "runtime" | "orchestration" => Some(Self::Runtime),
            "authorized" | "full" | "all" => Some(Self::Authorized),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Development => "development",
            Self::Research => "research",
            Self::Media => "media",
            Self::Desktop => "desktop",
            Self::Runtime => "runtime",
            Self::Dynamic => "dynamic",
            Self::Authorized => "authorized",
        }
    }
}

/// Resolve a profile from a user/config value. Empty values intentionally
/// retain the historical all-authorized behavior.
pub fn resolve_tool_profile(value: Option<&str>) -> ToolProfile {
    value
        .and_then(ToolProfile::parse)
        .unwrap_or(ToolProfile::Authorized)
}

/// Restrict an already permission-checked set of tools to one named profile.
/// Dynamic/user tools remain available to named profiles because they are
/// explicit capabilities selected by the caller; the profile only removes
/// unrelated built-in families. `Authorized` is the compatibility profile.
pub fn filter_tool_names_by_profile(
    names: std::collections::HashSet<String>,
    profile: ToolProfile,
) -> std::collections::HashSet<String> {
    if matches!(profile, ToolProfile::Authorized | ToolProfile::Dynamic) {
        return names;
    }
    names
        .into_iter()
        .filter(|name| {
            let canonical = super::resolve_tool_name(name);
            let category = tool_profile(&canonical);
            crate::services::goal::is_goal_tool_name(&canonical)
                || category == ToolProfile::Minimal
                || category == profile
                || category == ToolProfile::Dynamic
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    Read,
    Write,
    Execute,
    Interactive,
    Mixed,
}

#[derive(Debug, Clone, Serialize)]
pub struct TinyToolSpec {
    pub id: String,
    pub label: String,
    pub effect: ToolEffect,
    pub profile: ToolProfile,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolSpecification {
    pub full: ToolSpec,
    pub compact: ToolSpec,
    pub tiny: TinyToolSpec,
    pub reference: ToolSchemaReference,
}

impl ToolSpecification {
    pub fn from_full(full: ToolSpec) -> Self {
        let id = super::resolve_tool_name(&full.name);
        let reference = schema_reference(&id, &tool_schema_document(&full));
        let compact = compact_tool_spec(&full);
        let tiny = TinyToolSpec {
            id: id.clone(),
            label: full.title.clone().unwrap_or_else(|| full.name.clone()),
            effect: tool_effect(&id),
            profile: tool_profile(&id),
        };
        Self {
            full,
            compact,
            tiny,
            reference,
        }
    }
}

pub fn tool_schema_document(spec: &ToolSpec) -> Value {
    serde_json::json!({
        "name": spec.name,
        "description": spec.description,
        "parameters": spec.input_schema,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProfileBinding {
    pub profile: ToolProfile,
    pub catalog_version: String,
    pub revision: String,
    /// Canonical tool IDs selected when this thread first established its
    /// profile. An empty value is accepted only for pre-ID metadata and is
    /// upgraded on the next execution; it must never add a capability.
    #[serde(default)]
    pub tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolProfileSnapshot {
    pub profile: ToolProfile,
    pub catalog_version: String,
    pub revision: String,
    pub tool_ids: Vec<String>,
    pub tools: Vec<TinyToolSpec>,
}

pub fn tool_profile_snapshot(specs: &[ToolSpec]) -> ToolProfileSnapshot {
    tool_profile_snapshot_for_profile(specs, ToolProfile::Authorized)
}

pub fn tool_profile_snapshot_for_profile(
    specs: &[ToolSpec],
    profile: ToolProfile,
) -> ToolProfileSnapshot {
    let mut sorted = BTreeMap::new();
    // Callers pass the final permission-checked selected set. Do not apply
    // category filtering here: dynamic capabilities have no static category
    // but must still contribute to the thread revision when explicitly
    // authorized.
    for spec in specs {
        let id = super::resolve_tool_name(&spec.name);
        sorted.insert(id.clone(), ToolSpecification::from_full(spec.clone()));
    }
    let tool_ids = sorted.keys().cloned().collect::<Vec<_>>();
    let tools = sorted
        .values()
        .map(|item| item.tiny.clone())
        .collect::<Vec<_>>();
    let revision = canonical_hash(
        &serde_json::json!({"version":TOOL_CATALOG_VERSION,"tools":sorted.values().map(|item| tool_schema_document(&item.compact)).collect::<Vec<_>>() }),
    );
    ToolProfileSnapshot {
        profile,
        catalog_version: TOOL_CATALOG_VERSION.into(),
        revision,
        tool_ids,
        tools,
    }
}

pub fn tool_profile_binding(snapshot: &ToolProfileSnapshot) -> ToolProfileBinding {
    ToolProfileBinding {
        profile: snapshot.profile,
        catalog_version: snapshot.catalog_version.clone(),
        revision: snapshot.revision.clone(),
        tool_ids: snapshot.tool_ids.clone(),
    }
}

/// Apply the frozen canonical snapshot after the current permissions and
/// profile policy. This can only remove capabilities: a user access change or
/// temporarily unavailable tool never comes back merely because it appeared
/// in an older binding.
pub fn filter_tool_names_by_frozen_ids(
    names: HashSet<String>,
    frozen_tool_ids: &[String],
) -> HashSet<String> {
    let frozen: HashSet<&str> = frozen_tool_ids
        .iter()
        .map(String::as_str)
        .filter(|name| !name.trim().is_empty())
        .collect();
    names
        .into_iter()
        .filter(|name| frozen.contains(super::resolve_tool_name(name).as_str()))
        .collect()
}

pub fn select_tool_profile(specs: &[ToolSpec], profile: ToolProfile) -> Vec<ToolSpec> {
    let mut output = specs
        .iter()
        .filter(|spec| {
            let category = tool_profile(&super::resolve_tool_name(&spec.name));
            profile == ToolProfile::Authorized
                || category == ToolProfile::Minimal
                || category == profile
        })
        .cloned()
        .collect::<Vec<_>>();
    output.sort_by_cached_key(|spec| super::resolve_tool_name(&spec.name));
    output
}

/// Compact removes UI/examples annotations while preserving descriptions and all validation constraints.
pub fn compact_tool_spec(spec: &ToolSpec) -> ToolSpec {
    let mut result = spec.clone();
    result.title = None;
    result.input_schema = compact_schema_node(&spec.input_schema);
    result
}
/// Remove presentation-only annotations from schema *nodes*.
///
/// JSON Schema includes arbitrary JSON literals in `default`, `enum`, and
/// `const`, and user-controlled member names below `properties`. Applying a
/// key filter to every JSON object would corrupt those values (for example a
/// perfectly valid `properties.title` parameter). Recurse only through JSON
/// Schema keywords whose values are themselves schemas.
fn compact_schema_node(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    let mut compact = serde_json::Map::with_capacity(map.len());
    for (key, child) in map {
        if is_presentation_annotation(key) {
            continue;
        }
        compact.insert(key.clone(), compact_schema_child(key, child));
    }
    Value::Object(compact)
}

fn is_presentation_annotation(key: &str) -> bool {
    matches!(
        key,
        "title" | "examples" | "example" | "$comment" | "x-ui" | "x-display" | "x-order"
    )
}

fn compact_schema_child(keyword: &str, value: &Value) -> Value {
    match keyword {
        // Maps whose *values* are schemas. Their keys are user-visible
        // property/definition names and must remain intact.
        "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
            compact_schema_map_values(value)
        }
        // A dependency can be an array of property names or a schema.
        "dependencies" => compact_schema_map_object_values(value),
        // Single nested schemas. A boolean is also valid for several of them.
        "additionalProperties"
        | "unevaluatedProperties"
        | "propertyNames"
        | "contains"
        | "not"
        | "if"
        | "then"
        | "else"
        | "contentSchema"
        | "additionalItems" => compact_schema_node(value),
        // Arrays whose elements are schemas.
        "items" | "prefixItems" | "allOf" | "anyOf" | "oneOf" => compact_schema_or_array(value),
        // All other values are JSON-Schema data, including enum/default/const.
        _ => value.clone(),
    }
}

fn compact_schema_map_values(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    Value::Object(
        map.iter()
            .map(|(key, child)| (key.clone(), compact_schema_node(child)))
            .collect(),
    )
}

fn compact_schema_map_object_values(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    Value::Object(
        map.iter()
            .map(|(key, child)| {
                let value = if child.is_object() {
                    compact_schema_node(child)
                } else {
                    child.clone()
                };
                (key.clone(), value)
            })
            .collect(),
    )
}

fn compact_schema_or_array(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(compact_schema_node).collect()),
        _ => compact_schema_node(value),
    }
}

fn tool_profile(name: &str) -> ToolProfile {
    match name {
        "执行命令" | "命令会话" | "列出文件" | "文件名匹配" | "搜索内容" | "读取文件"
        | "写入文件" | "文本编辑" | "计划面板" | "问询面板" | "技能调用" => {
            ToolProfile::Minimal
        }
        "子智能体控制" | "ptc" => ToolProfile::Development,
        "网页抓取" | "网页搜索" => ToolProfile::Research,
        "读取图片" | "读图工具" | "语音生成" | "声转文" | "图像生成" | "视频生成" => {
            ToolProfile::Media
        }
        "桌面控制" => ToolProfile::Desktop,
        "goal" | "self_status" | "会话线程控制" | "会话让出" | "定时任务" => {
            ToolProfile::Runtime
        }
        "browser" => ToolProfile::Research,
        _ => ToolProfile::Dynamic,
    }
}
fn tool_effect(name: &str) -> ToolEffect {
    match name {
        "列出文件" | "文件名匹配" | "搜索内容" | "读取文件" | "读取图片" | "读图工具"
        | "技能调用" | "网页抓取" | "网页搜索" | "self_status" => ToolEffect::Read,
        "goal" => ToolEffect::Mixed,
        "写入文件" | "文本编辑" | "计划面板" => ToolEffect::Write,
        "执行命令" | "命令会话" | "ptc" => ToolEffect::Execute,
        "桌面控制" | "问询面板" => ToolEffect::Interactive,
        "browser" => ToolEffect::Interactive,
        _ => ToolEffect::Mixed,
    }
}

pub fn validate_dynamic_tool_spec(spec: &ToolSpec) -> Result<()> {
    validate_tool_spec_size(spec)?;
    validate_registration_schema(&spec.input_schema)
}

/// Validate a schema obtained from an external provider.
///
/// Provider schemas still need bounded size, depth and node count before they
/// enter the catalog, but imposing the local-registration closure rules would
/// reject valid remote schemas such as open objects or unbounded result
/// filters. Their execution remains isolated behind the provider boundary.
pub fn validate_external_tool_spec(spec: &ToolSpec) -> Result<()> {
    validate_tool_spec_size(spec)
}

pub fn validate_tool_spec_size(spec: &ToolSpec) -> Result<()> {
    if spec.name.is_empty() || spec.name.len() > 256 {
        bail!("tool name must contain 1..256 bytes")
    }
    if spec.description.len() > MAX_DYNAMIC_DESCRIPTION_BYTES {
        bail!("tool description exceeds 8192 bytes")
    }
    let mut writer = LimitedWriter {
        bytes: 0,
        limit: MAX_DYNAMIC_SCHEMA_BYTES,
    };
    serde_json::to_writer(&mut writer, &spec.input_schema)
        .map_err(|_| anyhow::anyhow!("tool schema exceeds 65536 bytes"))?;
    let mut nodes = 0;
    validate_tree(&spec.input_schema, 0, &mut nodes)
}
fn validate_tree(value: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    if depth > MAX_SCHEMA_DEPTH || *nodes > MAX_SCHEMA_NODES {
        bail!("tool schema exceeds depth or node limit")
    }
    match value {
        Value::Object(map) => {
            for v in map.values() {
                validate_tree(v, depth + 1, nodes)?
            }
        }
        Value::Array(items) => {
            for v in items {
                validate_tree(v, depth + 1, nodes)?
            }
        }
        _ => {}
    }
    Ok(())
}
fn validate_registration_schema(schema: &Value) -> Result<()> {
    let Some(map) = schema.as_object() else {
        return Ok(());
    };
    let is_array = map.get("type").is_some_and(|v| v == "array") || map.contains_key("items");
    if is_array
        && !map
            .get("maxItems")
            .and_then(Value::as_u64)
            .is_some_and(|n| n <= 4096)
    {
        bail!("registered arrays require maxItems <= 4096")
    };
    let is_object =
        map.get("type").is_some_and(|v| v == "object") || map.contains_key("properties");
    if is_object && map.get("additionalProperties") != Some(&Value::Bool(false)) {
        bail!("registered objects require additionalProperties=false")
    };
    for (keyword, child) in map {
        validate_registration_child(keyword, child)?;
    }
    Ok(())
}

/// Validate only values that the JSON Schema vocabulary defines as child
/// schemas. Recursing every JSON object would treat a property map as a
/// schema: a legitimate property named `items` would then be mistaken for the
/// array keyword and rejected for lacking `maxItems`.
fn validate_registration_child(keyword: &str, value: &Value) -> Result<()> {
    match keyword {
        "properties" | "patternProperties" | "$defs" | "definitions" | "dependentSchemas" => {
            if let Some(map) = value.as_object() {
                for child in map.values() {
                    validate_registration_schema(child)?;
                }
            }
        }
        "dependencies" => {
            if let Some(map) = value.as_object() {
                for child in map.values().filter(|child| child.is_object()) {
                    validate_registration_schema(child)?;
                }
            }
        }
        "additionalProperties"
        | "unevaluatedProperties"
        | "propertyNames"
        | "contains"
        | "not"
        | "if"
        | "then"
        | "else"
        | "contentSchema"
        | "additionalItems" => validate_registration_schema(value)?,
        "items" | "prefixItems" | "allOf" | "anyOf" | "oneOf" => {
            if let Some(items) = value.as_array() {
                for child in items {
                    validate_registration_schema(child)?;
                }
            } else {
                validate_registration_schema(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

struct LimitedWriter {
    bytes: usize,
    limit: usize,
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Write for LimitedWriter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.len() > self.limit.saturating_sub(self.bytes) {
            return Err(io::Error::other("limit"));
        }
        self.bytes += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn canonical_hash(value: &Value) -> String {
    fn write_value(value: &Value, out: &mut HashWriter) {
        match value {
            Value::Object(map) => {
                let _ = out.write_all(b"{");
                let mut keys = map.keys().collect::<Vec<_>>();
                keys.sort();
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        let _ = out.write_all(b",");
                    }
                    let _ = serde_json::to_writer(&mut *out, key);
                    let _ = out.write_all(b":");
                    write_value(&map[*key], out);
                }
                let _ = out.write_all(b"}");
            }
            Value::Array(items) => {
                let _ = out.write_all(b"[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        let _ = out.write_all(b",");
                    }
                    write_value(item, out);
                }
                let _ = out.write_all(b"]");
            }
            other => {
                let _ = serde_json::to_writer(out, other);
            }
        }
    }
    let mut out = HashWriter(Sha256::new());
    write_value(value, &mut out);
    format!("{:x}", out.0.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            title: Some("UI".into()),
            description: "Read a resource using its cursor.".into(),
            input_schema: serde_json::json!({
                "type":"object",
                "title":"Read input",
                "properties":{
                    "title":{"type":"string","description":"A user supplied title"},
                    "path":{"type":"string","title":"Path","examples":["x"],"minLength":1},
                    "settings":{
                        "type":"object",
                        "additionalProperties":false,
                        "default":{"title":"default title","examples":["literal example"]},
                        "const":{"title":"fixed title"},
                        "enum":[{"title":"enum title"}]
                    }
                },
                "required":["path"],
                "additionalProperties":false
            }),
        }
    }
    #[test]
    fn compact_keeps_constraints_and_removes_ui_annotations() {
        let layers = ToolSpecification::from_full(spec("read_file"));
        assert!(layers.full.input_schema["properties"]["path"]["examples"].is_array());
        assert!(layers.compact.input_schema["properties"]["path"]
            .get("examples")
            .is_none());
        assert_eq!(
            layers.compact.input_schema["properties"]["path"]["minLength"],
            1
        );
        assert!(layers.compact.input_schema.get("title").is_none());
        assert!(layers.compact.input_schema["properties"]
            .get("title")
            .is_some());
        assert_eq!(
            layers.compact.input_schema["properties"]["settings"]["default"]["title"],
            "default title"
        );
        assert_eq!(
            layers.compact.input_schema["properties"]["settings"]["const"]["title"],
            "fixed title"
        );
        assert_eq!(
            layers.compact.input_schema["properties"]["settings"]["enum"][0]["title"],
            "enum title"
        );
        assert_eq!(layers.tiny.effect, ToolEffect::Read)
    }
    #[test]
    fn hash_is_order_independent() {
        let a = serde_json::json!({"a":1,"b":2});
        let b = serde_json::json!({"b":2,"a":1});
        assert_eq!(schema_reference("x", &a), schema_reference("x", &b))
    }
    #[test]
    fn authorized_snapshot_is_stable() {
        let a = vec![spec("read_file"), spec("browser")];
        let b = a.iter().rev().cloned().collect::<Vec<_>>();
        assert_eq!(
            tool_profile_snapshot(&a).revision,
            tool_profile_snapshot(&b).revision
        );
        assert_eq!(select_tool_profile(&a, ToolProfile::Authorized).len(), 2)
    }
    #[test]
    fn named_profile_keeps_minimal_matching_and_dynamic_capabilities() {
        let names = ["执行命令", "网页抓取", "桌面控制", "remote_service@lookup"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let selected = filter_tool_names_by_profile(names, ToolProfile::Research);

        assert!(selected.contains("执行命令"));
        assert!(selected.contains("网页抓取"));
        assert!(selected.contains("remote_service@lookup"));
        assert!(!selected.contains("桌面控制"));
    }

    #[test]
    fn profile_binding_carries_selected_catalog_revision() {
        let snapshot =
            tool_profile_snapshot_for_profile(&[spec("read_file")], ToolProfile::Minimal);
        let binding = tool_profile_binding(&snapshot);

        assert_eq!(binding.profile, ToolProfile::Minimal);
        assert_eq!(binding.catalog_version, TOOL_CATALOG_VERSION);
        assert_eq!(binding.revision, snapshot.revision);
        assert_eq!(binding.tool_ids, vec!["读取文件".to_string()]);
    }

    #[test]
    fn frozen_ids_only_narrow_the_current_profile_surface() {
        let names = ["执行命令", "网页抓取", "remote_service@lookup"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let current = filter_tool_names_by_profile(names, ToolProfile::Research);
        let frozen = filter_tool_names_by_frozen_ids(
            current,
            &["网页抓取".to_string(), "removed_or_denied".to_string()],
        );

        assert_eq!(frozen, HashSet::from(["网页抓取".to_string()]));
    }
    #[test]
    fn registration_rejects_unbounded_array() {
        let mut s = spec("remote");
        assert!(validate_dynamic_tool_spec(&s).is_ok());
        s.input_schema["properties"]["items"] =
            serde_json::json!({"type":"array","items":{"type":"string"}});
        assert!(validate_dynamic_tool_spec(&s).is_err());
        s.input_schema["properties"]["items"]["maxItems"] = serde_json::json!(32);
        validate_dynamic_tool_spec(&s).expect("bounded array remains a valid registration schema");
        s.input_schema["properties"]["payload"] = serde_json::json!({
            "type":"object",
            "properties":{"items":{"type":"string"}},
            "additionalProperties":false
        });
        validate_dynamic_tool_spec(&s)
            .expect("a property named items is not an array schema keyword");
    }
}
