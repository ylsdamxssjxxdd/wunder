use super::*;

fn history(freeform: bool) -> Vec<Value> {
    let mut messages = vec![
        json!({"role": "system", "content": "frozen"}),
        json!({"role": "user", "content": "request"}),
    ];
    for index in 0..10 {
        let id = format!("call_{index}");
        let observation = json!({"tool": "read_file", "ok": true, "data": {"path": "file", "content": "字🙂".repeat(2500)}}).to_string();
        if freeform {
            messages.push(json!({"role": "assistant", "content": "read"}));
            messages.push(
                json!({"role": "user", "content": format!("{OBSERVATION_PREFIX}{observation}")}),
            );
        } else {
            messages.push(json!({"role": "assistant", "content": "", "tool_calls": [{"id": id, "type": "function", "function": {"name": "read_file", "arguments": "{}"}}]}));
            messages.push(json!({"role": "tool", "tool_call_id": id, "content": observation}));
        }
    }
    messages
}

#[test]
fn local_reduction_preserves_recent_groups_system_and_tool_protocol() {
    for freeform in [false, true] {
        let messages = history(freeform);
        let tokens = estimate_messages_tokens(&messages);
        let plan =
            plan_microcompaction(&messages, tokens, tokens).expect("sufficient local savings");
        let mut reduced = messages.clone();
        for (index, content) in plan.replacements {
            reduced[index]["content"] = json!(content);
            assert_eq!(reduced[index]["role"], messages[index]["role"]);
            assert_eq!(
                reduced[index]["tool_call_id"],
                messages[index]["tool_call_id"]
            );
            assert!(compact_old_observation(reduced[index]["content"].as_str().unwrap()).is_none());
        }
        assert_eq!(&reduced[..2], &messages[..2]);
        assert_eq!(
            &reduced[messages.len() - 10..],
            &messages[messages.len() - 10..]
        );
        assert_eq!(
            estimate_messages_tokens(&reduced),
            tokens - plan.tokens_saved
        );
        assert!(plan.tokens_saved >= tokens / 5);
        if !freeform {
            assert_eq!(ContextManager.normalize_messages(reduced.clone()), reduced);
        }
    }
}

#[test]
fn unique_errors_skills_media_and_pending_pagination_are_protected() {
    let source = json!({"tool": "read_file", "ok": true, "data": {"content": "x".repeat(5000)}});
    for patch in [
        json!({"ok": false}),
        json!({"tool": "execute_command"}),
        json!({"tool": "skill_call"}),
        json!({"tool": "external"}),
        json!({"has_more": true}),
        json!({"next_cursor": "cursor"}),
        json!({"data": {"type": "image", "data": "x".repeat(5000)}}),
    ] {
        let mut payload = source.clone();
        payload
            .as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(compact_old_observation(&payload.to_string()), None);
    }
}

#[test]
fn insufficient_savings_fall_back_without_mutating_history() {
    let messages = history(false);
    let before = messages.clone();
    assert!(plan_microcompaction(&messages, 1_000_000, 2000).is_none());
    assert!(plan_microcompaction(&messages, 0, 2000).is_none());
    assert!(plan_microcompaction(&messages[..10], 30_000, 20_000).is_none());
    assert_eq!(messages, before);
}

#[test]
fn parallel_results_count_as_one_recent_group() {
    let mut messages = history(false);
    for _ in 0..20 {
        messages.push(json!({"role": "tool", "tool_call_id": "parallel", "content": "{}"}));
    }
    let tokens = estimate_messages_tokens(&messages);
    // One large latest parallel batch cannot make its earlier siblings eligible.
    let plan = plan_microcompaction(&messages, tokens, tokens).unwrap();
    assert!(plan.replacements.iter().all(|(index, _)| *index < 12));
}

#[cfg(feature = "sqlite-storage")]
#[tokio::test]
async fn local_reduction_persists_canonical_checkpoint_without_rewriting_chat_history() {
    use crate::state::{AppState, AppStateInitOptions};

    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.data_root = root.path().join("data").to_string_lossy().into_owned();
    config.workspace.root = root.path().join("workspace").to_string_lossy().into_owned();
    let store = ConfigStore::new(root.path().join("config.yaml"));
    store
        .update(|current| *current = config.clone())
        .await
        .unwrap();
    let state =
        AppState::new_with_options(store, config, AppStateInitOptions::cli_default()).unwrap();
    let emitter = EventEmitter::new(
        "session".into(),
        "user".into(),
        None,
        None,
        state.monitor.clone(),
        false,
        None,
    );
    let messages = history(false);
    let tokens = estimate_messages_tokens(&messages);
    let orchestrator = &state.kernel.orchestrator;
    let accepted = orchestrator
        .committer
        .accept_turn("user", "session", &json!({"content":"request"}))
        .await
        .unwrap();
    let first_turn = uuid::Uuid::parse_str(accepted["turn_id"].as_str().unwrap()).unwrap();
    orchestrator
        .committer
        .freeze_session("user", "session", "frozen", "")
        .await
        .unwrap();
    orchestrator
        .committer
        .begin_turn(
            "user",
            "session",
            accepted["turn_id"].as_str().unwrap(),
            "fixture",
        )
        .await
        .unwrap();
    orchestrator
        .try_append_chat(
            "user",
            "session",
            "user",
            Some(&json!("request")),
            None,
            None,
            None,
            None,
            None,
            None,
            RoundInfo::user_only_thread(1, first_turn),
        )
        .await
        .unwrap();

    let result = orchestrator
        .try_microcompact_messages(
            &messages,
            "user",
            "session",
            &emitter,
            RoundInfo::new(1, 1),
            tokens,
            tokens,
        )
        .await
        .unwrap();
    assert!(result.model_context_replaced);
    assert_eq!(result.compaction_id, None);

    // The canonical checkpoint is the model-context boundary.  Reading the
    // model context after compaction expands its retained snapshot directly;
    // the checkpoint itself remains in the rollout and is not exposed as a
    // second ordinary chat row.
    let chat_after = orchestrator
        .workspace
        .load_history("user", "session", 0)
        .unwrap();
    // The snapshot excludes the in-flight current user message (index 1 here);
    // its chat row is appended after compaction in the real flow and replays it.
    let expected_snapshot_source: Vec<Value> = result
        .messages
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, message)| message.clone())
        .collect();
    let replacement = model_context_entries_from_messages(&expected_snapshot_source);
    assert_eq!(
        replacement,
        chat_after
    );

    // Mirror the production order: the current user chat row lands after the
    // checkpoint, then derivation replays snapshot + that row.
    let accepted = orchestrator
        .committer
        .accept_turn("user", "session", &json!({"content":"follow-up request"}))
        .await
        .unwrap();
    let second_turn = uuid::Uuid::parse_str(accepted["turn_id"].as_str().unwrap()).unwrap();
    orchestrator
        .committer
        .begin_turn(
            "user",
            "session",
            accepted["turn_id"].as_str().unwrap(),
            "fixture",
        )
        .await
        .unwrap();
    orchestrator
        .try_append_chat(
            "user",
            "session",
            "user",
            Some(&json!("follow-up request")),
            None,
            None,
            None,
            None,
            None,
            None,
            RoundInfo::user_only_thread(2, second_turn),
        )
        .await
        .unwrap();

    let derived = crate::history::HistoryManager
        .load_history_messages_async(
            orchestrator.workspace.clone(),
            "user".to_string(),
            "session".to_string(),
            0,
        )
        .await;
    assert_eq!(derived.len(), replacement.len() + 1);
    assert_eq!(
        derived.last(),
        Some(&json!({"role": "user", "content": "follow-up request"}))
    );
    let derived_text = derived
        .iter()
        .filter_map(|message| message.get("content").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(derived_text.contains("[older output omitted; repeat the read if needed]"));
    // Recent tool groups stay intact; only the older observations are elided.
    let needle = "字🙂".repeat(2500);
    assert!(derived_text.matches(needle.as_str()).count() < 10);

    assert_eq!(result.messages[0], messages[0]);
    assert!(plan_microcompaction(
        &result.messages,
        estimate_messages_tokens(&result.messages),
        tokens
    )
    .is_none());
}
