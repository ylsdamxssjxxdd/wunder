use super::*;
use serde_json::json;

fn llm_config(value: Value) -> LlmModelConfig {
    serde_json::from_value(value).expect("parse llm model config")
}

#[test]
fn test_should_compact_by_context_threshold() {
    let (by_history, should_compact) = should_compact_by_context(90, 100, Some(80));
    assert!(by_history);
    assert!(should_compact);
}

#[test]
fn test_should_compact_by_context_overflow_only() {
    let (by_history, should_compact) = should_compact_by_context(120, 100, None);
    assert!(!by_history);
    assert!(should_compact);
}

#[test]
fn test_should_compact_by_context_no_compaction() {
    let (by_history, should_compact) = should_compact_by_context(50, 100, Some(80));
    assert!(!by_history);
    assert!(!should_compact);
}

#[test]
fn test_resolve_compaction_limit_uses_configured_limit() {
    let cfg = llm_config(json!({
        "max_context": 8000,
        "max_output": 512
    }));
    let limit = resolve_compaction_limit(&cfg, 32000, false).unwrap_or_default();
    assert!(limit > 0);
}

#[test]
fn test_resolve_compaction_limit_skips_without_force_when_unknown() {
    let cfg = llm_config(json!({}));
    assert!(resolve_compaction_limit(&cfg, 32000, false).is_none());
}

#[test]
fn test_resolve_compaction_limit_uses_force_fallback_when_unknown() {
    let cfg = llm_config(json!({}));
    let limit = resolve_compaction_limit(&cfg, 48000, true).unwrap_or_default();
    assert!(limit >= COMPACTION_SUMMARY_MESSAGE_MAX_TOKENS);
    assert!(limit <= COMPACTION_FORCE_FALLBACK_LIMIT);
}

#[test]
fn test_resolve_compaction_limit_force_uses_adaptive_limit_with_configured_cap() {
    let cfg = llm_config(json!({
        "max_context": 64000,
        "max_output": 1024
    }));
    let configured = resolve_compaction_limit(&cfg, 80_000, false).unwrap_or_default();
    let forced = resolve_compaction_limit(&cfg, 80_000, true).unwrap_or_default();
    assert!(configured > 0);
    assert!(forced > 0);
    assert!(forced <= configured);
}

#[test]
fn test_resolve_force_compaction_limit_handles_small_configured_limit() {
    let forced = resolve_force_compaction_limit(4_000, 2_048);
    assert_eq!(forced, 2_048);
}

#[test]
fn test_resolve_message_budget_uses_limit_only() {
    assert_eq!(resolve_message_budget(4096), 4096);
    assert_eq!(resolve_message_budget(0), 1);
}

#[test]
fn test_build_compaction_summary_config_disables_reasoning_in_payload() {
    let cfg = llm_config(json!({
        "provider": "openai",
        "api_mode": "responses",
        "model": "gpt-5-mini",
        "max_output": 2048,
        "reasoning_effort": "high"
    }));
    let summary_config = build_compaction_summary_config(&cfg);
    let client = build_llm_client(&summary_config, reqwest::Client::new());
    let payload = client.build_request_payload(
        &[ChatMessage {
            role: "user".to_string(),
            content: json!("compress this"),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        false,
    );

    assert_eq!(summary_config.reasoning_effort.as_deref(), Some("none"));
    assert_eq!(summary_config.max_rounds, Some(1));
    assert_eq!(summary_config.max_output, Some(2048));
    assert_eq!(payload["reasoning"]["effort"], "none");
}

#[test]
fn test_prepare_compaction_summary_messages_compacts_observation_payload() {
    let preview = "X".repeat(12_000);
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "read_file",
                "ok": true,
                "data": {
                    "preview": preview,
                    "original_chars": 12000,
                }
            }))
        }),
    ];
    let prepared = prepare_compaction_summary_messages(messages, 2048);
    assert_eq!(prepared.len(), 2);
    assert_eq!(
        prepared[0].get("role").and_then(Value::as_str),
        Some("system")
    );
    let observation = prepared[1]
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(observation.contains("Tool observation (read_file): success"));
    assert!(!observation.contains(OBSERVATION_PREFIX));
    assert!(approx_token_count(observation) <= COMPACTION_SUMMARY_OBSERVATION_MAX_TOKENS);
}

#[test]
fn test_prepare_compaction_summary_messages_merges_system_and_skips_tool_only_assistant() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "system", "content": "artifact index" }),
        json!({
            "role": "assistant",
            "content": "",
            "tool_calls": [
                { "function": { "name": "search_content" } },
                { "function": { "name": "read_file" } }
            ]
        }),
    ];
    let prepared = prepare_compaction_summary_messages(messages, 2048);
    assert_eq!(prepared.len(), 1);
    assert_eq!(
        prepared[0].get("content").and_then(Value::as_str),
        Some("system prompt\n\nartifact index")
    );
}

#[test]
fn test_build_compaction_summary_input_keeps_instruction_last() {
    let system_message = json!({ "role": "system", "content": "system prompt" });
    let source_messages = vec![
        json!({ "role": "user", "content": "older request" }),
        json!({ "role": "assistant", "content": "attempted tool" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "ptc",
                "ok": false,
                "error_code": "TOOL_EXEC_FAILED",
                "error": "script failed",
            })),
        }),
    ];
    let compaction_message = json!({
        "role": "user",
        "content": "CONTEXT CHECKPOINT COMPACTION: summarize only",
    });

    let summary_input =
        build_compaction_summary_input(Some(&system_message), &source_messages, compaction_message);

    assert_eq!(summary_input.len(), 5);
    assert_eq!(
        summary_input
            .last()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
        Some("CONTEXT CHECKPOINT COMPACTION: summarize only")
    );
    assert!(summary_input[..summary_input.len() - 1]
        .iter()
        .any(
            |message| extract_guard_content_text(message.get("content").unwrap_or(&Value::Null))
                .contains("script failed")
        ));
}

#[test]
fn test_prepare_compaction_summary_messages_preserves_final_instruction_after_trimming() {
    let instruction = "CONTEXT CHECKPOINT COMPACTION: summarize only";
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "ptc",
                "ok": false,
                "error_code": "TOOL_EXEC_FAILED",
                "error": "failure detail ".repeat(1200),
            })),
        }),
        json!({ "role": "user", "content": instruction }),
    ];

    let prepared = prepare_compaction_summary_messages(messages, 256);

    assert_eq!(
        prepared
            .last()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
        Some(instruction)
    );
    assert!(prepared.iter().any(|message| message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("Tool observation (ptc): failed")));
}

#[test]
fn test_current_turn_replay_uses_tool_success_continuation_after_successful_tool() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "draw item" }),
        json!({ "role": "assistant", "content": "I will draw it" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "ptc",
                "ok": true,
                "data": { "path": "/workspaces/u/item.png" },
            })),
        }),
    ];
    let progress = classify_current_turn_progress(&messages, 1);

    let replay = build_current_turn_replay_message(
        messages.get(1),
        "draw item",
        2048,
        &progress,
        CompactionResumeAction::Continue,
        "resume_action: continue\n## What remains to be done\nRun the next step.",
    );

    assert_eq!(progress.state, CurrentTurnProgressState::ToolSucceeded);
    assert_eq!(replay.mode, CurrentUserReplayMode::ToolSuccessContinuation);
    let replay_message = replay.message.expect("continuation message");
    assert_eq!(
        replay_message.get("role").and_then(Value::as_str),
        Some("user")
    );
    assert!(is_compaction_inflight_current_user_message(&replay_message));
    let content = replay_message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(content.contains("does not by itself mean the user-facing task is complete"));
    assert!(content.contains("Latest retained tool observation"));
}

#[test]
fn test_current_turn_replay_for_successful_write_file_requires_next_step() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "generate chart" }),
        json!({ "role": "assistant", "content": "I will write the script" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "write_file",
                "ok": true,
                "data": {
                    "path": "chart.py",
                    "bytes": 1200,
                    "existed": false,
                },
            })),
        }),
    ];
    let progress = classify_current_turn_progress(&messages, 1);
    let replay = build_current_turn_replay_message(
        messages.get(1),
        "generate chart",
        2048,
        &progress,
        CompactionResumeAction::Continue,
        "resume_action: continue\n## What remains to be done\nRun and validate chart.py.",
    );
    let content = replay
        .message
        .as_ref()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    assert_eq!(replay.mode, CurrentUserReplayMode::ToolSuccessContinuation);
    assert!(content.contains("Created file chart.py"));
    assert!(content.contains("running or validating `chart.py`"));
    assert!(content.contains("before finalizing"));
}

#[test]
fn test_current_turn_replay_uses_final_continuation_when_summary_has_no_todo() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "draw armor nailong" }),
        json!({ "role": "assistant", "content": "I will draw it" }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "ptc",
                "ok": true,
                "data": {
                    "path": "/workspaces/admin__c__1/ptc_temp/nailong_armor.py",
                    "stderr": "/workspaces/admin__c__1/ptc_temp/nailong_armor.py:153: saved to /workspaces/admin__c__1/nailong_armor.png",
                    "stdout": "Done",
                },
            })),
        }),
    ];
    let progress = classify_current_turn_progress(&messages, 1);
    let replay = build_current_turn_replay_message(
        messages.get(1),
        "draw armor nailong",
        2048,
        &progress,
        detect_compaction_resume_action(
            "## 当前进展\n已完成绘制。\n## 剩余待办\n无明确待办。\nresume_action: final",
        ),
        "## 当前进展\n已完成绘制。\n## 剩余待办\n无明确待办。\nresume_action: final",
    );
    let content = replay
        .message
        .as_ref()
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    assert_eq!(replay.mode, CurrentUserReplayMode::FinalContinuation);
    assert!(content.contains("Provide the final response now"));
    assert!(content.contains("resume_action: final"));
    assert!(content.contains("Do not call tools, regenerate artifacts, rewrite files"));
    assert!(!content.contains("/workspaces/admin__c__1/nailong_armor.png"));
}

#[test]
fn test_compaction_resume_action_detects_no_remaining_work_without_path_parsing() {
    let summary = format!(
        "{}\n## 当前进展\n已完成绘制。\n## 剩余待办\n无明确待办。",
        i18n::t("history.compaction_prefix")
    );

    assert_eq!(
        detect_compaction_resume_action(&summary),
        CompactionResumeAction::Final
    );
}

#[test]
fn test_current_turn_replay_uses_user_continuation_after_tool_failure() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "repair generated file" }),
        json!({ "role": "assistant", "content": "I will inspect it" }),
        json!( {
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "apply_patch",
                "ok": false,
                "error_code": "PATCH_FORMAT_INVALID",
                "error": "bad patch",
            })),
        }),
        json!({ "role": "assistant", "content": "I found the bad line" }),
    ];
    let progress = classify_current_turn_progress(&messages, 1);

    let replay = build_current_turn_replay_message(
        messages.get(1),
        "repair generated file",
        2048,
        &progress,
        CompactionResumeAction::Retry,
        "resume_action: retry",
    );

    assert_eq!(progress.state, CurrentTurnProgressState::ToolFailed);
    assert_eq!(replay.mode, CurrentUserReplayMode::RepairContinuation);
    let replay_message = replay.message.expect("continuation message");
    assert_eq!(
        replay_message.get("role").and_then(Value::as_str),
        Some("user")
    );
    assert!(is_compaction_inflight_current_user_message(&replay_message));
    assert!(replay_message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("latest tool output reports failure"));
}

#[test]
fn test_current_turn_replay_keeps_original_before_progress() {
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "analyze input" }),
    ];
    let progress = classify_current_turn_progress(&messages, 1);

    let replay = build_current_turn_replay_message(
        messages.get(1),
        "analyze input",
        2048,
        &progress,
        CompactionResumeAction::Unknown,
        "",
    );

    assert_eq!(progress.state, CurrentTurnProgressState::Pending);
    assert_eq!(replay.mode, CurrentUserReplayMode::Original);
    assert_eq!(
        replay
            .message
            .as_ref()
            .and_then(|message| message.get("content"))
            .and_then(Value::as_str),
        Some("analyze input")
    );
}

#[test]
fn test_resolve_projected_request_tokens_uses_observed_context_only() {
    assert_eq!(resolve_projected_request_tokens(2000), 2000);
    assert_eq!(resolve_projected_request_tokens(5000), 5000);
}

#[test]
fn test_merge_compaction_system_message_keeps_existing_system_frozen() {
    let merged = merge_compaction_system_message(
        Some(json!({ "role": "system", "content": "system prompt" })),
        "Artifact index",
    )
    .expect("merged system message");
    assert_eq!(merged.get("role").and_then(Value::as_str), Some("system"));
    let content = merged
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert_eq!(content, "system prompt");
}

#[test]
fn test_merge_compaction_system_message_does_not_create_artifact_system() {
    assert!(merge_compaction_system_message(None, "Artifact index").is_none());
}

#[test]
fn test_trim_message_to_fit_tokens_reduces_large_content() {
    let message = json!({
        "role": "user",
        "content": "A".repeat(20_000),
    });
    let before = estimate_message_tokens(&message);
    let target = (before / 8).max(32);
    let trimmed = trim_message_to_fit_tokens(&message, target).expect("trimmed message");
    let after = estimate_message_tokens(&trimmed);
    assert!(after <= target);
    assert!(after < before);
}

#[test]
fn test_apply_rebuilt_context_guard_trims_current_user_message() {
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "summary line" }),
        json!({ "role": "user", "content": "B".repeat(24_000) }),
    ];
    let limit = 800;
    let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
    assert!(stats.applied);
    assert!(stats.current_user_trimmed || stats.fallback_trim_applied);
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_preserves_current_question_before_trimming_it() {
    let current_question =
            "Please analyze team size and salaries by department, draw a chart, and summarize in 3 points.";
    let summary = format!(
        "{}\n{}",
        i18n::t("history.compaction_prefix"),
        "S".repeat(48_000)
    );
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": summary }),
        json!({ "role": "user", "content": current_question }),
    ];
    let limit = 256;
    let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
    assert!(stats.applied);
    assert!(stats.summary_removed || stats.summary_trimmed);
    assert!(!stats.current_user_trimmed);
    assert_eq!(
        messages
            .last()
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str),
        Some(current_question)
    );
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_handles_array_content() {
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "summary line" }),
        json!({
            "role": "user",
            "content": [
                { "type": "text", "text": "C".repeat(12_000) },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAAA" } }
            ]
        }),
    ];
    let limit = 900;
    let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
    assert!(stats.applied);
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_trims_single_user_message() {
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "D".repeat(36_000) }),
    ];
    let limit = 900;
    let stats = apply_rebuilt_context_guard(&mut messages, limit, false);
    assert!(stats.applied);
    assert!(stats.current_user_trimmed || stats.fallback_trim_applied);
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_preserves_summary_in_summary_first_mode() {
    let summary = format!(
        "{}\nCompressed earlier context.\n- User: prior request\n- Assistant: prior answer",
        i18n::t("history.compaction_prefix")
    );
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "edge 1 ".repeat(600) }),
        json!({ "role": "assistant", "content": "edge 2 ".repeat(600) }),
        json!({ "role": "user", "content": summary }),
        json!({ "role": "user", "content": "current question ".repeat(900) }),
    ];
    let limit = 700;
    let stats = apply_rebuilt_context_guard(&mut messages, limit, true);
    assert!(stats.applied);
    assert!(
        messages
            .iter()
            .any(|message| HistoryManager::is_compaction_summary_item(message)),
        "summary-first compaction should keep the committed summary in the rebuilt request"
    );
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_summary_first_keeps_trimmed_retained_interaction() {
    let summary = format!(
        "{}\n{}",
        i18n::t("history.compaction_prefix"),
        "Compressed earlier context. ".repeat(120)
    );
    let mut retained_user = json!({ "role": "user", "content": "round-1 user marker" });
    mark_retained_interaction_message(&mut retained_user);
    let mut retained_assistant = json!({
        "role": "assistant",
        "content": "round-1 assistant marker ".repeat(120)
    });
    mark_retained_interaction_message(&mut retained_assistant);
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt ".repeat(220) }),
        retained_user,
        retained_assistant,
        json!({ "role": "user", "content": summary }),
    ];
    let limit = estimate_message_tokens(&messages[0])
        + estimate_message_tokens(&messages[3])
        + estimate_message_tokens(&messages[1])
        + 24;

    let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

    assert!(stats.applied);
    assert!(
        messages
            .iter()
            .any(|message| HistoryManager::is_compaction_summary_item(message)),
        "summary-first compaction should keep the summary"
    );
    assert!(
            messages
                .iter()
                .any(is_retained_interaction_message),
            "summary-first compaction should keep a trimmed retained interaction when budget still allows it"
        );
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_summary_first_trims_summary_to_keep_retained_budget() {
    let summary = format!(
        "{}\n{}",
        i18n::t("history.compaction_prefix"),
        "summary detail ".repeat(220)
    );
    let mut retained_user = json!({ "role": "user", "content": "round-1 user marker" });
    mark_retained_interaction_message(&mut retained_user);
    let mut retained_assistant = json!({
        "role": "assistant",
        "content": "round-1 assistant marker ".repeat(90)
    });
    mark_retained_interaction_message(&mut retained_assistant);
    let current_user = json!({ "role": "user", "content": "current user marker" });
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt ".repeat(260) }),
        retained_user,
        retained_assistant,
        json!({ "role": "user", "content": summary.clone() }),
        current_user,
    ];
    let limit = estimate_messages_tokens(&messages) - estimate_message_tokens(&messages[2])
        + COMPACTION_MIN_RETAINED_INTERACTION_TOKENS
        - 16;

    let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

    assert!(stats.applied);
    assert!(
        stats.summary_trimmed,
        "summary-first guard should trim summary before sacrificing the entire retained window"
    );
    assert!(
            messages
                .iter()
                .any(is_retained_interaction_message),
            "summary-first guard should keep some retained interaction content after trimming the summary"
        );
    assert!(
        messages
            .iter()
            .any(|message| HistoryManager::is_compaction_summary_item(message)),
        "summary-first guard should still keep the summary"
    );
    assert!(estimate_messages_tokens(&messages) <= limit);
}

#[test]
fn test_apply_rebuilt_context_guard_never_keeps_partial_compaction_prefix() {
    let summary = format!(
        "{}\n{}",
        i18n::t("history.compaction_prefix"),
        "summary detail ".repeat(220)
    );
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt ".repeat(260) }),
        json!({ "role": "user", "content": summary }),
    ];
    let limit = estimate_message_tokens(&messages[0]) + 8;

    let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

    assert!(stats.applied);
    assert!(estimate_messages_tokens(&messages) <= limit);
    assert!(
        messages.iter().all(|message| {
            let content = message.get("content").and_then(Value::as_str).unwrap_or("");
            content.is_empty()
                || !content.starts_with("[上下文")
                || starts_with_compaction_prefix(content)
        }),
        "guard should not keep a broken partial compaction prefix"
    );
}

#[test]
fn test_trim_compaction_summary_message_to_fit_tokens_preserves_prefix_shape() {
    let summary = json!({
        "role": "user",
        "content": format!(
            "{}\n{}",
            i18n::t("history.compaction_prefix"),
            "summary detail ".repeat(240)
        )
    });

    let trimmed = trim_compaction_summary_message_to_fit_tokens(&summary, 24)
        .expect("trimmed compaction summary");
    let content = trimmed
        .get("content")
        .and_then(Value::as_str)
        .expect("summary content");

    assert!(
        starts_with_compaction_prefix(content),
        "trimmed compaction summary should keep a valid compaction prefix: {content}"
    );
    assert!(
        !is_invalid_compaction_summary(content),
        "trimmed compaction summary should remain a valid summary: {content}"
    );
    assert!(estimate_message_tokens(&trimmed) <= 24);
}

#[test]
fn test_apply_rebuilt_context_guard_summary_first_keeps_valid_summary_and_retained_window() {
    let summary = format!(
        "{}\n{}",
        i18n::t("history.compaction_prefix"),
        "summary detail ".repeat(260)
    );
    let mut retained_user = json!({ "role": "user", "content": "retained user anchor" });
    mark_retained_interaction_message(&mut retained_user);
    let mut retained_assistant = json!({
        "role": "assistant",
        "content": "retained assistant anchor ".repeat(100)
    });
    mark_retained_interaction_message(&mut retained_assistant);
    let mut current_user = json!({ "role": "user", "content": "current follow-up request" });
    mark_current_user_message_inflight(&mut current_user);
    let mut messages = vec![
        json!({ "role": "system", "content": "system prompt ".repeat(120) }),
        retained_user,
        retained_assistant,
        json!({ "role": "user", "content": summary }),
        current_user,
    ];
    let limit = estimate_messages_tokens(&messages).saturating_sub(1);

    let stats = apply_rebuilt_context_guard(&mut messages, limit, true);

    assert!(stats.applied);
    assert!(estimate_messages_tokens(&messages) <= limit);
    assert!(
        messages.iter().any(is_retained_interaction_message),
        "summary-first guard should keep retained interaction content in the rebuilt request"
    );
    let summary_message = messages
        .iter()
        .find(|message| HistoryManager::is_compaction_summary_item(message))
        .expect("summary message");
    let summary_text = summary_message
        .get("content")
        .and_then(Value::as_str)
        .expect("summary text");
    assert!(
        starts_with_compaction_prefix(summary_text),
        "summary-first guard should keep a valid compaction summary prefix: {summary_text}"
    );
    assert!(
        !is_invalid_compaction_summary(summary_text),
        "summary-first guard should keep a valid compaction summary body: {summary_text}"
    );
}

#[test]
fn test_locate_compaction_summary_message_index_prefers_prefixed_summary() {
    let summary = format!("{}\nsummary", i18n::t("history.compaction_prefix"));
    let messages = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "user", "content": "older user message" }),
        json!({ "role": "user", "content": summary }),
        json!({ "role": "user", "content": "current question" }),
    ];
    assert_eq!(locate_compaction_summary_message_index(&messages), Some(2));
}

#[test]
fn test_collect_retained_interaction_messages_for_compaction_keeps_first_and_recent_blocks() {
    let messages = vec![
        json!({ "role": "user", "content": "round-1 user" }),
        json!({ "role": "assistant", "content": "round-1 assistant" }),
        json!({ "role": "user", "content": "round-2 user" }),
        json!({ "role": "assistant", "content": "round-2 assistant" }),
        json!({ "role": "user", "content": "round-3 user" }),
        json!({ "role": "assistant", "content": "round-3 assistant" }),
        json!({ "role": "user", "content": "round-4 user" }),
        json!({ "role": "assistant", "content": "round-4 assistant" }),
        json!({ "role": "user", "content": "round-5 user" }),
        json!({ "role": "assistant", "content": "round-5 assistant" }),
    ];

    let retained = collect_retained_interaction_messages_for_compaction(
        &messages,
        COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
        COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
        COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
    );
    let contents = retained
        .iter()
        .map(|message| message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        contents,
        vec![
            "round-1 user".to_string(),
            "round-1 assistant".to_string(),
            "round-2 user".to_string(),
            "round-2 assistant".to_string(),
            "round-4 user".to_string(),
            "round-4 assistant".to_string(),
            "round-5 user".to_string(),
            "round-5 assistant".to_string(),
        ]
    );
}

#[test]
fn test_collect_retained_interaction_messages_for_compaction_keeps_oldest_and_latest_task_blocks() {
    let messages = vec![
        json!({ "role": "user", "content": "[SWARM_CONTEXT]\\nolder task" }),
        json!({ "role": "assistant", "content": "older answer" }),
        json!({ "role": "user", "content": "current question" }),
        json!({ "role": "assistant", "content": "searching current task" }),
        json!({
            "role": "user",
            "content": format!(
                "{OBSERVATION_PREFIX}{}",
                json!({ "tool": "search_content", "ok": true, "summary": "11 hits" })
            )
        }),
        json!({ "role": "assistant", "content": "reading current task files" }),
    ];

    let retained = collect_retained_interaction_messages_for_compaction(
        &messages,
        COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
        COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
        COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
    );
    let roles = retained
        .iter()
        .map(|message| message["role"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    let contents = retained
        .iter()
        .map(|message| message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        roles,
        vec![
            "user".to_string(),
            "assistant".to_string(),
            "user".to_string(),
            "assistant".to_string(),
            "user".to_string(),
            "assistant".to_string(),
        ]
    );
    assert_eq!(
        contents,
        vec![
            "[SWARM_CONTEXT]\\nolder task".to_string(),
            "older answer".to_string(),
            "current question".to_string(),
            "searching current task".to_string(),
            "Tool observation (search_content): success\n11 hits".to_string(),
            "reading current task files".to_string(),
        ]
    );
}

#[test]
fn test_split_messages_into_interaction_turns_merges_same_side_content_and_observations() {
    let messages = vec![
        json!({ "role": "user", "content": "round-1 user" }),
        json!({
            "role": "assistant",
            "content": "Inspecting\nAssistant issued tool call(s): read_file",
            "tool_calls": [{ "function": { "name": "read_file" } }]
        }),
        json!({
            "role": "assistant",
            "content": "Reading file details",
        }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "read_file",
                "ok": true,
                "summary": "Loaded /tmp/demo.txt"
            }))
        }),
        json!({ "role": "assistant", "content": "round-1 answer" }),
    ];

    let turns = split_messages_into_interaction_turns(&messages);
    let roles = turns
        .iter()
        .map(|turn| turn.message["role"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    let contents = turns
        .iter()
        .map(|turn| turn.message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        roles,
        vec![
            "user".to_string(),
            "assistant".to_string(),
            "user".to_string(),
            "assistant".to_string(),
        ]
    );
    assert!(contents
        .iter()
        .all(|content| !content.contains("Assistant issued tool call(s):")));
    assert_eq!(
        contents,
        vec![
            "round-1 user".to_string(),
            "Inspecting\n\nReading file details".to_string(),
            "Tool observation (read_file): success\nLoaded /tmp/demo.txt".to_string(),
            "round-1 answer".to_string(),
        ]
    );
}

#[test]
fn test_split_messages_into_interaction_turns_respects_boundary_index() {
    let messages = vec![
        json!({ "role": "assistant", "content": "previous final answer" }),
        json!({ "role": "assistant", "content": "new round kickoff" }),
    ];

    let turns = split_messages_into_interaction_turns_with_boundary(&messages, Some(1));
    let contents = turns
        .iter()
        .map(|turn| turn.message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        contents,
        vec![
            "previous final answer".to_string(),
            "new round kickoff".to_string(),
        ]
    );
}

#[test]
fn test_collect_retained_interaction_segments_for_compaction_avoids_overlap_duplication() {
    let messages = vec![
        json!({ "role": "user", "content": "round-1 user" }),
        json!({ "role": "assistant", "content": "round-1 assistant" }),
        json!({ "role": "user", "content": "round-2 user" }),
        json!({ "role": "assistant", "content": "round-2 assistant" }),
        json!({ "role": "user", "content": "round-3 user" }),
        json!({ "role": "assistant", "content": "round-3 assistant" }),
    ];

    let (head, tail) = collect_retained_interaction_segments_for_compaction(
        &messages,
        COMPACTION_RETAINED_INTERACTION_BLOCK_COUNT_PER_SIDE,
        COMPACTION_RETAINED_HEAD_INTERACTION_TOKENS,
        COMPACTION_RETAINED_TAIL_INTERACTION_TOKENS,
    );

    let head_contents = head
        .iter()
        .map(|message| message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();
    let tail_contents = tail
        .iter()
        .map(|message| message["content"].as_str().unwrap_or("").to_string())
        .collect::<Vec<_>>();

    assert_eq!(
        head_contents,
        vec![
            "round-1 user".to_string(),
            "round-1 assistant".to_string(),
            "round-2 user".to_string(),
            "round-2 assistant".to_string(),
        ]
    );
    assert_eq!(
        tail_contents,
        vec!["round-3 user".to_string(), "round-3 assistant".to_string(),]
    );
}

#[test]
fn test_collect_retained_interaction_messages_for_compaction_trims_large_block_to_budget() {
    let large_assistant = "assistant detail ".repeat(8_000);
    let messages = vec![
        json!({ "role": "user", "content": "round-1 user" }),
        json!({ "role": "assistant", "content": large_assistant }),
        json!({ "role": "user", "content": "round-2 user" }),
        json!({ "role": "assistant", "content": "round-2 assistant" }),
    ];

    let retained = collect_retained_interaction_messages_for_compaction(&messages, 2, 256, 256);
    assert_eq!(retained.len(), 4);
    assert_eq!(retained[0]["content"], json!("round-1 user"));
    assert_eq!(retained[2]["content"], json!("round-2 user"));
    assert_eq!(retained[3]["content"], json!("round-2 assistant"));
    assert!(estimate_message_tokens(&retained[1]) <= 256);
    assert_ne!(retained[1]["content"], json!(large_assistant));
}

#[test]
fn test_build_compaction_message_debug_entries_marks_summary_and_current_user() {
    let mut current_user = json!({ "role": "user", "content": "current question" });
    mark_current_user_message_inflight(&mut current_user);
    let messages = vec![
        json!({
            "role": "user",
            "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
        }),
        current_user,
    ];

    let debug = build_compaction_message_debug_entries(&messages);

    assert_eq!(debug.len(), 2);
    assert_eq!(debug[0]["is_summary"], json!(true));
    assert_eq!(debug[0]["is_current_user"], json!(false));
    assert_eq!(debug[1]["is_summary"], json!(false));
    assert_eq!(debug[1]["is_current_user"], json!(true));
}

#[test]
fn test_build_committable_compaction_summary_rejects_placeholder_fragment() {
    assert!(build_committable_compaction_summary("...(truncated)", "").is_none());
    assert!(build_committable_compaction_summary("ok", "").is_none());
}

#[test]
fn test_build_committable_compaction_summary_clamps_to_char_limit() {
    let summary = format!(
        "Project status:\n{}\nNext steps:\n{}",
        "A".repeat(11_000),
        "B".repeat(11_000)
    );
    let memory_block = format!(
        "{}\n- User prefers concise diffs",
        i18n::t("memory.block_prefix")
    );

    let (committed, injected) =
        build_committable_compaction_summary(&summary, &memory_block).expect("summary");
    assert!(injected);
    assert!(committed.starts_with(&i18n::t("history.compaction_prefix")));
    assert!(committed.chars().count() <= COMPACTION_SUMMARY_MAX_CHARS);
    assert!(!is_invalid_compaction_summary(&committed));
}

#[test]
fn test_build_committed_replacement_history_from_rebuilt_strips_system_and_inflight_user() {
    let mut inflight_user = json!({ "role": "user", "content": "current question" });
    mark_current_user_message_inflight(&mut inflight_user);
    let rebuilt = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({ "role": "assistant", "content": "tail answer" }),
        json!({
            "role": "user",
            "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
        }),
        inflight_user,
    ];

    let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

    assert_eq!(committed.len(), 2);
    assert_eq!(committed[0]["role"], json!("assistant"));
    assert_eq!(committed[1]["role"], json!("user"));
    assert!(committed
        .iter()
        .all(|item| !is_compaction_inflight_current_user_message(item)));
}

#[test]
fn test_build_committed_replacement_history_from_rebuilt_strips_internal_compaction_artifacts() {
    let mut inflight_user = json!({ "role": "user", "content": "current question" });
    mark_current_user_message_inflight(&mut inflight_user);
    let rebuilt = vec![
        json!({ "role": "system", "content": "system prompt" }),
        json!({
            "role": "assistant",
            "content": "I will inspect the file.\nAssistant issued tool call(s): read_file",
            "tool_calls": [{
                "function": { "name": "read_file" }
            }],
            "tool_call_id": "call_1"
        }),
        json!({
            "role": "user",
            "content": format!("{OBSERVATION_PREFIX}{}", json!({
                "tool": "read_file",
                "ok": true,
                "data": {
                    "results_jsonl": "{\"path\":\"/tmp/demo.txt\"}"
                }
            })),
        }),
        json!({
            "role": "assistant",
            "content": "Assistant issued tool call(s): read_file",
            "tool_calls": [{
                "function": { "name": "read_file" }
            }]
        }),
        json!({
            "role": "user",
            "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
        }),
        inflight_user,
    ];

    let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

    assert_eq!(
        committed,
        vec![
            json!({ "role": "assistant", "content": "I will inspect the file." }),
            json!({
                "role": "user",
                "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
            }),
        ]
    );
}

#[test]
fn test_build_committed_replacement_history_from_rebuilt_strips_retained_markers() {
    let mut retained_user = json!({ "role": "user", "content": "head question" });
    mark_retained_interaction_message(&mut retained_user);
    let mut retained_assistant = json!({ "role": "assistant", "content": "head answer" });
    mark_retained_interaction_message(&mut retained_assistant);
    let rebuilt = vec![
        json!({ "role": "system", "content": "system prompt" }),
        retained_user,
        retained_assistant,
        json!({
            "role": "user",
            "content": format!("{}\nsummary", i18n::t("history.compaction_prefix"))
        }),
    ];

    let committed = build_committed_replacement_history_from_rebuilt(&rebuilt);

    assert_eq!(committed.len(), 3);
    assert!(committed.iter().all(|item| {
        item.get("meta")
            .and_then(Value::as_object)
            .and_then(|meta| meta.get(COMPACTION_RETAINED_INTERACTION_META_KEY))
            .is_none()
    }));
}

#[test]
fn test_merge_compaction_summary_with_fresh_memory_appends_block() {
    let summary = format!(
        "{}\nKeep this summary",
        i18n::t("history.compaction_prefix")
    );
    let memory_block = format!(
        "{}\n- Remember user prefers markdown",
        i18n::t("memory.block_prefix")
    );
    let (merged, injected) = merge_compaction_summary_with_fresh_memory(&summary, &memory_block);
    assert!(injected);
    assert!(merged.contains("Keep this summary"));
    assert!(merged.contains("Remember user prefers markdown"));
}

#[test]
fn test_merge_compaction_summary_with_fresh_memory_replaces_old_block() {
    let summary = format!(
        "{}\nKeep this summary\n\n{}\n- stale memory",
        i18n::t("history.compaction_prefix"),
        i18n::t("memory.block_prefix"),
    );
    let memory_block = format!("{}\n- fresh memory", i18n::t("memory.block_prefix"));
    let (merged, injected) = merge_compaction_summary_with_fresh_memory(&summary, &memory_block);
    assert!(injected);
    assert!(merged.contains("Keep this summary"));
    assert!(merged.contains("fresh memory"));
    assert!(!merged.contains("stale memory"));
}

#[test]
fn test_build_compaction_instruction_appends_current_request_constraints() {
    let instruction = build_compaction_instruction(
        "base prompt",
        "",
        "Summarize UNCLOS article 121 for disputed reefs",
        "",
    );
    assert!(instruction.contains("base prompt"));
    assert!(instruction.contains("[Current user request / 当前用户问题]"));
    assert!(instruction.contains("Summarize UNCLOS article 121 for disputed reefs"));
    assert!(instruction.contains("task unspecified"));
}

#[test]
fn test_build_compaction_instruction_deduplicates_request_candidates() {
    let instruction =
        build_compaction_instruction("base prompt", "", "same request", "same request");
    assert_eq!(instruction.matches("same request").count(), 1);
}
