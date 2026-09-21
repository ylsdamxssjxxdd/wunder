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
async fn local_reduction_persists_model_context_without_rewriting_chat_history() {
    use crate::state::{AppState, AppStateInitOptions};

    let root = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.storage.backend = "sqlite".into();
    config.storage.db_path = root.path().join("state.db").to_string_lossy().into_owned();
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
        0,
        None,
    );
    let messages = history(false);
    let tokens = estimate_messages_tokens(&messages);
    let orchestrator = &state.kernel.orchestrator;
    orchestrator
        .workspace
        .append_chat(
            "user",
            &json!({"session_id": "session", "role": "user", "content": "request"}),
        )
        .unwrap();
    assert!(orchestrator.workspace.flush_writes_async().await);
    let chat_before = orchestrator
        .workspace
        .load_history("user", "session", 0)
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
    assert_eq!(
        orchestrator
            .workspace
            .load_model_context_entries("user", "session", 0)
            .unwrap(),
        model_context_entries_from_messages(&result.messages)
    );
    assert_eq!(result.messages[0], messages[0]);
    assert_eq!(
        orchestrator
            .workspace
            .load_history("user", "session", 0)
            .unwrap(),
        chat_before
    );
    assert!(plan_microcompaction(
        &result.messages,
        estimate_messages_tokens(&result.messages),
        tokens
    )
    .is_none());
}
