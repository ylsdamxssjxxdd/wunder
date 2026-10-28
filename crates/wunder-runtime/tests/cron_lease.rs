use serde_json::json;
use wunder_server::storage::*;

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

#[test]
fn simultaneous_manual_claims_have_one_winner() {
    let directory = tempfile::tempdir().unwrap();
    let storage = std::sync::Arc::new(SqliteStorage::new(
        directory
            .path()
            .join("claims.db")
            .to_string_lossy()
            .into_owned(),
    ));
    let now = now_ts();
    let expected = build_job(now, "claim_job");
    storage.upsert_cron_job(&expected).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers = (0..2)
        .map(|worker| {
            let storage = storage.clone();
            let barrier = barrier.clone();
            let expected = expected.clone();
            std::thread::spawn(move || {
                let mut claimed = expected.clone();
                claimed.runner_id = Some(format!("runner_{worker}"));
                claimed.run_token = Some(format!("token_{worker}"));
                claimed.running_at = Some(now);
                claimed.heartbeat_at = Some(now);
                claimed.lease_expires_at = Some(now + 60.0);
                barrier.wait();
                storage
                    .compare_and_swap_cron_job(&expected, Some(&claimed), None)
                    .unwrap()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|claimed| *claimed)
            .count(),
        1
    );
    assert_eq!(storage.count_running_cron_jobs(now).unwrap(), 1);
}

#[test]
fn stale_manual_snapshot_cannot_replace_timer_claim_or_deleted_job() {
    let directory = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(
        directory
            .path()
            .join("stale.db")
            .to_string_lossy()
            .into_owned(),
    );
    let now = now_ts();
    let expected = build_job(now, "snapshot_job");
    storage.upsert_cron_job(&expected).unwrap();
    storage
        .claim_due_cron_jobs(now, 1, "timer_runner", now + 60.0)
        .unwrap();
    assert!(!storage
        .compare_and_swap_cron_job(&expected, Some(&expected), None)
        .unwrap());
    storage
        .delete_cron_job(&expected.user_id, &expected.job_id)
        .unwrap();
    assert!(!storage
        .compare_and_swap_cron_job(&expected, Some(&expected), None)
        .unwrap());
    assert!(storage
        .get_cron_job(&expected.user_id, &expected.job_id)
        .unwrap()
        .is_none());
}

#[test]
fn stale_completion_cannot_overwrite_edit_heartbeat_or_new_lease() {
    let directory = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(
        directory
            .path()
            .join("completion.db")
            .to_string_lossy()
            .into_owned(),
    );
    let now = now_ts();
    let job = build_job(now, "completion_job");
    storage.upsert_cron_job(&job).unwrap();
    let expected = storage
        .claim_due_cron_jobs(now, 1, "runner_old", now + 60.0)
        .unwrap()
        .remove(0);
    let mut completed = expected.clone();
    completed.running_at = None;
    completed.runner_id = None;
    completed.run_token = None;
    completed.heartbeat_at = None;
    completed.lease_expires_at = None;
    let run = CronRunRecord {
        run_id: "completion_run".into(),
        job_id: job.job_id.clone(),
        user_id: job.user_id.clone(),
        session_id: Some(job.session_id.clone()),
        agent_id: None,
        trigger: "timer".into(),
        status: "ok".into(),
        summary: None,
        error: None,
        duration_ms: 1,
        created_at: now,
    };
    let mut edited = expected.clone();
    edited.payload = json!({"message": "updated fixture"});
    assert!(storage
        .compare_and_swap_cron_job(&expected, Some(&edited), None)
        .unwrap());
    assert!(!storage
        .compare_and_swap_cron_job(&expected, Some(&completed), Some(&run))
        .unwrap());
    assert!(storage
        .renew_cron_job_lease(
            &job.user_id,
            &job.job_id,
            "runner_old",
            expected.run_token.as_deref().unwrap(),
            now + 1.0,
            now + 61.0
        )
        .unwrap());
    assert!(!storage
        .compare_and_swap_cron_job(&edited, Some(&completed), Some(&run))
        .unwrap());
    let current = storage
        .claim_due_cron_jobs(now + 62.0, 1, "runner_new", now + 120.0)
        .unwrap()
        .remove(0);
    assert!(!storage
        .compare_and_swap_cron_job(&expected, Some(&completed), Some(&run))
        .unwrap());
    assert_eq!(
        storage.get_cron_job(&job.user_id, &job.job_id).unwrap(),
        Some(current)
    );
    assert!(storage
        .list_cron_runs(&job.user_id, &job.job_id, 10)
        .unwrap()
        .is_empty());
}

#[test]
fn run_history_failure_rolls_back_update_and_delete() {
    let directory = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(
        directory
            .path()
            .join("rollback.db")
            .to_string_lossy()
            .into_owned(),
    );
    let now = now_ts();
    let expected = build_job(now, "atomic_job");
    storage.upsert_cron_job(&expected).unwrap();
    let run = CronRunRecord {
        run_id: "existing_run".into(),
        job_id: expected.job_id.clone(),
        user_id: expected.user_id.clone(),
        session_id: Some(expected.session_id.clone()),
        agent_id: None,
        trigger: "timer".into(),
        status: "ok".into(),
        summary: None,
        error: None,
        duration_ms: 1,
        created_at: now,
    };
    storage.insert_cron_run(&run).unwrap();
    let mut disabled = expected.clone();
    disabled.enabled = false;
    for replacement in [Some(&disabled), None] {
        assert!(storage
            .compare_and_swap_cron_job(&expected, replacement, Some(&run))
            .is_err());
        assert_eq!(
            storage
                .get_cron_job(&expected.user_id, &expected.job_id)
                .unwrap(),
            Some(expected.clone())
        );
    }
    assert_eq!(
        storage
            .list_cron_runs(&expected.user_id, &expected.job_id, 10)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cron_pages_stats_and_next_wake_are_bounded_and_lease_aware() {
    let directory = tempfile::tempdir().unwrap();
    let storage = SqliteStorage::new(
        directory
            .path()
            .join("page.db")
            .to_string_lossy()
            .into_owned(),
    );
    let now = now_ts();
    for index in 0..205 {
        let mut job = build_job(now, &format!("job_{index:03}"));
        job.updated_at = now + index as f64;
        job.enabled = index % 2 == 0;
        job.agent_id = Some(
            if index % 2 == 0 {
                "agent_even"
            } else {
                "agent_odd"
            }
            .into(),
        );
        storage.upsert_cron_job(&job).unwrap();
    }
    let first = storage
        .list_cron_jobs_page("cron_user", true, None, 500, 0)
        .unwrap();
    assert_eq!(first.len(), 201);
    let second = storage
        .list_cron_jobs_page("cron_user", true, None, 201, 201)
        .unwrap();
    assert_eq!(second.len(), 4);
    assert!(!first
        .iter()
        .any(|first| second.iter().any(|second| first.job_id == second.job_id)));
    assert_eq!(
        storage
            .list_cron_jobs_page("cron_user", false, Some("agent_even"), 200, 0)
            .unwrap()
            .len(),
        103
    );
    let stats = storage
        .summarize_cron_jobs("cron_user", Some("agent_even"), now)
        .unwrap();
    assert_eq!((stats.total, stats.enabled, stats.running), (103, 103, 0));
    assert_eq!(storage.get_next_cron_run_at(now).unwrap(), Some(now - 1.0));
    let claimed = storage
        .claim_due_cron_jobs(now, 200, "timer", now + 60.0)
        .unwrap();
    assert_eq!(claimed.len(), 103);
    assert_eq!(storage.get_next_cron_run_at(now).unwrap(), Some(now + 60.0));
    assert_eq!(
        storage
            .summarize_cron_jobs("cron_user", None, now)
            .unwrap()
            .running,
        103
    );
    assert!(storage
        .cron_workspace_bindings("cron_user", &vec!["session".into(); 202])
        .is_err());
}
