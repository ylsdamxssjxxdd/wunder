//! Append-only interlink audit trail (docs §3.1(6), §9.4).
//!
//! Audit rows carry digests, counts and codes - never message bodies, file
//! contents or tool arguments (docs §9.3).

use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::storage::InterlinkAuditRecord;

/// Page size used by the bridge audit views.
pub const AUDIT_PAGE_DEFAULT: i64 = 100;
pub const AUDIT_PAGE_MAX: i64 = 500;
/// Guard against a runaway detail string reaching the database.
const DETAIL_MAX_CHARS: usize = 2048;

/// Build one audit row. `seq` is assigned by storage (autoincrement).
///
/// Building the row is also the single hook point of the alerting governance
/// layer: every trail writer feeds the detector without knowing about it.
pub fn record(
    action: &str,
    actor: &str,
    from_node: Option<&str>,
    to_node: Option<&str>,
    command_id: Option<&str>,
    approval_id: Option<&str>,
    detail: Vec<(&str, Value)>,
) -> InterlinkAuditRecord {
    let row = InterlinkAuditRecord {
        seq: 0,
        command_id: command_id.map(str::to_string),
        approval_id: approval_id.map(str::to_string),
        actor: actor.to_string(),
        from_node: from_node.map(str::to_string),
        to_node: to_node.map(str::to_string),
        action: action.to_string(),
        detail_digest: Some(detail_json(detail)),
        created_at: now_unix_seconds(),
    };
    super::alerts::observe(&row);
    row
}

/// Serialize a bounded, ordered detail digest.
pub fn detail_json(parts: Vec<(&str, Value)>) -> String {
    let mut map = Map::new();
    for (key, value) in parts {
        let projected = match &value {
            Value::String(text) => {
                let trimmed: String = text.chars().take(256).collect();
                Value::String(trimmed)
            }
            other => other.clone(),
        };
        map.insert(key.to_string(), projected);
    }
    let json = serde_json::to_string(&Value::Object(map)).unwrap_or_else(|_| "{}".to_string());
    let mut out: String = json.chars().take(DETAIL_MAX_CHARS).collect();
    if out.len() < json.len() {
        out.push('…');
    }
    out
}

/// Project one stored detail column back into a response value.
///
/// The user and the 舰桥 audit surfaces both use it, so an alert's `trigger`,
/// `kind` and `level` reach every consumer as typed fields instead of a JSON
/// string each client parses on its own. Text that is not valid JSON (a
/// truncated digest) stays verbatim rather than being dropped.
pub fn detail_value(raw: Option<&str>) -> Value {
    match raw {
        None => Value::Null,
        Some(text) => {
            serde_json::from_str::<Value>(text).unwrap_or_else(|_| Value::String(text.to_string()))
        }
    }
}

/// CSV export for the bridge (docs §9.4). Values are quoted and escaped; the
/// detail column stays a digest, so the export never contains payload.
pub fn csv(rows: &[InterlinkAuditRecord]) -> String {
    const HEADER: [&str; 9] = [
        "seq",
        "created_at",
        "actor",
        "from_node",
        "to_node",
        "action",
        "command_id",
        "approval_id",
        "detail_digest",
    ];
    let mut out = HEADER.join(",");
    out.push('\n');
    for row in rows {
        let cells = [
            row.seq.to_string(),
            format!("{:.3}", row.created_at),
            row.actor.clone(),
            row.from_node.clone().unwrap_or_default(),
            row.to_node.clone().unwrap_or_default(),
            row.action.clone(),
            row.command_id.clone().unwrap_or_default(),
            row.approval_id.clone().unwrap_or_default(),
            row.detail_digest.clone().unwrap_or_default(),
        ];
        let line = cells
            .iter()
            .map(|cell| csv_cell(cell))
            .collect::<Vec<String>>()
            .join(",");
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn csv_cell(value: &str) -> String {
    let needs_quotes = value.contains(',') || value.contains('"') || value.contains('\n');
    let escaped = value.replace('"', "\"\"");
    if needs_quotes {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

/// Normalize a requested page limit into the bounded range this API accepts.
pub fn page_limit(requested: Option<i64>) -> i64 {
    requested
        .filter(|value| *value > 0)
        .unwrap_or(AUDIT_PAGE_DEFAULT)
        .min(AUDIT_PAGE_MAX)
}

pub fn page_offset(requested: Option<i64>) -> i64 {
    requested.filter(|value| *value >= 0).unwrap_or(0)
}

fn now_unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(seq: i64, detail: &str) -> InterlinkAuditRecord {
        InterlinkAuditRecord {
            seq,
            command_id: Some("cmd_1".to_string()),
            approval_id: None,
            actor: "u_1".to_string(),
            from_node: Some("web:conn".to_string()),
            to_node: Some("device:dev".to_string()),
            action: "file.read".to_string(),
            detail_digest: Some(detail.to_string()),
            created_at: 10.5,
        }
    }

    #[test]
    fn record_digests_are_ordered_and_bounded() {
        let detail = detail_json(vec![
            ("path", Value::String("notes/a.md".to_string())),
            ("size", Value::from(1234)),
            ("long", Value::String("x".repeat(400))),
        ]);
        assert!(detail.starts_with('{'));
        assert!(detail.contains("notes/a.md"));
        assert!(detail.chars().count() <= DETAIL_MAX_CHARS + 1);
        let row = record(
            "file.read",
            "u_1",
            Some("web:conn"),
            Some("device:dev"),
            Some("cmd_1"),
            None,
            vec![("path", Value::String("a/b.txt".to_string()))],
        );
        assert_eq!(row.action, "file.read");
        assert!(row.detail_digest.unwrap().contains("a/b.txt"));
    }

    #[test]
    fn detail_column_projects_back_to_typed_fields() {
        let stored = detail_json(vec![
            ("trigger", Value::String("l3_execution".to_string())),
            ("count", Value::from(1)),
        ]);
        let value = detail_value(Some(&stored));
        assert_eq!(value["trigger"].as_str(), Some("l3_execution"));
        assert_eq!(value["count"].as_i64(), Some(1));
        // A truncated digest is not valid JSON: it stays readable instead of
        // being replaced by null.
        assert_eq!(detail_value(Some("not json")).as_str(), Some("not json"));
        assert!(detail_value(None).is_null());
    }

    #[test]
    fn csv_escapes_delimiters_and_keeps_one_row_per_record() {
        let rows = vec![
            sample(1, "{\"path\":\"a,b\"}"),
            sample(2, "say \"hi\""),
        ];
        let text = csv(&rows);
        let lines: Vec<&str> = text.trim().split('\n').collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "seq,created_at,actor,from_node,to_node,action,command_id,approval_id,detail_digest");
        assert!(lines[1].contains("\"{\"\"path\"\":\"\"a,b\"\"}\""));
        assert!(lines[2].contains("\"say \"\"hi\"\"\""));
    }

    #[test]
    fn paging_is_bounded() {
        assert_eq!(page_limit(None), AUDIT_PAGE_DEFAULT);
        assert_eq!(page_limit(Some(0)), AUDIT_PAGE_DEFAULT);
        assert_eq!(page_limit(Some(-5)), AUDIT_PAGE_DEFAULT);
        assert_eq!(page_limit(Some(10_000)), AUDIT_PAGE_MAX);
        assert_eq!(page_offset(None), 0);
        assert_eq!(page_offset(Some(-1)), 0);
    }
}
