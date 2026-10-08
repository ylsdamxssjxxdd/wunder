use super::*;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs_f64()
}

fn cloud_log(
    device_id: &str,
    user_id: &str,
    seq: i64,
    level: &str,
    category: &str,
    event: &str,
    message: Option<&str>,
    local_session_id: Option<&str>,
    created_at: f64,
) -> CloudDeviceLogRecord {
    CloudDeviceLogRecord {
        seq,
        device_id: device_id.to_string(),
        user_id: user_id.to_string(),
        client: "desktop".to_string(),
        level: level.to_string(),
        category: category.to_string(),
        event: event.to_string(),
        message: message.map(str::to_string),
        local_session_id: local_session_id.map(str::to_string),
        created_at,
    }
}

fn exercise_cloud(storage: Arc<dyn StorageBackend>) {
    let user = "cloud_user";
    let device = CloudDeviceRecord {
        device_id: "device-alpha".to_string(),
        user_id: user.to_string(),
        client: "desktop".to_string(),
        name: "workstation".to_string(),
        os: Some("TestOS".to_string()),
        arch: Some("x86_64".to_string()),
        app_version: Some("0.1.0".to_string()),
        last_seen_at: 100.0,
        created_at: 100.0,
        revoked: false,
    };
    storage.upsert_cloud_device(&device).unwrap();

    // Login reuse resolves the same row per (user, client, name).
    let found = storage
        .find_cloud_device_by_identity(user, "desktop", "workstation")
        .unwrap()
        .unwrap();
    assert_eq!(found.device_id, "device-alpha");
    assert!(storage
        .find_cloud_device_by_identity(user, "cli", "workstation")
        .unwrap()
        .is_none());
    assert!(storage
        .get_cloud_device("device-missing")
        .unwrap()
        .is_none());

    // Re-login refreshes diagnostics and last_seen_at but never resets the
    // stored revoked flag.
    storage
        .set_cloud_device_revoked("device-alpha", true)
        .unwrap();
    let mut refresh = device.clone();
    refresh.os = Some("TestOS2".to_string());
    refresh.app_version = Some("0.2.0".to_string());
    refresh.last_seen_at = 200.0;
    refresh.revoked = false;
    storage.upsert_cloud_device(&refresh).unwrap();
    let current = storage.get_cloud_device("device-alpha").unwrap().unwrap();
    assert!(current.revoked);
    assert_eq!(current.os.as_deref(), Some("TestOS2"));
    assert_eq!(current.app_version.as_deref(), Some("0.2.0"));
    assert_eq!(current.last_seen_at, 200.0);

    // Heartbeat only advances last_seen_at; revoke state changes explicitly.
    storage
        .set_cloud_device_revoked("device-alpha", false)
        .unwrap();
    storage.touch_cloud_device("device-alpha", 300.0).unwrap();
    let current = storage.get_cloud_device("device-alpha").unwrap().unwrap();
    assert!(!current.revoked);
    assert_eq!(current.last_seen_at, 300.0);
    assert_eq!(current.created_at, 100.0);

    // Second device; the admin list orders by last_seen_at desc with totals.
    let second = CloudDeviceRecord {
        device_id: "device-beta".to_string(),
        user_id: user.to_string(),
        client: "cli".to_string(),
        name: "laptop".to_string(),
        os: None,
        arch: None,
        app_version: None,
        last_seen_at: 250.0,
        created_at: 250.0,
        revoked: false,
    };
    storage.upsert_cloud_device(&second).unwrap();
    let (rows, total) = storage.list_cloud_devices(Some(user), 0, 10).unwrap();
    assert_eq!(total, 2);
    assert_eq!(rows[0].device_id, "device-alpha");
    assert_eq!(rows[1].device_id, "device-beta");
    let (rows, total) = storage
        .list_cloud_devices(Some("other_user"), 0, 10)
        .unwrap();
    assert_eq!((rows.len(), total), (0, 0));

    // Call accounting: admission rows first, then settlement with tokens.
    let call = CloudCallRecord {
        call_id: "call-1".to_string(),
        device_id: "device-alpha".to_string(),
        user_id: user.to_string(),
        client: "desktop".to_string(),
        local_session_id: Some("local-thread-1".to_string()),
        model: "model-a".to_string(),
        provider: Some("provider-a".to_string()),
        status: "admitted".to_string(),
        quota_consumed: true,
        queue_waited_ms: 40,
        prompt_tokens: None,
        completion_tokens: None,
        started_at: 1000.0,
        finished_at: None,
        duration_ms: None,
        error_summary: None,
    };
    storage.insert_cloud_call_record(&call).unwrap();
    let mut blocked = call.clone();
    blocked.call_id = "call-2".to_string();
    blocked.status = "quota_blocked".to_string();
    blocked.quota_consumed = false;
    blocked.started_at = 2000.0;
    storage.insert_cloud_call_record(&blocked).unwrap();
    storage
        .finalize_cloud_call_record("call-1", "ok", Some(11), Some(7), 1002.5, None)
        .unwrap();
    storage
        .finalize_cloud_call_record(
            "call-2",
            "quota_blocked",
            None,
            None,
            2001.0,
            Some("insufficient quota"),
        )
        .unwrap();

    let (calls, total) = storage
        .list_cloud_call_records(ListCloudRecordsQuery {
            user_id: Some(user),
            device_id: None,
            model: None,
            status: None,
            offset: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(calls[0].call_id, "call-2");
    assert_eq!(calls[1].call_id, "call-1");
    assert_eq!(calls[1].duration_ms, Some(2500));
    assert_eq!(calls[1].prompt_tokens, Some(11));
    assert_eq!(calls[1].completion_tokens, Some(7));
    assert_eq!(calls[1].status, "ok");
    assert!(calls[1].quota_consumed);
    assert!(!calls[0].quota_consumed);
    assert_eq!(
        calls[0].error_summary.as_deref(),
        Some("insufficient quota")
    );
    assert_eq!(calls[0].finished_at, Some(2001.0));

    // Combined filters and out-of-range pages behave deterministically.
    let (calls, total) = storage
        .list_cloud_call_records(ListCloudRecordsQuery {
            user_id: None,
            device_id: Some("device-alpha"),
            model: Some("model-a"),
            status: Some("ok"),
            offset: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(calls[0].call_id, "call-1");
    let (calls, total) = storage
        .list_cloud_call_records(ListCloudRecordsQuery {
            user_id: None,
            device_id: None,
            model: None,
            status: None,
            offset: 2,
            limit: 1,
        })
        .unwrap();
    assert_eq!(total, 2);
    assert!(calls.is_empty());

    // Log upload: idempotent on (device_id, seq); last_seq tracks the request.
    let now = now_ts();
    let batch = vec![
        cloud_log(
            "device-alpha",
            user,
            41,
            "warn",
            "tool",
            "command_failed",
            Some("exit code 2 (redacted)"),
            Some("local-thread-1"),
            1000.0,
        ),
        cloud_log(
            "device-alpha",
            user,
            42,
            "info",
            "model",
            "model_call",
            None,
            None,
            now,
        ),
    ];
    let result = storage.insert_cloud_device_logs(&batch).unwrap();
    assert_eq!(result.accepted, 2);
    assert_eq!(result.last_seq, Some(42));

    // Full replay is fully deduplicated while still advancing the cursor.
    let replay = storage.insert_cloud_device_logs(&batch).unwrap();
    assert_eq!(replay.accepted, 0);
    assert_eq!(replay.last_seq, Some(42));

    // Partial overlap: only the new seq lands, the request max wins.
    let overlap = vec![
        batch[1].clone(),
        cloud_log(
            "device-alpha",
            user,
            43,
            "error",
            "chat",
            "turn_failed",
            Some("upstream timeout (redacted)"),
            Some("local-thread-1"),
            now,
        ),
    ];
    let result = storage.insert_cloud_device_logs(&overlap).unwrap();
    assert_eq!(result.accepted, 1);
    assert_eq!(result.last_seq, Some(43));

    // Empty batch reports nothing.
    let result = storage.insert_cloud_device_logs(&[]).unwrap();
    assert_eq!(result.accepted, 0);
    assert_eq!(result.last_seq, None);

    // Resume cursors per device.
    assert_eq!(
        storage.latest_cloud_device_log_seq("device-alpha").unwrap(),
        43
    );
    assert_eq!(
        storage.latest_cloud_device_log_seq("device-beta").unwrap(),
        0
    );

    // Log pagination (seq desc) and filters.
    let (logs, total) = storage
        .list_cloud_device_logs(ListCloudDeviceLogsQuery {
            user_id: Some(user),
            device_id: Some("device-alpha"),
            level: None,
            category: None,
            offset: 0,
            limit: 2,
        })
        .unwrap();
    assert_eq!(total, 3);
    assert_eq!(logs[0].seq, 43);
    assert_eq!(logs[1].seq, 42);
    let (logs, total) = storage
        .list_cloud_device_logs(ListCloudDeviceLogsQuery {
            user_id: Some(user),
            device_id: None,
            level: Some("warn"),
            category: Some("tool"),
            offset: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(total, 1);
    assert_eq!(logs[0].seq, 41);
    assert_eq!(logs[0].message.as_deref(), Some("exit code 2 (redacted)"));

    // Retention: zero days disables cleanup entirely.
    assert_eq!(storage.cleanup_cloud_device_logs(0).unwrap(), 0);
    // One day of retention removes only the stale row (created_at 1000.0).
    assert_eq!(storage.cleanup_cloud_device_logs(1).unwrap(), 1);
    assert_eq!(
        storage.latest_cloud_device_log_seq("device-alpha").unwrap(),
        43
    );
    let (logs, total) = storage
        .list_cloud_device_logs(ListCloudDeviceLogsQuery {
            user_id: Some(user),
            device_id: Some("device-alpha"),
            level: None,
            category: None,
            offset: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!(total, 2);
    assert_eq!(logs[0].seq, 43);
}

#[test]
fn sqlite_cloud_device_call_and_log_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.db").to_string_lossy().into_owned();
    exercise_cloud(Arc::new(SqliteStorage::new(path.clone())));

    // Everything survives a full reopen of the database.
    let storage = SqliteStorage::new(path);
    storage.ensure_initialized().unwrap();
    let device = storage.get_cloud_device("device-alpha").unwrap().unwrap();
    assert_eq!(device.client, "desktop");
    assert_eq!(
        storage.latest_cloud_device_log_seq("device-alpha").unwrap(),
        43
    );
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_cloud_device_call_and_log_lifecycle() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_cloud(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}
