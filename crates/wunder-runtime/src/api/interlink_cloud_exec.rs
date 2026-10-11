//! Cloud-node command execution (docs §8.1): the `to:"cloud"` half of the
//! ledger.
//!
//! A local client that targets `cloud` does not go through a tunnel - the
//! server runs the action with its OWN engine services (the same handlers the
//! 蜂巢 web plane uses) and only borrows the ledger for idempotency, audit and
//! a uniform result shape. Nothing here invents new execution semantics: every
//! kind is answered by `services::interlink::client::execute`, the exact code a
//! local node runs.
//!
//! The one adaptation is the data plane: with no tunnel there is nobody to
//! stream chunks to, so the pull ceiling is pinned to the inline cap and any
//! larger read fails closed with `PAYLOAD_TOO_LARGE` instead of streaming.

use std::sync::Arc;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use wunder_core::interlink::{AUDIT_COMMAND_FINISH, COMMAND_STATUS_FAILED};

use crate::services::interlink::client::execute::{self, CommandSpec, ExecContext};
use crate::services::interlink::client::shadow::ShadowCollector;
use crate::services::interlink::client::stream;
use crate::services::interlink::client::TunnelWriter;
use crate::state::AppState;

/// Run one cloud-target command and write its terminal state to the ledger.
pub async fn run(
    state: Arc<AppState>,
    command_id: String,
    kind: String,
    actor_user_id: String,
    args: Value,
) {
    let payload = json!({
        "kind": kind,
        "args": args,
        "from": "cloud",
        "actor": actor_user_id,
    });
    let Some(spec) = CommandSpec::parse(Some(&command_id), &payload) else {
        finish_failed(&state, &command_id, &kind, "BAD_COMMAND", "unparsable command").await;
        return;
    };

    // The writer is a discard sink: its receivers are kept alive so queued
    // pushes succeed, but nothing consumes them (no tunnel on this path).
    let (writer, _control_rx, _data_rx) = TunnelWriter::new();
    let actor = actor_user_id.clone();
    let ctx = ExecContext {
        state: state.clone(),
        local_user_id: actor,
        workspace_id: None,
        max_file_pull_bytes: stream::INLINE_MAX_BYTES as u64,
        chunk_bytes: 64 * 1024,
        rate_bps: stream::MAX_BYTES_PER_S,
        source_tag: "remote:cloud".to_string(),
    };
    let collector = ShadowCollector::new();
    let wake = Arc::new(tokio::sync::Notify::new());
    let report = execute::execute(
        &spec,
        &ctx,
        &writer,
        &collector,
        &wake,
        CancellationToken::new(),
    )
    .await;

    let payload = report.payload();
    let storage = state.storage.clone();
    let id = command_id.clone();
    let status = report.status.to_string();
    let error = report.error.clone();
    let _ = crate::services::interlink::commands::finish_local(
        storage.clone(),
        &id,
        &status,
        payload,
        error.as_ref().map(|(code, _)| code.as_str()),
        now_unix_seconds(),
    )
    .await;
    let row = crate::services::interlink::audit::record(
        AUDIT_COMMAND_FINISH,
        &actor_user_id,
        None,
        Some("cloud"),
        Some(&command_id),
        None,
        vec![
            ("kind", Value::String(kind.clone())),
            (
                "status",
                Value::String(status.clone()),
            ),
        ],
    );
    let _ = crate::core::blocking::run_db("api.interlink_cloud_exec.finish", move || {
        storage.insert_interlink_audit(&row)
    })
    .await;
}

async fn finish_failed(
    state: &AppState,
    command_id: &str,
    kind: &str,
    code: &str,
    summary: &str,
) {
    let _ = crate::services::interlink::commands::finish_local(
        state.storage.clone(),
        command_id,
        COMMAND_STATUS_FAILED,
        Value::Null,
        Some(code),
        now_unix_seconds(),
    )
    .await;
    let storage = state.storage.clone();
    let row = crate::services::interlink::audit::record(
        AUDIT_COMMAND_FINISH,
        "cloud",
        None,
        Some("cloud"),
        Some(command_id),
        None,
        vec![
            ("kind", Value::String(kind.to_string())),
            ("code", Value::String(code.to_string())),
            ("summary", Value::String(summary.to_string())),
        ],
    );
    let _ = crate::core::blocking::run_db("api.interlink_cloud_exec.refused", move || {
        storage.insert_interlink_audit(&row)
    })
    .await;
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
