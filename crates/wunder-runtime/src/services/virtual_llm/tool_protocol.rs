//! Replay the recorded call using the same protocol selected for a real provider.
use super::{capabilities::VirtualModelError, VirtualReplayTurn};
use crate::{
    config::LlmModelConfig,
    llm::{resolve_openai_api_mode, resolve_tool_call_mode, OpenAiApiMode, ToolCallMode},
};
use serde_json::{json, Value};

pub(super) fn adapt(
    turn: &mut VirtualReplayTurn,
    model: &LlmModelConfig,
    tools: Option<&[Value]>,
) -> Result<bool, VirtualModelError> {
    let calls = turn
        .tool_calls
        .as_ref()
        .and_then(Value::as_array)
        .filter(|calls| !calls.is_empty());
    let has_text_calls = turn.content.contains("<tool_call>")
        || turn.reasoning.contains("<tool_call>")
        || turn.content.contains("<function=")
        || turn.reasoning.contains("<function=");
    let has_calls = calls.is_some() || has_text_calls;
    if has_calls
        && model
            .simulation
            .as_ref()
            .is_some_and(|options| !options.support_tools)
    {
        return Err(VirtualModelError::new(
            "unsupported_tools",
            "tool_calls",
            "This model does not support tool calling.",
        ));
    }
    let Some(calls) = calls else {
        return Ok(has_calls);
    };
    let native = match resolve_tool_call_mode(model) {
        ToolCallMode::FunctionCall => true,
        ToolCallMode::ToolCall => false,
        ToolCallMode::FreeformCall => resolve_openai_api_mode(model) == OpenAiApiMode::Responses,
    };
    if native {
        let mut mapped = calls.clone();
        for call in &mut mapped {
            let name = call
                .pointer("/function/name")
                .or_else(|| call.pointer("/custom/name"))
                .and_then(Value::as_str);
            let offered = name.and_then(|name| {
                tools.and_then(|tools| {
                    tools.iter().find_map(|tool| {
                        tool.pointer("/function/name")
                            .or_else(|| tool.get("name"))
                            .or_else(|| tool.pointer("/custom/name"))
                            .and_then(Value::as_str)
                            .filter(|offered| {
                                *offered == name
                                    || crate::tools::resolve_tool_name(offered)
                                        == crate::tools::resolve_tool_name(name)
                            })
                    })
                })
            });
            let Some(offered) = offered else {
                return Err(VirtualModelError::new(
                    "tool_not_available",
                    "tool_calls",
                    "The replay tool is not offered in this request.",
                ));
            };
            // Localized replay names must resolve to the current request's native function name.
            if call.get("function").is_some() {
                call["function"]["name"] = json!(offered);
            } else {
                call["custom"]["name"] = json!(offered);
            }
        }
        turn.tool_calls = Some(Value::Array(mapped));
    } else {
        // Prompt protocols have no API tools field. The normal executor checks allowed tools.
        // Preserve text-only recordings verbatim and avoid emitting an already recorded block twice.
        if !has_text_calls {
            for call in calls {
                let function = call
                    .get("function")
                    .or_else(|| call.get("custom"))
                    .unwrap_or(call);
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        VirtualModelError::new(
                            "invalid_tool_call",
                            "tool_calls",
                            "The recorded tool call has no name.",
                        )
                    })?;
                let arguments = if let Some(input) = function.get("input") {
                    json!({"input": input})
                } else {
                    let raw = function
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    if let Some(text) = raw.as_str() {
                        serde_json::from_str(text).map_err(|_| {
                            VirtualModelError::new(
                                "invalid_tool_call",
                                "tool_calls",
                                "The recorded function arguments are not valid JSON.",
                            )
                        })?
                    } else {
                        raw
                    }
                };
                // Escape angle brackets so argument text cannot terminate the protocol block.
                let block = json!({"id":call.get("id"),"name":name,"arguments":arguments})
                    .to_string()
                    .replace('<', "\\u003c")
                    .replace('>', "\\u003e");
                turn.content
                    .push_str(&format!("\n<tool_call>{block}</tool_call>"));
            }
        }
        turn.tool_calls = None;
    }
    Ok(true)
}
