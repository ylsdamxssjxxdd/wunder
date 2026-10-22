//! One persistence policy for rollout writers, recovery and stress tests.
//!
//! Live deltas deliberately share canonical event types with completed items,
//! but policy makes their transient status explicit. This prevents a frontend
//! stream implementation from accidentally becoming a second durable log.

use crate::thread_store::{CanonicalEvent, CanonicalItemKind, CanonicalRecord, RolloutRecord};

/// The Codex-compatible cap applies only to the persisted display output of
/// completed command and MCP result items. It is not a universal cap on model
/// context or arbitrary canonical payloads.
pub const PERSISTED_COMMAND_OUTPUT_MAX_BYTES: usize = 64 * 1024;
pub const PERSISTED_MCP_RESULT_MAX_BYTES: usize = 64 * 1024;
pub const OUTPUT_TRUNCATION_MARKER: &str = "\n[output truncated]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceDecision {
    Persist,
    Transient(TransientReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientReason {
    TextDelta,
    ReasoningDelta,
    PlanDelta,
    CommandOutputDelta,
    ItemStarted,
    ToolStarted,
    Progress,
    Warning,
}

/// Returns whether the record is a durable semantic fact. Call
/// `prepare_for_persistence` before appending a record so the command/MCP
/// display-output bounds are applied consistently by every writer.
pub fn persistence_decision(record: &CanonicalRecord) -> PersistenceDecision {
    match record {
        CanonicalRecord::Event(event) => match event {
            CanonicalEvent::TextDelta { .. } => {
                PersistenceDecision::Transient(TransientReason::TextDelta)
            }
            CanonicalEvent::ReasoningDelta { .. } => {
                PersistenceDecision::Transient(TransientReason::ReasoningDelta)
            }
            CanonicalEvent::PlanDelta { .. } => {
                PersistenceDecision::Transient(TransientReason::PlanDelta)
            }
            CanonicalEvent::CommandOutputDelta { .. } => {
                PersistenceDecision::Transient(TransientReason::CommandOutputDelta)
            }
            CanonicalEvent::ItemStarted { .. } => {
                PersistenceDecision::Transient(TransientReason::ItemStarted)
            }
            CanonicalEvent::ToolStarted { .. } => {
                PersistenceDecision::Transient(TransientReason::ToolStarted)
            }
            CanonicalEvent::Progress(_) => {
                PersistenceDecision::Transient(TransientReason::Progress)
            }
            CanonicalEvent::Warning(_) => PersistenceDecision::Transient(TransientReason::Warning),
            _ => PersistenceDecision::Persist,
        },
        _ => PersistenceDecision::Persist,
    }
}

/// Normalizes a rollout record for durable storage. `None` means that the
/// caller must keep it only in its bounded live stream/activity snapshot.
pub fn prepare_for_persistence(record: &RolloutRecord) -> Option<RolloutRecord> {
    if !matches!(
        persistence_decision(&record.record),
        PersistenceDecision::Persist
    ) {
        return None;
    }

    let mut record = record.clone();
    if let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) = &mut record.record {
        let limit = match item.kind {
            CanonicalItemKind::CommandResult => Some(PERSISTED_COMMAND_OUTPUT_MAX_BYTES),
            CanonicalItemKind::McpResult => Some(PERSISTED_MCP_RESULT_MAX_BYTES),
            _ => None,
        };
        if let (Some(limit), Some(output)) = (limit, item.display_output.as_deref()) {
            let (bounded, truncated) = truncate_utf8_with_marker(output, limit);
            item.display_output = Some(bounded);
            item.display_output_truncated |= truncated;
        }
    }
    Some(record)
}

/// Truncates at a UTF-8 boundary and includes a marker within the byte budget.
/// A caller may provide a smaller test budget; production uses the constants
/// above, both larger than the marker.
pub fn truncate_utf8_with_marker(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_owned(), false);
    }

    if max_bytes <= OUTPUT_TRUNCATION_MARKER.len() {
        return (
            OUTPUT_TRUNCATION_MARKER[..max_bytes.min(OUTPUT_TRUNCATION_MARKER.len())].to_owned(),
            true,
        );
    }

    let target = max_bytes - OUTPUT_TRUNCATION_MARKER.len();
    let mut end = target;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = String::with_capacity(end + OUTPUT_TRUNCATION_MARKER.len());
    bounded.push_str(&value[..end]);
    bounded.push_str(OUTPUT_TRUNCATION_MARKER);
    (bounded, true)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::thread_store::{CanonicalItem, ItemId, RolloutRecord, TurnId};

    fn id<T>(value: &str) -> T
    where
        T: TryFrom<String>,
        T::Error: std::fmt::Debug,
    {
        value.to_owned().try_into().unwrap()
    }

    fn completed(kind: CanonicalItemKind, output: &str) -> RolloutRecord {
        RolloutRecord {
            timestamp_ms: 1,
            ordinal: 0,
            record: CanonicalRecord::Event(CanonicalEvent::ItemCompleted(CanonicalItem {
                item_id: id::<ItemId>("item"),
                turn_id: id::<TurnId>("turn"),
                root_turn_id: None,
                created_at_ms: 1,
                kind,
                payload: json!({"kept": "verbatim"}),
                display_output: Some(output.to_owned()),
                display_output_truncated: false,
            })),
        }
    }

    #[test]
    fn deltas_are_not_durable_facts() {
        let record = RolloutRecord {
            timestamp_ms: 1,
            ordinal: 0,
            record: CanonicalRecord::Event(CanonicalEvent::TextDelta {
                item_id: id::<ItemId>("item"),
                text: "partial".into(),
            }),
        };
        assert_eq!(
            persistence_decision(&record.record),
            PersistenceDecision::Transient(TransientReason::TextDelta)
        );
        assert!(prepare_for_persistence(&record).is_none());
    }

    #[test]
    fn command_display_output_is_bounded_without_changing_payload() {
        let output = "界".repeat(PERSISTED_COMMAND_OUTPUT_MAX_BYTES);
        let prepared =
            prepare_for_persistence(&completed(CanonicalItemKind::CommandResult, &output)).unwrap();
        let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) = prepared.record else {
            panic!("expected completed item")
        };
        assert!(item.display_output.as_ref().unwrap().len() <= PERSISTED_COMMAND_OUTPUT_MAX_BYTES);
        assert!(item.display_output_truncated);
        assert_eq!(item.payload, json!({"kept": "verbatim"}));
    }

    #[test]
    fn non_command_items_are_not_subject_to_the_display_cap() {
        let output = "x".repeat(PERSISTED_COMMAND_OUTPUT_MAX_BYTES + 1);
        let prepared =
            prepare_for_persistence(&completed(CanonicalItemKind::ToolResult, &output)).unwrap();
        let CanonicalRecord::Event(CanonicalEvent::ItemCompleted(item)) = prepared.record else {
            panic!("expected completed item")
        };
        assert_eq!(item.display_output.as_deref(), Some(output.as_str()));
        assert!(!item.display_output_truncated);
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let (value, truncated) = truncate_utf8_with_marker(&"ab界界".repeat(4), 22);
        assert!(truncated);
        assert!(value.is_char_boundary(value.len()));
        assert!(value.len() <= 22);
        assert!(value.ends_with(OUTPUT_TRUNCATION_MARKER));
    }
}
