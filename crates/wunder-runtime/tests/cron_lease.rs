use serde_json::json;
use wunder_server::storage::{CronJobRecord, SqliteStorage, StorageBackend};

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn build_job(now: f64, job_id: &str) -> CronJobRecord {
    CronJobRecord {
        job_id: job_id.to_string(),
        user_id: "cron_user".to_string(),
        session_id: "sess_main".to_string(),
        agent_id: None,
        name: Some(format!("job_{job_id}")),
        session_target: "shared".to_string(),
        payload: json!({ "message": "ping" }),
        deliver: None,
        enabled: true,
        delete_after_run: false,
        schedule_kind: "every".to_string(),
        schedule_at: None,
        schedule_every_ms: Some(1000),
        schedule_cron: None,
        schedule_tz: None,
        dedupe_key: None,
        next_run_at: Some(now - 1.0),
        running_at: None,
        runner_id: None,
        run_token: None,
        heartbeat_at: None,
        lease_expires_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        consecutive_failures: 0,
        auto_disabled_reason: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn expired_lease_can_be_reclaimed() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_lease_reclaim_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let mut job = build_job(now, "job_reclaim");
    job.running_at = Some(now - 10.0);
    job.runner_id = Some("runner_old".to_string());
    job.run_token = Some("token_old".to_string());
    job.heartbeat_at = Some(now - 10.0);
    job.lease_expires_at = Some(now - 1.0);
    storage.upsert_cron_job(&job).unwrap();

    let claimed = storage
        .claim_due_cron_jobs(now, 1, "runner_new", now + 300.0)
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].runner_id.as_deref(), Some("runner_new"));
    assert!(claimed[0].run_token.as_deref() != Some("token_old"));
    assert_eq!(storage.count_running_cron_jobs(now).unwrap(), 1);

    let _ = std::fs::remove_file(db_path);
}
