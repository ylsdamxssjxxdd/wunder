use super::*;
use crate::services::virtual_llm::{capabilities::*, random_virtual_turn};
use serde_json::json;
use wunder_core::virtual_model::VirtualModelOptions;

fn model() -> LlmModelConfig {
    LlmModelConfig {
        max_context: Some(4096),
        max_output: Some(256),
        ..Default::default()
    }
}

#[test]
fn exact_context_boundary_and_output_limit_return_provider_errors() {
    let messages = [json!({"role":"user","content":"abcd"})];
    let mut config = model();
    config.max_context = Some(261);
    assert_eq!(validate_request(&config, &messages, None, 256).unwrap(), 5);
    config.max_context = Some(260);
    let error = validate_request(&config, &messages, None, 256).unwrap_err();
    assert_eq!(
        error.response(),
        json!({"status":400,"error":{"type":"invalid_request_error","code":"context_length_exceeded","param":"messages","message":"This model's maximum context length is 260 tokens, but you requested 261 tokens (5 input + 256 output)."}})
    );
    assert_eq!(
        validate_request(&config, &[], None, 257).unwrap_err().code,
        "max_tokens_exceeded"
    );
    assert_eq!(
        validate_request(&config, &[], None, 0).unwrap_err().code,
        "max_tokens_exceeded"
    );
}

#[test]
fn media_capabilities_and_costs_are_applied_without_reading_media() {
    let mut config = model();
    let image = [
        json!({"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}]}),
    ];
    let audio = [
        json!({"role":"user","content":[{"type":"input_audio","input_audio":{"data":"AA==","format":"wav"}}]}),
    ];
    assert_eq!(
        validate_request(&config, &image, None, 256)
            .unwrap_err()
            .code,
        "unsupported_image"
    );
    assert_eq!(
        validate_request(&config, &audio, None, 256)
            .unwrap_err()
            .code,
        "unsupported_audio"
    );
    config.support_vision = Some(true);
    config.support_hearing = Some(true);
    config.simulation = Some(VirtualModelOptions {
        image_tokens: 300,
        audio_tokens: 600,
        ..Default::default()
    });
    assert_eq!(
        (
            validate_request(&config, &image, None, 256).unwrap(),
            validate_request(&config, &audio, None, 256).unwrap()
        ),
        (304, 604)
    );
    config.max_context = Some(559);
    assert_eq!(
        validate_request(&config, &image, None, 256)
            .unwrap_err()
            .code,
        "context_length_exceeded"
    );
}

#[test]
fn tool_schema_cost_and_disabled_support_are_enforced() {
    let tools =
        [json!({"type":"function","function":{"name":"test_tool","parameters":{"type":"object"}}})];
    let mut config = model();
    let tokens = validate_request(&config, &[], Some(&tools), 256).unwrap();
    assert_eq!(
        tokens,
        serde_json::to_vec(&tools).unwrap().len().div_ceil(4) as u64
    );
    config.max_context = Some(256);
    assert_eq!(
        validate_request(&config, &[], Some(&tools), 256)
            .unwrap_err()
            .code,
        "context_length_exceeded"
    );
    config.simulation = Some(VirtualModelOptions {
        support_tools: false,
        ..Default::default()
    });
    assert_eq!(
        validate_request(&config, &[], Some(&tools), 256)
            .unwrap_err()
            .code,
        "unsupported_tools"
    );
}

#[test]
fn replay_protocol_follows_selected_call_mode_and_does_not_invent_calls() {
    let tools =
        [json!({"type":"function","function":{"name":"test_tool","parameters":{"type":"object"}}})];
    let mut config = model();
    let mut turn = random_virtual_turn(1, Some(1));
    assert!(prepare_turn(turn.clone(), &config, &[], Some(&tools))
        .unwrap()
        .tool_calls
        .is_none());
    turn.content.clear();
    turn.reasoning.clear();
    turn.tool_calls = Some(
        json!([{"id":"call_1","type":"function","function":{"name":"test_tool","arguments":"{}"}}]),
    );
    for (mode, api_mode, native) in [
        ("function_call", "chat", true),
        ("tool_call", "chat", false),
        ("freeform_call", "chat", false),
        ("freeform_call", "responses", true),
    ] {
        config.tool_call_mode = Some(mode.into());
        config.api_mode = Some(api_mode.into());
        let output = prepare_turn(
            turn.clone(),
            &config,
            &[],
            if native { Some(&tools) } else { None },
        )
        .unwrap();
        assert_eq!(
            output.finish_reason.as_deref(),
            Some(if native { "tool_calls" } else { "stop" })
        );
        if native {
            assert_eq!(output.tool_calls, turn.tool_calls);
            assert_eq!(output.content, "");
        } else {
            assert_eq!(output.tool_calls, None);
            assert!(output.content.contains("<tool_call>"));
            assert!(output.content.contains("test_tool"));
        }
    }
    config.tool_call_mode = Some("function_call".into());
    assert_eq!(
        prepare_turn(turn.clone(), &config, &[], None)
            .unwrap_err()
            .code,
        "tool_not_available"
    );
    config.tool_call_mode = Some("tool_call".into());
    config.simulation = Some(VirtualModelOptions {
        support_tools: false,
        ..Default::default()
    });
    assert_eq!(
        prepare_turn(turn, &config, &[], None).unwrap_err().code,
        "unsupported_tools"
    );
}

#[test]
fn output_budget_truncates_utf8_and_never_executes_partial_tool_json() {
    let mut config = model();
    config.max_output = Some(3);
    config.simulation = Some(VirtualModelOptions {
        support_reasoning: false,
        ..Default::default()
    });
    let mut turn = random_virtual_turn(1, Some(1));
    turn.content = "字🙂".repeat(10);
    let trimmed = prepare_turn(turn.clone(), &config, &[], None).unwrap();
    assert_eq!(
        (
            trimmed.content,
            trimmed.reasoning,
            trimmed.finish_reason,
            trimmed.usage.unwrap().total
        ),
        ("字🙂字".into(), String::new(), Some("length".into()), 3)
    );
    let tools = [json!({"type":"function","function":{"name":"test_tool"}})];
    turn.format = "replay".into();
    turn.tool_calls = Some(
        json!([{"id":"call_1","type":"function","function":{"name":"test_tool","arguments":"{}"}}]),
    );
    let truncated = prepare_turn(turn, &config, &[], Some(&tools)).unwrap();
    assert_eq!(
        (truncated.tool_calls, truncated.finish_reason),
        (None, Some("length".into()))
    );
}

#[test]
fn invalid_config_and_reasoning_controls_are_checked() {
    let mut config = model();
    config.max_context = Some(0);
    assert_eq!(
        validate_config(&config).unwrap_err().code,
        "invalid_simulation_config"
    );
    config = model();
    config.thinking_token_budget = Some(2);
    assert_eq!(reasoning_budget(&config, 256), 2);
    config.reasoning_effort = Some("none".into());
    assert_eq!(reasoning_budget(&config, 256), 0);
    config.simulation = Some(VirtualModelOptions {
        image_tokens: 0,
        ..Default::default()
    });
    assert_eq!(
        validate_config(&config).unwrap_err().code,
        "invalid_simulation_config"
    );
}
