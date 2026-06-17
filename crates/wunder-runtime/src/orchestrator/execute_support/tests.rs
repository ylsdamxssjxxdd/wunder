use super::*;

#[test]
fn build_max_rounds_user_guidance_encourages_continue_or_raise_limit() {
    let answer = build_max_rounds_user_guidance(Some(10));
    assert!(!answer.trim().is_empty());
    assert!(answer.contains("10"));
    assert!(!answer.contains("{max_rounds}"));
}

#[test]
fn recover_from_context_overflow_when_code_matches() {
    let err = OrchestratorError::context_window_exceeded("context length exceeded".to_string());
    assert!(should_recover_from_context_overflow(&err));
}

#[test]
fn recover_from_context_overflow_when_message_matches() {
    let err = OrchestratorError::internal("LLM call failed: context_length_exceeded".to_string());
    assert!(should_recover_from_context_overflow(&err));
}

#[test]
fn recover_from_context_overflow_when_prompt_too_long_phrase_matches() {
    let err = OrchestratorError::internal("model call failed: prompt too long".to_string());
    assert!(should_recover_from_context_overflow(&err));
}

#[test]
fn exception_turn_terminal_status_maps_user_busy_to_rejected() {
    let err = OrchestratorError::user_busy("busy".to_string());
    assert_eq!(turn_terminal_status_for_error(&err), "rejected");
}

#[test]
fn exception_approval_resolution_status_distinguishes_scope() {
    assert_eq!(
        approval_resolution_status_and_scope(ApprovalResponse::ApproveOnce),
        ("approved", "once")
    );
    assert_eq!(
        approval_resolution_status_and_scope(ApprovalResponse::ApproveSession),
        ("approved", "session")
    );
    assert_eq!(
        approval_resolution_status_and_scope(ApprovalResponse::Deny),
        ("denied", "none")
    );
}

#[test]
fn skip_context_overflow_recovery_for_other_errors() {
    let err = OrchestratorError::internal("LLM call failed: invalid api key".to_string());
    assert!(!should_recover_from_context_overflow(&err));
}

#[test]
fn merge_context_window_limit_hint_prefers_smaller_positive_limit() {
    assert_eq!(merge_context_window_limit_hint(None, None), None);
    assert_eq!(
        merge_context_window_limit_hint(Some(8192), None),
        Some(8192)
    );
    assert_eq!(
        merge_context_window_limit_hint(None, Some(4096)),
        Some(4096)
    );
    assert_eq!(
        merge_context_window_limit_hint(Some(8192), Some(4096)),
        Some(4096)
    );
}

#[test]
fn apply_context_window_limit_hint_caps_max_context() {
    let llm_config: LlmModelConfig = serde_json::from_value(json!({
        "provider": "openai",
        "model": "gpt-4.1",
        "max_context": 64000
    }))
    .expect("llm config");
    let hinted = apply_context_window_limit_hint(&llm_config, Some(8192));
    assert_eq!(hinted.max_context, Some(8192));
}

#[test]
fn derive_recovery_context_window_limit_hint_halves_with_attempts() {
    let first = derive_recovery_context_window_limit_hint(64000, 1);
    let second = derive_recovery_context_window_limit_hint(64000, 2);
    let third = derive_recovery_context_window_limit_hint(64000, 3);
    assert!(first > second);
    assert!(second > third);
    assert!(third >= COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
}

#[test]
fn resolve_user_content_for_persist_prefers_trimmed_message_in_context() {
    let messages = vec![
        json!({ "role": "system", "content": "system" }),
        json!({ "role": "user", "content": "trimmed question ...(truncated)" }),
    ];
    let fallback = json!({ "role": "user", "content": "raw giant question" });
    let content = resolve_user_content_for_persist(&messages, &fallback)
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default();
    assert_eq!(content, "trimmed question ...(truncated)");
}

#[test]
fn resolve_user_content_for_persist_falls_back_to_original_message() {
    let messages = vec![json!({ "role": "assistant", "content": "done" })];
    let fallback = json!({ "role": "user", "content": "raw question" });
    let content = resolve_user_content_for_persist(&messages, &fallback)
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_default();
    assert_eq!(content, "raw question");
}

#[test]
fn extract_channel_display_question_override_reads_trimmed_value() {
    let overrides = json!({
        CHANNEL_DISPLAY_QUESTION_OVERRIDE_KEY: "  please compress this image  "
    });
    assert_eq!(
        extract_channel_display_question_override(Some(&overrides)).as_deref(),
        Some("please compress this image")
    );
    assert_eq!(extract_channel_display_question_override(None), None);
}

#[test]
fn tool_failure_signature_prefers_error_text() {
    let result = ToolResultPayload {
        ok: false,
        data: json!({"stderr":"ignored"}),
        error: "command failed".to_string(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: None,
    };
    let signature = build_tool_failure_signature("read_file", &result);
    assert!(signature.contains("read_file"));
    assert!(signature.contains("command failed"));
}

#[test]
fn tool_failure_guard_answer_encourages_continue_from_current_progress() {
    let result = ToolResultPayload {
        ok: false,
        data: json!({}),
        error: String::new(),
        sandbox: false,
        timestamp: Utc::now(),
        meta: None,
    };
    let answer = build_tool_failure_guard_answer("read_file", &result, 3, 5);
    assert!(answer.contains("read_file"));
    assert!(answer.contains("3"));
    assert!(answer.contains("5"));
    assert!(!answer.trim().is_empty());
    assert!(!answer.contains("{tool_name}"));
    assert!(!answer.contains("{repeat_count}"));
    assert!(!answer.contains("{threshold}"));
}

#[test]
fn reroute_reason_allows_soft_reroute_within_budget() {
    let mut routed = HashSet::new();
    let first_fingerprint = "PRECHECK_A:111";
    let second_fingerprint = "PRECHECK_B:222";
    assert!(should_request_tool_failure_reroute(
        "tool_failure_reroute_required",
        0,
        first_fingerprint,
        &routed,
    ));
    routed.insert(first_fingerprint.to_string());
    assert!(should_request_tool_failure_reroute(
        "same_retryable_failure_exhausted",
        1,
        second_fingerprint,
        &routed,
    ));
    assert!(should_request_tool_failure_reroute(
        "same_non_retryable_failure",
        1,
        "PRECHECK_C:333",
        &routed,
    ));
    assert!(!should_request_tool_failure_reroute(
        "same_non_retryable_failure",
        1,
        first_fingerprint,
        &routed,
    ));
    assert!(!should_request_tool_failure_reroute(
        "same_non_retryable_failure",
        2,
        "PRECHECK_D:444",
        &routed,
    ));
    assert!(!should_request_tool_failure_reroute(
        "tool_failure_reroute_required",
        2,
        "PRECHECK_E:555",
        &routed,
    ));
}

#[test]
fn tool_failure_reroute_notice_is_structured_observation() {
    let stop = super::retry_governor::RetryStopDecision {
        reason: "tool_failure_reroute_required",
        fingerprint: "TOOL_TIMEOUT:deadbeef".to_string(),
        repeat_count: 1,
        same_tool_failures: 5,
        threshold: 5,
        retryable: true,
        error_code: "TOOL_TIMEOUT".to_string(),
        detail: "timeout while calling service".to_string(),
    };
    let notice = build_tool_failure_reroute_model_notice(
        "read_file",
        &stop,
        stop.repeat_count,
        stop.threshold,
        stop.detail.as_str(),
    );
    assert_eq!(
        notice.get("type").and_then(Value::as_str),
        Some("tool_failure_reroute_notice")
    );
    assert_eq!(notice.get("ok").and_then(Value::as_bool), Some(false));
    assert_eq!(
        notice.get("tool").and_then(Value::as_str),
        Some("read_file")
    );
    assert_eq!(
        notice.get("fingerprint").and_then(Value::as_str),
        Some("TOOL_TIMEOUT:deadbeef")
    );
    assert_eq!(notice.get("retryable").and_then(Value::as_bool), Some(true));
    let encoded = encode_observation_prefixed_json(&notice);
    assert!(encoded.starts_with(OBSERVATION_PREFIX));
    let payload = encoded.trim_start_matches(OBSERVATION_PREFIX);
    let parsed: Value =
        serde_json::from_str(payload).expect("reroute notice should serialize to json");
    assert_eq!(
            parsed.get("instruction").and_then(Value::as_str),
            Some(
                "Do not repeat the same failing call pattern. Re-plan using current observations and switch execution strategy."
            )
        );
}

#[test]
fn next_step_hint_guides_shell_heredoc_failures() {
    let hint = build_tool_failure_next_step_hint(
        &resolve_tool_name("execute_command"),
        "PRECHECK_SHELL_BAD_HEREDOC",
        "bash: line 1: EOF: No such file or directory",
    );
    assert!(hint.contains("write_file"));
}

#[test]
fn normalize_workspace_changed_path_strips_workspace_public_prefix() {
    let path =
        normalize_workspace_changed_path("/workspaces/alice__c__2/docs/readme.md", "alice__c__2")
            .expect("path");
    assert_eq!(path, "docs/readme.md");
}

#[test]
fn normalize_workspace_changed_path_ignores_windows_absolute_path() {
    let path = normalize_workspace_changed_path("C:/repo/demo.txt", "alice__c__2");
    assert!(path.is_none());
}

#[test]
fn extract_workspace_changed_paths_merges_meta_data_and_args() {
    let meta = json!({
        "changed_paths": [
            "/workspaces/alice__c__2/docs/a.md",
            "docs/b.md"
        ]
    });
    let data = json!({
        "files": [
            { "path": "docs/c.md" },
            { "to_path": "docs/d.md" }
        ]
    });
    let args = json!({
        "destination": "docs/archive",
        "paths": ["docs/e.md"]
    });
    let paths = extract_workspace_changed_paths(Some(&meta), &data, &args, "alice__c__2");
    let expected = HashSet::from([
        "docs/a.md".to_string(),
        "docs/b.md".to_string(),
        "docs/c.md".to_string(),
        "docs/d.md".to_string(),
        "docs/e.md".to_string(),
        "docs/archive".to_string(),
    ]);
    assert_eq!(paths.len(), expected.len());
    let actual = paths.into_iter().collect::<HashSet<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn extract_workspace_changed_paths_reads_generated_resource_aliases() {
    let meta = json!({
        "public_path": "/workspaces/alice__c__2/images/output.png",
        "workspace_relative_path": "images/output.png",
        "outputPath": "reports/final.pdf"
    });
    let data = json!({
        "saved_path": "audio/result.mp3",
        "filePath": "video/result.mp4"
    });
    let args = json!({
        "targetPath": "images/output.png"
    });
    let paths = extract_workspace_changed_paths(Some(&meta), &data, &args, "alice__c__2");
    let expected = HashSet::from([
        "images/output.png".to_string(),
        "reports/final.pdf".to_string(),
        "audio/result.mp3".to_string(),
        "video/result.mp4".to_string(),
    ]);
    assert_eq!(paths.len(), expected.len());
    let actual = paths.into_iter().collect::<HashSet<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn extract_container_id_from_workspace_id_recovers_suffix() {
    assert_eq!(
        extract_container_id_from_workspace_id("alice__c__7"),
        crate::storage::normalize_workspace_container_id(7)
    );
    assert_eq!(
        extract_container_id_from_workspace_id("alice__agent__demo"),
        crate::storage::DEFAULT_SANDBOX_CONTAINER_ID
    );
    assert_eq!(
        extract_container_id_from_workspace_id("alice"),
        crate::storage::USER_PRIVATE_CONTAINER_ID
    );
}

#[test]
fn build_planned_tool_calls_filters_disallowed_name() {
    let allowed = HashSet::from([resolve_tool_name("read_file")]);
    let calls = vec![
        ToolCall {
            id: None,
            name: "read_file".to_string(),
            function_name: None,
            arguments: json!({ "path": "Cargo.toml" }),
        },
        ToolCall {
            id: None,
            name: "2026-03-03".to_string(),
            function_name: None,
            arguments: json!({ "timestamp": "..." }),
        },
    ];
    let result = build_planned_tool_calls(calls, &allowed);
    assert_eq!(result.planned.len(), 1);
    assert_eq!(result.planned[0].name, resolve_tool_name("read_file"));
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].name, "2026-03-03");
}

#[test]
fn build_planned_tool_calls_accepts_allowed_alias() {
    let allowed = HashSet::from([resolve_tool_name("final_response")]);
    let calls = vec![ToolCall {
        id: None,
        name: "final_response".to_string(),
        function_name: None,
        arguments: json!({ "content": "ok" }),
    }];
    let result = build_planned_tool_calls(calls, &allowed);
    assert_eq!(result.planned.len(), 1);
    assert_eq!(result.planned[0].name, resolve_tool_name("final_response"));
    assert!(result.rejected.is_empty());
}

#[test]
fn build_planned_tool_calls_reports_unknown_tool_call() {
    let allowed = HashSet::from([resolve_tool_name("programmatic_tool_call")]);
    let calls = vec![ToolCall {
        id: None,
        name: "programmatic_tool_calls".to_string(),
        function_name: None,
        arguments: json!({ "raw": "{\"calls\":[name\":\"programmatic_tool_call\"}" }),
    }];

    let result = build_planned_tool_calls(calls, &allowed);

    assert!(result.planned.is_empty());
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].name, "programmatic_tool_calls");
    assert_eq!(result.rejected[0].reason, "tool_not_allowed_or_unknown");
    assert!(result.rejected[0].arguments_preview.contains("calls"));
}

#[test]
fn invalid_tool_call_notice_instructs_repair_or_final_response() {
    let allowed = HashSet::from([
        resolve_tool_name("final_response"),
        resolve_tool_name("programmatic_tool_call"),
    ]);
    let rejected = vec![RejectedToolCall {
        name: "programmatic_tool_calls".to_string(),
        resolved_name: "programmatic_tool_calls".to_string(),
        reason: "tool_not_allowed_or_unknown",
        arguments_preview: "{\"raw\":\"bad\"}".to_string(),
    }];

    let notice = build_invalid_tool_call_model_notice(&rejected, &allowed);

    assert_eq!(notice.get("type"), Some(&json!("invalid_tool_call_notice")));
    let instruction = notice
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(instruction.contains("final_response"));
    assert!(instruction.contains("valid JSON arguments"));
}

#[test]
fn empty_final_answer_notice_instructs_continue_not_empty_stop() {
    let notice = build_empty_final_answer_model_notice(1, 3, false, true, false, true);

    assert_eq!(
        notice.get("type"),
        Some(&json!("empty_final_answer_notice"))
    );
    assert_eq!(notice.get("had_reasoning"), Some(&json!(true)));
    let instruction = notice
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(instruction.contains("Continue the task now"));
    assert!(instruction.contains("final_response"));
}

#[test]
fn empty_final_answer_notice_without_tools_requests_direct_answer() {
    let notice = build_empty_final_answer_model_notice(2, 3, true, false, true, false);

    let instruction = notice
        .get("instruction")
        .and_then(Value::as_str)
        .unwrap_or("");
    assert!(instruction.contains("Respond directly"));
    assert!(!instruction.contains("final_response"));
}

#[test]
fn assistant_history_snapshot_drops_terminal_tool_calls() {
    let allowed = HashSet::from([
        resolve_tool_name("final_response"),
        resolve_tool_name("read_file"),
    ]);
    let payload = json!([
        {
            "id": "call_final",
            "type": "function",
            "function": {
                "name": "final_response",
                "arguments": "{\"content\":\"ok\"}"
            }
        },
        {
            "id": "call_read",
            "type": "function",
            "function": {
                "name": "read_file",
                "arguments": "{\"path\":\"a.txt\"}"
            }
        }
    ]);
    let snapshot = build_assistant_history_snapshot(Some(&payload), &allowed);
    let persisted = snapshot
        .persisted_tool_calls
        .expect("persisted tool calls should remain");
    let calls = persisted.as_array().expect("array");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["function"]["name"].as_str(), Some("read_file"));
}

#[test]
fn assistant_history_snapshot_omits_pure_terminal_payload() {
    let allowed = HashSet::from([resolve_tool_name("final_response")]);
    let payload = json!({
        "id": "call_final",
        "type": "function",
        "function": {
            "name": "final_response",
            "arguments": "{\"content\":\"ok\"}"
        }
    });
    let snapshot = build_assistant_history_snapshot(Some(&payload), &allowed);
    assert!(snapshot.tool_calls.is_none());
    assert!(snapshot.persisted_tool_calls.is_none());
}

#[test]
fn assistant_history_snapshot_keeps_single_tool_call_as_array() {
    let allowed = HashSet::from([resolve_tool_name("skill_call")]);
    let payload = json!([{
        "id": "call_skill",
        "type": "function",
        "function": {
            "name": "skill_call",
            "arguments": "{\"name\":\"generic_skill\"}"
        }
    }]);
    let snapshot = build_assistant_history_snapshot(Some(&payload), &allowed);
    let persisted = snapshot
        .persisted_tool_calls
        .expect("persisted tool calls should remain");
    let calls = persisted.as_array().expect("tool calls should stay array");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["type"], json!("function"));
}

#[test]
fn assistant_history_snapshot_preserves_original_tool_arguments_order() {
    let allowed = HashSet::from([resolve_tool_name("web_fetch")]);
    let payload = json!([{
        "id": "call_fetch",
        "type": "function",
        "function": {
            "name": "web_fetch",
            "arguments": "{\"url\":\"https://example.invalid/item\",\"extract_mode\":\"markdown\"}"
        }
    }]);
    let snapshot = build_assistant_history_snapshot(Some(&payload), &allowed);
    let persisted = snapshot
        .persisted_tool_calls
        .expect("persisted tool calls should remain");
    assert_eq!(persisted, payload);
}

#[test]
fn model_context_tool_calls_keep_terminal_calls_for_exact_replay() {
    let allowed = HashSet::from([resolve_tool_name("final_response")]);
    let payload = json!([{
        "id": "call_final",
        "type": "function",
        "function": {
            "name": "final_response",
            "arguments": "{\"content\":\"ok\"}"
        }
    }]);

    let model_context =
        build_model_context_tool_calls_snapshot(Some(&payload), &allowed).expect("tool calls");
    let history = build_assistant_history_snapshot(Some(&payload), &allowed);

    assert_eq!(model_context, payload);
    assert!(history.tool_calls.is_none());
}

#[test]
fn cancelled_generation_marker_applies_after_prompt_tail() {
    assert!(should_append_cancelled_generation_context_marker(&[
        json!({
            "role": "tool",
            "tool_call_id": "call_1",
            "content": "{}",
        })
    ]));
    assert!(should_append_cancelled_generation_context_marker(&[
        json!({
            "role": "user",
            "content": "question",
        })
    ]));
    assert!(!should_append_cancelled_generation_context_marker(&[
        json!({
            "role": "assistant",
            "content": "partial",
        })
    ]));
    assert!(!should_append_cancelled_generation_context_marker(&[]));
}

#[test]
fn resolve_db_query_tool_budget_uses_extended_only_for_full_scan_intent() {
    assert_eq!(
        resolve_db_query_tool_budget("please export all records"),
        EXTENDED_DB_QUERY_TOOL_BUDGET_PER_TURN
    );
    assert_eq!(
        resolve_db_query_tool_budget("show latest 100 records summary"),
        DEFAULT_DB_QUERY_TOOL_BUDGET_PER_TURN
    );
}

#[test]
fn build_tool_budget_guard_model_notice_contains_usage_snapshot() {
    let block = ToolBudgetBlock {
        kind: ToolBudgetBlockKind::DbQuery,
        limit: 2000,
        attempted: 2001,
        tool: "extra_mcp@db_query".to_string(),
    };
    let limits = ToolBudgetLimits {
        total: 10_000,
        db_query: 2000,
        memory_recall: 2000,
    };
    let usage = ToolBudgetUsage {
        total: 1200,
        db_query: 2000,
        memory_recall: 11,
    };
    let notice = build_tool_budget_guard_model_notice(&block, &limits, &usage);
    assert!(notice.contains("soft guard reached"));
    assert!(notice.contains("Attempted 2001 > limit 2000"));
    assert!(notice.contains("db_query=2000/2000"));
    assert!(notice.contains("extra_mcp@db_query"));
}

#[test]
fn build_round_usage_payload_keeps_context_occupancy_distinct_from_consumed_total() {
    let payload = build_round_usage_payload(
        &TokenUsage {
            input: 120,
            output: 80,
            total: 200,
        },
        960,
        RoundInfo::new(3, 2),
    );
    assert_eq!(
        payload.get("total_tokens").and_then(Value::as_u64),
        Some(200)
    );
    assert_eq!(
        payload
            .get("request_consumed_tokens")
            .and_then(Value::as_u64),
        Some(200)
    );
    assert_eq!(
        payload
            .get("context_occupancy_tokens")
            .and_then(Value::as_i64),
        Some(960)
    );
    assert_eq!(payload.get("user_round").and_then(Value::as_i64), Some(3));
    assert_eq!(payload.get("model_round").and_then(Value::as_i64), Some(2));
}

#[test]
fn approval_kind_for_tool_routes_exec_control_and_patch_tools() {
    assert_eq!(
        approval_kind_for_tool(&resolve_tool_name("execute_command")),
        ApprovalRequestKind::Exec
    );
    assert_eq!(
        approval_kind_for_tool(&resolve_tool_name("ptc")),
        ApprovalRequestKind::Exec
    );
    assert_eq!(
        approval_kind_for_tool(&resolve_tool_name("desktop_controller")),
        ApprovalRequestKind::Control
    );
    assert_eq!(
        approval_kind_for_tool(&resolve_tool_name("desktop_monitor")),
        ApprovalRequestKind::Control
    );
    assert_eq!(
        approval_kind_for_tool(&resolve_tool_name("read_file")),
        ApprovalRequestKind::Patch
    );
}

#[test]
fn approval_summary_for_tool_prefers_command_text_and_path_hints() {
    let execute_command = resolve_tool_name("execute_command");
    let write_file = resolve_tool_name("write_file");
    let desktop_controller = resolve_tool_name("desktop_controller");
    let exec_summary = approval_summary_for_tool(
        &execute_command,
        &json!({ "content": "  cargo test  " }),
        ApprovalRequestKind::Exec,
    );
    let patch_summary = approval_summary_for_tool(
        &write_file,
        &json!({ "path": "  docs/notes.md  " }),
        ApprovalRequestKind::Patch,
    );
    let control_summary = approval_summary_for_tool(
        &desktop_controller,
        &json!({ "action": "click", "description": "  confirm button  " }),
        ApprovalRequestKind::Control,
    );

    assert_eq!(exec_summary, format!("{execute_command}: cargo test"));
    assert_eq!(patch_summary, format!("{write_file}: docs/notes.md"));
    assert_eq!(
        control_summary,
        format!("{desktop_controller}: action=click confirm button")
    );
}

#[test]
fn approval_summary_for_tool_falls_back_to_tool_name_when_details_missing() {
    let execute_command = resolve_tool_name("execute_command");
    let write_file = resolve_tool_name("write_file");
    let desktop_controller = resolve_tool_name("desktop_controller");
    assert_eq!(
        approval_summary_for_tool(
            &execute_command,
            &json!({ "content": "   " }),
            ApprovalRequestKind::Exec,
        ),
        execute_command
    );
    assert_eq!(
        approval_summary_for_tool(
            &write_file,
            &json!({ "path": "   " }),
            ApprovalRequestKind::Patch,
        ),
        write_file
    );
    assert_eq!(
        approval_summary_for_tool(
            &desktop_controller,
            &json!({ "wait_ms": 1200 }),
            ApprovalRequestKind::Control,
        ),
        format!("{desktop_controller}: wait_ms=1200")
    );
}

#[test]
fn local_full_event_logs_only_enable_for_embedded_modes() {
    assert!(should_enable_local_full_event_logs("desktop"));
    assert!(should_enable_local_full_event_logs("cli"));
    assert!(!should_enable_local_full_event_logs("server"));
    assert!(!should_enable_local_full_event_logs("api"));
}

#[test]
fn resolve_round_context_occupancy_prefers_latest_model_usage_total() {
    let first = resolve_usage_context_occupancy_tokens(&TokenUsage {
        input: 11675,
        output: 4104,
        total: 15779,
    });
    let second = resolve_usage_context_occupancy_tokens(&TokenUsage {
        input: 7509,
        output: 1,
        total: 7510,
    });
    assert_eq!(first, Some(15779));
    assert_eq!(second, Some(7510));
    assert_eq!(resolve_round_context_occupancy_tokens(second, 3241), 7510);
    assert_eq!(resolve_round_context_occupancy_tokens(None, 3241), 3241);
    assert_eq!(resolve_round_context_occupancy_tokens(Some(-4), -11), 0);
}

#[test]
fn is_memory_recall_tool_call_matches_memory_manager_recall_action() {
    let tool_name = resolve_tool_name("memory_manager");
    let args = json!({ "action": "query", "query": "generic query" });
    assert!(is_memory_recall_tool_call(&tool_name, &args, &tool_name,));
    let add_args = json!({ "action": "add", "content": "generic content" });
    assert!(!is_memory_recall_tool_call(
        &tool_name, &add_args, &tool_name
    ));
}

#[test]
fn resolve_cached_memory_recall_result_respects_revision() {
    let tool_name = resolve_tool_name("memory_manager");
    let planned = PlannedToolCall {
        call: ToolCall {
            id: Some("call_1".to_string()),
            name: tool_name.clone(),
            function_name: None,
            arguments: json!({ "action": "recall", "query": "generic query" }),
        },
        name: tool_name.clone(),
        function_name: tool_name.clone(),
    };
    let cache_key = normalize_memory_recall_query(Some("generic query")).expect("query key");
    let mut cache = HashMap::new();
    cache.insert(
        cache_key,
        CachedRecallResult {
            revision: 3,
            result: CachedToolResult {
                ok: true,
                data: json!({ "action": "recall", "count": 1 }),
                error: String::new(),
                sandbox: false,
                meta: None,
            },
        },
    );

    assert!(resolve_cached_memory_recall_result(&planned, &tool_name, &cache, 3).is_some());
    assert!(resolve_cached_memory_recall_result(&planned, &tool_name, &cache, 4).is_none());
}

#[test]
fn uses_native_tool_api_supports_freeform_only_on_responses_api() {
    let base_config = || LlmModelConfig {
        enable: None,
        provider: None,
        api_mode: None,
        base_url: None,
        api_key: None,
        model: None,
        temperature: None,
        timeout_s: None,
        max_rounds: None,
        max_context: None,
        max_output: None,
        thinking_token_budget: None,
        support_vision: None,
        support_hearing: None,
        stream: None,
        stream_include_usage: None,
        history_compaction_ratio: None,
        tool_call_mode: None,
        reasoning_effort: None,
        model_type: None,
        stop: None,
        mock_if_unconfigured: None,
        ..Default::default()
    };
    let mut function_call_config = base_config();
    function_call_config.tool_call_mode = Some("function_call".to_string());

    let mut freeform_responses_config = base_config();
    freeform_responses_config.provider = Some("openai".to_string());
    freeform_responses_config.model = Some("gpt-5.2".to_string());
    freeform_responses_config.tool_call_mode = Some("freeform_call".to_string());

    let mut freeform_chat_config = base_config();
    freeform_chat_config.provider = Some("openai_compatible".to_string());
    freeform_chat_config.model = Some("gpt-5.2".to_string());
    freeform_chat_config.tool_call_mode = Some("freeform_call".to_string());

    assert!(uses_native_tool_api(
        ToolCallMode::FunctionCall,
        &function_call_config,
    ));
    assert!(!uses_native_tool_api(
        ToolCallMode::ToolCall,
        &function_call_config
    ));
    assert!(uses_native_tool_api(
        ToolCallMode::FreeformCall,
        &freeform_responses_config,
    ));
    assert!(!uses_native_tool_api(
        ToolCallMode::FreeformCall,
        &freeform_chat_config,
    ));
}

#[test]
fn args_with_approved_flag_preserves_object_payloads_and_wraps_scalars() {
    let object = args_with_approved_flag(&json!({ "path": "docs/readme.md" }));
    assert_eq!(object.get("approved").and_then(Value::as_bool), Some(true));
    assert_eq!(
        object.get("path").and_then(Value::as_str),
        Some("docs/readme.md")
    );

    let wrapped = args_with_approved_flag(&json!("raw"));
    assert_eq!(wrapped.get("approved").and_then(Value::as_bool), Some(true));
    assert_eq!(wrapped.get("raw").and_then(Value::as_str), Some("raw"));
}

#[test]
fn tool_call_mode_key_uses_config_values() {
    assert_eq!(
        tool_call_mode_key(ToolCallMode::FunctionCall),
        "function_call"
    );
    assert_eq!(tool_call_mode_key(ToolCallMode::ToolCall), "tool_call");
    assert_eq!(
        tool_call_mode_key(ToolCallMode::FreeformCall),
        "freeform_call"
    );
}

#[test]
fn infers_tool_call_mode_from_frozen_system_prompt() {
    assert_eq!(
        infer_tool_call_mode_from_frozen_system_prompt(
            "stable system\n\n<tools></tools>\n<tool_call>{}</tool_call>"
        ),
        Some(ToolCallMode::ToolCall)
    );
    assert_eq!(
        infer_tool_call_mode_from_frozen_system_prompt(
            "stable system\n\n[Tools Protocol]\nfreeform\n<tools></tools>\n<tool_call></tool_call>"
        ),
        Some(ToolCallMode::FreeformCall)
    );
    assert_eq!(
        infer_tool_call_mode_from_frozen_system_prompt("stable system without tools"),
        Some(ToolCallMode::FunctionCall)
    );
    assert_eq!(infer_tool_call_mode_from_frozen_system_prompt(" "), None);
}
