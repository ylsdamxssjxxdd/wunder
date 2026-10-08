use super::*;
use crate::services::user_store::UserStore;
use std::sync::{Arc, Barrier};

fn exercise_quota(storage: Arc<dyn StorageBackend>) {
    let store = UserStore::new(storage.clone());
    let user = store
        .create_user(
            "quota_user",
            None,
            "test-password",
            None,
            None,
            vec!["user".into()],
            "active",
            false,
        )
        .unwrap();
    let today = UserStore::today_string();
    assert_eq!(
        (
            user.quota_balance,
            user.quota_granted_total,
            user.quota_used_total
        ),
        (1000, 1000, 0)
    );
    storage
        .set_user_quota_balance(&user.user_id, &today, 1000, 1)
        .unwrap();

    // All workers race for the final credit. Only one may contact the provider.
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|_| {
            let storage = storage.clone();
            let barrier = barrier.clone();
            let today = today.clone();
            let user_id = user.user_id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                storage
                    .consume_user_quota(&user_id, &today, 1000, 1)
                    .unwrap()
                    .unwrap()
                    .allowed
            })
        })
        .collect::<Vec<_>>();
    let admitted = handles
        .into_iter()
        .filter_map(|h| h.join().unwrap().then_some(()))
        .count();
    assert_eq!(admitted, 1);
    let current = storage.get_user_account(&user.user_id).unwrap().unwrap();
    assert_eq!(
        (
            current.quota_balance,
            current.quota_granted_total,
            current.quota_used_total
        ),
        (0, 1000, 1)
    );

    // Stale profile snapshots cannot restore credits or erase usage.
    let mut stale = user;
    stale.email = Some("user@example.test".into());
    store.update_user(&stale).unwrap();
    let current = storage.get_user_account(&stale.user_id).unwrap().unwrap();
    assert_eq!(
        (
            current.quota_balance,
            current.quota_used_total,
            current.email
        ),
        (0, 1, stale.email)
    );

    let tomorrow = (chrono::Local::now().date_naive() + chrono::Duration::days(1)).to_string();
    let first = storage
        .prepare_user_quota(&stale.user_id, &tomorrow, 1000)
        .unwrap()
        .unwrap();
    let second = storage
        .prepare_user_quota(&stale.user_id, &tomorrow, 1000)
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            first.balance,
            second.balance,
            second.granted_total,
            second.used_total
        ),
        (1000, 1000, 2000, 1)
    );
    let denied = storage
        .consume_user_quota(&stale.user_id, &tomorrow, 1000, 1001)
        .unwrap()
        .unwrap();
    assert_eq!(
        (denied.allowed, denied.balance, denied.used_total),
        (false, 1000, 1)
    );
    let set = storage
        .set_user_quota_balance(&stale.user_id, &tomorrow, 1000, 0)
        .unwrap()
        .unwrap();
    assert_eq!(
        (set.balance, set.used_total, set.granted_total),
        (0, 1, 2000)
    );
}

#[test]
fn sqlite_quota_concurrent_admission_and_profile_isolation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.db").to_string_lossy().into_owned();
    exercise_quota(Arc::new(SqliteStorage::new(path.clone())));
    let storage = SqliteStorage::new(path);
    storage.ensure_initialized().unwrap();
    let user = storage.get_user_account("quota_user").unwrap().unwrap();
    assert_eq!(
        (
            user.quota_balance,
            user.quota_used_total,
            user.quota_granted_total
        ),
        (0, 1, 2000)
    );
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated database in WUNDER_TEST_POSTGRES_DSN"]
fn postgres_quota_concurrent_admission_and_profile_isolation() {
    let dsn = std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated PostgreSQL test database");
    // Seed the retired schema so this test covers migration as well as admission.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (client, connection) = tokio_postgres::connect(&dsn, tokio_postgres::NoTls).await.unwrap();
        let connection = tokio::spawn(connection);
        client.batch_execute(
            "CREATE TABLE user_accounts (
                user_id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, email TEXT,
                password_hash TEXT NOT NULL, roles TEXT NOT NULL, status TEXT NOT NULL,
                access_level TEXT NOT NULL, unit_id TEXT,
                token_balance BIGINT NOT NULL DEFAULT 0,
                token_granted_total BIGINT NOT NULL DEFAULT 0,
                token_used_total BIGINT NOT NULL DEFAULT 0, last_token_grant_date TEXT,
                daily_quota BIGINT NOT NULL DEFAULT 0, daily_quota_used BIGINT NOT NULL DEFAULT 0,
                daily_quota_date TEXT, experience_total BIGINT NOT NULL DEFAULT 0,
                is_demo INTEGER NOT NULL DEFAULT 0, created_at DOUBLE PRECISION NOT NULL,
                updated_at DOUBLE PRECISION NOT NULL, last_login_at DOUBLE PRECISION
            );
            INSERT INTO user_accounts (user_id, username, password_hash, roles, status,
                access_level, token_balance, created_at, updated_at)
            VALUES ('legacy_user', 'legacy_user', 'hash', '[\"user\"]', 'active', 'A', 90000, 1, 1);"
        ).await.unwrap();
        drop(client);
        connection.await.unwrap().unwrap();
    });
    let initial = PostgresStorage::new(dsn.clone(), 5, 8).unwrap();
    initial.ensure_initialized().unwrap();
    let migrated = initial.get_user_account("legacy_user").unwrap().unwrap();
    assert_eq!(
        (
            migrated.quota_balance,
            migrated.quota_granted_total,
            migrated.quota_used_total
        ),
        (1000, 1000, 0)
    );
    initial
        .consume_user_quota("legacy_user", &UserStore::today_string(), 1000, 1000)
        .unwrap();
    drop(initial);
    exercise_quota(Arc::new(PostgresStorage::new(dsn.clone(), 5, 8).unwrap()));
    let storage = PostgresStorage::new(dsn, 5, 8).unwrap();
    storage.ensure_initialized().unwrap();
    let user = storage.get_user_account("quota_user").unwrap().unwrap();
    assert_eq!(
        (
            user.quota_balance,
            user.quota_used_total,
            user.quota_granted_total
        ),
        (0, 1, 2000)
    );
    let legacy = storage.get_user_account("legacy_user").unwrap().unwrap();
    assert_eq!((legacy.quota_balance, legacy.quota_used_total), (0, 1000));
}
