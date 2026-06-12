pub(super) fn default_auto_memory_extract_prompt() -> &'static str {
    r#"You are a long-term memory extraction engine.
Extract up to 4 stable memory fragments from the current user message and the recent user-message window.

Keep only information that is likely to be useful across future turns:
- user identity or stable profile facts
- enduring preferences and constraints
- current ongoing plans that may matter in later turns
- explicit long-term notes the user asked the system to remember

Do not extract:
- temporary process chatter
- one-off execution details
- tool-call details
- facts stated only by the assistant
- guesses or inferred facts
- questions such as “what is my name” or “who am I” as if they were facts

Output only JSON in this exact shape:
<memory_fragments>
{
  "items": [
    {
      "category": "response-preference | profile | plan | preference | working-note",
      "slot": "reply_language | response_style | response_format | name | identity | background | current | generic | custom_stable_slot",
      "title": "",
      "summary": "",
      "content": "",
      "tags": [""],
      "tier": "core | working | peripheral",
      "importance": 0.0,
      "confidence": 0.0
    }
  ]
}
</memory_fragments>"#
}

pub(super) fn build_auto_memory_extract_request(
    question: &str,
    answer: &str,
    window: &[String],
) -> String {
    let mut lines = vec![
        "[Current User Message]".to_string(),
        truncate_auto_memory_extract_text(question, 1200),
    ];
    if !window.is_empty() {
        lines.push(String::new());
        lines.push("[Recent User Message Window]".to_string());
        for (index, item) in window.iter().enumerate() {
            lines.push(format!(
                "{}. {}",
                index + 1,
                truncate_auto_memory_extract_text(item, 600)
            ));
        }
    }
    if !answer.trim().is_empty() {
        lines.push(String::new());
        lines.push("[Latest Assistant Reply For Context Only]".to_string());
        lines.push(
            "Use this only for context disambiguation. Do not turn assistant-only claims into memories."
                .to_string(),
        );
        lines.push(truncate_auto_memory_extract_text(answer, 1000));
    }
    lines.join("\n")
}

pub(super) fn truncate_auto_memory_extract_text(text: &str, char_limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= char_limit {
        return trimmed.to_string();
    }
    let mut output = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if index >= char_limit {
            break;
        }
        output.push(ch);
    }
    output.push('…');
    output
}
