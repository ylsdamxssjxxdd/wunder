use super::*;
use crate::orchestrator_constants::{TOOL_RESULT_HEAD_CHARS, TOOL_RESULT_TAIL_CHARS};

#[test]
fn goal_success_keeps_wrapup_for_scheduler_and_events_only() {
    let goal = json!({"id": "goal-1", "revision": 3, "phase": "complete", "remainingRounds": 2});
    let wrapup = json!({
        "kind": "complete",
        "objective": "Finish the sample task",
        "blocked_reason": null,
        "text": "Summarize the verified result"
    });
    let result = ToolResultPayload::from_value(json!({
        "goal": goal,
        "__wrapup": wrapup
    }));

    assert_eq!(
        result.to_compact_payload("goal")["data"],
        json!({"goal": goal})
    );
    assert_eq!(result.to_event_payload("goal")["data"]["__wrapup"], wrapup);
    assert_eq!(
        crate::services::goal::read_wrapup(&result.data).as_deref(),
        Some("Summarize the verified result")
    );
    assert_eq!(
        ToolResultPayload::from_value(json!({"goal": null})).to_compact_payload("goal")["data"],
        json!({"goal": null})
    );
}

#[test]
fn desktop_observation_preserves_unsettled_frame_without_audit_details() {
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {
            "action": "left_click",
            "action_applied": true,
            "observation_stable": false,
            "observation_settled_ms": 1520,
            "screenshot_artifact": {"id": "desktop/frame.png", "mime": "image/png", "bytes": 64},
            "screenshot_path": "private/frame.png",
            "followup_prompt": "wait for the display to settle"
        }
    }));

    assert_eq!(
        result.to_compact_payload("desktop_control")["data"],
        json!({
            "action": "left_click",
            "action_applied": true,
            "observation_stable": false,
            "screenshot_artifact": {"id": "desktop/frame.png", "mime": "image/png", "bytes": 64}
        })
    );
    assert_eq!(
        result.to_event_payload("desktop_control")["data"]["observation_settled_ms"],
        1520
    );
}

#[test]
fn browser_attachment_is_compact_for_model_and_complete_in_events() {
    let content = json!([
        {"type":"text","text":"Captured screenshot."},
        {"type":"image","attachment":{"path":"browser/screenshots/sample.png","bytes":64}}
    ]);
    let result = ToolResultPayload::from_value(json!({
        "ok":true,"data":{"action":"screenshot","format":"png","target_id":"tab-1"},"content":content
    }));
    let event = result.to_event_payload("browser");
    assert_eq!(event["data"]["content"], content);
    assert_eq!(event["data"]["format"], "png");
    let observation = result.to_compact_payload("browser");
    assert_eq!(
        observation["data"]["artifact"],
        json!({"id":"browser/screenshots/sample.png","bytes":64})
    );
    assert!(observation["data"].get("content").is_none());
    assert!(observation["data"].get("format").is_none());
    let result = ToolResultPayload::from_value(json!({
        "ok":true,"data":{"content":"page text"},"content":content
    }));
    assert_eq!(result.data["content"], "page text");
    assert_eq!(result.data["content_blocks"], content);
}

#[test]
fn browser_failure_keeps_partial_progress_in_model_observation() {
    let result = ToolResultPayload::from_value(json!({
        "ok":false,"error":"Step failed", "content":[],
        "data":{"action":"batch","completed_steps":2,"failed_step":3,"results":[{"kind":"click"}]}
    }));
    let observation = result.to_compact_payload("browser");
    assert_eq!(observation["ok"], false);
    assert_eq!(observation["data"]["completed_steps"], 2);
    assert_eq!(observation["data"]["failed_step"], 3);
}

#[test]
fn browser_canonical_actions_are_projected_to_next_step_fields() {
    let navigate = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"action":"navigate", "url": "https://example.invalid/next", "title": "Next", "tabs": [{"target_id":"tab-1"}]}
    }));
    assert_eq!(
        navigate.to_compact_payload("browser")["data"],
        json!({"url":"https://example.invalid/next", "title":"Next"})
    );

    let action = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"action": "click", "kind": "click", "target_id": "tab-1", "ref": "e1", "url": "https://example.invalid/done", "snapshot": "large"}
    }));
    assert_eq!(
        action.to_compact_payload("browser")["data"],
        json!({"action":"click", "target_ref":"e1", "url":"https://example.invalid/done"})
    );
}

#[test]
fn packaged_mcp_call_projects_remote_content_without_raw_blobs() {
    let oversized = "x".repeat(6_000);
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "action": "mcp_package",
        "package_action": "call",
        "data": {
            "server": "remote",
            "tool": "inspect",
            "result": {
                "is_error": false,
                "content": [
                    {"type":"text", "text": oversized},
                    {"type":"image", "data":"aGVsbG8=", "mimeType":"image/png"},
                    {"type":"resource_link", "uri":"mcp://remote/report", "name":"report"}
                ],
                "structured_content": {
                    "visible": "ok",
                    "blob": "must-not-reach-model",
                    "rows": (0..20).collect::<Vec<_>>(),
                    "nested": {"a":{"b":{"c":{"d":{"leak":"hidden"}}}}}
                },
                "meta": {"provider_secret": "audit-only"}
            }
        }
    }));

    let event = result.to_event_payload("remote@__mcp_pack__");
    assert_eq!(
        event["data"]["result"]["meta"]["provider_secret"],
        "audit-only"
    );
    assert_eq!(event["data"]["result"]["content"][1]["data"], "aGVsbG8=");

    let observation = result.to_compact_payload("remote@__mcp_pack__");
    assert_eq!(observation["data"]["server"], "remote");
    assert_eq!(observation["data"]["tool"], "inspect");
    assert_eq!(observation["data"]["remote_error"], false);
    assert!(observation["data"]["content"][0]["text"]
        .as_str()
        .is_some_and(|text| text.len() < oversized.len()));
    assert!(observation["data"]["content"][1].get("data").is_none());
    assert_eq!(observation["data"]["content"][1]["mimeType"], "image/png");
    assert!(observation["data"]["structured_content"]
        .get("blob")
        .is_none());
    assert_eq!(
        observation["data"]["structured_content"]["rows"]
            .as_array()
            .unwrap()
            .len(),
        13
    );
    assert_eq!(
        observation["data"]["structured_content"]["nested"]["a"]["b"]["c"],
        json!({"__truncated":true})
    );
}

#[test]
fn self_status_and_command_success_drop_runtime_details() {
    let status = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {
            "session": {"user_id":"user", "session_id":"session"},
            "model": {"provider":"provider", "base_url":"https://llm.invalid"},
            "monitor": {"status":"running", "stage":"tools", "updated_time":"now"},
            "context": {"context_occupancy_tokens": 1234, "context_overflow": false},
            "rounds": {"model_rounds_peak": 4},
            "events": {"by_type": {"tool_result": 20}}
        }
    }));
    let status = status.to_compact_payload("self_status");
    assert_eq!(
        status["data"],
        json!({"stage":"tools", "status":"running", "round":4, "context_tokens":1234})
    );

    let command = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"results":[{"command":"echo ok", "returncode":0, "stdout":"ok", "stderr":"", "output_meta":{"total_bytes":2}}], "budget":{"max":1000}, "output_guard":{"truncated":false}}
    }));
    let command = command.to_compact_payload("execute_command");
    assert_eq!(command["data"]["exit_code"], 0);
    assert_eq!(command["data"]["stdout"], "ok");
    assert!(command["data"].get("budget").is_none());
    assert!(command["data"].get("output_guard").is_none());
}

#[test]
fn event_payload_keeps_projection_source_for_audit() {
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"path":"notes.txt", "bytes":9, "existed":true, "previous_bytes":4, "lsp":{"state":"ok"}},
        "meta": {"duration_ms": 3, "trace_id":"trace"}
    }));
    let event = result.to_event_payload("write_file");
    assert_eq!(event["data"]["bytes"], 9);
    assert_eq!(event["data"]["previous_bytes"], 4);
    assert_eq!(event["meta"]["trace_id"], "trace");
    let model = result.to_compact_payload("write_file");
    assert!(model["data"].get("bytes").is_none());
    assert!(model.get("meta").is_none());
}

#[test]
fn browser_content_only_failure_is_not_treated_as_success() {
    let result = ToolResultPayload::from_value(json!({
        "ok":false,"error":"Browser failed", "content":[{"type":"text","text":"Browser failed"}]
    }));
    assert!(!result.ok);
    assert_eq!(result.error, "Browser failed");
    assert_eq!(result.data["content"][0]["type"], "text");
}

#[test]
fn browser_page_observation_keeps_text_and_truncation_without_page_noise() {
    let result = ToolResultPayload::from_value(json!({
        "ok":true, "data":{"action":"observe_text","target_id":"tab-1","truncated":true},
        "content":[{"type":"text","text":"{\"sample\":1}"}]
    }));
    let observation = result.to_compact_payload("browser");
    assert!(observation["data"].get("target_id").is_none());
    assert_eq!(observation["data"]["truncated"], true);
    assert_eq!(observation["data"]["text"], "{\"sample\":1}");
}

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
    assert_eq!(data.get("items"), Some(&json!(["a", "b"])));
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
fn test_observation_payload_omits_duration_but_event_retains_it() {
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
    assert!(compacted.get("duration_ms").is_none());
    assert_eq!(
        payload.to_event_payload("tool")["meta"]["duration_ms"],
        json!(1280)
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
    strip_compact_payload_noise(&mut compacted, "search_content");

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
    assert!(data.get("hits").is_some());
    assert!(data.get("matches").is_none());
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
    let value = json!({
        "items": ["file-0.md", {"__truncated": true, "omitted_items": 898}, "file-899.md"],
        "cursor": "0",
        "next_cursor": "900",
        "has_more": true
    });

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
    assert_eq!(data["files"][0]["path"], json!("notes.md"));
    assert_eq!(data["files"][0]["read_lines"], json!(1));
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
                "before": [{"line": idx, "content": "context before"}],
                "after": [{"line": idx + 2, "content": "context after"}],
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
    assert!(data.get("hits").and_then(Value::as_array).is_some());
    assert!(data.get("matches").is_none());
    let first_hit = data["hits"][0].as_object().cloned().unwrap_or_default();
    assert!(first_hit.get("content").is_none());
    assert!(first_hit
        .get("content_head")
        .and_then(Value::as_str)
        .is_some());
    assert!(first_hit
        .get("content_head")
        .and_then(Value::as_str)
        .is_some_and(|text| !text.contains(TOOL_RESULT_TRUNCATION_MARKER)));
    assert_eq!(first_hit["before"][0]["content"], json!("context before"));
    assert_eq!(first_hit["after"][0]["content"], json!("context after"));
}

#[test]
fn observation_preserves_array_types_and_semantic_fields() {
    let data = json!({"scope": "value", "meta": {"value": 1}, "items": [null, "", false, 0, [], {"timestamp": 3, "metrics": [1, 2]}]});
    let payload = ToolResultPayload::from_value(json!({"ok": true, "data": data}));
    assert_eq!(payload.to_compact_payload("external")["data"], data);
}

#[test]
fn command_rows_retain_followup_handles() {
    let mut data = json!({"results": [{
        "command": "operation", "returncode": 0, "stdout": "", "stderr": "",
        "command_session_id": "session", "status": "running", "command_index": 0
    }]});
    compact_command_observation_rows(&mut data, "执行命令");
    assert_eq!(
        data,
        json!({"results": [{
            "command": "operation", "returncode": 0,
            "command_session_id": "session", "status": "running"
        }]})
    );
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
    assert!(data.get("rows").and_then(Value::as_array).is_some());
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
fn test_compact_observation_payload_bounds_skill_call_truncation() {
    let text = "x".repeat(OBSERVATION_MAX_CHARS + 500);
    let result = ToolResultPayload::from_value(json!({
        "tool": "技能调用",
        "ok": true,
        "data": {
            "skill_md": text,
            "truncated": true,
        }
    }));
    let payload = result.to_compact_payload("skill_read");

    assert_eq!(
        payload.get("truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert!(payload.get("continuation_required").is_some());
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    assert!(estimate_tool_result_chars(&data) <= OBSERVATION_MAX_CHARS);
    assert!(result.to_event_payload("skill_read")["data"]["skill_md"]
        .as_str()
        .is_some_and(|value| value.chars().count() > OBSERVATION_MAX_CHARS));
}

#[test]
fn test_compact_observation_payload_bounds_large_browser_observation() {
    let text = "x".repeat(OBSERVATION_MAX_CHARS + 500);
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"action":"observe_text", "text": text, "next_cursor": "cursor-2", "has_more": true}
    }));
    let observation = result.to_compact_payload("browser");
    assert_eq!(
        observation.get("truncated").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        observation
            .get("continuation_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    let data = observation.get("data").cloned().unwrap_or(Value::Null);
    assert!(estimate_tool_result_chars(&data) <= OBSERVATION_MAX_CHARS);
    assert!(result.to_event_payload("browser")["data"]["text"]
        .as_str()
        .is_some_and(|value| value.chars().count() > OBSERVATION_MAX_CHARS));
}

#[test]
fn test_compact_observation_payload_keeps_small_browser_projection_unmarked() {
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {"action": "click", "ref": "e1", "url": "https://example.invalid/next"}
    }));
    let observation = result.to_compact_payload("browser");
    assert!(observation.get("truncated").is_none());
    assert!(observation.get("observation_output_chars").is_none());
    assert_eq!(observation["data"]["action"], "click");
}

#[test]
fn test_compact_observation_payload_write_file_keeps_change_summary_only() {
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
    strip_compact_payload_noise(&mut payload, "write_file");

    let data = payload
        .get("data")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        data.get("path").and_then(Value::as_str),
        Some("nai_long2.py")
    );
    assert_eq!(data.get("changed"), Some(&json!(true)));
    assert_eq!(data.get("dry_run"), Some(&json!(false)));
    assert!(data.get("bytes").is_none());
    assert!(data.get("existed").is_none());
    assert!(data.get("previous_bytes").is_none());
}

#[test]
fn test_compact_observation_payload_projects_apply_patch_to_change_counts() {
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
    assert_eq!(data.get("changed"), Some(&json!(true)));
    assert_eq!(data.get("changed_files"), Some(&json!(1)));
    assert_eq!(data.get("hunks_applied"), Some(&json!(1)));
    assert!(data.get("files").is_none());
}

#[test]
fn observation_keeps_content_siblings_and_pagination() {
    let data = json!({"content": "value", "next_cursor": "cursor", "total": 7, "has_more": true});
    let payload = ToolResultPayload::from_value(json!({"ok": true, "data": data}));
    assert_eq!(payload.to_compact_payload("external")["data"], data);
    let payload = ToolResultPayload::from_value(json!({"ok": true,
        "data": {"structured_content": {"items": [1]}},
        "meta": {"next_cursor": "cursor", "has_more": true, "truncated": true}
    }));
    let compacted = payload.to_compact_payload("external");
    assert_eq!(compacted["next_cursor"], json!("cursor"));
    assert_eq!(compacted["has_more"], json!(true));
    assert_eq!(compacted["truncated"], json!(true));
    assert_eq!(compacted["data"], json!({"items": [1]}));
}

#[test]
fn observation_failure_uses_stdout_and_failed_batch_row() {
    for data in [
        json!({"stderr": "", "stdout": "Error: invalid value", "returncode": 1}),
        json!({"results": [
            {"returncode": 0, "stdout": "complete"},
            {"returncode": 1, "stderr": "", "stdout": "Error: invalid value"}
        ]}),
    ] {
        let payload =
            ToolResultPayload::from_value(json!({"ok": false, "data": data, "error": "failed"}));
        assert_eq!(
            payload.to_compact_payload("execute_command"),
            json!({
                "tool": "execute_command", "ok": false, "error": "failed stdout: Error: invalid value"
            })
        );
    }
}

#[test]
fn native_arrays_cost_less_than_nested_jsonl_without_losing_values() {
    let rows = (0..24)
        .map(|index| json!({"index": index, "value": "value"}))
        .collect::<Vec<_>>();
    let native = json!({"items": rows});
    let legacy = json!({"items_count": rows.len(), "items_jsonl": rows.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")});
    assert!(approx_token_count(&native.to_string()) < approx_token_count(&legacy.to_string()));
}

#[test]
fn scalar_structured_content_keeps_outer_cursor() {
    let payload = ToolResultPayload::from_value(json!({"ok": true, "data": {
        "structured_content": "value", "next_cursor": "cursor", "has_more": true
    }}));
    let result = payload.to_compact_payload("external");
    assert_eq!(result["data"], json!("value"));
    assert_eq!(result["next_cursor"], json!("cursor"));
    assert_eq!(result["has_more"], json!(true));
}

#[test]
fn observation_token_budget_bounds_full_serialized_envelope_and_keeps_cursor() {
    let result = ToolResultPayload::from_value(json!({
        "ok": true,
        "data": {
            "text": "中🙂".repeat(20_000),
            "next_cursor": "page-2",
            "has_more": true
        }
    }));

    let budget = 512;
    let observation = result.to_compact_payload_with_token_budget("browser", Some(budget));
    assert!(observation_token_count(&observation) <= budget);
    assert_eq!(observation["truncated"], true);
    assert_eq!(observation["data"]["truncated"], true);
    assert_eq!(observation["next_cursor"], "page-2");
    assert!(observation["recovery_hint"]
        .as_str()
        .unwrap()
        .contains("next_page_skips"));
    assert!(
        observation["data"]["omitted_bytes"]
            .as_u64()
            .unwrap_or_default()
            > 0
    );
}

#[test]
fn observation_token_budget_does_not_change_event_payload() {
    let text = "x".repeat(32_000);
    let result = ToolResultPayload::from_value(json!({"ok": true, "data": {"text": text}}));
    let observation = result.to_compact_payload_with_token_budget("browser", Some(256));

    assert!(observation_token_count(&observation) <= 256);
    assert!(result.to_event_payload("browser")["data"]["text"]
        .as_str()
        .is_some_and(|value| value.len() == 32_000));
}

#[test]
fn observation_preserves_opaque_cursor_exactly_or_omits_it_explicitly() {
    for (cursor, expected) in [
        ("opaque/".repeat(60), true),
        ("opaque/".repeat(2_000), false),
    ] {
        let result = ToolResultPayload::from_value(json!({"ok": true, "data": {
            "text": "value".repeat(8_000), "next_cursor": cursor, "has_more": true
        }}));
        let observation = result.to_compact_payload_with_token_budget("external", Some(512));
        assert!(observation_token_count(&observation) <= 512);
        if expected {
            assert_eq!(observation["next_cursor"], cursor);
        } else {
            assert!(observation.get("next_cursor").is_none());
            assert_eq!(observation["continuation_omitted"], true);
        }
        assert!(observation["recovery_hint"]
            .as_str()
            .unwrap()
            .contains("reread"));
    }
}

#[test]
fn observation_budget_keeps_failure_status_and_error_ends() {
    let error = format!("reason-start {} reason-end", "\"\\\n中🙂".repeat(10_000));
    let result = ToolResultPayload::from_value(json!({
        "ok": false, "error": error, "data": {},
        "meta": {"error_code": "TOOL_FAILED", "error_retryable": false}
    }));
    let observation = result.to_compact_payload_with_token_budget("external", Some(256));
    assert!(observation_token_count(&observation) <= 256);
    assert_eq!(observation["ok"], false);
    assert_eq!(observation["error_code"], "TOOL_FAILED");
    assert_eq!(observation["retryable"], false);
    let error = observation["error"].as_str().unwrap();
    assert!(error.starts_with("reason-start"));
    assert!(error.ends_with("reason-end"));
}

#[test]
fn observation_budget_retains_transport_loss_state() {
    for tool in ["browser", "execute_command", "external"] {
        let result = ToolResultPayload::from_value(json!({"ok": true, "data": {
            "truncated": true, "truncation_scope": "transport_safety", "recoverable": false,
            "preview": "value".repeat(8_000), "original_bytes": 2_000_000,
            "recovery_hint": "Full result was not stored."
        }}));
        let observation = result.to_compact_payload_with_token_budget(tool, Some(256));
        assert!(observation_token_count(&observation) <= 256);
        assert_eq!(observation["ok"], true);
        assert_eq!(observation["recoverable"], false);
        assert_eq!(observation["truncation_scope"], "transport_safety");
        assert!(observation["data"]["preview"]
            .as_str()
            .unwrap()
            .contains("value"));
    }
}

#[test]
fn saved_result_reference_survives_tight_budgets_for_success_and_failure() {
    let handle = "tool-result:0123456789abcdef0123456789abcdef";
    for ok in [true, false] {
        for budget in [256, 512, 2000] {
            let result = ToolResultPayload::from_value(json!({
                "ok": ok, "data": {"text": "中🙂\"\\\n".repeat(10_000)},
                "error": if ok {String::new()} else {"error-head ".to_string() + &"x".repeat(40_000) + " error-tail"},
                "meta": {"saved_result": {"handle": handle, "bytes": 200_000, "expires_at": 2_000_000_000}}
            }));
            let observation = result.to_compact_payload_with_token_budget("external", Some(budget));
            assert!(
                observation_token_count(&observation) <= budget,
                "{observation}"
            );
            assert_eq!(observation["result_handle"], handle);
            assert_eq!(observation["ok"], ok);
            assert_eq!(observation["recoverable"], true);
            assert!(observation["recovery_hint"]
                .as_str()
                .unwrap()
                .contains("read_file"));
        }
    }
}

#[test]
fn saved_result_read_keeps_same_handle_and_offset_when_page_is_reduced() {
    let handle = "tool-result:0123456789abcdef0123456789abcdef";
    let result = ToolResultPayload::from_value(json!({"ok": true, "data": {
        "result_read": true, "result_handle": handle, "offset": 4096,
        "content": "中🙂\"\\\n".repeat(1000), "next_offset": 14096, "has_more": true
    }}));
    let observation = result.to_compact_payload_with_token_budget("read_file", Some(256));
    assert!(observation_token_count(&observation) <= 256);
    assert_eq!(observation["result_handle"], handle);
    assert_eq!(observation["offset"], 4096);
    assert!(observation["recovery_hint"]
        .as_str()
        .unwrap()
        .contains("same offset"));
}

#[test]
fn omission_detection_includes_table_and_search_samples() {
    assert!(observation_has_omissions(
        &json!({"data":{"rows_omitted": 5}}),
        0
    ));
    assert!(observation_has_omissions(
        &json!({"data":{"hits":[{"__truncated":true}]}}),
        0
    ));
    assert!(!observation_has_omissions(
        &json!({"data":{"text":"done"}}),
        0
    ));
}

#[test]
fn failed_result_store_never_claims_recovery() {
    let result = ToolResultPayload::from_value(json!({"ok":true,
        "data":{"text":"x".repeat(20_000)}, "meta":{"result_store_failed":true}
    }));
    let observation = result.to_compact_payload_with_token_budget("external", Some(256));
    assert_eq!(observation["recoverable"], false);
    assert!(observation.get("result_handle").is_none());
}

#[test]
fn observation_array_preview_requires_rereading_the_current_page() {
    let result = ToolResultPayload::from_value(json!({"ok": true, "data": {
        "items": (0..40).collect::<Vec<_>>(), "next_cursor": "page-2"
    }}));
    let observation = result.to_compact_payload_with_token_budget("external", Some(512));
    assert_eq!(observation["truncated"], true);
    assert_eq!(observation["next_cursor"], "page-2");
    assert!(observation["recovery_hint"]
        .as_str()
        .unwrap()
        .contains("next_page_skips"));
}
