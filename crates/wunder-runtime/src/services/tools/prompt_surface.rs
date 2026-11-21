//! Model-facing projection of the tool catalogue.
//!
//! The authoritative `ToolSpec` values (see `builtin_tool_specs_with_language`)
//! remain the contract used by execution, argument recovery and schema-hash
//! snapshots. Everything the model actually receives — the XML tool block in
//! text protocols and the API `tools` array in function-call protocols — goes
//! through [`prompt_tool_spec`], which produces a deliberately smaller surface:
//!
//! * field-level `description` and validation annotations are dropped (the
//!   backend still validates them, and parameter names are self-describing);
//! * low-frequency actions and parameters are removed from the visible schema;
//! * tool descriptions are replaced by high-density one-paragraph briefs;
//! * unused `$defs` and dangling `required` entries are pruned.
//!
//! Keeping this as one projection (instead of hand-editing 27 tool definitions)
//! means execution remains fully backward compatible with older sessions while
//! every first-turn prompt pays the same reduced fixed cost.

use crate::schemas::ToolSpec;
use serde_json::{Map, Value};
use std::collections::{BTreeSet, HashSet};

/// Schema keywords that carry presentation prose or bounds the backend checks
/// anyway. `enum`/`const`/`required` are data, not annotations, so they stay.
const DROP_KEYS: &[&str] = &[
    "description",
    "title",
    "examples",
    "example",
    "$comment",
    "x-ui",
    "x-display",
    "x-order",
    "default",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "minItems",
    "maxItems",
    "minProperties",
    "maxProperties",
    "multipleOf",
    "pattern",
    "format",
    "uniqueItems",
    "contentEncoding",
    "contentMediaType",
];

fn is_schema_map_keyword(key: &str) -> bool {
    matches!(
        key,
        "properties" | "$defs" | "definitions" | "patternProperties" | "dependentSchemas"
    )
}

fn is_schema_node_keyword(key: &str) -> bool {
    matches!(
        key,
        "items"
            | "prefixItems"
            | "oneOf"
            | "anyOf"
            | "allOf"
            | "not"
            | "if"
            | "then"
            | "else"
            | "additionalProperties"
            | "unevaluatedProperties"
            | "propertyNames"
            | "contains"
            | "additionalItems"
    )
}

fn minify_schema_node(value: &Value, keep_descriptions: bool) -> Value {
    match value {
        Value::Object(map) => {
            let mut result = Map::with_capacity(map.len());
            for (key, child) in map {
                if key == "description" {
                    if keep_descriptions {
                        result.insert(key.clone(), child.clone());
                    }
                    continue;
                }
                if DROP_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let next = if is_schema_map_keyword(key) {
                    minify_schema_map(child, keep_descriptions)
                } else if is_schema_node_keyword(key) {
                    minify_schema_node(child, keep_descriptions)
                } else {
                    // `enum`, `const`, `required`, `$ref` and plain data must be
                    // copied verbatim; recursing into them would corrupt values.
                    child.clone()
                };
                result.insert(key.clone(), next);
            }
            Value::Object(result)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| minify_schema_node(item, keep_descriptions))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn minify_schema_map(value: &Value, keep_descriptions: bool) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, child)| (key.clone(), minify_schema_node(child, keep_descriptions)))
                .collect(),
        ),
        other => minify_schema_node(other, keep_descriptions),
    }
}

/// One tool's model-side policy. Empty briefs keep the authoritative
/// description; empty drop lists keep the full action/parameter surface.
struct SurfaceRule {
    tool: &'static str,
    brief_en: &'static str,
    drop_actions: &'static [&'static str],
    drop_fields: &'static [&'static str],
}

/// Rules are keyed by canonical tool names; `resolve_tool_name` maps English
/// aliases before lookup. Keep briefs factual: parameter names, action names
/// and the one or two rules that change a call's correctness.
const SURFACE_RULES: &[SurfaceRule] = &[
    SurfaceRule {
        tool: "计划面板",
        brief_en: "Progress ledger for multi-step work. Shape: {\"plan\":[{\"step\":\"a\",\"status\":\"in_progress\"}]}; each step = step, status (pending/in_progress/completed) and optional explanation. Full replace, one in_progress max. Re-call with the full list after each finished step, and once more when all are completed so the panel checks off.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "问询面板",
        brief_en: "Ask all decision questions in one panel call, then end the turn; include options and mark a recommendation when useful.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "定时任务",
        brief_en: "Manage scheduled jobs. add needs message + schedule_text (natural language or cron); others use job_id. session=main injects into the bound thread, isolated starts a child thread; enabled toggles a job; run is async (queued = accepted, not finished).",
        drop_actions: &["get", "status"],
        drop_fields: &["schedule"],
    },
    SurfaceRule {
        tool: "记忆管理",
        brief_en: "Manage long-term memory: the prompt lists only id/title indexes, so list/search for memory_id then get for full detail; add/update/remove by id. For add/update, title is the index title, content the durable detail.",
        drop_actions: &["clear"],
        drop_fields: &["memory_time", "related_memory_id", "order"],
    },
    SurfaceRule {
        tool: "执行命令",
        brief_en: "Run a shell command. content is the command; optional workdir and yield_time_ms (default 10000). If still running, the result carries command_session_id and seq; continue with command_session instead of restarting.",
        drop_actions: &[],
        drop_fields: &["description", "dry_run"],
    },
    SurfaceRule {
        tool: "命令会话",
        brief_en: "Continue a running command. command_session_id comes from execute_command. Empty chars polls new output, non-empty writes stdin. Both wait up to yield_time_ms (default 1000, max 30000) and return status plus new output; after_seq resumes from a seq, wait_for=exit blocks until exit. Reuse the id until exit_code appears. chars=\\u0003 interrupts, \\u0004 is EOF.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "列出文件",
        brief_en: "List a directory, directories first; top_level_only skips recursion, max_depth sets depth, cursor/limit page large results.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "文件名匹配",
        brief_en: "Match file names with a glob pattern under path; 32 paths per page, pass offset=next_offset when has_more.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "搜索内容",
        brief_en: "Ripgrep-style search: query (literal|regex via query_mode) or pattern. Narrow with path/glob/include, subtract with exclude. total_matches is the true count; page with offset=next_offset, not by raising max_matches. context_before/after apply to every hit, context_matches=N to the top N only; search_binary=true for binary files.",
        drop_actions: &[],
        drop_fields: &["timeout_ms"],
    },
    SurfaceRule {
        tool: "读取文件",
        brief_en: "Read UTF-8 text files. path required (also a tool-result: handle). Files use 1-based start_line/end_line (limit = first N lines); handles use 0-based byte offset/limit and next_offset, never mix. mode=outline|indentation|slice, pattern for matches. Default <=500 lines/16KiB; continue via has_more/next_line. Text only; use the image tool for images.",
        drop_actions: &[],
        drop_fields: &["output_budget_bytes"],
    },
    SurfaceRule {
        tool: "读图工具",
        brief_en: "Read local image/GIF/video. Default view auto-fits (<=2048px); >2x downscale blurs small text. crop=x,y,w,h is x/y/width/height; crop_xyxy=x1,y1,x2,y2 (never both). scale for detail; stats adds region numbers, stats_only omits the image; frame_rate samples video. GIF: frame=N returns that exact frame (0-based), frames=[...] several, sheet=true one contact sheet (<=16 cells, sheet.frame_indices maps cells).",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "语音生成",
        brief_en: "Synthesize speech from text with voice/speed; path sets the output file, reference_path clones a voice. For multi-segment output keep the configured voice: omit voice/reference_path unless the user asks for another voice.",
        drop_actions: &[],
        drop_fields: &[
            "instructions",
            "model_specific_params",
            "ref_audio",
            "ref_text",
            "response_format",
        ],
    },
    SurfaceRule {
        tool: "声转文",
        brief_en: "Transcribe an audio file at path to text; language and model_name are optional.",
        drop_actions: &[],
        drop_fields: &[
            "content_type",
            "filename",
            "prompt",
            "response_format",
            "source_public_path",
            "temperature",
        ],
    },
    SurfaceRule {
        tool: "图像生成",
        // 修图入口必须写进 brief：内置工具的参数说明会被整体裁掉，模型只能
        // 从这里得知"改图传 input_paths、prompt 只写改动、mask 白黑语义"。
        // strength 等调参项不进 brief：默认值由模型配置给，模型自己设只会带来
        // 无谓的试错（参数写错还被上游拒），见 drop_fields。
        brief_en: "Generate/edit into path. Edits: input_paths (array, first = primary; input_path deprecated). size WxH: /16, short >= 32, <= model limit (default 1024x1024); omit size for HD. mask_path: white = repaint, black = keep, same size as source. output_format must match suffix. seed omitted = auto + echoed. Existing path needs overwrite:true. Prefer for retouching.",
        drop_actions: &[],
        drop_fields: &[
            "guidance_scale",
            "layers",
            "num_inference_steps",
            "output_compression",
            "resolution",
            "strength",
            "true_cfg_scale",
        ],
    },
    SurfaceRule {
        tool: "视频生成",
        brief_en: "Generate a video from prompt into path; optional seconds, size, fps, seed and negative_prompt.",
        drop_actions: &[],
        drop_fields: &[
            "boundary_ratio",
            "enable_frame_interpolation",
            "flow_shift",
            "guidance_scale",
            "guidance_scale_2",
            "num_frames",
            "num_inference_steps",
        ],
    },
    SurfaceRule {
        tool: "技能调用",
        brief_en: "Load a mounted skill by name before other tools when a skill matches; the default action returns SKILL.md plus its resource index, path/cursor/line_limit for bounded reads.",
        drop_actions: &["manifest"],
        drop_fields: &["bytes_limit", "manifest_cursor", "section"],
    },
    SurfaceRule {
        tool: "写入文件",
        brief_en: "Create or fully replace a UTF-8 file with path + content; an empty content clears the file. dry_run previews the write.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "文本编辑",
        brief_en: "Edit UTF-8 files. str_replace (default): file_path + old_string + new_string (exact single match unless replace_all); create: file_text; insert: insert_line; view: view_range; range replace: start_line + end_line + new_string; dry_run previews.",
        drop_actions: &[],
        drop_fields: &["input"],
    },
    SurfaceRule {
        tool: "子智能体控制",
        brief_en: "spawn async by default: returns accepted + session_id/run_id, then auto-wakes you on completion — continue or end the turn; never poll/sleep/schedule a wait. run_in_background=false only when this turn needs the result. task must be self-contained; the child can't see this thread, so use context_summary/fork_turns. Send independent spawns in one message. run_timeout_seconds caps the child only; accepted ≠ finished.",
        drop_actions: &["interrupt", "resume"],
        drop_fields: &[
            "announce_persist_history",
            "announce_emit_parent_events",
            "cleanup",
            "run_ids",
            "session_ids",
            "poll_interval_seconds",
            "wait_mode",
            "include_tools",
            "model",
            "agent_id",
            "label",
        ],
    },
    SurfaceRule {
        tool: "会话线程控制",
        brief_en: "List, inspect, create, switch or archive task threads. Most actions take session_id; create takes title/parent_session_id.",
        drop_actions: &["back", "restore", "update_title"],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "网页搜索",
        brief_en: "Search the public web with 1-4 independent queries; returns URLs, titles and snippets. Narrow with site and count.",
        drop_actions: &[],
        drop_fields: &["query", "sites", "sources", "categories", "scrape_results"],
    },
    SurfaceRule {
        tool: "网页抓取",
        brief_en: "Fetch one public URL and extract its main content (markdown|text). Use browser navigate for login, forms or clicks.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "桌面控制",
        brief_en: "Drive this desktop. Pointer actions take bbox=[x1,y1,x2,y2]; type_text without bbox types into the focused control (newlines become Enter); press_key/scroll/wait take only their own field. Verify the returned screenshot before retrying.",
        drop_actions: &[
            "middle_click",
            "left_hold",
            "right_hold",
            "middle_hold",
            "left_release",
            "right_release",
            "middle_release",
            "move_mouse",
            "drag_drop",
        ],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "手机控制",
        brief_en: "Drive this Android phone: screenshot/ui_tree to inspect, tap/long_press/swipe (bbox = screenshot pixels, size = normalized_width/normalized_height), type_text, press_key, open_app/open_url/open_settings, list_apps/device_info/clipboard/wait. Touch, text and screenshots need the accessibility service; check accessibility_connected, then verify each frame.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "goal",
        brief_en: "Track the task goal. get reads it; edit/pause/resume/complete/blocked need goal_id + revision from get. Only the user starts a goal; never create one.",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "browser",
        brief_en: "Built-in browser: navigate -> observe_refs -> click/type/select/press -> observe_text/screenshot, re-observe after changes. Target by ref, selector, text; type overwrites; select needs value; press needs key; timeout_ms on element actions and waits, wait_ms only applies to wait; tabs_* manage tabs by target_id. navigate path previews a workspace file; scroll takes delta_x/delta_y; console reads console output.",
        drop_actions: &["hover", "drag"],
        drop_fields: &[
            // `drag` 不在模型面上，配套的起止目标与坐标字段一并收掉，
            // 否则首轮固定成本里只留下对不上任何 action 的孤儿字段。
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
            "from_x",
            "from_y",
            "to_x",
            "to_y",
            "state",
            "load_state",
            "snapshot_timeout_ms",
            "submit",
        ],
    },
];

fn rule_for(name: &str) -> Option<&'static SurfaceRule> {
    let canonical = super::resolve_tool_name(name);
    SURFACE_RULES
        .iter()
        .find(|rule| rule.tool == canonical.as_str() || rule.tool == name)
}

/// Produce the tool definition the model sees. The returned spec is a strict
/// subset of the authoritative one: no new keys, no changed types, no removed
/// required parameters.
pub fn prompt_tool_spec(spec: &ToolSpec) -> ToolSpec {
    let rule = rule_for(&spec.name);
    // Built-in tools have hand-written briefs that carry the essential prose;
    // MCP and user-defined tools have no such fallback, so keep their field
    // descriptions while still dropping purely decorative annotations.
    let mut schema = minify_schema_node(&spec.input_schema, rule.is_none());
    if let Some(rule) = rule {
        if !rule.drop_actions.is_empty() {
            prune_actions(&mut schema, rule.drop_actions);
        }
        if !rule.drop_fields.is_empty() {
            prune_fields(&mut schema, rule.drop_fields);
        }
    }
    tidy_schema(&mut schema);
    // 工具说明只维护英文一份：模型面文案不随界面语言切换。
    let description = rule
        .and_then(|rule| (!rule.brief_en.is_empty()).then_some(rule.brief_en))
        .map(str::to_string)
        .unwrap_or_else(|| spec.description.clone());
    ToolSpec {
        name: spec.name.clone(),
        title: None,
        description,
        input_schema: schema,
    }
}

fn branch_action(branch: &Value) -> Option<&Value> {
    branch.get("properties")?.get("action")
}

fn prune_actions(schema: &mut Value, drop: &[&str]) {
    // Flat action object: `properties.action.enum`.
    if let Some(values) = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|props| props.get_mut("action"))
        .and_then(Value::as_object_mut)
        .and_then(|action| action.get_mut("enum"))
        .and_then(Value::as_array_mut)
    {
        values.retain(|value| !value.as_str().is_some_and(|item| drop.contains(&item)));
    }
    // Discriminated union: drop whole branches, or shrink a shared enum.
    if let Some(branches) = schema.get_mut("oneOf").and_then(Value::as_array_mut) {
        branches.retain(|branch| {
            let Some(action) = branch_action(branch) else {
                return true;
            };
            if let Some(value) = action.get("const").and_then(Value::as_str) {
                return !drop.contains(&value);
            }
            action
                .get("enum")
                .and_then(Value::as_array)
                .map_or(true, |values| {
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .any(|item| !drop.contains(&item))
                })
        });
        for branch in branches.iter_mut() {
            if let Some(values) = branch
                .get_mut("properties")
                .and_then(Value::as_object_mut)
                .and_then(|props| props.get_mut("action"))
                .and_then(Value::as_object_mut)
                .and_then(|action| action.get_mut("enum"))
                .and_then(Value::as_array_mut)
            {
                values.retain(|value| !value.as_str().is_some_and(|item| drop.contains(&item)));
            }
        }
    }
}

fn prune_properties(owner: &mut Map<String, Value>, drop: &[&str]) {
    let Some(props) = owner.get_mut("properties").and_then(Value::as_object_mut) else {
        return;
    };
    props.retain(|key, _| !drop.contains(&key.as_str()));
    let available: HashSet<String> = props.keys().cloned().collect();
    if let Some(required) = owner.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|value| value.as_str().is_some_and(|key| available.contains(key)));
    }
}

fn prune_fields(schema: &mut Value, drop: &[&str]) {
    if let Some(root) = schema.as_object_mut() {
        prune_properties(root, drop);
    }
    if let Some(branches) = schema.get_mut("oneOf").and_then(Value::as_array_mut) {
        for branch in branches {
            if let Some(owner) = branch.as_object_mut() {
                prune_properties(owner, drop);
            }
        }
    }
}

fn collect_refs(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                if let Some(name) = reference.strip_prefix("#/$defs/") {
                    out.insert(name.to_string());
                }
            }
            for child in map.values() {
                collect_refs(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_refs(item, out);
            }
        }
        _ => {}
    }
}

/// Remove definitions that are no longer reachable, then repair every
/// `required` list that points at a property removed above.
fn tidy_schema(schema: &mut Value) {
    // `collect_refs` walks the whole schema, including definitions themselves,
    // so the returned set is already the transitive closure of reachable refs.
    let mut used = BTreeSet::new();
    collect_refs(schema, &mut used);
    for key in ["$defs", "definitions"] {
        if let Some(defs) = schema.get_mut(key).and_then(Value::as_object_mut) {
            defs.retain(|name, _| used.contains(name));
        }
        let empty = schema
            .get(key)
            .and_then(Value::as_object)
            .is_some_and(|defs| defs.is_empty());
        if empty {
            schema.as_object_mut().expect("schema object").remove(key);
        }
    }
    tidy_required(schema);
}

fn tidy_required(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let available: HashSet<String> = map
                .get("properties")
                .and_then(Value::as_object)
                .map(|props| props.keys().cloned().collect())
                .unwrap_or_default();
            if let Some(required) = map.get_mut("required").and_then(Value::as_array_mut) {
                required.retain(|item| item.as_str().is_some_and(|key| available.contains(key)));
            }
            for child in map.values_mut() {
                tidy_required(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                tidy_required(item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_brief_carries_the_update_discipline() {
        let rule = rule_for("计划面板").expect("plan rule");
        // 工具面是模型唯一必读的位置：何时建、何时更新、收尾要求都要在这里。
        for needle in [
            "multi-step",
            "in_progress",
            "Full replace",
            "completed",
            "checks off",
        ] {
            assert!(rule.brief_en.contains(needle), "en brief missing {needle}");
        }
        // 只宣传一种写法：旧的 current 形态不许再出现，否则模型又会混着用。
        assert!(!rule.brief_en.contains("current"));
    }

    #[test]
    fn subagent_brief_leads_with_background_and_briefing_rules() {
        let rule = rule_for("子智能体控制").expect("subagent rule");
        for needle in [
            "async by default",
            "auto-wakes",
            "never poll",
            "run_in_background=false",
            "context_summary",
            "independent spawns",
        ] {
            assert!(rule.brief_en.contains(needle), "en brief missing {needle}");
        }
        let spec = crate::services::tools::catalog::builtin_tool_specs()
            .into_iter()
            .find(|spec| spec.name == "子智能体控制")
            .expect("subagent spec");
        let projected = prompt_tool_spec(&spec);
        let props = projected.input_schema["properties"]
            .as_object()
            .expect("subagent properties");
        // 后台开关与交办背景字段必须让模型看见，否则只能靠猜。
        for field in ["run_in_background", "context_summary", "fork_turns", "task"] {
            assert!(props.contains_key(field), "{field} must stay visible");
        }
        assert!(!props.contains_key("label"));
    }

    #[test]
    fn plan_surface_exposes_one_shape_and_status_enum() {
        let spec = crate::services::tools::catalog::builtin_tool_specs()
            .into_iter()
            .find(|spec| spec.name == "计划面板")
            .expect("plan spec");
        let projected = prompt_tool_spec(&spec);
        // 只有一种形态：current 不再出现在模型看到的 schema 里。
        assert!(projected.input_schema["properties"]
            .get("current")
            .is_none());
        // 步骤条目 step + status 都必填，模型不会漏 status 再被提示补一次。
        assert_eq!(
            projected.input_schema["properties"]["plan"]["items"]["required"],
            json!(["step", "status"])
        );
        // status 只有三个合法值，枚举要让模型直接看见。
        let status = &projected.input_schema["properties"]["plan"]["items"]["properties"]["status"];
        assert_eq!(
            status["enum"],
            serde_json::json!(["pending", "in_progress", "completed"])
        );
    }

    #[test]
    fn image_surface_keeps_edit_entry_points_visible() {
        let spec = crate::services::tools::catalog::builtin_tool_specs()
            .into_iter()
            .find(|spec| spec.name == "图像生成")
            .expect("image spec");
        {
            let projected = prompt_tool_spec(&spec);
            let props = projected.input_schema["properties"]
                .as_object()
                .expect("image properties");
            // 改图入口和局部重绘参数必须留在模型可见的精简 schema 里，
            // 否则模型只会把它当成文生图工具。
            for field in [
                "prompt",
                "input_path",
                "input_paths",
                "mask_path",
                "size",
                "output_format",
            ] {
                assert!(
                    props.contains_key(field),
                    "{field} should stay visible to the model"
                );
            }
            // strength 是调参项：默认值由模型配置给，不再暴露给模型自己填。
            assert!(
                !props.contains_key("strength"),
                "strength should stay out of the model-facing schema"
            );
            assert!(projected.description.contains("input_path"));
            assert!(projected.description.contains("retouch"));
            // 高清化必须被描述成“保持原尺寸”，否则模型会传超大 size 去放大图片。
            assert!(projected.description.contains("omit size"));
        }
    }

    #[test]
    fn minify_drops_prose_but_preserves_schema_data() {
        let schema = json!({
            "type": "object",
            "properties": {
                "action": {"const": "add"},
                "path": {"type": "string", "minLength": 1, "description": "a long explanation"},
                "mode": {"type": "string", "enum": ["a", "b"], "default": "a"}
            },
            "required": ["action", "path"],
            "additionalProperties": false
        });
        let minified = minify_schema_node(&schema, false);
        assert_eq!(minified["properties"]["path"], json!({"type": "string"}));
        assert_eq!(minified["properties"]["mode"]["enum"], json!(["a", "b"]));
        assert!(minified["properties"]["mode"].get("default").is_none());
        assert_eq!(minified["required"], json!(["action", "path"]));
    }

    #[test]
    fn prompt_projection_trims_actions_fields_and_unused_defs() {
        let spec = ToolSpec {
            name: "browser".to_string(),
            title: Some("browser".to_string()),
            description: "long original description".to_string(),
            input_schema: json!({
                "type": "object",
                "$defs": {
                    "used": {"type": "string"},
                    "orphan": {"type": "integer"}
                },
                "properties": {
                    "action": {"type": "string", "enum": ["navigate", "click", "select", "drag", "hover"]},
                    "url": {"$ref": "#/$defs/used"},
                    "from_ref": {"type": "string"},
                    "value": {"type": "string"}
                },
                "required": ["action", "from_ref"],
                "additionalProperties": false
            }),
        };
        let projected = prompt_tool_spec(&spec);
        assert_eq!(
            projected.input_schema["properties"]["action"]["enum"],
            json!(["navigate", "click", "select"])
        );
        assert!(projected.input_schema["properties"]
            .get("from_ref")
            .is_none());
        assert!(projected.input_schema["properties"].get("value").is_some());
        assert_eq!(projected.input_schema["required"], json!(["action"]));
        assert!(projected.input_schema["$defs"]
            .get("used")
            .is_some_and(Value::is_object));
        assert!(projected.input_schema["$defs"].get("orphan").is_none());
        assert!(projected.title.is_none());
        assert!(projected.description.starts_with("Built-in browser"));
        assert!(projected.description.contains("re-observe after changes"));
        assert_ne!(projected.description, spec.description);
    }

    #[test]
    fn browser_prompt_surface_hides_wait_only_fields() {
        let spec = crate::tools::catalog::builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "browser")
            .expect("browser spec");
        let projected = prompt_tool_spec(&spec);
        let properties = projected.input_schema["properties"]
            .as_object()
            .expect("browser properties");
        // `load_state` belongs only to action=wait, but a flat action schema
        // made it look usable with navigate; that produced a runtime rejection.
        assert!(!properties.contains_key("load_state"));
        assert!(!properties.contains_key("state"));
        assert!(properties.contains_key("wait_ms"));
        assert!(projected
            .description
            .contains("wait_ms only applies to wait"));
    }

    #[test]
    fn briefs_stay_within_a_bounded_length() {
        for rule in SURFACE_RULES {
            assert!(
                !rule.brief_en.is_empty(),
                "{} lacks an English brief",
                rule.tool
            );
            assert!(
                rule.brief_en.chars().count() <= 420,
                "{} English brief grew to {} chars",
                rule.tool,
                rule.brief_en.chars().count()
            );
        }
    }

    #[test]
    fn external_tools_keep_field_descriptions() {
        let spec = ToolSpec {
            name: "mcp_package".to_string(),
            title: None,
            description: "external tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "what to look for"}
                },
                "required": ["query"]
            }),
        };
        let projected = prompt_tool_spec(&spec);
        assert_eq!(
            projected.input_schema["properties"]["query"]["description"],
            json!("what to look for")
        );
        assert_eq!(projected.description, "external tool");
    }

    #[test]
    fn projection_never_adds_required_fields_or_types() {
        // 英文是模型面唯一的语言，投影只在这一套 schema 上校验不变量。
        let specs = crate::tools::catalog::builtin_tool_specs_with_language("en-US");
        for spec in specs {
            let projected = prompt_tool_spec(&spec);
            let original_required: HashSet<String> = spec.input_schema["required"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let projected_required: HashSet<String> = projected.input_schema["required"]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                projected_required.is_subset(&original_required),
                "{} introduced a required field",
                spec.name
            );
            let original_props: HashSet<String> = spec.input_schema["properties"]
                .as_object()
                .map(|map| map.keys().cloned().collect())
                .unwrap_or_default();
            let projected_props: HashSet<String> = projected.input_schema["properties"]
                .as_object()
                .map(|map| map.keys().cloned().collect())
                .unwrap_or_default();
            assert!(
                projected_props.is_subset(&original_props),
                "{} introduced a property",
                spec.name
            );
        }
    }
}
