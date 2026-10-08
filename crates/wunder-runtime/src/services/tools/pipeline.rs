//! 工具执行管线：进入具体工具前统一做参数对齐。
//!
//! 与 dsh 的 `tool/call -> pre-execute -> execute -> post-execute` 管线呼应，
//! 这里承担最小但关键的一步：把外部（模型/连接器）传入的 dsh 风格参数名，
//! 回填为内置工具既有的主键名。只新增缺省的兼容键，绝不覆盖调用方显式传入
//! 的字段，因此对既有行为是安全增量。

use super::registry;
use serde_json::Value;

/// 按注册表描述符对齐参数：若主键缺省而别名存在，则用别名回填主键。
///
/// 仅在确有回填时返回 `Some`，否则返回 `None` 以避免热路径上的无谓克隆。
pub(crate) fn normalize_args(name: &str, args: &Value) -> Option<Value> {
    let descriptor = registry::descriptor(name)?;
    if descriptor.arg_aliases.is_empty() {
        return None;
    }
    let Value::Object(map) = args else {
        return None;
    };
    let mut normalized: Option<serde_json::Map<String, Value>> = None;
    for (primary, alias) in descriptor.arg_aliases {
        if map.contains_key(*primary) {
            continue;
        }
        if let Some(value) = map.get(*alias) {
            if !value.is_null() {
                normalized
                    .get_or_insert_with(|| map.clone())
                    .insert((*primary).to_string(), value.clone());
            }
        }
    }
    normalized.map(Value::Object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fills_primary_from_alias() {
        let normalized = normalize_args("编辑", &json!({"path": "a.txt", "old_string": "x"}))
            .expect("should normalize");
        assert_eq!(normalized["file_path"], "a.txt");
        assert_eq!(normalized["old_string"], "x");
    }

    #[test]
    fn does_not_override_explicit_primary() {
        let normalized = normalize_args(
            "编辑",
            &json!({"file_path": "primary.txt", "path": "alias.txt"}),
        );
        // primary 已存在，无需回填 → 返回 None，调用方沿用原参数。
        assert!(normalized.is_none());
    }

    #[test]
    fn maps_read_file_path_alias() {
        let normalized =
            normalize_args("read_file", &json!({"file_path": "b.txt"})).expect("normalized");
        assert_eq!(normalized["path"], "b.txt");
    }

    #[test]
    fn unknown_tool_is_unchanged() {
        assert!(normalize_args("不存在的工具", &json!({"a": 1})).is_none());
        assert!(normalize_args("列出文件", &json!({"path": "x"})).is_none());
    }
}
