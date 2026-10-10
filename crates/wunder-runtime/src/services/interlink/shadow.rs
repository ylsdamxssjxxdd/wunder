//! Workspace shadow pipeline (I5): project a node's `shadow_full` /
//! `shadow_delta` tunnel frame into an `interlink_node_shadows` record,
//! enforcing revision monotonicity and delta field merge.
//!
//! The projection is pure (no database access) so it is unit-testable; the
//! tunnel ws loop owns the read-merge-upsert around it.
//!
//! Contract: docs/云端本地互通方案.md 3.1 (table) + 6.1 (projection) + 6.2
//! (sync). No new frame or ack type is introduced - a rejected frame is answered
//! with the existing `error` frame.

use serde_json::Value;

use wunder_core::interlink::{FRAME_SHADOW_DELTA, FRAME_SHADOW_FULL};
use wunder_core::storage_records::InterlinkShadowRecord;

/// Hard per-frame cap for a shadow payload (bytes of its compact JSON form).
///
/// The websocket layer already caps a single message at 512 KiB
/// (`WS_MAX_MESSAGE_BYTES` in `api::interlink_ws`), so this is a second,
/// semantic guard: a node whose projection exceeds it is rejected with
/// `payload_too_large` instead of being allowed to bloat the shadow store.
pub const SHADOW_PAYLOAD_MAX_BYTES: usize = 2 * 1024 * 1024;

/// Stable error codes returned through the existing `error` frame.
pub const ERR_INVALID_REVISION: &str = "invalid_revision";
pub const ERR_PAYLOAD_TOO_LARGE: &str = "payload_too_large";

/// Outcome of projecting one shadow frame.
#[derive(Debug, Clone)]
pub enum ShadowOutcome {
    /// Persist this record (already merged / validated).
    Apply(InterlinkShadowRecord),
    /// Stale delta (revision not greater than the stored one): ignore silently.
    Ignore,
    /// Reject with a stable error code (`invalid_revision` / `payload_too_large`).
    Reject(&'static str),
}

/// Project a `shadow_full` / `shadow_delta` frame.
///
/// * `full` is authoritative: it replaces every projected field (a field absent
///   from the payload clears the stored value) and is accepted at any valid
///   revision, because a node re-sends `revision=1` on every (re)connect
///   (docs 6.2.1) and must be able to resync past a higher stored revision.
/// * `delta` merges: only fields present in the payload change (an explicit
///   `null` clears a field), and it is applied only when its `revision` is
///   strictly greater than the stored one (docs 6.2.2/6.2.4).
///
/// `previous` is the currently stored shadow (None before the first sync).
pub fn project_shadow_frame(
    kind: &str,
    payload: &Value,
    device_id: &str,
    user_id: &str,
    previous: Option<&InterlinkShadowRecord>,
    now: f64,
) -> ShadowOutcome {
    if !is_shadow_kind(kind) {
        return ShadowOutcome::Reject("unknown_frame");
    }
    if payload_size(payload) > SHADOW_PAYLOAD_MAX_BYTES {
        return ShadowOutcome::Reject(ERR_PAYLOAD_TOO_LARGE);
    }

    let Some(revision) = payload.get("revision").and_then(Value::as_i64) else {
        return ShadowOutcome::Reject(ERR_INVALID_REVISION);
    };
    if revision <= 0 {
        return ShadowOutcome::Reject(ERR_INVALID_REVISION);
    }

    let stored_revision = previous.map(|record| record.revision).unwrap_or(0);
    let is_full = kind == FRAME_SHADOW_FULL;

    // A delta at or below the stored revision is stale: keep the newer state.
    if !is_full && revision <= stored_revision {
        return ShadowOutcome::Ignore;
    }

    let (summary, threads, tasks, workspace) = if is_full {
        (
            field_to_string(payload.get("summary")),
            field_to_string(payload.get("threads")),
            field_to_string(payload.get("tasks")),
            field_to_string(payload.get("workspace")),
        )
    } else {
        (
            delta_field(payload, "summary", previous.and_then(|r| r.summary.clone())),
            delta_field(payload, "threads", previous.and_then(|r| r.threads.clone())),
            delta_field(payload, "tasks", previous.and_then(|r| r.tasks.clone())),
            delta_field(payload, "workspace", previous.and_then(|r| r.workspace.clone())),
        )
    };

    ShadowOutcome::Apply(InterlinkShadowRecord {
        device_id: device_id.to_string(),
        user_id: user_id.to_string(),
        revision,
        summary,
        threads,
        tasks,
        workspace,
        synced_at: now,
    })
}

fn is_shadow_kind(kind: &str) -> bool {
    kind == FRAME_SHADOW_FULL || kind == FRAME_SHADOW_DELTA
}

/// Compact-JSON byte length of a payload, used as the oversize guard.
fn payload_size(payload: &Value) -> usize {
    serde_json::to_string(payload)
        .map(|text| text.len())
        .unwrap_or(usize::MAX)
}

/// Convert a projected field to its stored string form: `null`/absent -> None,
/// a string is kept verbatim, anything else (object/array) is JSON-serialized.
fn field_to_string(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text.clone()),
        Some(other) => serde_json::to_string(other).ok(),
    }
}

/// Delta field merge: a key present in the payload overwrites (an explicit
/// `null` clears it); an absent key keeps the previous value.
fn delta_field(payload: &Value, key: &str, previous: Option<String>) -> Option<String> {
    match payload.get(key) {
        Some(value) => field_to_string(Some(value)),
        None => previous,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn previous(revision: i64) -> InterlinkShadowRecord {
        InterlinkShadowRecord {
            device_id: "d1".to_string(),
            user_id: "u1".to_string(),
            revision,
            summary: Some("{\"os\":\"win\"}".to_string()),
            threads: Some("[{\"id\":\"t1\"}]".to_string()),
            tasks: Some("[]".to_string()),
            workspace: Some("{\"roots\":1}".to_string()),
            synced_at: 100.0,
        }
    }

    fn applied(outcome: ShadowOutcome) -> InterlinkShadowRecord {
        match outcome {
            ShadowOutcome::Apply(record) => record,
            other => panic!("expected Apply, got {other:?}"),
        }
    }

    #[test]
    fn full_replaces_every_field_and_clears_absent() {
        // Only summary + revision are sent; the rest must be cleared.
        let payload = json!({ "revision": 7, "summary": { "os": "linux" } });
        let record = applied(project_shadow_frame(
            FRAME_SHADOW_FULL,
            &payload,
            "d1",
            "u1",
            Some(&previous(5)),
            999.0,
        ));
        assert_eq!(record.revision, 7);
        assert_eq!(record.summary.as_deref(), Some("{\"os\":\"linux\"}"));
        assert!(record.threads.is_none());
        assert!(record.tasks.is_none());
        assert!(record.workspace.is_none());
        assert_eq!(record.synced_at, 999.0);
        assert_eq!(record.device_id, "d1");
        assert_eq!(record.user_id, "u1");
    }

    #[test]
    fn full_accepts_lower_revision_for_reconnect_resync() {
        // Reconnect sends revision=1 while the server holds 5: it must apply.
        let payload = json!({ "revision": 1, "summary": "{}" });
        let record = applied(project_shadow_frame(
            FRAME_SHADOW_FULL,
            &payload,
            "d1",
            "u1",
            Some(&previous(5)),
            1.0,
        ));
        assert_eq!(record.revision, 1);
    }

    #[test]
    fn delta_merges_and_keeps_absent_fields() {
        let payload = json!({ "revision": 6, "threads": [ { "id": "t2" } ] });
        let record = applied(project_shadow_frame(
            FRAME_SHADOW_DELTA,
            &payload,
            "d1",
            "u1",
            Some(&previous(5)),
            200.0,
        ));
        assert_eq!(record.revision, 6);
        // threads replaced, everything else retained from revision 5.
        assert_eq!(record.threads.as_deref(), Some("[{\"id\":\"t2\"}]"));
        assert_eq!(record.summary.as_deref(), Some("{\"os\":\"win\"}"));
        assert_eq!(record.tasks.as_deref(), Some("[]"));
        assert_eq!(record.workspace.as_deref(), Some("{\"roots\":1}"));
    }

    #[test]
    fn delta_explicit_null_clears_field() {
        let payload = json!({ "revision": 6, "tasks": null });
        let record = applied(project_shadow_frame(
            FRAME_SHADOW_DELTA,
            &payload,
            "d1",
            "u1",
            Some(&previous(5)),
            1.0,
        ));
        assert!(record.tasks.is_none());
        assert_eq!(record.threads.as_deref(), Some("[{\"id\":\"t1\"}]"));
    }

    #[test]
    fn stale_delta_is_ignored() {
        let payload = json!({ "revision": 5, "summary": "{}" });
        let outcome = project_shadow_frame(
            FRAME_SHADOW_DELTA,
            &payload,
            "d1",
            "u1",
            Some(&previous(5)),
            1.0,
        );
        assert!(matches!(outcome, ShadowOutcome::Ignore));
    }

    #[test]
    fn missing_or_invalid_revision_is_rejected() {
        for payload in [
            json!({ "summary": "{}" }),
            json!({ "revision": "x" }),
            json!({ "revision": 0 }),
            json!({ "revision": -3 }),
        ] {
            let outcome = project_shadow_frame(
                FRAME_SHADOW_DELTA,
                &payload,
                "d1",
                "u1",
                Some(&previous(1)),
                1.0,
            );
            match outcome {
                ShadowOutcome::Reject(code) => assert_eq!(code, ERR_INVALID_REVISION),
                other => panic!("expected Reject(invalid_revision), got {other:?}"),
            }
        }
    }

    #[test]
    fn oversize_payload_is_rejected() {
        // A summary string just over the cap must be refused.
        let big = "a".repeat(SHADOW_PAYLOAD_MAX_BYTES + 16);
        let payload = json!({ "revision": 2, "summary": big });
        let outcome = project_shadow_frame(
            FRAME_SHADOW_FULL,
            &payload,
            "d1",
            "u1",
            None,
            1.0,
        );
        match outcome {
            ShadowOutcome::Reject(code) => assert_eq!(code, ERR_PAYLOAD_TOO_LARGE),
            other => panic!("expected Reject(payload_too_large), got {other:?}"),
        }
    }

    #[test]
    fn unknown_frame_kind_is_rejected() {
        let payload = json!({ "revision": 1 });
        let outcome = project_shadow_frame("ping", &payload, "d1", "u1", None, 1.0);
        match outcome {
            ShadowOutcome::Reject(code) => assert_eq!(code, "unknown_frame"),
            other => panic!("expected Reject(unknown_frame), got {other:?}"),
        }
    }
}