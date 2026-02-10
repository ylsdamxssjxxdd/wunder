use anyhow::Result;
use serde_json::Value;
use std::io::{self, Write};
use wunder_server::schemas::StreamEvent;

const MAX_INLINE_JSON_CHARS: usize = 180;

#[derive(Debug, Clone, Default)]
pub struct FinalEvent {
    pub answer: String,
    pub usage: Option<Value>,
    pub stop_reason: Option<String>,
}

pub struct StreamRenderer {
    json: bool,
    line_open: bool,
    saw_delta: bool,
}

impl StreamRenderer {
    pub fn new(json: bool) -> Self {
        Self {
            json,
            line_open: false,
            saw_delta: false,
        }
    }

    pub fn render_event(&mut self, event: &StreamEvent) -> Result<Option<FinalEvent>> {
        if self.json {
            println!("{}", serde_json::to_string(event)?);
            return Ok(parse_final(event));
        }

        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(delta) = event.data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        print!("{delta}");
                        io::stdout().flush().ok();
                        self.line_open = true;
                        self.saw_delta = true;
                    }
                }
            }
            "llm_output" => {
                if !self.saw_delta {
                    if let Some(content) = event.data.get("content").and_then(Value::as_str) {
                        if !content.is_empty() {
                            print!("{content}");
                            io::stdout().flush().ok();
                            self.line_open = true;
                        }
                    }
                }
            }
            "progress" => {
                self.ensure_newline();
                let stage = event
                    .data
                    .get("stage")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let summary = event
                    .data
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !stage.is_empty() || !summary.is_empty() {
                    let line = format!("[progress] {stage} {summary}");
                    println!("{}", line.trim());
                }
            }
            "tool_call" => {
                self.ensure_newline();
                let tool = event
                    .data
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = event
                    .data
                    .get("args")
                    .map(compact_json)
                    .unwrap_or_else(|| "{}".to_string());
                println!("[tool_call] {tool} {args}");
            }
            "tool_result" => {
                self.ensure_newline();
                let tool = event
                    .data
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let result = event
                    .data
                    .get("result")
                    .map(compact_json)
                    .unwrap_or_else(|| compact_json(&event.data));
                println!("[tool_result] {tool} {result}");
            }
            "error" => {
                self.ensure_newline();
                let nested_message = event
                    .data
                    .get("data")
                    .and_then(Value::as_object)
                    .and_then(|inner| inner.get("message"))
                    .and_then(Value::as_str);
                let message = event
                    .data
                    .as_str()
                    .or_else(|| event.data.get("message").and_then(Value::as_str))
                    .or_else(|| event.data.get("detail").and_then(Value::as_str))
                    .or_else(|| event.data.get("error").and_then(Value::as_str))
                    .or(nested_message)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| compact_json(&event.data));
                eprintln!("[error] {message}");
            }
            "final" => {
                self.ensure_newline();
                let final_event = parse_final(event).unwrap_or_default();
                if !self.saw_delta && !final_event.answer.is_empty() {
                    println!("{}", final_event.answer);
                }
                return Ok(Some(final_event));
            }
            _ => {}
        }
        Ok(None)
    }

    pub fn finish(&mut self) {
        self.ensure_newline();
    }

    fn ensure_newline(&mut self) {
        if self.line_open {
            println!();
            self.line_open = false;
        }
    }
}

fn parse_final(event: &StreamEvent) -> Option<FinalEvent> {
    if event.event != "final" {
        return None;
    }
    Some(FinalEvent {
        answer: event
            .data
            .get("answer")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        usage: event
            .data
            .get("usage")
            .cloned()
            .filter(|value| !value.is_null()),
        stop_reason: event
            .data
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn compact_json(value: &Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
    if text.len() > MAX_INLINE_JSON_CHARS {
        text.truncate(MAX_INLINE_JSON_CHARS);
        text.push_str("...");
    }
    text
}
