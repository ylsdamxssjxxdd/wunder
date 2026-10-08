//! Lazy recovery is bounded by the requested thread, shared by server and local runtimes.
use crate::storage::StorageBackend;
use anyhow::Result;

pub(crate) const LEASE_SECONDS: f64 = 300.0;

pub(crate) fn recover(storage: &dyn StorageBackend, user: &str, session: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let Some(mut run) = storage
        .list_session_runs_by_session(user, session, 1)?
        .into_iter()
        .next()
    else {
        return Ok(());
    };
    if matches!(run.status.as_str(), "queued" | "running" | "waiting")
        && run.updated_time < now - LEASE_SECONDS
    {
        storage.interrupt_stale_session_runs(user, session, now - LEASE_SECONDS, now)?;
        run = storage.get_session_run(&run.run_id)?.unwrap_or(run);
    }
    // Read the latest run even after a previous recovery attempt: repairing the
    // visible turn is idempotent and must survive a crash between these writes.
    if run.status == "cancelled"
        && run.error.as_deref() == Some("execution lease expired; send a new task to continue")
    {
        if let Some(turn) = run
            .metadata
            .as_ref()
            .and_then(|meta| meta.pointer("/subagent_progress/child_turn_id"))
            .and_then(serde_json::Value::as_str)
        {
            storage.update_thread_turn(
                user,
                session,
                turn,
                "interrupted",
                "Execution interrupted; awaiting a new task",
                &serde_json::json!({"reason":"execution_lease_expired"}),
            )?;
        }
    }
    Ok(())
}
