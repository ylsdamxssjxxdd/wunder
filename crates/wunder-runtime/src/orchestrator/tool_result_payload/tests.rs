use super::*;

#[test]
fn test_truncate_tool_result_string() {
    let head_chars = 2;
    let tail_chars = 3;
    let input = "abcdefghijklmnopqrstuvwxyz";
    let value =
        truncate_tool_result_string(input, head_chars, tail_chars, TOOL_RESULT_TRUNCATION_MARKER);
    assert!(value.starts_with("ab"));
    assert!(value.ends_with("xyz"));
    assert!(value.contains(TOOL_RESULT_TRUNCATION_MARKER));
    assert_eq!(
        value.chars().count(),
        head_chars + tail_chars + TOOL_RESULT_TRUNCATION_MARKER.chars().count()
    );
}

#[test]
fn test_truncate_tool_result_data() {
    let head_chars = 1;
    let tail_chars = 2;
    let stdout = "0123456789";
    let mut value = json!({ "stdout": stdout });
    let truncated = truncate_tool_result_data(
        &mut value,
        head_chars,
        tail_chars,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    assert!(truncated);
    let stdout = value.get("stdout").and_then(Value::as_str).unwrap_or("");
    assert!(stdout.starts_with("0"));
    assert!(stdout.ends_with("89"));
    assert!(stdout.contains(TOOL_RESULT_TRUNCATION_MARKER));
}

#[test]
fn test_truncate_tool_result_data_limits_large_arrays() {
    let mut rows = Vec::new();
    for idx in 0..200 {
        rows.push(json!({ "id": idx }));
    }
    let mut value = json!({ "rows": rows });
    let truncated = truncate_tool_result_data(
        &mut value,
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    assert!(truncated);
    let rows = value
        .get("rows")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(rows.len() <= TOOL_RESULT_MAX_ARRAY_ITEMS + 1);
    let has_marker = rows.iter().any(|item| {
        item.get("omitted_items")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
            && item.get("__truncated").and_then(Value::as_bool) == Some(true)
    });
    assert!(has_marker);
}

#[test]
fn test_truncate_tool_result_data_keeps_paginated_arrays_under_500() {
    let mut items = Vec::new();
    for idx in 0..200 {
        items.push(json!(format!("file-{idx}.md")));
    }
    let mut value = json!({
        "items": items,
        "cursor": "0",
        "limit": 200,
        "next_cursor": "200",
        "has_more": true
    });
    let truncated = truncate_tool_result_data(
        &mut value,
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    assert!(!truncated);
    let rows = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(rows.len(), 200);
    let has_marker = rows
        .iter()
        .any(|item| item.get("__truncated").and_then(Value::as_bool) == Some(true));
    assert!(!has_marker);
}

#[test]
fn test_truncate_tool_result_data_keeps_final_paginated_page_under_500() {
    let mut items = Vec::new();
    for idx in 0..74 {
        items.push(json!(format!("file-{idx}.md")));
    }
    let mut value = json!({
        "items": items,
        "cursor": "0",
        "limit": 200,
        "has_more": false,
        "next_cursor": null
    });
    let truncated = truncate_tool_result_data(
        &mut value,
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    assert!(!truncated);
    let rows = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(rows.len(), 74);
    let has_marker = rows
        .iter()
        .any(|item| item.get("__truncated").and_then(Value::as_bool) == Some(true));
    assert!(!has_marker);
}

#[test]
fn test_collect_truncation_reasons_from_value_detects_array_and_string() {
    let value = json!({
        "items": [
            "a",
            {"__truncated": true, "omitted_items": 10}
        ],
        "preview": format!("head{}tail", TOOL_RESULT_TRUNCATION_MARKER)
    });
    let reasons = collect_truncation_reasons_from_value(&value, TOOL_RESULT_TRUNCATION_MARKER);
    assert!(reasons.iter().any(|item| item == "array_items"));
    assert!(reasons.iter().any(|item| item == "string_chars"));
}

#[test]
fn test_observation_payload_is_compact_for_model_context() {
    let payload = ToolResultPayload {
        ok: true,
        data: json!({"items": ["a", "b"]}),
        error: String::new(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: Some(json!({
            "normalized_transport_ok": true,
            "normalized_business_ok": true,
            "normalized_final_ok": true,
            "error_retryable": false,
            "duration_ms": 12,
            "truncated": true,
            "continuation_required": true,
            "continuation_hint": TRUNCATION_CONTINUATION_HINT,
        })),
    };

    let observation = payload.to_compact_payload("list_files");
    assert!(observation.get("timestamp").is_none());
    assert!(observation.get("final_ok").is_none());
    assert!(observation.get("transport_ok").is_none());
    assert!(observation.get("business_ok").is_none());
    assert!(observation.get("meta").is_none());
    let data = observation
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(data.get("items").is_none());
    assert!(data.get("items_jsonl").is_some());
}

#[test]
fn test_event_payload_keeps_meta_for_frontend() {
    let payload = ToolResultPayload {
        ok: false,
        data: json!({"error": "boom"}),
        error: "boom".to_string(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: Some(json!({
            "normalized_transport_ok": true,
            "normalized_business_ok": false,
            "normalized_final_ok": false,
            "error_retryable": false,
            "error_code": "TOOL_BUSINESS_FAILED"
        })),
    };

    let event_payload = payload.to_event_payload("demo_tool");
    assert!(event_payload.get("timestamp").is_none());
    assert!(event_payload.get("meta").is_some());
    assert!(event_payload.get("final_ok").is_none());
    assert_eq!(
        event_payload.get("error_code").and_then(Value::as_str),
        Some("TOOL_BUSINESS_FAILED")
    );
}

#[test]
fn test_observation_payload_keeps_duration_for_workflow_display() {
    let payload = ToolResultPayload {
        ok: true,
        data: json!({"ok": true}),
        error: String::new(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: Some(json!({
            "duration_ms": 1280,
            "error_retryable": false
        })),
    };

    let compacted = payload.to_compact_payload("demo_tool");
    assert_eq!(
        compacted.get("duration_ms").and_then(Value::as_i64),
        Some(1280)
    );
}

#[test]
fn test_observation_payload_keeps_compact_preflight_rewrite_summary() {
    let payload = ToolResultPayload {
        ok: true,
        data: json!({"stdout": "ok"}),
        error: String::new(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: Some(json!({
            "preflight": {
                "status": "rewrite",
                "code": "PRECHECK_PYTHON_INDENTATION_NORMALIZED",
                "summary": "Auto-fixed before run: dedented common leading indentation; converted leading tabs to spaces.",
                "changes": [
                    "dedented common leading indentation",
                    "converted leading tabs to spaces"
                ],
                "diagnostics": [
                    {
                        "rule": "python.indent.global_offset",
                        "severity": "warn",
                        "message": "Detected global leading indentation (4 spaces); auto-dedented."
                    }
                ]
            }
        })),
    };

    let compacted = payload.to_compact_payload("ptc");
    let preflight = compacted
        .get("preflight")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        preflight.get("status").and_then(Value::as_str),
        Some("rewrite")
    );
    assert_eq!(
            preflight.get("summary").and_then(Value::as_str),
            Some(
                "Auto-fixed before run: dedented common leading indentation; converted leading tabs to spaces."
            )
        );
    assert_eq!(
        preflight
            .get("changes")
            .and_then(Value::as_array)
            .map(|items| items.len()),
        Some(2)
    );
}

#[test]
fn test_failed_observation_payload_drops_data_and_preflight_noise() {
    let payload = ToolResultPayload {
        ok: false,
        data: json!({
            "tool": "ptc",
            "preflight": {
                "status": "reject",
                "code": "PRECHECK_PYTHON_INDENTATION",
                "summary": "Script preflight blocked: non-standard Python indentation detected.",
                "diagnostics": [
                    {
                        "rule": "python.indent.non_standard",
                        "severity": "error",
                        "message": "Line 35 uses indentation width 11.",
                        "hint": "Use consistent 4-space indentation or only tabs (not mixed)."
                    }
                ]
            },
            "error_meta": {
                "code": "PRECHECK_PYTHON_INDENTATION",
                "hint": "Use consistent 4-space indentation or only tabs (not mixed).",
                "retryable": false,
                "retry_after_ms": null
            }
        }),
        error: "Script preflight blocked: non-standard Python indentation detected.".to_string(),
        sandbox: true,
        timestamp: Utc::now(),
        meta: Some(json!({
            "error_code": "PRECHECK_PYTHON_INDENTATION",
            "error_retryable": false,
            "preflight": {
                "status": "reject",
                "summary": "Script preflight blocked: non-standard Python indentation detected."
            },
            "duration_ms": 25
        })),
    };

    let compacted = payload.to_compact_payload("ptc");
    assert!(compacted.get("data").is_none());
    assert!(compacted.get("preflight").is_none());
    assert!(compacted.get("sandbox").is_none());
    assert!(compacted.get("duration_ms").is_none());
    assert_eq!(
            compacted.get("error").and_then(Value::as_str),
            Some(
                "Script preflight blocked: non-standard Python indentation detected. Line 35 uses indentation width 11."
            )
        );
    assert_eq!(
        compacted.get("error_code").and_then(Value::as_str),
        Some("PRECHECK_PYTHON_INDENTATION")
    );
    assert_eq!(
        compacted.get("retryable").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn test_failed_execute_command_observation_uses_single_concise_error() {
    let payload = ToolResultPayload {
        ok: false,
        data: json!({
            "command": "python3 draw_heart.py",
            "returncode": 1,
            "stderr": "  File \"/tmp/draw_heart.py\", line 6\n    y = 13\nIndentationError: unindent does not match any outer indentation level\n",
            "error_meta": {
                "code": "TOOL_EXEC_NON_ZERO_EXIT",
                "hint": "命令返回非 0，请先根据 stderr 修正后再重试。",
                "retryable": false,
                "retry_after_ms": null
            }
        }),
        error: "命令退出码 1。".to_string(),
        sandbox: true,
        timestamp: Utc::now(),
        meta: Some(json!({
            "error_code": "TOOL_EXEC_NON_ZERO_EXIT",
            "error_retryable": false,
            "duration_ms": 105
        })),
    };

    let compacted = payload.to_compact_payload("执行命令");
    assert!(compacted.get("data").is_none());
    assert_eq!(
            compacted.get("error").and_then(Value::as_str),
            Some(
                "命令退出码 1。 stderr: File \"/tmp/draw_heart.py\", line 6 | y = 13 | IndentationError: unindent does not match any outer indentation level"
            )
        );
    assert_eq!(
        compacted.get("error_code").and_then(Value::as_str),
        Some("TOOL_EXEC_NON_ZERO_EXIT")
    );
}

#[test]
fn test_compact_payload_strips_ids_and_budget_noise() {
    let payload = ToolResultPayload {
        ok: true,
        data: json!({
            "query": "九段线",
            "path": "kb",
            "budget": {"max_matches": 200},
            "scope": {"kind": "workspace_local"},
            "scope_note": "local only",
            "meta": {"search": {"elapsed_ms": 20}},
            "hits": [{"path": "a.md", "line": 1, "content": "九段线"}],
            "matches": ["a.md:1:九段线"]
        }),
        error: String::new(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: Some(json!({
            "error_retryable": false,
        })),
    };

    let mut compacted = payload.to_compact_payload("search_content");
    if let Value::Object(ref mut map) = compacted {
        map.insert(
            "tool_call_id".to_string(),
            Value::String("call_x".to_string()),
        );
        map.insert("trace_id".to_string(), Value::String("trace_x".to_string()));
        map.insert("model_round".to_string(), json!(2));
        map.insert("user_round".to_string(), json!(1));
    }
    strip_compact_payload_noise(&mut compacted, 0);

    let obj = compacted.as_object().cloned().unwrap_or_default();
    assert!(obj.get("tool_call_id").is_none());
    assert!(obj.get("trace_id").is_none());
    assert!(obj.get("model_round").is_none());
    assert!(obj.get("user_round").is_none());

    let data = obj
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(data.get("meta").is_none());
    assert!(data.get("budget").is_none());
    assert!(data.get("scope").is_none());
    assert!(data.get("scope_note").is_none());
    assert!(data.get("hits_jsonl").is_some());
    assert!(data.get("matches_jsonl").is_some());
}

#[test]
fn test_compact_large_tool_result_data_includes_preview() {
    let mut rows = Vec::new();
    for idx in 0..160 {
        rows.push(json!({
            "id": idx,
            "text": format!("row-{idx:03}-{}", "x".repeat(64)),
        }));
    }
    let value = json!({ "rows": rows });
    let chars = estimate_tool_result_chars(&value);
    let compacted = compact_large_tool_result_data(
        &value,
        chars,
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
        true,
        &["char_budget".to_string()],
    );
    assert_eq!(
        compacted.get("truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        compacted
            .get("original_chars")
            .and_then(Value::as_u64)
            .unwrap_or_default() as usize,
        chars
    );
    let preview = compacted
        .get("preview")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(!preview.is_empty());
    let reasons = compacted
        .get("truncation_reasons")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(reasons
        .iter()
        .any(|item| item.as_str() == Some("char_budget")));
    assert_eq!(
        compacted
            .get("continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        compacted
            .get("continuation_hint")
            .and_then(Value::as_str)
            .unwrap_or(""),
        TRUNCATION_CONTINUATION_HINT
    );
}

#[test]
fn test_compact_large_tool_result_data_keeps_combined_truncation_reasons() {
    let items = (0..900)
        .map(|idx| json!(format!("file-{idx}.md")))
        .collect::<Vec<_>>();
    let mut value = json!({
        "items": items,
        "cursor": "0",
        "next_cursor": "900",
        "has_more": true
    });
    let truncated = truncate_tool_result_data(
        &mut value,
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
    );
    assert!(truncated);

    let mut reasons = collect_truncation_reasons_from_value(&value, TOOL_RESULT_TRUNCATION_MARKER);
    append_truncation_reason(&mut reasons, "char_budget");
    dedupe_truncation_reasons(&mut reasons);

    let compacted = compact_large_tool_result_data(
        &value,
        estimate_tool_result_chars(&value),
        TOOL_RESULT_HEAD_CHARS,
        TOOL_RESULT_TAIL_CHARS,
        TOOL_RESULT_TRUNCATION_MARKER,
        true,
        &reasons,
    );
    let reasons = compacted
        .get("truncation_reasons")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(reasons
        .iter()
        .any(|item| item.as_str() == Some("array_items")));
    assert!(reasons
        .iter()
        .any(|item| item.as_str() == Some("char_budget")));
}

#[test]
fn test_compact_observation_payload_marks_truncation_fields() {
    let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
    let mut payload = json!({
        "tool": "extra_mcp@db_query",
        "ok": true,
        "data": {
            "structured_content": {
                "rows": [
                    {"text": text}
                ]
            }
        },
        "meta": {
            "duration_ms": 12
        }
    });

    compact_observation_payload(&mut payload, "执行命令");

    assert_eq!(
        payload.get("truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("continuation_required")
            .and_then(Value::as_bool),
        None
    );
    assert!(
        payload
            .get("observation_output_chars")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0
    );
    assert!(payload.get("meta").is_none());
}

#[test]
fn test_compact_observation_payload_marks_continuation_when_resumable() {
    let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
    let mut payload = json!({
        "tool": "extra_mcp@db_query",
        "ok": true,
        "data": {
            "structured_content": {
                "query_handle": "handle_123",
                "rows": [
                    {"text": text}
                ]
            }
        }
    });

    compact_observation_payload(&mut payload, "extra_mcp@db_query");

    assert_eq!(
        payload
            .get("continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("continuation_hint").and_then(Value::as_str),
        Some(TRUNCATION_CONTINUATION_HINT)
    );
}

#[test]
fn test_compact_observation_payload_marks_continuation_for_read_file() {
    let text = "x".repeat(OBSERVATION_HEAD_CHARS + OBSERVATION_TAIL_CHARS + 80);
    let mut payload = json!({
        "tool": "读取文件",
        "ok": true,
        "data": {
            "content": text,
            "meta": {
                "files": [
                    {
                        "path": "notes.md",
                        "used_default_range": true,
                        "read_lines": 2000,
                        "total_lines": 4907
                    }
                ],
                "read": {
                    "requested_files": 1,
                    "processed_files": 1
                }
            }
        }
    });

    compact_observation_payload(&mut payload, "读取文件");

    assert_eq!(
        payload
            .get("continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload.get("continuation_hint").and_then(Value::as_str),
        Some(TRUNCATION_CONTINUATION_HINT)
    );
    let data = payload.get("data").and_then(Value::as_object);
    assert!(data.and_then(|value| value.get("meta")).is_none());
    assert!(data.and_then(|value| value.get("content")).is_some());
    let compacted = Value::Object(data.cloned().unwrap_or_default());
    let content = data
        .and_then(|value| value.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(content.contains(TOOL_RESULT_TRUNCATION_MARKER));
    assert!(estimate_tool_result_chars(&compacted) <= OBSERVATION_MAX_CHARS);
}

#[test]
fn test_compact_observation_payload_read_file_strips_read_output_notice() {
    let mut payload = json!({
        "tool": "读取文件",
        "ok": true,
        "data": {
            "content": "line1\nline2\n...(truncated read output, omitted 512 bytes)...",
            "meta": {
                "files": [
                    {
                        "path": "notes.md",
                        "read_lines": 2,
                        "total_lines": 100,
                        "complete": false
                    }
                ],
                "read": {
                    "requested_files": 1,
                    "processed_files": 1
                }
            }
        }
    });

    compact_observation_payload(&mut payload, "读取文件");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        data.get("read_output_omitted_bytes")
            .and_then(Value::as_u64),
        Some(512)
    );
    assert_eq!(
        data.get("continuation_required").and_then(Value::as_bool),
        Some(true)
    );
    let content = data.get("content").and_then(Value::as_str).unwrap_or("");
    assert!(!content.contains("truncated read output"));
}

#[test]
fn test_compact_observation_payload_read_file_limits_content_without_marker() {
    let long_content = "x".repeat(8_000);
    let mut payload = json!({
        "tool": "读取文件",
        "ok": true,
        "data": {
            "content": long_content,
            "patch_usage_hint": "do not copy display markers",
            "meta": {
                "files": [
                    {
                        "path": "notes.md",
                        "read_lines": 800,
                        "total_lines": 2000,
                        "complete": false
                    }
                ],
                "read": {
                    "requested_files": 1,
                    "processed_files": 1
                }
            }
        }
    });

    compact_observation_payload(&mut payload, "读取文件");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        data.get("continuation_required").and_then(Value::as_bool),
        None
    );
    assert_eq!(
        data.get("patch_usage_hint").and_then(Value::as_str),
        Some("do not copy display markers")
    );
    let content = data.get("content").and_then(Value::as_str).unwrap_or("");
    assert_eq!(content.chars().count(), 8_000);
    assert!(!content.contains(TOOL_RESULT_TRUNCATION_MARKER));
}

#[test]
fn test_compact_observation_payload_read_file_keeps_patch_usage_hint_from_flat_data() {
    let mut payload = json!({
        "tool": "读取文件",
        "ok": true,
        "data": {
            "content": ">>> notes.md\n1: alpha",
            "patch_usage_hint": "do not copy >>> path or N: prefixes",
            "files": [
                {
                    "path": "notes.md",
                    "read_lines": 1,
                    "total_lines": 4,
                    "complete": false
                }
            ]
        }
    });

    compact_observation_payload(&mut payload, "读取文件");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        data.get("patch_usage_hint").and_then(Value::as_str),
        Some("do not copy >>> path or N: prefixes")
    );
    let files_jsonl = data
        .get("files_jsonl")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(files_jsonl.contains("\"path\":\"notes.md\""));
    assert!(files_jsonl.contains("\"read_lines\":1"));
}

#[test]
fn test_compact_observation_payload_read_file_uses_unified_budget_for_large_content() {
    let long_content = "x".repeat(OBSERVATION_MAX_CHARS + 240);
    let mut payload = json!({
        "tool": "读取文件",
        "ok": true,
        "data": {
            "content": long_content,
            "meta": {
                "files": [
                    {
                        "path": "notes.md",
                        "read_lines": 800,
                        "total_lines": 2000,
                        "complete": false
                    }
                ],
                "read": {
                    "requested_files": 1,
                    "processed_files": 1
                }
            }
        }
    });

    compact_observation_payload(&mut payload, "读取文件");

    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    assert_eq!(
        payload
            .get("continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    let maybe_content = data.get("content").and_then(Value::as_str);
    let maybe_preview = data.get("preview").and_then(Value::as_str);
    let visible = maybe_content.or(maybe_preview).unwrap_or("");
    assert!(visible.chars().count() > OBSERVATION_HEAD_CHARS);
    assert!(data.get("content_head").and_then(Value::as_str).is_some());
    assert!(
        data.get("content_omitted_chars")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn test_compact_observation_payload_compacts_search_payload() {
    let hits = (0..12)
        .map(|idx| {
            json!({
                "path": format!("docs/{idx}.md"),
                "line": idx + 1,
                "content": format!("match-{idx}-{}", "x".repeat(240)),
                "before": [],
                "after": [],
                "segments": [{"matched": true, "text": "match"}]
            })
        })
        .collect::<Vec<_>>();
    let mut payload = json!({
        "tool": "search_content",
        "ok": true,
        "data": {
            "query": "match",
            "query_mode": "literal",
            "path": "docs",
            "strategy": "literal_exact",
            "returned_match_count": 12,
            "matched_file_count": 12,
            "hits": hits,
            "matches": ["a:1:match", "b:2:match"],
            "meta": {"search": {"elapsed_ms": 20}},
            "scope": {"kind": "workspace_local"},
            "scope_note": "debug"
        }
    });

    compact_observation_payload(&mut payload, "search_content");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(data.get("meta").is_none());
    assert!(data.get("scope").is_none());
    assert!(data.get("scope_note").is_none());
    assert!(data.get("hits").is_none());
    assert!(data.get("hits_jsonl").and_then(Value::as_str).is_some());
    assert!(data.get("matches").is_none());
    assert!(data.get("matches_jsonl").and_then(Value::as_str).is_some());
    let first_hit = data
        .get("hits_jsonl")
        .and_then(Value::as_str)
        .and_then(|value| value.lines().next())
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    assert!(first_hit.get("content").is_none());
    assert!(first_hit
        .get("content_head")
        .and_then(Value::as_str)
        .is_some());
    assert!(first_hit
        .get("content_head")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.contains(TOOL_RESULT_TRUNCATION_MARKER)));
}

#[test]
fn test_compact_dense_arrays_to_jsonl_converts_all_array_keys() {
    let mut data = json!({
        "items": ["a", "b"],
        "summary": {
            "top_files": ["x.md", "y.md"],
            "scores": [1, 2, 3]
        },
        "empty_list": []
    });

    compact_dense_arrays_to_jsonl(&mut data);

    let obj = data.as_object().cloned().unwrap_or_default();
    assert!(obj.get("items").is_none());
    assert_eq!(obj.get("items_count").and_then(Value::as_u64), Some(2));
    assert!(obj.get("items_jsonl").and_then(Value::as_str).is_some());
    assert_eq!(obj.get("empty_list_count").and_then(Value::as_u64), Some(0));
    assert_eq!(
        obj.get("empty_list_jsonl").and_then(Value::as_str),
        Some("")
    );

    let summary = obj
        .get("summary")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert!(summary.get("top_files").is_none());
    assert!(summary.get("scores").is_none());
    assert_eq!(
        summary.get("top_files_count").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(summary.get("scores_count").and_then(Value::as_u64), Some(3));
    assert!(summary
        .get("top_files_jsonl")
        .and_then(Value::as_str)
        .is_some());
    assert!(summary
        .get("scores_jsonl")
        .and_then(Value::as_str)
        .is_some());
}

#[test]
fn test_compact_dense_arrays_to_jsonl_slims_execute_command_rows() {
    let mut data = json!({
        "results": [
            {
                "command": "python draw_heart.py",
                "command_index": 0,
                "command_session_id": "cmd_123",
                "returncode": 127,
                "stdout": "",
                "stderr": "python: command not found",
                "output_meta": {
                    "truncated": false,
                    "total_bytes": 40
                }
            }
        ]
    });

    compact_dense_arrays_to_jsonl(&mut data);

    let obj = data.as_object().cloned().unwrap_or_default();
    assert!(obj.get("results").is_none());
    assert_eq!(obj.get("results_count").and_then(Value::as_u64), Some(1));
    let line = obj
        .get("results_jsonl")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parsed = serde_json::from_str::<Value>(line).unwrap_or(Value::Null);
    let parsed_obj = parsed.as_object().cloned().unwrap_or_default();
    assert_eq!(
        parsed_obj.get("command").and_then(Value::as_str),
        Some("python draw_heart.py")
    );
    assert_eq!(
        parsed_obj.get("returncode").and_then(Value::as_i64),
        Some(127)
    );
    assert_eq!(
        parsed_obj.get("stderr").and_then(Value::as_str),
        Some("python: command not found")
    );
    assert!(parsed_obj.get("stdout").is_none());
    assert!(parsed_obj.get("output_meta").is_none());
    assert!(parsed_obj.get("command_session_id").is_none());
    assert!(parsed_obj.get("command_index").is_none());
}

#[test]
fn test_compact_observation_payload_samples_large_rows() {
    let rows = (0..24)
        .map(|idx| json!({ "employee_id": format!("E{idx:06}"), "eligible": "yes" }))
        .collect::<Vec<_>>();
    let mut payload = json!({
        "tool": "extra_mcp@db_query",
        "ok": true,
        "data": {
            "structured_content": {
                "ok": true,
                "row_count": 24,
                "rows": rows
            }
        }
    });

    compact_observation_payload(&mut payload, "extra_mcp@db_query");

    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    assert!(data.get("rows").is_none());
    assert!(data.get("rows_jsonl").and_then(Value::as_str).is_some());
    assert_eq!(
        data.get("rows_sampled").and_then(Value::as_u64),
        Some(OBSERVATION_TABLE_SAMPLE_ROWS as u64)
    );
    assert_eq!(data.get("rows_omitted").and_then(Value::as_u64), Some(20));
}

#[test]
fn test_compact_observation_payload_preserves_truncated_wrapper_preview_budget() {
    let preview = "x".repeat(OBSERVATION_MAX_CHARS + 4_096);
    let mut payload = json!({
        "tool": "extra_mcp@db_query",
        "ok": true,
        "data": {
            "truncated": true,
            "original_chars": 4096,
            "preview": preview,
            "truncation_reasons": ["array_items", "char_budget"],
            "continuation_required": true,
            "continuation_hint": TRUNCATION_CONTINUATION_HINT
        }
    });

    compact_observation_payload(&mut payload, "extra_mcp@db_query");

    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    let preview = data.get("preview").and_then(Value::as_str).unwrap_or("");
    let mut data_without_preview = data.as_object().cloned().unwrap_or_default();
    data_without_preview.remove("preview");
    let expected_budget = OBSERVATION_MAX_CHARS.saturating_sub(
        estimate_tool_result_chars(&Value::Object(data_without_preview))
            .saturating_add("preview".chars().count()),
    );
    assert!(preview.contains(TOOL_RESULT_TRUNCATION_MARKER));
    assert!(expected_budget > 18_000);
    assert_eq!(preview.chars().count(), expected_budget);
    assert!(estimate_tool_result_chars(&data) <= OBSERVATION_MAX_CHARS);
    assert_eq!(
        data.get("continuation_required").and_then(Value::as_bool),
        Some(true)
    );
    let reasons = data
        .get("truncation_reasons")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(reasons
        .iter()
        .any(|item| item.as_str() == Some("array_items")));
    assert!(reasons
        .iter()
        .any(|item| item.as_str() == Some("char_budget")));
}

#[test]
fn test_compact_observation_payload_skips_skill_call_truncation() {
    let text = "x".repeat(OBSERVATION_MAX_CHARS + 500);
    let mut payload = json!({
        "tool": "技能调用",
        "ok": true,
        "data": {
            "skill_md": text,
        }
    });

    compact_observation_payload(&mut payload, "技能调用");

    assert!(payload.get("meta").is_none());
    assert!(payload.get("truncated").is_none());
    assert!(
        payload
            .get("data")
            .and_then(|value| value.get("skill_md"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .count()
            > OBSERVATION_MAX_CHARS
    );
}

#[test]
fn test_compact_observation_payload_write_file_keeps_precise_path() {
    let mut payload = json!({
        "tool": "写入文件",
        "ok": true,
        "data": {
            "path": "nai_long2.py",
            "bytes": 9297,
            "dry_run": false,
            "existed": false,
            "previous_bytes": 0
        },
        "meta": {
            "duration_ms": 4
        }
    });

    compact_observation_payload(&mut payload, "写入文件");
    strip_compact_payload_noise(&mut payload, 0);

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        data.get("path").and_then(Value::as_str),
        Some("nai_long2.py")
    );
    assert_eq!(data.get("bytes").and_then(Value::as_u64), Some(9297));
}

#[test]
fn test_compact_observation_payload_compacts_apply_patch_files_without_diff_blocks() {
    let mut payload = json!({
        "tool": "apply_patch",
        "ok": true,
        "data": {
            "changed_files": 1,
            "added": 0,
            "updated": 1,
            "deleted": 0,
            "moved": 0,
            "hunks_applied": 1,
            "files": [
                {
                    "action": "update",
                    "path": "src/main.rs",
                    "to_path": null,
                    "hunks": 1,
                    "diff_blocks": [
                        {
                            "header": "@@ -1,1 +1,1 @@",
                            "lines": [
                                { "kind": "delete", "old_line": 1, "new_line": null, "text": "old" },
                                { "kind": "add", "old_line": null, "new_line": 1, "text": "new" }
                            ]
                        }
                    ]
                }
            ]
        }
    });

    compact_observation_payload(&mut payload, "apply_patch");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let files = data
        .get("files")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let first = files
        .first()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        first.get("path").and_then(Value::as_str),
        Some("src/main.rs")
    );
    assert_eq!(first.get("hunks").and_then(Value::as_u64), Some(1));
    assert!(first.get("diff_blocks").is_none());
}
