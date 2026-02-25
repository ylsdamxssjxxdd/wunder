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

fn event_payload(data: &Value) -> &Value {
    data.get("data").unwrap_or(data)
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

        let payload = event_payload(&event.data);
        match event.event.as_str() {
            "llm_output_delta" => {
                if let Some(delta) = payload.get("delta").and_then(Value::as_str) {
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
                    if let Some(content) = payload.get("content").and_then(Value::as_str) {
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
                let stage = payload.get("stage").and_then(Value::as_str).unwrap_or("");
                let summary = payload.get("summary").and_then(Value::as_str).unwrap_or("");
                if !stage.is_empty() || !summary.is_empty() {
                    let line = format!("[progress] {stage} {summary}");
                    println!("{}", line.trim());
                }
            }
            "tool_call" => {
                self.ensure_newline();
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = payload
                    .get("args")
                    .map(compact_json)
                    .unwrap_or_else(|| "{}".to_string());
                println!("[tool_call] {tool} {args}");
            }
            "tool_result" => {
                self.ensure_newline();
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let result = payload
                    .get("result")
                    .map(compact_json)
                    .unwrap_or_else(|| compact_json(payload));
                println!("[tool_result] {tool} {result}");
                let tool_key = tool.trim().to_ascii_lowercase();
                let is_question_tool = tool_key == "question_panel"
                    || tool_key == "ask_panel"
                    || tool.contains("问询面板");
                if is_question_tool {
                    for panel_payload in [
                        payload.get("data"),
                        payload.get("result"),
                        payload.get("result").and_then(|value| value.get("data")),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        if render_question_panel_lines(panel_payload) {
                            break;
                        }
                    }
                }
            }
            "question_panel" => {
                self.ensure_newline();
                let _ = render_question_panel_lines(payload);
            }
            "error" => {
                self.ensure_newline();
                let nested_message = payload
                    .get("data")
                    .and_then(Value::as_object)
                    .and_then(|inner| inner.get("message"))
                    .and_then(Value::as_str);
                let message = payload
                    .as_str()
                    .or_else(|| payload.get("message").and_then(Value::as_str))
                    .or_else(|| payload.get("detail").and_then(Value::as_str))
                    .or_else(|| payload.get("error").and_then(Value::as_str))
                    .or(nested_message)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| compact_json(payload));
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

fn render_question_panel_lines(payload: &Value) -> bool {
    let question = payload
        .get("question")
        .or_else(|| payload.get("prompt"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let routes = payload
        .get("routes")
        .or_else(|| payload.get("options"))
        .or_else(|| payload.get("choices"))
        .and_then(Value::as_array);
    let has_routes = routes.map(|value| !value.is_empty()).unwrap_or(false);
    if !has_routes && question.is_empty() {
        return false;
    }

    let is_zh = looks_like_zh(question.as_str())
        || routes
            .map(|items| {
                items.iter().any(|item| {
                    item.get("label")
                        .or_else(|| item.get("title"))
                        .or_else(|| item.get("name"))
                        .and_then(Value::as_str)
                        .map(looks_like_zh)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
    let display_question = if question.is_empty() {
        if is_zh {
            "请选择一条路线继续"
        } else {
            "Choose a route to continue"
        }
    } else {
        question.as_str()
    };
    println!("[question_panel] {display_question}");

    if let Some(routes) = routes {
        for (index, route) in routes.iter().enumerate() {
            let (label, description, recommended) = match route {
                Value::String(value) => (value.trim().to_string(), String::new(), false),
                Value::Object(map) => {
                    let label = map
                        .get("label")
                        .or_else(|| map.get("title"))
                        .or_else(|| map.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let description = map
                        .get("description")
                        .or_else(|| map.get("detail"))
                        .or_else(|| map.get("desc"))
                        .or_else(|| map.get("summary"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let recommended = map
                        .get("recommended")
                        .or_else(|| map.get("preferred"))
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    (label, description, recommended)
                }
                _ => (String::new(), String::new(), false),
            };
            if label.is_empty() {
                continue;
            }
            let recommended_tag = if recommended {
                if is_zh {
                    "（推荐）"
                } else {
                    " (recommended)"
                }
            } else {
                ""
            };
            if description.is_empty() {
                println!("  {}. {}{}", index + 1, label, recommended_tag);
            } else {
                let separator = if is_zh { "：" } else { ": " };
                println!(
                    "  {}. {}{}{}{}",
                    index + 1,
                    label,
                    recommended_tag,
                    separator,
                    description
                );
            }
        }
    }

    if is_zh {
        println!("  输入序号选择，例如 1 或 1,3");
    } else {
        println!("  choose by typing route number(s), e.g. 1 or 1,3");
    }
    true
}

fn looks_like_zh(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || ('\u{20000}'..='\u{2a6df}').contains(&ch)
    })
}

fn parse_final(event: &StreamEvent) -> Option<FinalEvent> {
    if event.event != "final" {
        return None;
    }
    let payload = event_payload(&event.data);
    Some(FinalEvent {
        answer: payload
            .get("answer")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        usage: payload
            .get("usage")
            .cloned()
            .filter(|value| !value.is_null()),
        stop_reason: payload
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
