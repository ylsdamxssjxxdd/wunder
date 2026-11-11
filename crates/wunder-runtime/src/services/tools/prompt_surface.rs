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
    brief_zh: &'static str,
    drop_actions: &'static [&'static str],
    drop_fields: &'static [&'static str],
}

/// Rules are keyed by canonical tool names; `resolve_tool_name` maps English
/// aliases before lookup. Keep briefs factual: parameter names, action names
/// and the one or two rules that change a call's correctness.
const SURFACE_RULES: &[SurfaceRule] = &[
    SurfaceRule {
        tool: "计划面板",
        brief_en: "Show a compact plan panel. Provide plan steps and an optional explanation.",
        brief_zh: "显示紧凑计划面板，传入 plan 步骤与可选 explanation。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "问询面板",
        brief_en: "Ask all decision questions in one panel call, then end the turn. Include options and mark a recommendation when useful.",
        brief_zh: "一次调用问完全部决策问题并结束本轮；可带选项并标注推荐项。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "会话让出",
        brief_en: "Yield the turn with a short message for the user.",
        brief_zh: "用一条简短消息结束当前轮次。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "定时任务",
        brief_en: "Manage scheduled jobs. add needs message + schedule_text (English or Chinese natural language, or cron); other actions use job_id. session=main injects into the bound thread, isolated starts a fresh child thread; enabled toggles a job; run is async (queued = accepted, not finished).",
        brief_zh: "管理定时任务：add 需 message + schedule_text（支持中文如“每天 09:00”“每 5 分钟”，也支持英文与 cron），其余动作用 job_id；session=main 注入绑定线程、isolated 新建子线程，enabled 控制开关；run 是异步的（queued 仅表示已接收，不要轮询等结果）。",
        drop_actions: &["get", "status"],
        drop_fields: &["schedule"],
    },
    SurfaceRule {
        tool: "记忆管理",
        brief_en: "Manage long-term memory: the prompt only has id-and-title indexes, so list/search to find memory_id then get for full detail; add/update/remove by id. For add/update, title is the index title and content the full durable detail.",
        brief_zh: "管理长期记忆：系统提示里只有 id 与标题索引，先 list/search 找 memory_id 再 get 读全文；add/update/remove 按 id 操作，其中 title 是索引标题、content 是完整持久内容。",
        drop_actions: &["clear"],
        drop_fields: &["memory_time", "related_memory_id", "order"],
    },
    SurfaceRule {
        tool: "执行命令",
        brief_en: "Run a shell command in the workspace. content is the command; optional workdir, timeout_s, yield_time_ms, run_in_background. Large output becomes a paged tool-result handle.",
        brief_zh: "在工作目录执行命令：content 为命令，可选 workdir、timeout_s、yield_time_ms、run_in_background；大输出转为可分页的结果句柄。",
        drop_actions: &[],
        drop_fields: &["description", "dry_run"],
    },
    SurfaceRule {
        tool: "命令会话",
        brief_en: "Read a background command session or feed its stdin. Empty input just returns new output (default yield_time_ms=250); non-empty input writes stdin and then waits for the command to exit within the same window. Pass wait_for=exit to block on a plain read. A finished session still returns its final output.",
        brief_zh: "读取后台命令会话或喂 stdin：input 为空只取新输出（默认 yield_time_ms=250），非空先写 stdin，随后在同一个窗口内等命令退出；纯轮询要阻塞到退出才传 wait_for=exit。会话已退出时仍会返回最后的输出。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "列出文件",
        brief_en: "List a directory. Directories come first by default; top_level_only skips recursion, max_depth controls depth, cursor/limit page large results.",
        brief_zh: "列目录：默认目录优先；top_level_only 只看顶层，max_depth 控制深度，cursor/limit 分页。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "文件名匹配",
        brief_en: "Match file names with a glob pattern under path.",
        brief_zh: "按 glob 模式匹配 path 下的文件名。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "搜索内容",
        brief_en: "Ripgrep-style search by query (literal|regex via query_mode) or pattern. Narrow with path/glob/include, cap with max_matches, add context_before/context_after.",
        brief_zh: "按 query（query_mode=literal|regex）或 pattern 检索文件内容；path/glob/include 收窄范围，max_matches 限量，context_before/after 带上下文。",
        drop_actions: &[],
        drop_fields: &["timeout_ms"],
    },
    SurfaceRule {
        tool: "读取文件",
        brief_en: "Read UTF-8 text files. path is required (also accepts a tool-result: handle). Files use 1-based start_line/end_line (limit = first N lines); handles use 0-based byte offset/limit and next_offset — never mix the two. mode=outline|indentation|slice, pattern for matches. Default <=500 lines/16KiB; continue with has_more/next_line. Text files only; use the image tool for images.",
        brief_zh: "读取 UTF-8 文本：path 必填（也接受 tool-result: 句柄）；文件用 1 起始的 start_line/end_line（limit 读前 N 行），句柄用 0 起始的字节 offset/limit 并按 next_offset 续读，二者不可混用；mode=outline|indentation|slice，pattern 检索；默认 ≤500 行/16KiB，按 has_more/next_line 续读。仅文本文件，图片用读图工具。",
        drop_actions: &[],
        drop_fields: &["output_budget_bytes"],
    },
    SurfaceRule {
        tool: "读图工具",
        brief_en: "Read a local image/GIF/video into vision by path. The default view is auto-fitted (long side <=2048px); crop=x,y,w,h + scale inspect original-resolution details. stats adds region luma/non-background numbers; stats_only returns numbers without an image. Videos: frame_rate extracts frames. Images stay in context across rounds; re-read the path only when you need the pixels again.",
        brief_zh: "按 path 把本地图片/GIF/视频送入视觉。默认视图自动压到长边 ≤2048px；要看清原尺寸细节用 crop=x,y,w,h 指定区域并配合 scale。stats 附带区域亮度与非背景像素占比数值，stats_only 只要数值不回图。视频用 frame_rate 抽帧。图像随上下文保留，不必每轮重读；确实需要再看像素时再按路径重读。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "语音生成",
        brief_en: "Synthesize speech from text with voice/speed; path sets the output file, reference_path clones a voice.",
        brief_zh: "按 text 合成语音，voice/speed 控制音色语速，path 指定输出文件，reference_path 复刻音色。",
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
        brief_zh: "将 path 指向的音频转写为文本；language、model_name 可选。",
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
        brief_en: "Generate an image from prompt into path; optional size, seed, negative_prompt, reference_path or input_path for edits.",
        brief_zh: "按 prompt 生成图片写入 path；可选 size、seed、negative_prompt，reference_path/input_path 用于图生图或编辑。",
        drop_actions: &[],
        drop_fields: &[
            "guidance_scale",
            "input_paths",
            "layers",
            "mask_path",
            "num_inference_steps",
            "output_compression",
            "output_format",
            "resolution",
            "strength",
            "true_cfg_scale",
        ],
    },
    SurfaceRule {
        tool: "视频生成",
        brief_en: "Generate a video from prompt into path; optional seconds, size, fps, seed and negative_prompt.",
        brief_zh: "按 prompt 生成视频写入 path；可选 seconds、size、fps、seed、negative_prompt。",
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
        brief_en: "Load a mounted skill by name before other tools when a skill matches the task. Default action returns SKILL.md plus its resource index; use path/cursor/line_limit for bounded reads.",
        brief_zh: "任务命中技能时须先按 name 调用本工具；默认返回 SKILL.md 与资源索引，path/cursor/line_limit 用于分段读取。",
        drop_actions: &["manifest"],
        drop_fields: &["bytes_limit", "manifest_cursor", "section"],
    },
    SurfaceRule {
        tool: "写入文件",
        brief_en: "Create or fully replace a UTF-8 file with path + content. An empty content clears the file; dry_run previews the write.",
        brief_zh: "用 path + content 新建或整体覆盖 UTF-8 文件；content 为空即清空，dry_run 只预演。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "文本编辑",
        brief_en: "Edit UTF-8 files. Default command=str_replace needs file_path + old_string + new_string (exact single match unless replace_all); create needs file_path + file_text; insert needs insert_line; view needs view_range. dry_run previews.",
        brief_zh: "编辑 UTF-8 文件：默认 str_replace 需 file_path + old_string + new_string（精确唯一匹配，replace_all 可放宽）；create 需 file_text；insert 需 insert_line；view 需 view_range；dry_run 预演。",
        drop_actions: &[],
        drop_fields: &["input"],
    },
    SurfaceRule {
        tool: "子智能体控制",
        brief_en: "Manage child agent sessions: list/history/status to inspect, spawn(task) to create, send/report/wait for messages, close for lifecycle. Select by session_id or run_id; list defaults to direct children, parent_id=/root selects root children, and selectors stay in the current root tree. accepted means received, not finished.",
        brief_zh: "管理子智能体会话：list/history/status 查看，spawn(task) 创建，send/report/wait 通信，close 结束；用 session_id 或 run_id 定位，list 默认只看直接子级（parent_id=/root 选根级子级），选择器限于当前根任务树；accepted 仅表示已接收，不等于完成。",
        drop_actions: &["interrupt", "resume"],
        drop_fields: &[
            "announce_persist_history",
            "announce_emit_parent_events",
            "context_summary",
            "fork_turns",
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
        brief_zh: "列出、查看、创建、切换或归档任务线程；多数动作用 session_id，create 用 title/parent_session_id。",
        drop_actions: &["back", "restore", "update_title"],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "网页搜索",
        brief_en: "Search the public web with 1-4 independent queries; returns URLs, titles and snippets. Narrow with site and count.",
        brief_zh: "用 1-4 条独立 queries 搜索公网，返回链接、标题与摘要；site、count 可收窄结果。",
        drop_actions: &[],
        drop_fields: &["query", "sites", "sources", "categories", "scrape_results"],
    },
    SurfaceRule {
        tool: "网页抓取",
        brief_en: "Fetch one public URL and extract its main content (markdown|text). Use browser navigate for login, forms or clicks.",
        brief_zh: "抓取单个公开 URL 正文（markdown|text）；登录、表单、点击请改用 browser 的 navigate。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "桌面控制",
        brief_en: "Drive the local desktop. Pointer actions take bbox=[x1,y1,x2,y2]; type_text omits bbox to type into the focused control (newlines/tabs become Enter/Tab), press_key/scroll/wait take only their own field. Verify the returned screenshot before retrying the same action.",
        brief_zh: "控制本地桌面：指针动作传 bbox=[x1,y1,x2,y2]；type_text 可省略 bbox 直接输入当前焦点（换行/Tab 按 Enter/Tab 发送），press_key/scroll/wait 只带各自参数；重试同一动作前先核对返回的截图。",
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
        brief_en: "Drive and observe this Android phone: screenshot/ui_tree to inspect, tap/long_press/swipe to touch (bbox in a fixed 1000x1000 space), type_text, press_key for system keys, open_app/open_url/open_settings, list_apps/device_info/clipboard/wait. Touch, text and screenshots need the user-enabled accessibility service; check accessibility_connected and verify the returned frame after each action.",
        brief_zh: "操作并观察这台 Android 手机：screenshot/ui_tree 观察，tap/long_press/swipe 触摸（bbox 为 1000x1000 归一化坐标），type_text 输入，press_key 系统键，open_app/open_url/open_settings 打开应用、链接或设置，list_apps/device_info/clipboard/wait 辅助。触摸、输入和截图需要无障碍服务；先看 accessibility_connected，动作后核对截图再继续。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "自我状态",
        brief_en: "Report the current agent's own status; detail_level=basic|full.",
        brief_zh: "查询当前智能体自身状态；detail_level=basic|full。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "goal",
        brief_en: "Track the current task goal. get reads it; edit/pause/resume/complete/blocked need goal_id + revision from get. Goals are started by the user only; never create one.",
        brief_zh: "跟踪当前任务目标：get 读取，edit/pause/resume/complete/blocked 需 get 返回的 goal_id + revision。目标只能由用户发起，模型不得创建。",
        drop_actions: &[],
        drop_fields: &[],
    },
    SurfaceRule {
        tool: "browser",
        brief_en: "Built-in browser, no install needed. Use it for login, forms, clicks, pagination, JS pages or screenshots. Flow: navigate -> observe_refs -> click/type/press -> observe_text/screenshot; re-observe after changes. tabs_* manage tabs, target_id selects one; wait_ms only applies to wait.",
        brief_zh: "工作台内置浏览器，无需安装。登录、表单、点击、翻页、JS 页面或截图直接用：navigate → observe_refs → click/type/press → observe_text/screenshot，变化后重新 observe_refs；tabs_* 管理标签页，target_id 指定标签页，wait_ms 仅用于 wait。",
        drop_actions: &["hover", "select", "drag"],
        drop_fields: &[
            "from_ref",
            "from_selector",
            "to_ref",
            "to_selector",
            "value",
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
pub fn prompt_tool_spec(spec: &ToolSpec, language: &str) -> ToolSpec {
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
    let description = rule
        .and_then(|rule| {
            if language.to_ascii_lowercase().starts_with("zh") {
                (!rule.brief_zh.is_empty()).then_some(rule.brief_zh)
            } else {
                (!rule.brief_en.is_empty()).then_some(rule.brief_en)
            }
        })
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
                    "action": {"type": "string", "enum": ["navigate", "click", "drag", "hover"]},
                    "url": {"$ref": "#/$defs/used"},
                    "from_ref": {"type": "string"},
                    "value": {"type": "string"}
                },
                "required": ["action", "from_ref"],
                "additionalProperties": false
            }),
        };
        let projected = prompt_tool_spec(&spec, "en-US");
        assert_eq!(
            projected.input_schema["properties"]["action"]["enum"],
            json!(["navigate", "click"])
        );
        assert!(projected.input_schema["properties"]
            .get("from_ref")
            .is_none());
        assert!(projected.input_schema["properties"].get("value").is_none());
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
        let projected = prompt_tool_spec(&spec, "en-US");
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
                !rule.brief_zh.is_empty(),
                "{} lacks a Chinese brief",
                rule.tool
            );
            assert!(
                rule.brief_en.chars().count() <= 420,
                "{} English brief grew to {} chars",
                rule.tool,
                rule.brief_en.chars().count()
            );
            assert!(
                rule.brief_zh.chars().count() <= 260,
                "{} Chinese brief grew to {} chars",
                rule.tool,
                rule.brief_zh.chars().count()
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
        let projected = prompt_tool_spec(&spec, "en-US");
        assert_eq!(
            projected.input_schema["properties"]["query"]["description"],
            json!("what to look for")
        );
        assert_eq!(projected.description, "external tool");
    }

    #[test]
    fn projection_never_adds_required_fields_or_types() {
        for language in ["zh-CN", "en-US"] {
            let specs = crate::tools::catalog::builtin_tool_specs_with_language(language);
            for spec in specs {
                let projected = prompt_tool_spec(&spec, language);
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
                    "{} introduced a required field in {language}",
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
                    "{} introduced a property in {language}",
                    spec.name
                );
            }
        }
    }
}
