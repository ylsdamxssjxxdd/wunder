//! 统一的工具 input_schema 构建器。
//!
//! 目标：让内置工具的 JSON Schema 由同一套 DSL 生成，避免每个工具手写
//! `json!({...})` 造成字段约定漂移，并让参数命名与描述能集中对齐到
//! dsh 的基础工具约定（file_path / offset / limit / old_string …）。
//!
//! 设计要点：
//! - 只暴露「参数构造器 + 链式修饰」，内部统一产出 JSON Schema 关键字；
//! - 覆盖 dsh 基础工具常用关键字：type / enum / minimum / maximum /
//!   minLength / maxLength / minItems / maxItems / pattern / format / default /
//!   items / properties / required / additionalProperties；
//! - 顶层 `object()` 默认 `additionalProperties=false`，并通过 required
//!   自动收集必填项；嵌套对象用 `object_param()` + `.props()` 构造。
//!
//! 注意：类型白名单见 `wunder-core/src/json_schema.rs`（object / array /
//! string / number / integer / boolean，不含 null），不要产出 `null` 类型；
//! 可选性一律通过 `required` 控制，不要使用 `type:["x","null"]`。

use serde_json::{json, Map, Value};

/// 单个工具参数描述。
///
/// 内部直接持有该参数的 JSON Schema 片段（`schema`），描述与必填性单独存放，
/// 最终由 [`Param::to_schema`] 合并输出。`name` 仅在作为对象属性时使用。
#[derive(Clone)]
pub(crate) struct Param {
    name: &'static str,
    schema: Map<String, Value>,
    description: String,
    required: bool,
}

fn base(name: &'static str, kind: &str) -> Param {
    let mut schema = Map::new();
    schema.insert("type".to_string(), json!(kind));
    Param {
        name,
        schema,
        description: String::new(),
        required: true,
    }
}

/// 构造一个必填的字符串参数。
pub(crate) fn string(name: &'static str) -> Param {
    base(name, "string")
}

/// 构造一个必填的布尔参数。
pub(crate) fn boolean(name: &'static str) -> Param {
    base(name, "boolean")
}

/// 构造一个必填的整数参数。
pub(crate) fn integer(name: &'static str) -> Param {
    base(name, "integer")
}

/// 构造一个必填的数值（浮点）参数。
pub(crate) fn number(name: &'static str) -> Param {
    base(name, "number")
}

/// 构造一个必填的数组参数；元素 schema 通过 [`Param::items`] 设置。
pub(crate) fn array(name: &'static str) -> Param {
    base(name, "array")
}

/// 构造一个必填的对象参数；属性通过 [`Param::props`] 设置。
pub(crate) fn object_param(name: &'static str) -> Param {
    base(name, "object")
}

impl Param {
    pub(crate) fn desc(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// 标记为可选（默认所有参数均为必填）。
    pub(crate) fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    /// 限定字符串枚举取值。
    pub(crate) fn enums(mut self, values: Vec<&'static str>) -> Self {
        self.schema.insert("enum".to_string(), json!(values));
        self
    }

    /// 数值下界（`minimum`）。
    pub(crate) fn min(mut self, value: i64) -> Self {
        self.schema.insert("minimum".to_string(), json!(value));
        self
    }

    /// 数值上界（`maximum`）。
    pub(crate) fn max(mut self, value: i64) -> Self {
        self.schema.insert("maximum".to_string(), json!(value));
        self
    }

    /// 字符串最小长度（`minLength`）。
    pub(crate) fn min_len(mut self, value: usize) -> Self {
        self.schema.insert("minLength".to_string(), json!(value));
        self
    }

    /// 字符串最大长度（`maxLength`）。
    pub(crate) fn max_len(mut self, value: usize) -> Self {
        self.schema.insert("maxLength".to_string(), json!(value));
        self
    }

    /// 数组最小元素数（`minItems`）。
    pub(crate) fn min_items(mut self, value: usize) -> Self {
        self.schema.insert("minItems".to_string(), json!(value));
        self
    }

    /// 数组最大元素数（`maxItems`）。
    pub(crate) fn max_items(mut self, value: usize) -> Self {
        self.schema.insert("maxItems".to_string(), json!(value));
        self
    }

    /// 字符串正则约束（`pattern`）。
    pub(crate) fn pattern(mut self, value: &str) -> Self {
        self.schema.insert("pattern".to_string(), json!(value));
        self
    }

    /// 字符串格式提示（`format`）。
    pub(crate) fn format(mut self, value: &str) -> Self {
        self.schema.insert("format".to_string(), json!(value));
        self
    }

    /// 默认值（`default`）。
    pub(crate) fn default_value(mut self, value: Value) -> Self {
        self.schema.insert("default".to_string(), value);
        self
    }

    /// 数组元素 schema（取传入参数的 schema 片段，忽略其 name/required）。
    pub(crate) fn items(mut self, item: Param) -> Self {
        self.schema.insert("items".to_string(), item.to_schema());
        self
    }

    /// 对象参数的属性集合，并按必填性自动收集 `required`。
    pub(crate) fn props(mut self, params: Vec<Param>) -> Self {
        let (properties, required) = collect_properties(params);
        self.schema
            .insert("properties".to_string(), Value::Object(properties));
        if !required.is_empty() {
            let required: Vec<Value> = required.into_iter().map(|name| json!(name)).collect();
            self.schema
                .insert("required".to_string(), Value::Array(required));
        }
        self
    }

    /// 允许携带额外字段（`additionalProperties = true`）。
    pub(crate) fn open(mut self) -> Self {
        self.schema
            .insert("additionalProperties".to_string(), json!(true));
        self
    }

    /// 拒绝额外字段（`additionalProperties = false`）。
    pub(crate) fn closed(mut self) -> Self {
        self.schema
            .insert("additionalProperties".to_string(), json!(false));
        self
    }

    /// 逃生口：直接挂载 `anyOf` 子 schema 列表（DSL 尚未覆盖的约束，如
    /// 「长度为 4 或 2 的整数数组」）。仅在无法用现有修饰器表达时使用。
    pub(crate) fn any_of(mut self, schemas: Vec<Value>) -> Self {
        self.schema
            .insert("anyOf".to_string(), Value::Array(schemas));
        self
    }

    fn to_schema(&self) -> Value {
        let mut map = self.schema.clone();
        if !self.description.is_empty() {
            map.insert("description".to_string(), json!(self.description));
        }
        Value::Object(map)
    }
}

/// 收集对象属性与必填项列表。
fn collect_properties(params: Vec<Param>) -> (Map<String, Value>, Vec<&'static str>) {
    let mut properties = Map::new();
    let mut required: Vec<&'static str> = Vec::new();
    for param in params {
        if param.required {
            required.push(param.name);
        }
        properties.insert(param.name.to_string(), param.to_schema());
    }
    (properties, required)
}

/// 由参数列表构建 object schema；未知字段一律拒绝（additionalProperties=false）。
pub(crate) fn object(params: Vec<Param>) -> Value {
    let (properties, required) = collect_properties(params);
    let mut root = Map::new();
    root.insert("type".to_string(), json!("object"));
    root.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        let required: Vec<Value> = required.into_iter().map(|name| json!(name)).collect();
        root.insert("required".to_string(), Value::Array(required));
    }
    root.insert("additionalProperties".to_string(), json!(false));
    Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_schema_collects_required_and_properties() {
        let schema = object(vec![
            string("file_path").desc("path"),
            string("old_string").desc("old"),
            string("new_string").desc("new"),
            boolean("replace_all").desc("all").optional(),
        ]);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["file_path"]["type"], "string");
        assert_eq!(schema["properties"]["replace_all"]["type"], "boolean");
        let required = schema["required"].as_array().expect("required array");
        assert_eq!(required.len(), 3);
        assert!(required.iter().any(|v| v == "file_path"));
        assert!(!required.iter().any(|v| v == "replace_all"));
    }

    #[test]
    fn object_schema_omits_required_when_all_optional() {
        let schema = object(vec![string("path").optional()]);
        assert!(schema.get("required").is_none());
        assert_eq!(schema["additionalProperties"], false);
    }

    #[test]
    fn enum_values_are_emitted() {
        let schema = object(vec![string("mode").desc("m").enums(vec!["a", "b"])]);
        assert_eq!(schema["properties"]["mode"]["enum"], json!(["a", "b"]));
    }

    #[test]
    fn number_and_bounds_are_emitted() {
        let schema = object(vec![
            number("timeout_s").desc("t"),
            integer("limit").desc("l").min(1).max(500).optional(),
        ]);
        assert_eq!(schema["properties"]["timeout_s"]["type"], "number");
        assert_eq!(schema["properties"]["limit"]["minimum"], 1);
        assert_eq!(schema["properties"]["limit"]["maximum"], 500);
    }

    #[test]
    fn array_items_and_bounds_are_emitted() {
        let schema = object(vec![
            array("sites").desc("s").items(string("_")).optional(),
            array("counts")
                .desc("c")
                .items(integer("_"))
                .min_items(1)
                .max_items(10)
                .optional(),
        ]);
        assert_eq!(schema["properties"]["sites"]["type"], "array");
        assert_eq!(schema["properties"]["sites"]["items"]["type"], "string");
        assert_eq!(schema["properties"]["counts"]["items"]["type"], "integer");
        assert_eq!(schema["properties"]["counts"]["minItems"], 1);
        assert_eq!(schema["properties"]["counts"]["maxItems"], 10);
    }

    #[test]
    fn string_length_and_pattern_and_format_are_emitted() {
        let schema = object(vec![
            string("message")
                .desc("m")
                .min_len(1)
                .max_len(20000)
                .optional(),
            string("id").desc("i").pattern("^[a-z]+$").optional(),
            string("when").desc("w").format("date-time").optional(),
        ]);
        assert_eq!(schema["properties"]["message"]["minLength"], 1);
        assert_eq!(schema["properties"]["message"]["maxLength"], 20000);
        assert_eq!(schema["properties"]["id"]["pattern"], "^[a-z]+$");
        assert_eq!(schema["properties"]["when"]["format"], "date-time");
    }

    #[test]
    fn nested_object_props_and_closed_and_open() {
        let schema = object(vec![
            object_param("schedule")
                .desc("s")
                .props(vec![
                    string("kind").desc("k").enums(vec!["at", "every", "cron"]),
                    integer("every_ms").desc("e").min(1000).optional(),
                ])
                .closed()
                .optional(),
            object_param("extra").desc("x").open().optional(),
        ]);
        assert_eq!(schema["properties"]["schedule"]["type"], "object");
        assert_eq!(
            schema["properties"]["schedule"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema["properties"]["schedule"]["required"],
            json!(["kind"])
        );
        assert_eq!(
            schema["properties"]["schedule"]["properties"]["kind"]["enum"],
            json!(["at", "every", "cron"])
        );
        assert_eq!(schema["properties"]["extra"]["additionalProperties"], true);
    }

    #[test]
    fn array_of_objects_matches_plan_shape() {
        // 对齐「计划面板」plan 字段的原始手写 json! 形状。
        let schema = object(vec![
            string("explanation").desc("e").optional(),
            array("plan").desc("p").min_items(1).max_items(12).items(
                object_param("_")
                    .props(vec![
                        string("step").desc("s"),
                        string("status").desc("st").enums(vec![
                            "pending",
                            "in_progress",
                            "completed",
                        ]),
                    ])
                    .closed(),
            ),
        ]);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"], json!(["plan"]));
        let plan = &schema["properties"]["plan"];
        assert_eq!(plan["type"], "array");
        assert_eq!(plan["minItems"], 1);
        assert_eq!(plan["maxItems"], 12);
        assert_eq!(plan["items"]["type"], "object");
        assert_eq!(plan["items"]["additionalProperties"], false);
        assert_eq!(plan["items"]["required"], json!(["step", "status"]));
        assert_eq!(
            plan["items"]["properties"]["status"]["enum"],
            json!(["pending", "in_progress", "completed"])
        );
    }
}
