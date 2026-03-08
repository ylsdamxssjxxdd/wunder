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
        runner_id: Some("runner_a".to_string()),
        run_token: Some(format!("token_{job_id}")),
        heartbeat_at: Some(now),
        lease_expires_at: Some(now + 300.0),
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
    assert!(runs.is_empty());

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

#[test]
fn cron_finish_auto_disables_after_consecutive_errors() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_finish_autodisable_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let job = build_job(now, "job_autodisable");
    storage.upsert_cron_job(&job).unwrap();

    for attempt in 0..5 {
        let run_now = now + attempt as f64;
        let mut claimed_job = storage
            .get_cron_job(&job.user_id, &job.job_id)
            .unwrap()
            .expect("job should exist");
        claimed_job.running_at = Some(run_now);
        claimed_job.runner_id = Some(format!("runner_{attempt}"));
        claimed_job.run_token = Some(format!("token_{attempt}"));
        claimed_job.heartbeat_at = Some(run_now);
        claimed_job.lease_expires_at = Some(run_now + 300.0);
        storage.upsert_cron_job(&claimed_job).unwrap();

        persist_cron_run_and_update_job(
            &storage,
            claimed_job,
            "timer".to_string(),
            "error".to_string(),
            None,
            Some(format!("error attempt {}", attempt + 1)),
            run_now,
            400,
            run_now,
        )
        .unwrap();
    }

    let fetched = storage
        .get_cron_job(&job.user_id, &job.job_id)
        .unwrap()
        .expect("job should remain");
    assert!(!fetched.enabled);
    assert!(fetched.next_run_at.is_none());
    assert_eq!(fetched.consecutive_failures, 5);
    assert!(fetched.auto_disabled_reason.is_some());

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn cron_finish_ignores_stale_lease_owner() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_finish_stale_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let stale_job = build_job(now, "job_stale");
    let mut current_job = stale_job.clone();
    current_job.runner_id = Some("runner_b".to_string());
    current_job.run_token = Some("token_b".to_string());
    current_job.heartbeat_at = Some(now);
    current_job.lease_expires_at = Some(now + 300.0);
    storage.upsert_cron_job(&current_job).unwrap();

    persist_cron_run_and_update_job(
        &storage,
        stale_job,
        "timer".to_string(),
        "ok".to_string(),
        Some("done".to_string()),
        None,
        now,
        200,
        now,
    )
    .unwrap();

    let fetched = storage
        .get_cron_job(&current_job.user_id, &current_job.job_id)
        .unwrap()
        .expect("job should remain");
    assert_eq!(fetched.runner_id.as_deref(), Some("runner_b"));
    assert_eq!(fetched.run_token.as_deref(), Some("token_b"));
    assert_eq!(fetched.last_status, None);

    let runs = storage
        .list_cron_runs(&current_job.user_id, &current_job.job_id, 10)
        .unwrap();
    assert!(runs.is_empty());

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn cron_finish_applies_backoff_to_recurring_errors() {
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_finish_backoff_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());
    storage.ensure_initialized().unwrap();
    let now = now_ts();
    let job = build_job(now, "job_backoff");
    storage.upsert_cron_job(&job).unwrap();

    persist_cron_run_and_update_job(
        &storage,
        job.clone(),
        "timer".to_string(),
        "error".to_string(),
        None,
        Some("temporary failure".to_string()),
        now,
        400,
        now,
    )
    .unwrap();

    let fetched = storage
        .get_cron_job(&job.user_id, &job.job_id)
        .unwrap()
        .expect("job should remain");
    assert!(fetched.enabled);
    assert_eq!(fetched.consecutive_failures, 1);
    assert!(fetched.next_run_at.is_some());
    assert!(fetched.next_run_at.unwrap() >= now + 30.0);

    let _ = std::fs::remove_file(db_path);
}
