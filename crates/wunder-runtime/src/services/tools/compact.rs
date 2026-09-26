//! Model-facing tool descriptions.
//!
//! Admin pages and execution diagnostics keep the full tool catalog.  The
//! model request only needs a short action description and the JSON shape, so
//! this module removes prose and schema metadata that do not affect dispatch.

use crate::schemas::ToolSpec;
use serde_json::{Map, Value};

/// Return a compact copy of a tool spec for a model request.
pub(crate) fn compact_tool_spec_for_model(spec: &ToolSpec) -> ToolSpec {
    ToolSpec {
        name: spec.name.clone(),
        title: None,
        description: compact_tool_description(&spec.name, &spec.description),
        input_schema: compact_schema(&spec.input_schema),
    }
}

/// Compact all model-facing specs while preserving names and validation keys.
pub(crate) fn compact_tool_specs_for_model(specs: &[ToolSpec]) -> Vec<ToolSpec> {
    specs.iter().map(compact_tool_spec_for_model).collect()
}

fn compact_tool_description(name: &str, original: &str) -> String {
    let canonical = name
        .trim()
        .rsplit('@')
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    let fixed = match canonical.as_str() {
        "最终回复" | "final_response" => Some("提交最终回复；content 必填。"),
        "定时任务" | "schedule_task" => Some("管理定时任务；action 必填。"),
        "休眠等待" | "sleep_wait" | "sleep" => {
            Some("主动等待指定秒数；仅用于确实需要延迟的流程，不用于等待命令完成。")
        }
        "记忆管理" | "memory_manager" => {
            Some("管理长期记忆；系统提示词只放索引，详情按需读取。")
        }
        "执行命令" | "execute_command" => Some(
            "执行命令；短命令直接返回，仍在运行时返回 command_session_id；继续其他工作后用命令会话轮询，不要用休眠等待命令。",
        ),
        "命令会话" | "command_session" | "write_command_stdin" => Some(
            "轮询后台命令或写入 stdin；使用 execute_command 返回的 command_session_id，直到 status=completed。",
        ),
        "ptc" => Some("执行 PTC 脚本；受权限、工作区和超时限制。"),
        "列出文件" | "list_files" => Some("列出允许范围内的文件；结果限量。"),
        "搜索内容" | "search_content" => Some("搜索允许路径中的文本；结果限量，优先缩小范围。"),
        "读取文件" | "read_file" => Some("读取允许路径的纯文本；支持行范围，结果会裁剪。"),
        "读图工具" | "read_image" => Some("读取允许范围内的图片；大结果会裁剪。"),
        "语音生成" | "generate_speech" => Some("生成语音；输入和输出受配置限制。"),
        "声转文" | "transcribe_speech" => Some("将语音转为文字；输入受大小和格式限制。"),
        "图像生成" | "generate_image" => Some("生成图像；prompt 必填，输出路径受限。"),
        "视频生成" | "generate_video" => Some("生成视频；prompt 必填，输出路径受限。"),
        "技能调用" | "skill_call" => Some("加载已挂载技能；命中技能后先调用。"),
        "写入文件" | "write_file" => Some("写入文件；路径受允许范围限制，支持 dry_run。"),
        "文本编辑" | "edit_file" | "edit_file2" => {
            Some("按文本匹配编辑文件；路径受限，支持 dry_run。")
        }
        "应用补丁" | "apply_patch" => Some("应用精确补丁；文件路径受限，支持 dry_run。"),
        "lsp查询" | "lsp_query" => Some("查询代码定义、引用和符号；路径受限。"),
        "子智能体控制" | "subagent_control" => Some(
            "管理主智能体创建的子智能体；可 send、resume、wait、cancel、report，运行中子线程随主线程中断取消。",
        ),
        "智能体蜂群" | "agent_swarm" => {
            Some("调用已存在的智能体协作；默认阻塞并汇总，不等同子智能体。")
        }
        "节点调用" | "node_invoke" => Some("调用已授权节点；受权限、参数和超时限制。"),
        "网页搜索" | "web_search" => Some("搜索网页；query 必填，结果限量。"),
        "网页抓取" | "web_fetch" => Some("抓取明确 URL；不是搜索，不猜测 URL。"),
        "浏览器" | "browser" => Some("操作浏览器；受会话、域名和超时限制。"),
        "用户世界工具" | "user_world" => Some("查询或联系用户；动作和权限由 action 决定。"),
        "频道工具" | "channel" => Some("查询或发送渠道消息；发送动作需明确目标。"),
        "a2ui" => Some("创建或更新界面；消息必须符合 A2UI 结构。"),
        _ if canonical.contains("数据库导出")
            || canonical.starts_with("db_export")
            || (original.contains("导出") && original.contains("SQL")) =>
        {
            Some("导出绑定范围内的只读 SQL 结果；路径受限。")
        }
        _ if canonical.contains("数据库查询") || original.contains("只读 SQL") => {
            Some("执行绑定范围内的只读 SQL；结果限量，超出需分页。")
        }
        _ if canonical.contains("ppt") || canonical.contains("演示文稿") => {
            Some("创建、读取、修改或删除 PPTX；内容和路径受限。")
        }
        _ if canonical.starts_with("db_query") => {
            Some("执行绑定范围内的只读 SQL；结果限量，超出需分页。")
        }
        _ if canonical.starts_with("kb_query") || canonical.starts_with("knowledge") => {
            Some("检索知识库并返回紧凑片段；query 或 keywords 必填。")
        }
        _ if canonical.starts_with("ppt_") => {
            Some("创建、读取、修改或删除 PPTX；内容和路径受限。")
        }
        _ if canonical.contains("drawing") || canonical.contains("chart") => {
            Some("绘图工具包；按 action=list_tools、get_tool_schema、call_tool 使用。")
        }
        _ if canonical.contains("geo") => Some("地理编码或地点搜索；输入需完整，结果限量。"),
        _ => None,
    };
    fixed.map(str::to_string).unwrap_or_else(|| {
        let normalized = original.split_whitespace().collect::<Vec<_>>().join(" ");
        truncate_text(&normalized, 240)
    })
}

const SCHEMA_KEYS: &[&str] = &[
    "type",
    "properties",
    "required",
    "items",
    "additionalItems",
    "$ref",
    "$defs",
    "definitions",
    "additionalProperties",
    "enum",
    "const",
    "default",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minItems",
    "maxItems",
    "minLength",
    "maxLength",
    "pattern",
    "format",
    "prefixItems",
    "patternProperties",
    "contains",
    "propertyNames",
    "dependentRequired",
    "dependentSchemas",
    "minProperties",
    "maxProperties",
    "uniqueItems",
    "anyOf",
    "oneOf",
    "allOf",
    "not",
];

/// Compact a schema while preserving the map keys that carry parameter names.
/// `properties`/`$defs`/`definitions` are registries, not schemas themselves;
/// filtering their keys as if they were JSON Schema keywords silently removes
/// the arguments presented to the model.
fn compact_schema(value: &Value) -> Value {
    compact_schema_value(value, false)
}

fn compact_schema_value(value: &Value, registry: bool) -> Value {
    match value {
        Value::Object(map) if registry => Value::Object(
            map.iter()
                .map(|(name, schema)| (name.clone(), compact_schema_value(schema, false)))
                .collect(),
        ),
        Value::Object(map) => {
            let mut output = Map::new();
            // Keep validation and dispatch semantics; omit presentation-only JSON Schema fields.
            for key in SCHEMA_KEYS {
                if let Some(item) = map.get(*key) {
                    let child_is_registry = matches!(
                        *key,
                        "properties"
                            | "$defs"
                            | "definitions"
                            | "patternProperties"
                            | "dependentSchemas"
                    );
                    let compacted = if is_literal_schema_keyword(*key) {
                        // These keywords carry literal values, not nested schemas.
                        item.clone()
                    } else {
                        compact_schema_value(item, child_is_registry)
                    };
                    output.insert((*key).to_string(), compacted);
                }
            }
            Value::Object(output)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| compact_schema_value(item, false))
                .collect(),
        ),
        Value::String(text) => Value::String(text.clone()),
        _ => value.clone(),
    }
}

fn is_literal_schema_keyword(key: &str) -> bool {
    matches!(
        key,
        "type"
            | "required"
            | "dependentRequired"
            | "$ref"
            | "enum"
            | "const"
            | "default"
            | "minimum"
            | "maximum"
            | "exclusiveMinimum"
            | "exclusiveMaximum"
            | "minItems"
            | "maxItems"
            | "minLength"
            | "maxLength"
            | "pattern"
            | "format"
            | "uniqueItems"
    )
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut result = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::compact_tool_spec_for_model;
    use crate::schemas::ToolSpec;
    use serde_json::json;

    #[test]
    fn compact_tool_keeps_required_enum_and_limits() {
        let spec = ToolSpec {
            name: "db_export".to_string(),
            title: Some("long title".to_string()),
            description: "a very long description with examples".to_string(),
            input_schema: json!({
                "type": "object", "title": "Arguments", "examples": [{"x": 1}],
                "properties": {"format": {"type": "string", "enum": ["csv", "xlsx"], "description": "long field"}},
                "required": ["sql"], "additionalProperties": false, "maxItems": 3
            }),
        };
        let compact = compact_tool_spec_for_model(&spec);
        assert!(compact.description.contains("只读 SQL"));
        assert!(compact.input_schema.get("title").is_none());
        assert!(compact.input_schema.get("examples").is_none());
        assert_eq!(compact.input_schema["required"], json!(["sql"]));
        assert_eq!(
            compact.input_schema["properties"]["format"]["enum"],
            json!(["csv", "xlsx"])
        );
        assert_eq!(compact.input_schema["additionalProperties"], json!(false));
    }

    #[test]
    fn compact_unknown_tool_has_bounded_description() {
        let spec = ToolSpec {
            name: "custom".to_string(),
            title: None,
            description: "x ".repeat(500),
            input_schema: json!({"type": "object", "properties": {}}),
        };
        assert!(
            compact_tool_spec_for_model(&spec)
                .description
                .chars()
                .count()
                <= 240
        );
    }

    #[test]
    fn compact_command_tools_distinguish_polling_from_sleep() {
        let exec = compact_tool_spec_for_model(&ToolSpec {
            name: "执行命令".to_string(),
            title: None,
            description: String::new(),
            input_schema: json!({"type": "object"}),
        });
        let session = compact_tool_spec_for_model(&ToolSpec {
            name: "命令会话".to_string(),
            title: None,
            description: String::new(),
            input_schema: json!({"type": "object"}),
        });
        let sleep = compact_tool_spec_for_model(&ToolSpec {
            name: "休眠等待".to_string(),
            title: None,
            description: String::new(),
            input_schema: json!({"type": "object"}),
        });
        assert!(exec.description.contains("command_session_id"));
        assert!(session.description.contains("轮询后台命令"));
        assert!(sleep.description.contains("不用于等待命令完成"));
    }

    #[test]
    fn compact_schema_keeps_reference_and_subagent_lifecycle_constraints() {
        let spec = ToolSpec {
            name: "subagent_control".to_string(),
            title: Some("ignored".to_string()),
            description: "long description".to_string(),
            input_schema: json!({
                "type": "object",
                "$defs": {"target": {"type": "string"}},
                "properties": {
                    "action": {"type": "string", "enum": ["send", "resume", "wait", "cancel"]},
                    "target": {"$ref": "#/$defs/target"}
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        };
        let compact = compact_tool_spec_for_model(&spec);
        assert!(compact.description.contains("send"));
        assert_eq!(compact.title, None);
        assert_eq!(compact.input_schema["required"], json!(["action"]));
        assert_eq!(
            compact.input_schema["properties"]["action"]["enum"],
            json!(["send", "resume", "wait", "cancel"])
        );
        assert_eq!(
            compact.input_schema["properties"]["target"]["$ref"],
            "#/$defs/target"
        );
        assert_eq!(compact.input_schema["$defs"]["target"]["type"], "string");
    }

    #[test]
    fn compact_schema_preserves_argument_names_and_nested_definition_names() {
        let spec = ToolSpec {
            name: "write_file".to_string(),
            title: None,
            description: "write".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "ignored"},
                    "content": {"type": "string"},
                    "options": {
                        "type": "object",
                        "properties": {"dry_run": {"type": "boolean"}},
                        "required": ["dry_run"]
                    }
                },
                "required": ["path", "content"],
                "$defs": {
                    "write_options": {
                        "type": "object",
                        "properties": {"encoding": {"type": "string"}}
                    }
                }
            }),
        };
        let compact = compact_tool_spec_for_model(&spec);
        assert_eq!(compact.input_schema["properties"]["path"]["type"], "string");
        assert_eq!(
            compact.input_schema["properties"]["content"]["type"],
            "string"
        );
        assert_eq!(
            compact.input_schema["properties"]["options"]["properties"]["dry_run"]["type"],
            "boolean"
        );
        assert_eq!(
            compact.input_schema["$defs"]["write_options"]["properties"]["encoding"]["type"],
            "string"
        );
    }

    #[test]
    #[ignore = "explicit prompt budget measurement"]
    fn write_qwen_prompt_budget_report() {
        use crate::config::{load_config_from_path, Config};
        use crate::i18n;
        use crate::services::tools::catalog::{
            collect_available_tool_names, collect_prompt_tool_specs_with_language,
        };
        use crate::skills::SkillRegistry;
        use std::path::Path;

        let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo_root = manifest_root
            .parent()
            .and_then(Path::parent)
            .unwrap_or(manifest_root)
            .to_path_buf();
        let repo_root = if repo_root.join("config/wunder.yaml").exists() {
            repo_root
        } else {
            manifest_root.to_path_buf()
        };
        let config_path = repo_root.join("config/wunder.yaml");
        let config = if config_path.exists() {
            load_config_from_path(&config_path)
        } else {
            Config::default()
        };
        i18n::configure_i18n(
            Some(config.i18n.default_language.clone()),
            Some(config.i18n.supported_languages.clone()),
            Some(config.i18n.aliases.clone()),
        );
        let skills = SkillRegistry::default();
        let allowed = collect_available_tool_names(&config, &skills, None);
        let specs =
            collect_prompt_tool_specs_with_language(&config, &skills, &allowed, None, "zh-CN");
        let compact = specs
            .iter()
            .map(compact_tool_spec_for_model)
            .collect::<Vec<_>>();
        let measure = |value: &serde_json::Value| {
            let text = serde_json::to_string(value).expect("serialize");
            serde_json::json!({
                "bytes": text.len(),
                "chars": text.chars().count(),
                "approx_tokens_utf8_bytes_div_4": (text.len() as f64 / 4.0).ceil() as u64
            })
        };
        let original_value = serde_json::to_value(&specs).expect("original specs");
        let compact_value = serde_json::to_value(&compact).expect("compact specs");
        let per_tool = compact
            .iter()
            .enumerate()
            .map(|(index, spec)| {
                let value = serde_json::to_value(spec).expect("tool spec");
                let text = serde_json::to_string(&value).expect("tool json");
                serde_json::json!({
                    "index": index,
                    "bytes": text.len(),
                    "approx_tokens_utf8_bytes_div_4": (text.len() as f64 / 4.0).ceil() as u64
                })
            })
            .collect::<Vec<_>>();
        let mut template_blocks = Vec::new();
        for name in [
            "role.txt",
            "engineering.txt",
            "inner_visible_protocol.txt",
            "skills_protocol.txt",
            "memory.txt",
            "extra.txt",
        ] {
            let path = repo_root.join("config/prompts/zh/system").join(name);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            template_blocks.push(serde_json::json!({
                "name": name,
                "bytes": text.len(),
                "chars": text.chars().count(),
                "approx_tokens_utf8_bytes_div_4": (text.len() as f64 / 4.0).ceil() as u64
            }));
        }
        let report = serde_json::json!({
            "model": config.llm.models.get("魔搭").and_then(|m| m.model.clone()).unwrap_or_default(),
            "config_exists": config_path.exists(),
            "builtin_enabled_count": config.tools.builtin.enabled.len(),
            "mcp_server_count": config.mcp.servers.len(),
            "tool_call_mode": "function_call",
            "tokenizer": "Qwen tokenizer unavailable locally; estimates use UTF-8 bytes / 4",
            "tool_count": specs.len(),
            "original_tools": measure(&original_value),
            "compact_tools": measure(&compact_value),
            "compact_tools_by_tool": per_tool,
            "tool_savings_bytes": original_value.to_string().len().saturating_sub(compact_value.to_string().len()),
            "system_template_blocks": template_blocks,
            "function_call_system_note": "Function-call mode omits tools_protocol and native tool schemas are sent in the request tools field."
        });
        let output = repo_root.join("docs/性能基线/assets/2026-09-23-qwen-prompt-budget.json");
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent).expect("report directory");
        }
        std::fs::write(
            output,
            serde_json::to_string_pretty(&report).expect("report json"),
        )
        .expect("write report");
    }
}
