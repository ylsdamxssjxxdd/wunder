//! Normalize recorded calls with the same parser used by the real execution chain.
use super::{
    tool_calls::{collect_tool_calls_from_output, strip_tool_calls},
    *,
};
use crate::services::virtual_llm::VirtualReplayTurn;

pub(super) fn normalize(mut turn: VirtualReplayTurn, mode: ToolCallMode) -> VirtualReplayTurn {
    let calls = collect_tool_calls_from_output(
        &turn.content,
        &turn.reasoning,
        turn.tool_calls.as_ref(),
        mode,
    );
    if !calls.is_empty() {
        // Remove the recorded textual envelope before selecting the request's output protocol.
        turn.content = strip_tool_calls(&turn.content);
        turn.reasoning = strip_tool_calls(&turn.reasoning);
        turn.tool_calls = Some(Value::Array(calls.into_iter().enumerate().map(|(index, call)| {
            let id = call.id.unwrap_or_else(|| format!("replay_{}_{}_{index}", turn.source_round, turn.source_model_round.unwrap_or(1)));
            json!({"id":id,"type":"function","function":{"name":call.name,"arguments":call.arguments.to_string()}})
        }).collect()));
    }
    turn
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::virtual_llm::request::prepare_turn;

    fn recorded(content: String, tool_calls: Option<Value>) -> VirtualReplayTurn {
        VirtualReplayTurn {
            content,
            reasoning: String::new(),
            tool_calls,
            usage: None,
            finish_reason: None,
            source_log_id: "test".into(),
            source_log_name: "test".into(),
            source_round: 1,
            source_model_round: Some(1),
            format: "replay".into(),
        }
    }

    #[test]
    fn virtual_replay_protocol_normalizes_once_and_preserves_arguments_in_both_modes() {
        let arguments = json!({"text":"</tool_call><tool_call>"});
        let payload = json!([{"id":"call_1","type":"function","function":{"name":"test_tool","arguments":arguments.to_string()}}]);
        let tools = [
            json!({"type":"function","function":{"name":"test_tool","parameters":{"type":"object"}}}),
        ];
        for (mode, key) in [
            (ToolCallMode::FunctionCall, "function_call"),
            (ToolCallMode::ToolCall, "tool_call"),
            (ToolCallMode::FreeformCall, "freeform_call"),
        ] {
            let config = LlmModelConfig {
                tool_call_mode: Some(key.into()),
                max_output: Some(512),
                ..Default::default()
            };
            let turn = normalize(recorded(String::new(), Some(payload.clone())), mode);
            let output = prepare_turn(
                turn,
                &config,
                &[],
                (mode == ToolCallMode::FunctionCall).then_some(tools.as_slice()),
            )
            .unwrap();
            let parsed = collect_tool_calls_from_output(
                &output.content,
                &output.reasoning,
                output.tool_calls.as_ref(),
                mode,
            );
            assert_eq!(parsed.len(), 1);
            assert_eq!(
                (&parsed[0].id, parsed[0].name.as_str(), &parsed[0].arguments),
                (&Some("call_1".into()), "test_tool", &arguments)
            );
        }
        let text =
            "<tool_call>{\"id\":\"call_1\",\"name\":\"test_tool\",\"arguments\":{}}</tool_call>";
        let once = normalize(
            recorded(
                text.into(),
                Some(
                    json!([{"id":"call_1","type":"function","function":{"name":"test_tool","arguments":"{}"}}]),
                ),
            ),
            ToolCallMode::FunctionCall,
        );
        assert_eq!(once.content, "");
        assert_eq!(once.tool_calls.unwrap().as_array().unwrap().len(), 1);
    }

    #[test]
    fn virtual_replay_text_call_cannot_execute_after_output_truncation() {
        let mode = ToolCallMode::ToolCall;
        let turn = normalize(
            recorded(
                "<tool_call>{\"name\":\"test_tool\",\"arguments\":{}}</tool_call>".into(),
                None,
            ),
            mode,
        );
        let config = LlmModelConfig {
            tool_call_mode: Some("tool_call".into()),
            max_output: Some(2),
            ..Default::default()
        };
        let output = prepare_turn(turn, &config, &[], None).unwrap();
        assert_eq!(output.finish_reason.as_deref(), Some("length"));
        assert!(collect_tool_calls_from_output(
            &output.content,
            &output.reasoning,
            output.tool_calls.as_ref(),
            mode
        )
        .is_empty());
    }
}
