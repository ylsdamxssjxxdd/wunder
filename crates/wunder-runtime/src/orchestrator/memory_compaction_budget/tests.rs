use super::*;

fn retained(role: &str, content: String) -> Value {
    let mut message = json!({"role": role, "content": content});
    mark_retained_interaction_message(&mut message);
    message
}

fn summary() -> Value {
    json!({"role": "user", "content": format!(
        "{}\nRetain constraints and completed steps.",
        i18n::t("history.compaction_prefix")
    )})
}

#[test]
fn oversized_current_request_keeps_each_edge_and_frozen_system() {
    for text in ["x", "字", "🙂"] {
        let mut messages = vec![json!({"role": "system", "content": "s".repeat(4_000)})];
        for index in 1..=4 {
            if index == 3 {
                messages.push(summary());
            }
            messages.push(retained(
                "user",
                format!("edge-{index} {}", text.repeat(5_000)),
            ));
            messages.push(retained("assistant", format!("reply-{index}")));
        }
        let mut current =
            json!({"role": "user", "content": format!("current {}", text.repeat(6_000))});
        mark_current_user_message_inflight(&mut current);
        messages.push(current);
        let original_system = messages[0].clone();
        let original_roles = messages
            .iter()
            .map(|message| message["role"].clone())
            .collect::<Vec<_>>();
        let stats = apply_rebuilt_context_guard(&mut messages, 2_000, true);
        assert!(stats.current_user_trimmed);
        assert!(estimate_messages_tokens(&messages) <= 2_000);
        assert_eq!(messages[0], original_system);
        assert_eq!(
            messages
                .iter()
                .map(|message| message["role"].clone())
                .collect::<Vec<_>>(),
            original_roles
        );
        let replay = build_committed_replacement_history_from_rebuilt(&messages);
        let actual = replay
            .iter()
            .map(|message| {
                message["content"]
                    .as_str()
                    .unwrap()
                    .split_whitespace()
                    .next()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let summary_prefix = replay[4]["content"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert_eq!(
            actual,
            vec![
                "edge-1",
                "reply-1",
                "edge-2",
                "reply-2",
                summary_prefix,
                "edge-3",
                "reply-3",
                "edge-4",
                "reply-4"
            ]
        );
        assert!(HistoryManager::is_compaction_summary_item(&replay[4]));
        assert!(is_compaction_inflight_current_user_message(
            messages.last().unwrap()
        ));
    }
}

#[test]
fn manual_compaction_does_not_duplicate_or_reorder_retained_users() {
    let mut messages = vec![
        json!({"role": "system", "content": "system"}),
        retained("user", format!("head {}", "x".repeat(4_000))),
        retained("assistant", "head-reply".to_string()),
        summary(),
        retained("user", format!("tail {}", "x".repeat(4_000))),
        retained("assistant", "tail-reply".to_string()),
    ];
    let original = messages.clone();
    assert_eq!(locate_rebuilt_current_user_index(&messages), None);
    apply_rebuilt_context_guard(&mut messages, 400, true);
    assert!(estimate_messages_tokens(&messages) <= 400);
    assert_eq!(messages.len(), original.len());
    for index in [0, 2, 3, 5] {
        assert_eq!(messages[index], original[index]);
    }
    assert!(messages[1]["content"]
        .as_str()
        .unwrap()
        .starts_with("head "));
    assert!(messages[4]["content"]
        .as_str()
        .unwrap()
        .starts_with("tail "));
}

#[test]
fn small_budget_never_emits_over_budget_excerpts() {
    for limit in [8, 16, 32, 64, 128] {
        let mut messages = vec![
            json!({"role": "system", "content": "system"}),
            retained("user", "字".repeat(100)),
            retained("assistant", "🙂".repeat(100)),
            summary(),
        ];
        apply_rebuilt_context_guard(&mut messages, limit, true);
        assert!(estimate_messages_tokens(&messages) <= limit);
        assert!(messages.iter().all(|message| {
            let text = message["content"].as_str().unwrap();
            !text.starts_with("[上下文") || starts_with_compaction_prefix(text)
        }));
    }
}
