use crate::state::AppState;
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use url::Url;

const AUDIT_SAMPLE_LIMIT: usize = 12;

#[derive(Debug, Clone, Serialize, Default)]
pub struct LlmRequestAudit {
    pub total_requests: usize,
    pub mock_requests: usize,
    pub external_requests: usize,
    pub unknown_requests: usize,
    pub base_urls: Vec<String>,
    pub suspicious_samples: Vec<LlmRequestSample>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmRequestSample {
    pub session_id: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
}

impl LlmRequestAudit {
    pub fn suspicious_total(&self) -> usize {
        self.external_requests + self.unknown_requests
    }

    pub fn is_mock_only(&self) -> bool {
        self.suspicious_total() == 0
    }
}

pub fn collect_llm_request_audit(
    state: &AppState,
    user_id: &str,
    expected_mock_base_url: &str,
) -> Result<LlmRequestAudit> {
    let cleaned_user = user_id.trim();
    if cleaned_user.is_empty() {
        return Ok(LlmRequestAudit::default());
    }

    let expected_endpoint = parse_endpoint(expected_mock_base_url);
    let mut audit = LlmRequestAudit::default();
    let mut base_urls = BTreeSet::new();

    let (sessions, _) = state
        .user_store
        .list_chat_sessions(cleaned_user, None, None, 0, 4096)?;
    for session in sessions {
        let session_id = session.session_id;
        let Some(record) = state.monitor.get_record(&session_id) else {
            continue;
        };
        let Some(events) = record.get("events").and_then(Value::as_array) else {
            continue;
        };

        for event in events {
            if event.get("type").and_then(Value::as_str) != Some("llm_request") {
                continue;
            }
            audit.total_requests += 1;

            let payload = event.get("data").and_then(Value::as_object);
            let provider = payload
                .and_then(|map| map.get("provider"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let model = payload
                .and_then(|map| map.get("model"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);
            let base_url = payload
                .and_then(|map| map.get("base_url"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string);

            if let Some(base_url) = base_url.as_ref() {
                base_urls.insert(base_url.clone());
            } else {
                base_urls.insert("<missing>".to_string());
            }

            let class = classify_request(base_url.as_deref(), expected_endpoint.as_ref());
            match class {
                RequestClass::Mock => audit.mock_requests += 1,
                RequestClass::External => {
                    audit.external_requests += 1;
                    push_sample(
                        &mut audit.suspicious_samples,
                        session_id.as_str(),
                        provider.clone(),
                        model.clone(),
                        base_url.clone(),
                    );
                }
                RequestClass::Unknown => {
                    audit.unknown_requests += 1;
                    push_sample(
                        &mut audit.suspicious_samples,
                        session_id.as_str(),
                        provider.clone(),
                        model.clone(),
                        base_url.clone(),
                    );
                }
            }
        }
    }

    audit.base_urls = base_urls.into_iter().collect::<Vec<_>>();
    Ok(audit)
}

#[derive(Debug, Clone)]
struct Endpoint {
    host: String,
    port: Option<u16>,
}

#[derive(Debug, Clone, Copy)]
enum RequestClass {
    Mock,
    External,
    Unknown,
}

fn push_sample(
    samples: &mut Vec<LlmRequestSample>,
    session_id: &str,
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
) {
    if samples.len() >= AUDIT_SAMPLE_LIMIT {
        return;
    }
    samples.push(LlmRequestSample {
        session_id: session_id.to_string(),
        provider,
        model,
        base_url,
    });
}

fn classify_request(base_url: Option<&str>, expected_endpoint: Option<&Endpoint>) -> RequestClass {
    let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) else {
        return RequestClass::Unknown;
    };
    let Some(actual_endpoint) = parse_endpoint(base_url) else {
        return RequestClass::Unknown;
    };
    let Some(expected_endpoint) = expected_endpoint else {
        return RequestClass::Unknown;
    };
    if actual_endpoint.host == expected_endpoint.host
        && actual_endpoint.port == expected_endpoint.port
    {
        RequestClass::Mock
    } else {
        RequestClass::External
    }
}

fn parse_endpoint(value: &str) -> Option<Endpoint> {
    let parsed = Url::parse(value).ok()?;
    let host = parsed.host_str()?.trim().to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(Endpoint {
        host,
        port: parsed.port_or_known_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_request_marks_mock_for_same_endpoint() {
        let expected = parse_endpoint("http://127.0.0.1:19091/v1").expect("endpoint");
        let class = classify_request(
            Some("http://127.0.0.1:19091/chat/completions"),
            Some(&expected),
        );
        assert!(matches!(class, RequestClass::Mock));
    }

    #[test]
    fn classify_request_marks_external_for_different_endpoint() {
        let expected = parse_endpoint("http://127.0.0.1:19091/v1").expect("endpoint");
        let class = classify_request(
            Some("https://api.openai.com/v1/chat/completions"),
            Some(&expected),
        );
        assert!(matches!(class, RequestClass::External));
    }

    #[test]
    fn classify_request_marks_unknown_for_missing_base_url() {
        let expected = parse_endpoint("http://127.0.0.1:19091/v1").expect("endpoint");
        let class = classify_request(None, Some(&expected));
        assert!(matches!(class, RequestClass::Unknown));
    }
}
