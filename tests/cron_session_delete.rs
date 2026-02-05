use serde_json::json;
use wunder_server::storage::{CronJobRecord, SqliteStorage, StorageBackend};

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn build_job(now: f64, job_id: &str, session_id: &str) -> CronJobRecord {
    CronJobRecord {
        job_id: job_id.to_string(),
        user_id: "cron_user".to_string(),
        session_id: session_id.to_string(),
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
        next_run_at: Some(now + 1.0),
        running_at: None,
        last_run_at: None,
        last_status: None,
        last_error: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn cron_jobs_deleted_with_session() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_session_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let job_a = build_job(now, "job_a", "sess_a");
    let job_b = build_job(now, "job_b", "sess_a");
    let job_c = build_job(now, "job_c", "sess_b");
    storage.upsert_cron_job(&job_a).unwrap();
    storage.upsert_cron_job(&job_b).unwrap();
    storage.upsert_cron_job(&job_c).unwrap();

    let affected = storage
        .delete_cron_jobs_by_session(&job_a.user_id, &job_a.session_id)
        .unwrap();
    assert_eq!(affected, 2);

    let missing_a = storage.get_cron_job(&job_a.user_id, &job_a.job_id).unwrap();
    let missing_b = storage.get_cron_job(&job_b.user_id, &job_b.job_id).unwrap();
    let remain_c = storage.get_cron_job(&job_c.user_id, &job_c.job_id).unwrap();
    assert!(missing_a.is_none());
    assert!(missing_b.is_none());
    assert!(remain_c.is_some());

    let _ = std::fs::remove_file(db_path);
}
