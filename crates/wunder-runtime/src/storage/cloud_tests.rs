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
        interlink: None,
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
        interlink: None,
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

// ---------------------------------------------------------------------------
// Interlink: cloud-device extension, channels, shadows, commands, approvals
// ---------------------------------------------------------------------------

fn interlink_device(
    device_id: &str,
    user_id: &str,
    name: &str,
    last_seen_at: f64,
) -> CloudDeviceRecord {
    CloudDeviceRecord {
        device_id: device_id.to_string(),
        user_id: user_id.to_string(),
        client: "desktop".to_string(),
        name: name.to_string(),
        os: Some("TestOS".to_string()),
        arch: Some("x86_64".to_string()),
        app_version: Some("0.1.0".to_string()),
        last_seen_at,
        created_at: last_seen_at,
        revoked: false,
        interlink: None,
    }
}

/// Every interlink column filled, as the tunnel handshake and the capability
/// projection would write them.
fn full_interlink_patch() -> CloudDeviceInterlinkPatch {
    CloudDeviceInterlinkPatch {
        node_secret_hash: Some("node-secret-digest-v1".to_string()),
        secret_version: 1,
        interlink_enabled: Some(true),
        capabilities: Some(r#"["query.basic","thread.drive"]"#.to_string()),
        policy_overrides: Some(r#"{"workspace.write":"deny"}"#.to_string()),
        tunnel_connected: Some(true),
        last_tunnel_at: Some(1_700_000_000.5),
        secret_rotated_at: Some(1_700_000_001.25),
    }
}

/// The defaults a row gets from the schema alone: no secret, no capabilities,
/// version 0 and an explicitly closed tunnel.
fn default_interlink_patch() -> CloudDeviceInterlinkPatch {
    CloudDeviceInterlinkPatch {
        node_secret_hash: None,
        secret_version: 0,
        interlink_enabled: None,
        capabilities: None,
        policy_overrides: None,
        tunnel_connected: Some(false),
        last_tunnel_at: None,
        secret_rotated_at: None,
    }
}

fn read_patch(record: &CloudDeviceRecord) -> CloudDeviceInterlinkPatch {
    *record
        .interlink
        .clone()
        .expect("the device reader always projects the interlink extension")
}

fn exercise_interlink_device(storage: Arc<dyn StorageBackend>) {
    let user = "interlink_user";
    storage
        .upsert_cloud_device(&interlink_device("device-gamma", user, "node-gamma", 100.0))
        .unwrap();
    storage
        .upsert_cloud_device(&interlink_device(
            "device-epsilon",
            user,
            "node-epsilon",
            90.0,
        ))
        .unwrap();

    // A device that never received a patch still reads back sane defaults from
    // every reader path.
    let untouched = storage.get_cloud_device("device-epsilon").unwrap().unwrap();
    assert_eq!(read_patch(&untouched), default_interlink_patch());
    let via_identity = storage
        .find_cloud_device_by_identity(user, "desktop", "node-epsilon")
        .unwrap()
        .unwrap();
    assert_eq!(read_patch(&via_identity), default_interlink_patch());

    // A patch for an unknown device is a bounded no-op, never an error.
    storage
        .update_cloud_device_interlink("device-missing", &full_interlink_patch())
        .unwrap();
    storage
        .update_cloud_device_interlink("", &full_interlink_patch())
        .unwrap();

    // Full patch survives all three readers.
    let expected = full_interlink_patch();
    storage
        .update_cloud_device_interlink("device-gamma", &expected)
        .unwrap();
    let patched = storage.get_cloud_device("device-gamma").unwrap().unwrap();
    assert_eq!(read_patch(&patched), expected);
    let via_identity = storage
        .find_cloud_device_by_identity(user, "desktop", "node-gamma")
        .unwrap()
        .unwrap();
    assert_eq!(read_patch(&via_identity), expected);
    let (rows, total) = storage.list_cloud_devices(Some(user), 0, 10).unwrap();
    assert_eq!(total, 2);
    let listed = rows
        .iter()
        .find(|record| record.device_id == "device-gamma")
        .unwrap();
    assert_eq!(read_patch(listed), expected);
    // The list keeps base fields and the extension side by side.
    assert_eq!(listed.last_seen_at, 100.0);
    assert!(!listed.revoked);

    // An empty patch touches nothing (no SET clause is built).
    storage
        .update_cloud_device_interlink("device-gamma", &CloudDeviceInterlinkPatch::default())
        .unwrap();
    assert_eq!(
        read_patch(&storage.get_cloud_device("device-gamma").unwrap().unwrap()),
        expected
    );

    // Tunnel-only patch: heartbeat writes must not clobber the secret, the
    // capability set, the policy overrides or the rotation instant.
    let mut after_tunnel = expected.clone();
    after_tunnel.tunnel_connected = Some(false);
    after_tunnel.last_tunnel_at = Some(1_700_000_060.0);
    storage
        .update_cloud_device_interlink(
            "device-gamma",
            &CloudDeviceInterlinkPatch {
                tunnel_connected: Some(false),
                last_tunnel_at: Some(1_700_000_060.0),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        read_patch(&storage.get_cloud_device("device-gamma").unwrap().unwrap()),
        after_tunnel
    );

    // Rotation patch: new digest, bumped version, fresh rotation instant;
    // capabilities and policy overrides still survive.
    let mut after_rotation = after_tunnel.clone();
    after_rotation.node_secret_hash = Some("node-secret-digest-v2".to_string());
    after_rotation.secret_version = 2;
    after_rotation.secret_rotated_at = Some(1_700_000_120.0);
    storage
        .update_cloud_device_interlink(
            "device-gamma",
            &CloudDeviceInterlinkPatch {
                node_secret_hash: Some("node-secret-digest-v2".to_string()),
                secret_version: 2,
                secret_rotated_at: Some(1_700_000_120.0),
                ..Default::default()
            },
        )
        .unwrap();
    let rotated = storage.get_cloud_device("device-gamma").unwrap().unwrap();
    assert_eq!(read_patch(&rotated), after_rotation);
    assert_eq!(
        rotated
            .interlink
            .as_deref()
            .and_then(|p| p.capabilities.as_deref()),
        Some(r#"["query.basic","thread.drive"]"#)
    );
    // A re-login never resets the extension either.
    storage
        .upsert_cloud_device(&interlink_device("device-gamma", user, "node-gamma", 120.0))
        .unwrap();
    let reregistered = storage.get_cloud_device("device-gamma").unwrap().unwrap();
    assert_eq!(read_patch(&reregistered), after_rotation);
    assert_eq!(reregistered.last_seen_at, 120.0);
}

fn interlink_channel(
    channel_id: &str,
    device_id: &str,
    user_id: &str,
    connected_at: f64,
) -> InterlinkChannelRecord {
    InterlinkChannelRecord {
        channel_id: channel_id.to_string(),
        device_id: device_id.to_string(),
        user_id: user_id.to_string(),
        client: "desktop".to_string(),
        instance_id: "instance-a".to_string(),
        protocol_version: 1,
        caps: Some(r#"["query.basic"]"#.to_string()),
        connected_at,
        last_seen_at: connected_at,
        rtt_ms: Some(25),
        resumed_count: 0,
        closed_reason: None,
    }
}

fn exercise_interlink_channels_and_shadows(storage: Arc<dyn StorageBackend>) {
    let user = "interlink_user";
    let other = "interlink_other";
    storage
        .upsert_interlink_channel(&interlink_channel("channel-a", "device-gamma", user, 300.0))
        .unwrap();
    storage
        .upsert_interlink_channel(&interlink_channel("channel-b", "device-gamma", user, 200.0))
        .unwrap();
    storage
        .upsert_interlink_channel(&interlink_channel(
            "channel-c",
            "device-delta",
            other,
            100.0,
        ))
        .unwrap();

    let one = storage.get_interlink_channel("channel-a").unwrap().unwrap();
    assert_eq!(one.user_id, user);
    assert_eq!(one.instance_id, "instance-a");
    assert_eq!((one.rtt_ms, one.resumed_count), (Some(25), 0));
    assert!(one.closed_reason.is_none());
    assert!(storage
        .get_interlink_channel("channel-missing")
        .unwrap()
        .is_none());

    // A heartbeat upsert refreshes liveness columns and keeps connected_at.
    let mut beat = one.clone();
    beat.last_seen_at = 350.0;
    beat.rtt_ms = Some(30);
    beat.resumed_count = 2;
    storage.upsert_interlink_channel(&beat).unwrap();
    let current = storage.get_interlink_channel("channel-a").unwrap().unwrap();
    assert_eq!(
        (current.last_seen_at, current.rtt_ms, current.resumed_count),
        (350.0, Some(30), 2)
    );
    assert_eq!(current.connected_at, 300.0);

    // The first close reason wins; a channel is never re-opened by a close.
    storage
        .close_interlink_channel("channel-a", "tunnel_reset")
        .unwrap();
    storage
        .close_interlink_channel("channel-a", "second_close")
        .unwrap();
    storage.close_interlink_channel("", "ignored").unwrap();
    assert_eq!(
        storage
            .get_interlink_channel("channel-a")
            .unwrap()
            .unwrap()
            .closed_reason
            .as_deref(),
        Some("tunnel_reset")
    );

    // Listing is user-scoped, ordered by connection instant and always bounded
    // by the requested limit while the total reports the filtered row count.
    let (rows, total) = storage.list_interlink_channels(Some(user), 0, 10).unwrap();
    assert_eq!((rows.len(), total), (2, 2));
    assert_eq!(rows[0].channel_id, "channel-a");
    let (rows, total) = storage.list_interlink_channels(None, 0, 2).unwrap();
    assert_eq!((rows.len(), total), (2, 3));
    let (rows, total) = storage.list_interlink_channels(None, 2, 2).unwrap();
    assert_eq!((rows.len(), total), (1, 3));
    assert_eq!(rows[0].channel_id, "channel-c");
    let (rows, total) = storage.list_interlink_channels(Some(other), 0, 0).unwrap();
    assert_eq!((rows.len(), total), (0, 1));
    let (rows, total) = storage
        .list_interlink_channels(Some("interlink_nobody"), 0, 10)
        .unwrap();
    assert_eq!((rows.len(), total), (0, 0));

    // Shadows: revision, payload fields, replace and delete.
    assert_eq!(
        storage
            .get_interlink_shadow_revision("device-gamma")
            .unwrap(),
        0
    );
    assert!(storage
        .get_interlink_shadow("device-gamma")
        .unwrap()
        .is_none());
    let shadow = InterlinkShadowRecord {
        device_id: "device-gamma".to_string(),
        user_id: user.to_string(),
        revision: 3,
        summary: Some(r#"{"os":"TestOS"}"#.to_string()),
        threads: Some("[]".to_string()),
        tasks: None,
        workspace: Some(r#"{"files":12}"#.to_string()),
        synced_at: 400.0,
    };
    storage.upsert_interlink_shadow(&shadow).unwrap();
    let stored = storage
        .get_interlink_shadow("device-gamma")
        .unwrap()
        .unwrap();
    assert_eq!(stored.revision, 3);
    assert_eq!(stored.summary.as_deref(), Some(r#"{"os":"TestOS"}"#));
    assert_eq!(stored.threads.as_deref(), Some("[]"));
    assert!(stored.tasks.is_none());
    assert_eq!(stored.synced_at, 400.0);
    assert_eq!(
        storage
            .get_interlink_shadow_revision("device-gamma")
            .unwrap(),
        3
    );

    let mut newer = shadow.clone();
    newer.revision = 4;
    newer.summary = Some(r#"{"os":"TestOS2"}"#.to_string());
    storage.upsert_interlink_shadow(&newer).unwrap();
    assert_eq!(
        storage
            .get_interlink_shadow_revision("device-gamma")
            .unwrap(),
        4
    );

    storage.delete_interlink_shadow("device-gamma").unwrap();
    assert!(storage
        .get_interlink_shadow("device-gamma")
        .unwrap()
        .is_none());
    assert_eq!(
        storage
            .get_interlink_shadow_revision("device-gamma")
            .unwrap(),
        0
    );
    // Deleting a missing or blank shadow is a no-op.
    storage.delete_interlink_shadow("device-missing").unwrap();
    storage.delete_interlink_shadow("").unwrap();
    assert!(storage.get_interlink_shadow("").unwrap().is_none());
    assert_eq!(storage.get_interlink_shadow_revision("").unwrap(), 0);
}

fn interlink_command(
    command_id: &str,
    actor: &str,
    to_node: &str,
    kind: &str,
    status: &str,
    created_at: f64,
) -> InterlinkCommandRecord {
    InterlinkCommandRecord {
        command_id: command_id.to_string(),
        direction: "cloud_to_local".to_string(),
        actor_user_id: actor.to_string(),
        from_node: "cloud".to_string(),
        to_node: to_node.to_string(),
        kind: kind.to_string(),
        args_digest: Some("digest-alpha".to_string()),
        approval_state: "none".to_string(),
        status: status.to_string(),
        created_at,
        acked_at: None,
        finished_at: None,
        error_code: None,
        error_summary: None,
    }
}

fn exercise_interlink_commands(storage: Arc<dyn StorageBackend>) {
    let user = "interlink_user";
    let now = now_ts();
    let aged = now - 10.0 * 86_400.0;

    // Insert is idempotent on command_id: a network retry never stores twice.
    assert!(storage
        .insert_interlink_command(&interlink_command(
            "cmd-1",
            user,
            "device-gamma",
            "thread.send",
            "issued",
            now,
        ))
        .unwrap());
    assert!(!storage
        .insert_interlink_command(&interlink_command(
            "cmd-1",
            user,
            "device-gamma",
            "thread.send",
            "issued",
            now,
        ))
        .unwrap());
    // A blank id never becomes a row.
    assert!(!storage
        .insert_interlink_command(&interlink_command(
            "",
            user,
            "device-gamma",
            "thread.send",
            "issued",
            now
        ))
        .unwrap());

    assert!(storage
        .insert_interlink_command(&interlink_command(
            "cmd-2",
            user,
            "device-gamma",
            "workspace.read",
            "issued",
            now + 1.0,
        ))
        .unwrap());
    assert!(storage
        .insert_interlink_command(&interlink_command(
            "cmd-3",
            "interlink_other",
            "device-delta",
            "thread.send",
            "issued",
            aged,
        ))
        .unwrap());
    assert!(storage
        .insert_interlink_command(&interlink_command(
            "cmd-4",
            user,
            "device-delta",
            "cron.list",
            "issued",
            aged + 1.0,
        ))
        .unwrap());

    // Status machine: ack fills timestamps, the first terminal state wins.
    storage
        .update_interlink_command_status("cmd-1", "acked", Some(1_000.0), None, None, None)
        .unwrap();
    let acked = storage.get_interlink_command("cmd-1").unwrap().unwrap();
    assert_eq!(acked.status, "acked");
    assert_eq!(acked.acked_at, Some(1_000.0));
    assert!(acked.finished_at.is_none());

    storage
        .update_interlink_command_status(
            "cmd-1",
            "failed",
            Some(2_000.0),
            Some(2_000.0),
            Some("node_busy"),
            Some("inflight limit reached (redacted)"),
        )
        .unwrap();
    storage
        .update_interlink_command_status(
            "cmd-1",
            "succeeded",
            Some(9_000.0),
            Some(9_000.0),
            None,
            None,
        )
        .unwrap();
    let settled = storage.get_interlink_command("cmd-1").unwrap().unwrap();
    assert_eq!(settled.status, "failed");
    assert_eq!(settled.finished_at, Some(2_000.0));
    assert_eq!(settled.acked_at, Some(1_000.0));
    assert_eq!(settled.error_code.as_deref(), Some("node_busy"));
    assert_eq!(
        settled.error_summary.as_deref(),
        Some("inflight limit reached (redacted)")
    );
    // Unknown and blank ids are no-ops on every mutator.
    storage
        .update_interlink_command_status("", "failed", None, None, None, None)
        .unwrap();
    storage
        .update_interlink_command_status("cmd-missing", "failed", None, None, None, None)
        .unwrap();
    assert!(storage
        .get_interlink_command("cmd-missing")
        .unwrap()
        .is_none());
    assert!(storage.get_interlink_command("").unwrap().is_none());

    // The approval column on the ledger is written independently of status.
    storage
        .set_interlink_command_approval("cmd-2", "pending")
        .unwrap();
    storage
        .set_interlink_command_approval("cmd-2", "approved")
        .unwrap();
    assert_eq!(
        storage
            .get_interlink_command("cmd-2")
            .unwrap()
            .unwrap()
            .approval_state,
        "approved"
    );
    storage
        .set_interlink_command_approval("", "denied")
        .unwrap();
    storage
        .set_interlink_command_approval("cmd-missing", "denied")
        .unwrap();
    assert_eq!(
        storage
            .get_interlink_command("cmd-1")
            .unwrap()
            .unwrap()
            .approval_state,
        "none"
    );

    // Filters: each query dimension narrows the same ledger and the total
    // always describes the filtered set, not the page.
    let (rows, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            user_id: Some(user),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (3, 3));
    assert_eq!(rows[0].command_id, "cmd-2");

    let (rows, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            user_id: Some(user),
            device_id: Some("device-gamma"),
            kind: Some("workspace.read"),
            status: Some("issued"),
            direction: Some("cloud_to_local"),
            offset: 0,
            limit: 10,
        })
        .unwrap();
    assert_eq!((rows.len(), total), (1, 1));
    assert_eq!(rows[0].command_id, "cmd-2");
    assert_eq!(rows[0].args_digest.as_deref(), Some("digest-alpha"));

    let (_, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            status: Some("failed"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 1);
    let (_, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            direction: Some("local_to_cloud"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 0);
    let (_, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            device_id: Some("device-delta"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 2);

    // Bounded pages: the limit is respected and offsets past the end are empty.
    let (rows, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            offset: 1,
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (2, 4));
    assert_eq!(rows[0].command_id, "cmd-1");
    let (rows, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            offset: 9,
            limit: 5,
            ..Default::default()
        })
        .unwrap();
    assert!(
        (rows.is_empty() && total == 4),
        "rows={} total={}",
        rows.len(),
        total
    );
    let (rows, total) = storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            user_id: Some(user),
            offset: 0,
            limit: 0,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (0, 3));

    // Retention: zero disables cleanup, otherwise only aged rows leave.
    assert_eq!(storage.cleanup_interlink_commands(0).unwrap(), 0);
    assert_eq!(storage.cleanup_interlink_commands(7).unwrap(), 2);
    assert!(storage.get_interlink_command("cmd-3").unwrap().is_none());
    assert!(storage.get_interlink_command("cmd-4").unwrap().is_none());
    assert!(storage.get_interlink_command("cmd-1").unwrap().is_some());
    assert!(storage.get_interlink_command("cmd-2").unwrap().is_some());
    assert_eq!(storage.cleanup_interlink_commands(7).unwrap(), 0);
}

fn interlink_approval(approval_id: &str, command_id: &str) -> InterlinkApprovalRecord {
    InterlinkApprovalRecord {
        approval_id: approval_id.to_string(),
        command_id: command_id.to_string(),
        device_id: "device-gamma".to_string(),
        user_id: "interlink_user".to_string(),
        prompt: "allow writing one workspace file?".to_string(),
        risk_level: "L2".to_string(),
        state: "pending".to_string(),
        decided_by: None,
        decided_at: None,
        expires_at: 2_000.0,
    }
}

fn exercise_interlink_approvals(storage: Arc<dyn StorageBackend>) {
    assert!(storage
        .get_interlink_approval("approval-1")
        .unwrap()
        .is_none());
    storage
        .insert_interlink_approval(&interlink_approval("approval-1", "cmd-1"))
        .unwrap();
    let pending = storage
        .get_interlink_approval("approval-1")
        .unwrap()
        .unwrap();
    assert_eq!(pending.state, "pending");
    assert_eq!(pending.command_id, "cmd-1");
    assert_eq!(pending.risk_level, "L2");
    assert!(pending.decided_by.is_none());
    assert!(pending.decided_at.is_none());
    assert_eq!(pending.expires_at, 2_000.0);

    // The ticket id is the anchor: a re-insert never rewrites a live decision.
    let mut duplicate = interlink_approval("approval-1", "cmd-1");
    duplicate.prompt = "changed prompt".to_string();
    storage.insert_interlink_approval(&duplicate).unwrap();
    assert_eq!(
        storage
            .get_interlink_approval("approval-1")
            .unwrap()
            .unwrap()
            .prompt,
        "allow writing one workspace file?"
    );

    // Only the first decision is kept; a later one is ignored.
    storage
        .decide_interlink_approval("approval-1", "approved", "approver-a", 1_500.0)
        .unwrap();
    storage
        .decide_interlink_approval("approval-1", "rejected", "approver-b", 1_600.0)
        .unwrap();
    let decided = storage
        .get_interlink_approval("approval-1")
        .unwrap()
        .unwrap();
    assert_eq!(decided.state, "approved");
    assert_eq!(decided.decided_by.as_deref(), Some("approver-a"));
    assert_eq!(decided.decided_at, Some(1_500.0));

    // Blank and unknown ids never error and never create rows.
    storage
        .insert_interlink_approval(&interlink_approval("", "cmd-1"))
        .unwrap();
    assert!(storage.get_interlink_approval("").unwrap().is_none());
    storage
        .decide_interlink_approval("", "approved", "approver-a", 1.0)
        .unwrap();
    storage
        .decide_interlink_approval("approval-missing", "approved", "approver-a", 1.0)
        .unwrap();
    assert!(storage
        .get_interlink_approval("approval-missing")
        .unwrap()
        .is_none());
}

fn interlink_audit(
    actor: &str,
    from_node: Option<&str>,
    to_node: Option<&str>,
    action: &str,
    created_at: f64,
) -> InterlinkAuditRecord {
    InterlinkAuditRecord {
        seq: 0,
        command_id: Some("cmd-1".to_string()),
        approval_id: None,
        actor: actor.to_string(),
        from_node: from_node.map(str::to_string),
        to_node: to_node.map(str::to_string),
        action: action.to_string(),
        detail_digest: Some("digest-audit".to_string()),
        created_at,
    }
}

fn exercise_interlink_audit(storage: Arc<dyn StorageBackend>) {
    let user = "interlink_user";
    let now = now_ts();
    let aged = now - 10.0 * 86_400.0;

    // Append-only: the same action twice is two events with distinct sequences.
    // The list is ordered by the storage-issued seq, so the inserts below are
    // chronological and the newest event is also the last written one.
    storage
        .insert_interlink_audit(&interlink_audit(
            "interlink_other",
            None,
            Some("device-delta"),
            "shadow_sync",
            aged,
        ))
        .unwrap();
    storage
        .insert_interlink_audit(&interlink_audit(
            user,
            Some("cloud"),
            Some("device-gamma"),
            "channel_open",
            now - 3.0,
        ))
        .unwrap();
    storage
        .insert_interlink_audit(&interlink_audit(
            user,
            Some("cloud"),
            Some("device-gamma"),
            "channel_open",
            now - 2.0,
        ))
        .unwrap();
    storage
        .insert_interlink_audit(&interlink_audit(
            user,
            Some("device-gamma"),
            None,
            "command_issued",
            now - 1.0,
        ))
        .unwrap();

    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (4, 4));
    // Newest first with storage-issued sequences.
    assert_eq!(rows[0].action, "command_issued");
    assert!(rows[0].seq > rows[1].seq && rows[1].seq > rows[3].seq);
    assert_eq!(rows[0].command_id.as_deref(), Some("cmd-1"));
    assert_eq!(rows[0].detail_digest.as_deref(), Some("digest-audit"));
    assert!(rows[0].approval_id.is_none());
    assert_eq!(rows[3].from_node, None);

    // Time window (inclusive) and actor/device/action filters.
    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            since: Some(now - 2.0),
            until: Some(now - 1.0),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (2, 2));
    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            since: Some(now + 1.0),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (0, 0));
    let (_, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            user_id: Some(user),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 3);
    let (_, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            device_id: Some("device-delta"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 1);
    let (_, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            action: Some("channel_open"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 2);

    // Bounded pages, combined filters and an out-of-range offset.
    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            user_id: Some(user),
            offset: 1,
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (1, 3));
    assert_eq!(rows[0].action, "channel_open");
    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            offset: 20,
            limit: 5,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (0, 4));
    let (rows, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            user_id: Some("interlink_nobody"),
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!((rows.len(), total), (0, 0));

    // Retention drops only the aged row; zero retention is disabled.
    assert_eq!(storage.cleanup_interlink_audit(0).unwrap(), 0);
    assert_eq!(storage.cleanup_interlink_audit(7).unwrap(), 1);
    assert_eq!(storage.cleanup_interlink_audit(7).unwrap(), 0);
    let (_, total) = storage
        .list_interlink_audit(ListInterlinkAuditQuery {
            offset: 0,
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn sqlite_interlink_device_extension_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.db").to_string_lossy().into_owned();
    exercise_interlink_device(Arc::new(SqliteStorage::new(path.clone())));

    // The interlink columns are real storage: they survive a full reopen.
    let storage = SqliteStorage::new(path);
    storage.ensure_initialized().unwrap();
    let mut rotated = full_interlink_patch();
    rotated.node_secret_hash = Some("node-secret-digest-v2".to_string());
    rotated.secret_version = 2;
    rotated.secret_rotated_at = Some(1_700_000_120.0);
    rotated.tunnel_connected = Some(false);
    rotated.last_tunnel_at = Some(1_700_000_060.0);
    let device = storage.get_cloud_device("device-gamma").unwrap().unwrap();
    assert_eq!(read_patch(&device), rotated);
    assert_eq!(
        read_patch(
            &storage
                .list_cloud_devices(Some("interlink_user"), 0, 10)
                .unwrap()
                .0
                .into_iter()
                .find(|record| record.device_id == "device-gamma")
                .unwrap()
        ),
        rotated
    );
    assert_eq!(
        read_patch(&storage.get_cloud_device("device-epsilon").unwrap().unwrap()),
        default_interlink_patch()
    );
}

#[test]
fn sqlite_interlink_channel_and_shadow_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    exercise_interlink_channels_and_shadows(Arc::new(SqliteStorage::new(
        root.path().join("state.db").to_string_lossy().into_owned(),
    )));
}

#[test]
fn sqlite_interlink_command_ledger_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    exercise_interlink_commands(Arc::new(SqliteStorage::new(
        root.path().join("state.db").to_string_lossy().into_owned(),
    )));
}

#[test]
fn sqlite_interlink_approval_and_audit_lifecycle() {
    let root = tempfile::tempdir().unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
        root.path().join("state.db").to_string_lossy().into_owned(),
    ));
    exercise_interlink_approvals(Arc::clone(&storage));
    exercise_interlink_audit(storage);
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_interlink_device_extension_roundtrip() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_interlink_device(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_interlink_channel_and_shadow_lifecycle() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_interlink_channels_and_shadows(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_interlink_command_ledger_lifecycle() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_interlink_commands(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_interlink_approval_and_audit_lifecycle() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    let storage: Arc<dyn StorageBackend> = Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap());
    exercise_interlink_approvals(Arc::clone(&storage));
    exercise_interlink_audit(storage);
}
