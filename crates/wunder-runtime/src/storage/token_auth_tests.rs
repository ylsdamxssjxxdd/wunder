use super::*;
use std::sync::Arc;

fn exercise_token_auth_snapshot(storage: Arc<dyn StorageBackend>) {
    let suffix = uuid::Uuid::new_v4().simple().to_string();
    let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
    let mut user = UserAccountRecord {
        user_id: format!("fixture-auth-{suffix}"),
        username: format!("fixture-auth-{suffix}"),
        email: None,
        password_hash: "fixture-unused-hash".into(),
        roles: vec!["user".into()],
        status: "active".into(),
        access_level: "A".into(),
        unit_id: None,
        quota_balance: 7,
        quota_granted_total: 7,
        quota_used_total: 0,
        last_quota_grant_date: None,
        experience_total: 0,
        is_demo: true,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    };
    storage.upsert_user_account(&user).unwrap();
    let record = UserTokenRecord {
        token: format!("fixture-token-{suffix}"),
        user_id: user.user_id.clone(),
        session_scope: "default".into(),
        family_id: None,
        expires_at: now + 3600.0,
        created_at: now,
        last_used_at: now,
    };
    storage.create_user_token(&record).unwrap();
    let loaded = storage
        .token_auth_snapshot(&record.token, now)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.0.user_id, user.user_id);
    assert_eq!(loaded.1.quota_balance, 7);
    assert!(storage
        .token_auth_snapshot("fixture-missing", now)
        .unwrap()
        .is_none());
    let mut expired = record.clone();
    expired.token = format!("fixture-expired-{suffix}");
    expired.expires_at = now;
    storage.create_user_token(&expired).unwrap();
    assert!(storage
        .token_auth_snapshot(&expired.token, now)
        .unwrap()
        .is_none());
    assert!(storage.get_user_token(&expired.token).unwrap().is_none());
    storage
        .upsert_user_session_scope(&UserSessionScopeRecord {
            user_id: user.user_id.clone(),
            session_scope: "other".into(),
            last_login_at: now + 10.0,
        })
        .unwrap();
    assert!(storage
        .token_auth_snapshot(&record.token, now)
        .unwrap()
        .is_some());
    storage
        .upsert_user_session_scope(&UserSessionScopeRecord {
            user_id: user.user_id.clone(),
            session_scope: record.session_scope.clone(),
            last_login_at: now + 1.0,
        })
        .unwrap();
    assert!(storage
        .token_auth_snapshot(&record.token, now)
        .unwrap()
        .is_none());
    assert!(storage.get_user_token(&record.token).unwrap().is_none());
    storage.create_user_token(&record).unwrap();
    storage
        .upsert_user_session_scope(&UserSessionScopeRecord {
            user_id: user.user_id.clone(),
            session_scope: record.session_scope.clone(),
            last_login_at: now,
        })
        .unwrap();
    user.status = "disabled".into();
    storage.upsert_user_account(&user).unwrap();
    assert!(storage
        .token_auth_snapshot(&record.token, now)
        .unwrap()
        .is_none());
    user.status = "active".into();
    storage.upsert_user_account(&user).unwrap();
    assert!(storage
        .token_auth_snapshot(&record.token, now)
        .unwrap()
        .is_some());
    let store = crate::user_store::UserStore::new(storage.clone());
    assert!(store
        .authenticate_token_details(&record.token)
        .unwrap()
        .is_some());
    let mut bound = record.clone();
    bound.token = format!("fixture-bound-{suffix}");
    bound.family_id = Some(format!("fixture-family-{suffix}"));
    storage.create_user_token(&bound).unwrap();
    let device = CloudDeviceRecord {
        device_id: format!("fixture-device-{suffix}"),
        user_id: user.user_id.clone(),
        client: "desktop".into(),
        name: "fixture-device".into(),
        os: None,
        arch: None,
        app_version: None,
        last_seen_at: now,
        created_at: now,
        revoked: false,
        interlink: None,
    };
    storage.upsert_cloud_device(&device).unwrap();
    storage
        .bind_user_session_device(&UserSessionDeviceRecord {
            family_id: bound.family_id.clone().unwrap(),
            user_id: user.user_id.clone(),
            device_id: device.device_id.clone(),
        })
        .unwrap();
    assert!(store
        .authenticate_token_details(&bound.token)
        .unwrap()
        .is_some());
    storage
        .set_cloud_device_revoked(&device.device_id, true)
        .unwrap();
    assert!(store
        .authenticate_token_details(&bound.token)
        .unwrap()
        .is_none());
    storage.delete_user_token(&bound.token).unwrap();
    storage.delete_user_token(&record.token).unwrap();
    assert!(store
        .authenticate_token_details(&record.token)
        .unwrap()
        .is_none());
    storage.delete_user_account(&user.user_id).unwrap();
}

#[test]
fn sqlite_token_auth_snapshot_observes_revocation_and_scope_generation() {
    let directory = tempfile::tempdir().unwrap();
    exercise_token_auth_snapshot(Arc::new(SqliteStorage::new(
        directory
            .path()
            .join("auth.db")
            .to_string_lossy()
            .into_owned(),
    )));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated WUNDER_TEST_POSTGRES_DSN"]
fn postgres_token_auth_snapshot_observes_revocation_and_scope_generation() {
    exercise_token_auth_snapshot(Arc::new(
        PostgresStorage::new(std::env::var("WUNDER_TEST_POSTGRES_DSN").unwrap(), 5, 8).unwrap(),
    ));
}
