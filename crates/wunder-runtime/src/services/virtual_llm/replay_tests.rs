use super::*;

#[test]
fn strict_selection_never_repeats_tools_or_skips_round_gaps() {
    let parsed = parse_virtual_log(
        r#"{"event":"llm_output","data":{"user_round":1,"model_round":1,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"tool_a","arguments":"{}"}}]}}
{"event":"llm_output","data":{"user_round":1,"model_round":3,"content":"A"}}
{"event":"llm_output","data":{"user_round":3,"model_round":1,"content":"B"}}"#,
        "log_1", "replay",
    ).unwrap();
    assert!(replay_selection::select(&parsed, 1, 1)
        .unwrap()
        .tool_calls
        .is_some());
    assert_eq!(
        replay_selection::select(&parsed, 1, 3).unwrap().content,
        "A"
    );
    for (user, model) in [(1, 2), (1, 4), (2, 1), (4, 1)] {
        assert!(replay_selection::select(&parsed, user, model).is_err());
    }
}

#[test]
fn legacy_rounds_preserve_file_order_and_ambiguous_rounds_are_rejected() {
    let parsed = parse_virtual_log(
        r#"{"event":"llm_output","data":{"content":"A"}}
{"event":"llm_output","data":{"content":"B"}}"#,
        "log_1",
        "replay",
    )
    .unwrap();
    assert_eq!(
        replay_selection::select(&parsed, 1, 2).unwrap().content,
        "B"
    );
    assert!(replay_selection::select(&parsed, 1, 3).is_err());
    for second_round in [None, Some(1)] {
        let text = format!(
            "{}\n{}",
            json!({"event":"llm_output","data":{"model_round":1,"content":"A"}}),
            json!({"event":"llm_output","data":{"model_round":second_round,"content":"B"}})
        );
        assert!(parse_virtual_log(&text, "log_1", "replay").is_err());
    }
}

#[test]
fn simple_dialogue_retains_tool_only_turns_and_user_round_gaps() {
    let parsed = parse_virtual_log(
        r#"{"role":"user","content":"A"}
{"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":"tool_a","arguments":"{}"}}]}
{"role":"tool","content":"B"}
{"role":"assistant","content":"C"}
{"role":"user","content":"D"}
{"role":"user","content":"E"}
{"role":"assistant","content":"F"}"#,
        "log_1", "replay",
    ).unwrap();
    assert_eq!(
        parsed
            .turns
            .iter()
            .map(|turn| (
                turn.source_round,
                turn.source_model_round,
                turn.content.as_str(),
                turn.tool_calls.is_some()
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, Some(1), "", true),
            (1, Some(2), "C", false),
            (3, Some(1), "F", false)
        ]
    );
    assert!(replay_selection::select(&parsed, 2, 1).is_err());
}

#[tokio::test]
async fn explicit_missing_or_disabled_logs_fail_instead_of_random_fallback() {
    let model = LlmModelConfig {
        provider: Some(VIRTUAL_REPLAY_PROVIDER.into()),
        model: Some("log_1".into()),
        ..Default::default()
    };
    let mut config = Config::default();
    assert!(
        load_turn_for_round(config.clone(), &model, Some(1), Some(1))
            .await
            .is_err()
    );
    config
        .llm
        .virtual_replay
        .enabled_logs
        .push(VirtualLlmLogConfig {
            id: "log_1".into(),
            name: "replay".into(),
            file: "replay.jsonl".into(),
            enabled: false,
            format: WUNDER_REPLAY_FORMAT.into(),
            user_rounds: 1,
            size_bytes: 0,
            uploaded_at: String::new(),
        });
    assert!(load_turn_for_round(config, &model, Some(1), Some(1))
        .await
        .is_err());
}

#[tokio::test]
async fn stream_emits_reasoning_before_content_and_propagates_cancellation() {
    let mut turn = random_virtual_turn(1, Some(1));
    turn.content = "A".into();
    turn.reasoning = "B".into();
    let mut deltas = Vec::new();
    emit_virtual_deltas(
        &turn,
        true,
        timing::VirtualModelSpeed::Fast,
        |content, reasoning| {
            deltas.push((content, reasoning));
            std::future::ready(Ok(()))
        },
    )
    .await
    .unwrap();
    assert_eq!(
        deltas,
        vec![(String::new(), "B".into()), ("A".into(), String::new())]
    );
    let mut count = 0;
    assert!(
        emit_virtual_deltas(&turn, true, timing::VirtualModelSpeed::Fast, |_, _| {
            count += 1;
            std::future::ready(Err(anyhow!("cancelled")))
        })
        .await
        .is_err()
    );
    assert_eq!(count, 1);
}

#[tokio::test]
async fn simulation_preserves_utf8_and_nonstream_generation_still_takes_time() {
    let mut turn = random_virtual_turn(1, Some(1));
    assert!(!turn.reasoning.trim().is_empty());
    turn.reasoning = "字🙂".repeat(5);
    turn.content = "abcd".repeat(5);
    let mut content = String::new();
    let mut reasoning = String::new();
    emit_virtual_deltas(
        &turn,
        true,
        timing::VirtualModelSpeed::Fast,
        |text, thought| {
            assert!(content.is_empty() || thought.is_empty());
            content.push_str(&text);
            reasoning.push_str(&thought);
            std::future::ready(Ok(()))
        },
    )
    .await
    .unwrap();
    assert_eq!(
        (content, reasoning),
        (turn.content.clone(), turn.reasoning.clone())
    );
    let usage = estimate_virtual_usage(&[], &turn);
    assert_eq!(
        (usage.output, usage.reasoning, usage.total),
        (5, Some(9), 14)
    );
    let started = std::time::Instant::now();
    let mut callbacks = 0;
    emit_virtual_deltas(&turn, false, timing::VirtualModelSpeed::Fast, |_, _| {
        callbacks += 1;
        std::future::ready(Ok(()))
    })
    .await
    .unwrap();
    assert!(started.elapsed() >= timing::VirtualModelSpeed::Fast.generation_duration(13));
    assert_eq!(callbacks, 0);
}
