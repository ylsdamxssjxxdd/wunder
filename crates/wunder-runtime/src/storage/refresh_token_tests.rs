//! Refresh-token storage semantics: rotation records, family revocation and
//! retention cleanup, exercised against both storage backends.
use super::*;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_secs_f64()
}

fn refresh_record(token: &str, user: &str, family: &str, now: f64) -> UserRefreshTokenRecord {
    UserRefreshTokenRecord {
        refresh_token: token.to_string(),
        user_id: user.to_string(),
        session_scope: "local_desktop".to_string(),
        family_id: family.to_string(),
        expires_at: now + 30.0 * 24.0 * 3600.0,
        created_at: now,
        last_used_at: now,
        revoked: false,
    }
}

fn exercise_refresh_tokens(storage: Arc<dyn StorageBackend>) {
    let now = now_ts();
    let user = "refresh_user";

    // Roundtrip keeps the family binding.
    let first = refresh_record("wundr_first", user, "family-a", now);
    storage.create_user_refresh_token(&first).unwrap();
    let loaded = storage
        .get_user_refresh_token("wundr_first")
        .unwrap()
        .expect("first record");
    assert_eq!(loaded.user_id, user);
    assert_eq!(loaded.family_id, "family-a");
    assert!(!loaded.revoked);

    // Rotation marks the old token revoked but keeps the row for reuse
    // detection.
    let second = refresh_record("wundr_second", user, "family-a", now);
    storage.create_user_refresh_token(&second).unwrap();
    storage.revoke_refresh_token("wundr_first").unwrap();
    let rotated = storage
        .get_user_refresh_token("wundr_first")
        .unwrap()
        .expect("rotated row is retained");
    assert!(rotated.revoked, "rotated token must stay flagged");

    // Reuse detection: revoking the family flips every live token of the
    // chain and reports only the newly revoked rows.
    storage
        .create_user_refresh_token(&refresh_record("wundr_third", user, "family-b", now))
        .unwrap();
    let revoked = storage.revoke_refresh_token_family("family-a").unwrap();
    assert_eq!(revoked, 1, "only the live family-a token is newly revoked");
    assert!(
        storage
            .get_user_refresh_token("wundr_second")
            .unwrap()
            .expect("second row")
            .revoked
    );
    assert!(
        !storage
            .get_user_refresh_token("wundr_third")
            .unwrap()
            .expect("third row")
            .revoked
    );
    assert_eq!(storage.revoke_refresh_token_family("family-a").unwrap(), 0);

    // Family-bound access tokens are removed together; unbound and foreign
    // family tokens survive.
    let token = |token: &str, family: Option<&str>| UserTokenRecord {
        token: token.to_string(),
        user_id: user.to_string(),
        session_scope: "local_desktop".to_string(),
        family_id: family.map(str::to_string),
        expires_at: now + 3600.0,
        created_at: now,
        last_used_at: now,
    };
    storage
        .create_user_token(&token("wund_family_a", Some("family-a")))
        .unwrap();
    storage
        .create_user_token(&token("wund_family_a2", Some("family-a")))
        .unwrap();
    storage
        .create_user_token(&token("wund_unbound", None))
        .unwrap();
    storage
        .create_user_token(&token("wund_family_b", Some("family-b")))
        .unwrap();
    let removed = storage.delete_user_tokens_by_family("family-a").unwrap();
    assert_eq!(removed, 2);
    assert!(storage.get_user_token("wund_family_a").unwrap().is_none());
    assert!(storage.get_user_token("wund_family_a2").unwrap().is_none());
    assert!(storage.get_user_token("wund_unbound").unwrap().is_some());
    assert!(storage.get_user_token("wund_family_b").unwrap().is_some());

    // Retention: expired rows and stale revoked rows are deleted; recently
    // revoked rows survive one cleanup window.
    let mut expired = refresh_record("wundr_expired", user, "family-c", now);
    expired.expires_at = now - 100.0;
    storage.create_user_refresh_token(&expired).unwrap();
    let mut stale_revoked = refresh_record("wundr_stale_revoked", user, "family-c", now);
    stale_revoked.expires_at = now + 3600.0;
    stale_revoked.last_used_at = now - 100.0;
    stale_revoked.revoked = true;
    storage.create_user_refresh_token(&stale_revoked).unwrap();
    let mut fresh_revoked = refresh_record("wundr_fresh_revoked", user, "family-c", now);
    fresh_revoked.revoked = true;
    storage.create_user_refresh_token(&fresh_revoked).unwrap();

    let cleaned = storage.cleanup_expired_refresh_tokens(now - 50.0).unwrap();
    assert_eq!(cleaned, 2, "expired and stale revoked rows are removed");
    assert!(storage
        .get_user_refresh_token("wundr_expired")
        .unwrap()
        .is_none());
    assert!(storage
        .get_user_refresh_token("wundr_stale_revoked")
        .unwrap()
        .is_none());
    assert!(storage
        .get_user_refresh_token("wundr_fresh_revoked")
        .unwrap()
        .is_some());
}

#[test]
fn sqlite_refresh_token_rotation_and_family_revocation() {
    let root = tempfile::tempdir().unwrap();
    let path = root
        .path()
        .join("refresh.db")
        .to_string_lossy()
        .into_owned();
    exercise_refresh_tokens(Arc::new(SqliteStorage::new(path)));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_refresh_token_rotation_and_family_revocation() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_refresh_tokens(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}

fn scoped_refresh_record(
    token: &str,
    user: &str,
    scope: &str,
    family: &str,
    revoked: bool,
    now: f64,
) -> UserRefreshTokenRecord {
    UserRefreshTokenRecord {
        refresh_token: token.to_string(),
        user_id: user.to_string(),
        session_scope: scope.to_string(),
        family_id: family.to_string(),
        expires_at: now + 30.0 * 24.0 * 3600.0,
        created_at: now,
        last_used_at: now,
        revoked,
    }
}

fn exercise_revoke_families_by_user_scope(storage: Arc<dyn StorageBackend>) {
    let now = now_ts();
    let access = |token: &str, user: &str, family: Option<&str>| UserTokenRecord {
        token: token.to_string(),
        user_id: user.to_string(),
        session_scope: "local_desktop".to_string(),
        family_id: family.map(str::to_string),
        expires_at: now + 3600.0,
        created_at: now,
        last_used_at: now,
    };

    // user-a, local_desktop: two live families, one of them already holding a
    // rotated (revoked) row; only live rows count as newly revoked.
    storage
        .create_user_token(&access("wund_a1", "user-a", Some("fam-a1")))
        .unwrap();
    storage
        .create_user_refresh_token(&scoped_refresh_record(
            "wundr_a1",
            "user-a",
            "local_desktop",
            "fam-a1",
            false,
            now,
        ))
        .unwrap();
    storage
        .create_user_token(&access("wund_a2", "user-a", Some("fam-a2")))
        .unwrap();
    storage
        .create_user_refresh_token(&scoped_refresh_record(
            "wundr_a2",
            "user-a",
            "local_desktop",
            "fam-a2",
            false,
            now,
        ))
        .unwrap();
    storage
        .create_user_refresh_token(&scoped_refresh_record(
            "wundr_a2_old",
            "user-a",
            "local_desktop",
            "fam-a2",
            true,
            now,
        ))
        .unwrap();
    // Same user, different scope: must stay untouched.
    storage
        .create_user_token(&UserTokenRecord {
            token: "wund_acli".to_string(),
            user_id: "user-a".to_string(),
            session_scope: "local_cli".to_string(),
            family_id: Some("fam-acli".to_string()),
            expires_at: now + 3600.0,
            created_at: now,
            last_used_at: now,
        })
        .unwrap();
    storage
        .create_user_refresh_token(&scoped_refresh_record(
            "wundr_acli",
            "user-a",
            "local_cli",
            "fam-acli",
            false,
            now,
        ))
        .unwrap();
    // Other user: must stay untouched.
    storage
        .create_user_token(&access("wund_b1", "user-b", Some("fam-b1")))
        .unwrap();
    storage
        .create_user_refresh_token(&scoped_refresh_record(
            "wundr_b1",
            "user-b",
            "local_desktop",
            "fam-b1",
            false,
            now,
        ))
        .unwrap();

    let revoked = storage
        .revoke_refresh_token_families_by_user_scope("user-a", "local_desktop")
        .unwrap();
    assert_eq!(revoked, 2, "only live refresh tokens are newly revoked");

    // Kicked refresh rows are retained but flagged, keeping reuse detection.
    for kicked in ["wundr_a1", "wundr_a2", "wundr_a2_old"] {
        let row = storage
            .get_user_refresh_token(kicked)
            .unwrap()
            .unwrap_or_else(|| panic!("{kicked} row must be retained"));
        assert!(row.revoked, "{kicked} must be revoked");
    }
    // Family-bound access tokens of the kicked scope are gone.
    assert!(storage.get_user_token("wund_a1").unwrap().is_none());
    assert!(storage.get_user_token("wund_a2").unwrap().is_none());
    // Other scope and other user survive intact.
    assert!(storage.get_user_token("wund_acli").unwrap().is_some());
    assert!(
        !storage
            .get_user_refresh_token("wundr_acli")
            .unwrap()
            .expect("cli row")
            .revoked
    );
    assert!(storage.get_user_token("wund_b1").unwrap().is_some());
    assert!(
        !storage
            .get_user_refresh_token("wundr_b1")
            .unwrap()
            .expect("user-b row")
            .revoked
    );
    // Idempotent: a second pass finds nothing live to revoke.
    assert_eq!(
        storage
            .revoke_refresh_token_families_by_user_scope("user-a", "local_desktop")
            .unwrap(),
        0
    );
}

#[test]
fn sqlite_revoke_refresh_families_by_user_scope() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("kick.db").to_string_lossy().into_owned();
    exercise_revoke_families_by_user_scope(Arc::new(SqliteStorage::new(path)));
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_revoke_refresh_families_by_user_scope() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    exercise_revoke_families_by_user_scope(Arc::new(PostgresStorage::new(dsn, 5, 8).unwrap()));
}
