use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use wunder_server::config::Config;
use wunder_server::cron::{handle_cron_action, CronActionRequest, CronJobInput, CronScheduleInput};
use wunder_server::skills::SkillRegistry;
use wunder_server::storage::{SqliteStorage, StorageBackend};
use wunder_server::user_store::UserStore;
use wunder_server::user_tools::{UserToolManager, UserToolStore};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cron_action_add_update_remove() {
    let config = Config::default();
    let db_path = std::env::temp_dir().join(format!(
        "wunder_cron_it_{}.db",
        uuid::Uuid::new_v4().simple()
    ));
    let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    storage.ensure_initialized().unwrap();

    let user_store = Arc::new(UserStore::new(storage.clone()));
    let user_tool_store = Arc::new(UserToolStore::new(&config).unwrap());
    let user_tool_manager = Arc::new(UserToolManager::new(user_tool_store));
    let skills = Arc::new(RwLock::new(SkillRegistry::default()));

    let empty_status_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "status".to_string(),
            job: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(empty_status_resp["action"], "status");
    assert_eq!(empty_status_resp["user_jobs"]["total"], 0);
    assert!(empty_status_resp["user_jobs"]["next_run_at"].is_null());

    let add_payload = CronActionRequest {
        action: "add".to_string(),
        job: Some(CronJobInput {
            job_id: None,
            name: Some("cron test".to_string()),
            schedule: Some(CronScheduleInput {
                kind: "every".to_string(),
                at: None,
                every_ms: Some(5000),
                cron: None,
                tz: None,
            }),
            schedule_text: None,
            session: Some("main".to_string()),
            payload: Some(json!({ "message": "ping" })),
            deliver: None,
            enabled: Some(true),
            delete_after_run: Some(false),
            dedupe_key: None,
            session_id: None,
            agent_id: None,
        }),
    };
    let add_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        Some("session_a"),
        Some("agent_a"),
        add_payload,
    )
    .await
    .unwrap();
    let job_id = add_resp["job"]["job_id"].as_str().unwrap().to_string();
    assert_eq!(add_resp["action"], "add");
    assert_eq!(add_resp["job"]["session_id"], "session_a");
    assert!(add_resp["job"]["next_run_at"].is_number());

    let status_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "status".to_string(),
            job: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(status_resp["action"], "status");
    assert_eq!(status_resp["scheduler"]["enabled"], true);
    assert_eq!(status_resp["user_jobs"]["total"], 1);
    assert_eq!(status_resp["user_jobs"]["enabled"], 1);
    assert!(status_resp["user_jobs"]["next_run_at"].is_number());

    let list_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "list".to_string(),
            job: None,
        },
    )
    .await
    .unwrap();
    let jobs = list_resp["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 1);

    let get_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "get".to_string(),
            job: Some(CronJobInput {
                job_id: Some(job_id.clone()),
                name: None,
                schedule: None,
                schedule_text: None,
                session: None,
                payload: None,
                deliver: None,
                enabled: None,
                delete_after_run: None,
                dedupe_key: None,
                session_id: None,
                agent_id: None,
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(get_resp["job"]["job_id"], job_id);

    let update_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "update".to_string(),
            job: Some(CronJobInput {
                job_id: Some(job_id.clone()),
                name: Some("cron updated".to_string()),
                schedule: None,
                schedule_text: None,
                session: None,
                payload: None,
                deliver: None,
                enabled: Some(false),
                delete_after_run: None,
                dedupe_key: None,
                session_id: None,
                agent_id: None,
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(update_resp["job"]["name"], "cron updated");
    assert_eq!(update_resp["job"]["enabled"], false);
    assert!(update_resp["job"]["next_run_at"].is_null());

    let mut stored_job = storage
        .get_cron_job("cron_user", &job_id)
        .unwrap()
        .expect("cron job should exist");
    stored_job.consecutive_failures = 3;
    stored_job.auto_disabled_reason = Some("auto disabled in previous run".to_string());
    storage.upsert_cron_job(&stored_job).unwrap();

    let enable_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "enable".to_string(),
            job: Some(CronJobInput {
                job_id: Some(job_id.clone()),
                name: None,
                schedule: None,
                schedule_text: None,
                session: None,
                payload: None,
                deliver: None,
                enabled: None,
                delete_after_run: None,
                dedupe_key: None,
                session_id: None,
                agent_id: None,
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(enable_resp["job"]["enabled"], true);
    assert_eq!(enable_resp["job"]["consecutive_failures"], 0);
    assert!(enable_resp["job"]["auto_disabled_reason"].is_null());
    assert!(enable_resp["job"]["next_run_at"].is_number());

    let disable_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "disable".to_string(),
            job: Some(CronJobInput {
                job_id: Some(job_id.clone()),
                name: None,
                schedule: None,
                schedule_text: None,
                session: None,
                payload: None,
                deliver: None,
                enabled: None,
                delete_after_run: None,
                dedupe_key: None,
                session_id: None,
                agent_id: None,
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(disable_resp["job"]["enabled"], false);
    assert!(disable_resp["job"]["next_run_at"].is_null());

    let remove_resp = handle_cron_action(
        config.clone(),
        storage.clone(),
        None,
        user_store.clone(),
        user_tool_manager.clone(),
        skills.clone(),
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "remove".to_string(),
            job: Some(CronJobInput {
                job_id: Some(job_id),
                name: None,
                schedule: None,
                schedule_text: None,
                session: None,
                payload: None,
                deliver: None,
                enabled: None,
                delete_after_run: None,
                dedupe_key: None,
                session_id: None,
                agent_id: None,
            }),
        },
    )
    .await
    .unwrap();
    assert_eq!(remove_resp["removed"], true);

    let list_resp = handle_cron_action(
        config,
        storage,
        None,
        user_store,
        user_tool_manager,
        skills,
        "cron_user",
        None,
        None,
        CronActionRequest {
            action: "list".to_string(),
            job: None,
        },
    )
    .await
    .unwrap();
    let jobs = list_resp["jobs"].as_array().unwrap();
    assert!(jobs.is_empty());

    let _ = std::fs::remove_file(db_path);
}
