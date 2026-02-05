use serde_json::json;
use wunder_server::cron::persist_cron_run_and_update_job;
use wunder_server::storage::{CronJobRecord, SqliteStorage, StorageBackend};

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn build_job(now: f64, job_id: &str) -> CronJobRecord {
    CronJobRecord {
        job_id: job_id.to_string(),
        user_id: "cron_user".to_string(),
        session_id: "sess_main".to_string(),
        agent_id: Some("agent_a".to_string()),
        name: Some("cron test".to_string()),
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
        next_run_at: Some(now + 1.0),
        running_at: Some(now),
        last_run_at: None,
        last_status: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn cron_finish_does_not_resurrect_removed_job() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_finish_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let job = build_job(now, "job_removed");
    storage.upsert_cron_job(&job).unwrap();
    storage.delete_cron_job(&job.user_id, &job.job_id).unwrap();

    persist_cron_run_and_update_job(
        &storage,
        job.clone(),
        "timer".to_string(),
        "ok".to_string(),
        Some("done".to_string()),
        None,
        now,
        1200,
        now,
    )
    .unwrap();

    let fetched = storage.get_cron_job(&job.user_id, &job.job_id).unwrap();
    assert!(fetched.is_none());

    let runs = storage
        .list_cron_runs(&job.user_id, &job.job_id, 10)
        .unwrap();
    assert_eq!(runs.len(), 1);

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn cron_finish_respects_disabled_job() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_finish_disabled_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let job = build_job(now, "job_disabled");
    storage.upsert_cron_job(&job).unwrap();
    let mut disabled = job.clone();
    disabled.enabled = false;
    disabled.next_run_at = None;
    storage.upsert_cron_job(&disabled).unwrap();

    persist_cron_run_and_update_job(
        &storage,
        job,
        "timer".to_string(),
        "ok".to_string(),
        Some("done".to_string()),
        None,
        now,
        800,
        now,
    )
    .unwrap();

    let fetched = storage
        .get_cron_job(&disabled.user_id, &disabled.job_id)
        .unwrap()
        .expect("job should remain");
    assert!(!fetched.enabled);
    assert!(fetched.next_run_at.is_none());

    let _ = std::fs::remove_file(db_path);
}
