use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCardPrompt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_prompt: Option<String>,
}

impl WorkerCardPrompt {
    pub fn is_empty(&self) -> bool {
        self.system_prompt
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
            && self
                .extra_prompt
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerCardPromptEnvelope {
    pub prompt: WorkerCardPrompt,
    pub system_prompt: Option<String>,
    pub extra_prompt: Option<String>,
}

pub fn build_worker_card_prompt_envelope(prompt_text: &str) -> WorkerCardPromptEnvelope {
    WorkerCardPromptEnvelope {
        prompt: WorkerCardPrompt::default(),
        system_prompt: None,
        extra_prompt: trim_non_empty(prompt_text),
    }
}

pub fn resolve_worker_card_prompt_text(
    system_prompt: Option<&str>,
    extra_prompt: Option<&str>,
    prompt: &WorkerCardPrompt,
) -> String {
    [
        system_prompt.or(prompt.system_prompt.as_deref()),
        extra_prompt.or(prompt.extra_prompt.as_deref()),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn trim_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_worker_card_prompt_envelope, resolve_worker_card_prompt_text, WorkerCardPrompt,
    };

    #[test]
    fn resolve_prompt_text_prefers_top_level_fields_and_joins_sections() {
        let prompt = WorkerCardPrompt {
            system_prompt: Some("legacy system".to_string()),
            extra_prompt: Some("legacy extra".to_string()),
        };
        assert_eq!(
            resolve_worker_card_prompt_text(Some(" system "), Some(" extra "), &prompt),
            "system\n\nextra"
        );
    }

    #[test]
    fn resolve_prompt_text_falls_back_to_legacy_prompt_block() {
        let prompt = WorkerCardPrompt {
            system_prompt: Some("legacy system".to_string()),
            extra_prompt: Some("legacy extra".to_string()),
        };
        assert_eq!(
            resolve_worker_card_prompt_text(None, None, &prompt),
            "legacy system\n\nlegacy extra"
        );
    }

    #[test]
    fn build_prompt_envelope_emits_top_level_extra_prompt_only() {
        let envelope = build_worker_card_prompt_envelope("  planner prompt  ");
        assert!(envelope.prompt.is_empty());
        assert_eq!(envelope.system_prompt, None);
        assert_eq!(envelope.extra_prompt, Some("planner prompt".to_string()));
    }
}
