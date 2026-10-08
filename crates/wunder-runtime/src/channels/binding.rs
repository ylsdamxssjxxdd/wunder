use crate::channels::types::ChannelMessage;
use crate::storage::ChannelBindingRecord;

const WILDCARD_PEER_ID: &str = "*";

#[derive(Debug, Clone)]
pub struct BindingResolution {
    pub agent_id: Option<String>,
    pub tool_overrides: Vec<String>,
    pub binding_id: Option<String>,
}

pub fn resolve_binding(
    bindings: &[ChannelBindingRecord],
    message: &ChannelMessage,
) -> Option<BindingResolution> {
    let mut best: Option<(i64, i64, &ChannelBindingRecord)> = None;
    for binding in bindings {
        if !binding.enabled {
            continue;
        }
        if !binding.channel.is_empty() && !eq_ignore_case(&binding.channel, &message.channel) {
            continue;
        }
        if !binding.account_id.is_empty()
            && !eq_ignore_case(&binding.account_id, &message.account_id)
        {
            continue;
        }
        if let Some(peer_kind) = binding.peer_kind.as_ref() {
            if !peer_kind.is_empty() && !peer_kind_matches(peer_kind, &message.peer.kind) {
                continue;
            }
        }
        if let Some(peer_id) = binding.peer_id.as_ref() {
            if !peer_id.is_empty()
                && !is_wildcard_peer_id(peer_id)
                && !eq_ignore_case(peer_id, &message.peer.id)
            {
                continue;
            }
        }
        let specificity = compute_specificity(binding);
        let priority = binding.priority;
        let candidate = (specificity, priority, binding);
        let is_better = match best {
            None => true,
            Some((best_spec, best_prio, _)) => {
                specificity > best_spec || (specificity == best_spec && priority > best_prio)
            }
        };
        if is_better {
            best = Some(candidate);
        }
    }
    best.map(|(_, _, binding)| BindingResolution {
        agent_id: binding
            .agent_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        tool_overrides: binding.tool_overrides.clone(),
        binding_id: Some(binding.binding_id.clone()),
    })
}

fn compute_specificity(binding: &ChannelBindingRecord) -> i64 {
    let mut score = 0;
    if !binding.channel.is_empty() {
        score += 1;
    }
    if !binding.account_id.is_empty() {
        score += 2;
    }
    if let Some(peer_kind) = binding.peer_kind.as_ref() {
        if !peer_kind.is_empty() {
            score += 4;
        }
    }
    if let Some(peer_id) = binding.peer_id.as_ref() {
        if !peer_id.is_empty() && !is_wildcard_peer_id(peer_id) {
            score += 8;
        }
    }
    score
}

fn peer_kind_matches(left: &str, right: &str) -> bool {
    eq_ignore_case(left, right) || (is_direct_peer_kind(left) && is_direct_peer_kind(right))
}

fn is_direct_peer_kind(kind: &str) -> bool {
    matches!(
        kind.trim().to_ascii_lowercase().as_str(),
        "dm" | "direct" | "single" | "user"
    )
}

fn eq_ignore_case(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn is_wildcard_peer_id(peer_id: &str) -> bool {
    peer_id.trim() == WILDCARD_PEER_ID
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::types::{ChannelMessage, ChannelPeer};

    fn message(peer_kind: &str, peer_id: &str) -> ChannelMessage {
        ChannelMessage {
            channel: "feishu".to_string(),
            account_id: "acc".to_string(),
            peer: ChannelPeer {
                kind: peer_kind.to_string(),
                id: peer_id.to_string(),
                name: None,
            },
            thread: None,
            message_id: None,
            sender: None,
            message_type: "text".to_string(),
            text: Some("hello".to_string()),
            attachments: Vec::new(),
            location: None,
            ts: None,
            meta: None,
        }
    }

    fn binding(peer_id: &str, priority: i64) -> ChannelBindingRecord {
        ChannelBindingRecord {
            binding_id: format!("b_{peer_id}"),
            channel: "feishu".to_string(),
            account_id: "acc".to_string(),
            peer_kind: Some("group".to_string()),
            peer_id: Some(peer_id.to_string()),
            agent_id: None,
            tool_overrides: Vec::new(),
            priority,
            enabled: true,
            created_at: 0.0,
            updated_at: 0.0,
        }
    }

    #[test]
    fn wildcard_peer_id_matches_any_peer() {
        let bindings = vec![binding("*", 10)];
        let resolved = resolve_binding(&bindings, &message("group", "chat_1"));
        assert!(resolved.is_some());
    }

    #[test]
    fn specific_peer_beats_wildcard() {
        let bindings = vec![binding("*", 100), binding("chat_1", 1)];
        let resolved = resolve_binding(&bindings, &message("group", "chat_1"));
        assert_eq!(
            resolved.and_then(|item| item.binding_id),
            Some("b_chat_1".to_string())
        );
    }
}
