//! Periodic interlink maintenance (docs §4.4, §9.4, §10).
//!
//! One bounded loop for the whole surface: stale tunnel rows, command timeouts,
//! abandoned data streams and ledger/audit retention. Everything it touches is
//! a bounded query - no full-table scans beyond the retention deletes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::core::blocking;
use crate::state::AppState;
use crate::storage::StorageBackend;
use serde_json::Value;
use super::{commands, registry};

/// Sweep cadence. Command timeouts default to 120s, so 30s is well inside the
/// accuracy the doc asks for while staying cheap.
pub const JANITOR_INTERVAL_S: u64 = 30;
/// Retention deletes run at most once per day (docs §9.4).
const RETENTION_INTERVAL_S: f64 = 24.0 * 3600.0;
/// Rows inspected per channel reconciliation tick.
const CHANNEL_PAGE: i64 = 200;

static RUNNING: AtomicBool = AtomicBool::new(false);

/// Start the maintenance loop once per process; a disabled interlink section
/// leaves the loop stopped (docs §3.3).
pub fn spawn(state: Arc<AppState>) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = tokio::runtime::Handle::current();
    handle.spawn(async move {
        let mut last_retention = 0.0f64;
        loop {
            tokio::time::sleep(Duration::from_secs(JANITOR_INTERVAL_S)).await;
            if !RUNNING.load(Ordering::SeqCst) {
                break;
            }
            let config = state.config_store.get().await;
            if !config.interlink.enabled {
                drop(config);
                continue;
            }
            let limits = commands::Limits::from_config(&config.interlink);
            let presence_ttl = config.interlink.presence_ttl_s.max(1) as f64;
            let command_retention = config.interlink.command_retention_days;
            let audit_retention = config.interlink.audit_retention_days;
            drop(config);

            let now = now_unix_seconds();
            // 1) Live channels that stopped beating are dropped locally...
            let stale: Vec<String> = registry()
                .snapshot()
                .iter()
                .filter(|live| now - live.last_seen_at > presence_ttl * 2.0)
                .map(|live| live.device_id.clone())
                .collect();
            let _dropped = registry().gc(now, presence_ttl * 2.0);
            // 2) ...their unfinished commands fail structurally and their
            //    remote watchers lose the stream (docs §13.2 7, §10.3).
            for device_id in stale {
                let _ = commands::fail_device_commands(state.storage.clone(), &device_id, now).await;
                super::remote::hub().close_node(&device_id);
            }
            // 3) Command timeouts and approval expiry (docs §4.3, §7.3 4).
            let _ = commands::sweep(state.storage.clone(), &limits, now).await;
            // 4) Abandoned data-plane buffers.
            let _ = super::blob::store().cleanup_expired(now);
            // 5) Persisted channel rows left open by a crashed instance.
            let _ = close_stale_channels(state.storage.clone(), now, presence_ttl * 2.0).await;
            // 6) Retention, once a day.
            if now - last_retention >= RETENTION_INTERVAL_S {
                last_retention = now;
                let storage = state.storage.clone();
                let _ = blocking::run_db("interlink.janitor.retention", move || {
                    if command_retention > 0 {
                        let _ = storage.cleanup_interlink_commands(command_retention)?;
                    }
                    if audit_retention > 0 {
                        let _ = storage.cleanup_interlink_audit(audit_retention)?;
                    }
                    Ok::<(), anyhow::Error>(())
                })
                .await;
            }
        }
    });
}

/// Stop the loop (used by shutdown paths and by tests).
pub fn stop() {
    RUNNING.store(false, Ordering::SeqCst);
}

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Close `interlink_channels` rows whose heartbeat went stale without a clean
/// teardown, so the bridge never shows a phantom tunnel.
async fn close_stale_channels(
    storage: Arc<dyn StorageBackend>,
    now: f64,
    stale_s: f64,
) -> anyhow::Result<usize> {
    let rows = {
        let storage = storage.clone();
        blocking::run_db("interlink.janitor.channels", move || {
            storage.list_interlink_channels(None, 0, CHANNEL_PAGE)
        })
        .await
        .map(|(rows, _total)| rows)?
    };

    let mut closed = 0usize;
    for row in rows {
        if row.closed_reason.is_some() {
            continue;
        }
        // A live channel in the registry owns the row; only orphans are closed.
        if registry().by_device(&row.device_id).is_some() {
            continue;
        }
        if now - row.last_seen_at <= stale_s {
            continue;
        }
        closed += 1;
        close_one(storage.clone(), &row.channel_id).await;
    }
    Ok(closed)
}

async fn close_one(storage: Arc<dyn StorageBackend>, channel_id: &str) {
    let lookup = channel_id.to_string();
    let _ = blocking::run_db("interlink.janitor.close_channel", move || {
        storage.close_interlink_channel(&lookup, "stale")
    })
    .await;
}

/// Mark a channel closed and stop claiming it is live (teardown path).
pub async fn record_close(storage: Arc<dyn StorageBackend>, channel_id: &str, reason: &str) {
    let lookup = channel_id.to_string();
    let reason = reason.to_string();
    let _ = blocking::run_db("interlink.janitor.record_close", move || {
        storage.close_interlink_channel(&lookup, &reason)
    })
    .await;
}

/// Snapshot of what the janitor owns, for the bridge monitoring page.
pub fn snapshot() -> serde_json::Value {
    let channels: Vec<serde_json::Value> = registry()
        .snapshot()
        .into_iter()
        .map(|live| {
            serde_json::json!({
                "channel_id": live.channel_id,
                "device_id": live.device_id,
                "user_id": live.user_id,
                "client": live.client,
                "instance_id": crate::api::interlink_ws::INSTANCE_ID,
                "protocol_version": live.protocol_version,
                "caps": live.capabilities,
                "connected_at": live.connected_at,
                "last_seen_at": live.last_seen_at,
                "rtt_ms": live.rtt_ms,
                "presence_status": live.presence_status,
                "active_threads": live.active_threads,
                "resumed": live.resumed,
            })
        })
        .collect();
    let mut rtt: Vec<i64> = channels
        .iter()
        .filter_map(|row| row.get("rtt_ms").and_then(Value::as_i64))
        .collect();
    rtt.sort_unstable();
    let percentile = |fraction: f64| -> Option<i64> {
        if rtt.is_empty() {
            return None;
        }
        let index = (((rtt.len() - 1) as f64) * fraction).round() as usize;
        rtt.get(index).copied()
    };
    serde_json::json!({
        "live_channels": channels.len(),
        "rtt_p50_ms": percentile(0.5),
        "rtt_p95_ms": percentile(0.95),
        "channels": channels,
        "commands": commands::hub().stats(),
        "watched_threads": super::remote::hub().watch_list()
            .into_iter()
            .map(|(device, thread, watchers)| serde_json::json!({
                "device_id": device, "thread_id": thread, "watchers": watchers
            }))
            .collect::<Vec<_>>(),
        "blob_cache_bytes": super::blob::store().cache_bytes(),
        "open_streams": super::blob::store().open_streams(),
    })
}

fn now_unix_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_is_idempotent_and_stoppable() {
        // No runtime here: assert only the guard flags behave.
        stop();
        assert!(!is_running());
        RUNNING.store(true, Ordering::SeqCst);
        assert!(is_running());
        stop();
        assert!(!is_running());
    }

    #[test]
    fn snapshot_reports_the_live_channels_and_command_totals() {
        let reg = registry();
        let (sender, _receiver) = super::super::LiveChannelRegistry::outbound_channel();
        let before = reg.count();
        reg.register(
            super::super::LiveChannel {
                device_id: "dev-janitor".to_string(),
                user_id: "u_1".to_string(),
                client: "desktop".to_string(),
                channel_id: "ch_janitor".to_string(),
                connection_id: "itl_janitor".to_string(),
                protocol_version: 1,
                capabilities: vec!["query.basic".to_string()],
                connected_at: now_unix_seconds(),
                last_seen_at: now_unix_seconds(),
                rtt_ms: Some(12),
                presence_status: None,
                active_threads: 0,
                resumed: false,
            },
            sender,
        );
        let value = snapshot();
        assert!(value["live_channels"].as_i64().unwrap() >= 1);
        assert_eq!(value["channels"][0]["device_id"], "dev-janitor");
        assert_eq!(value["rtt_p50_ms"], 12);
        assert!(value["commands"]["queued_total"].is_number());
        let _ = before;
        reg.unregister("dev-janitor", "ch_janitor");
    }
}
