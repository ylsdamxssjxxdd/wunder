use crate::storage::*;
use anyhow::Result;

fn seed(mut execute: impl FnMut(&str) -> Result<()>) {
    for id in [
        "cleared", "legacy", "partial", "draft", "live", "queued", "locked", "goal", "other",
    ] {
        let owner = if id == "other" { "user-b" } else { "user-a" };
        let last = if id == "draft" { 20 } else { 80 };
        execute(&format!("INSERT INTO chat_sessions (session_id,user_id,title,status,created_at,updated_at,last_message_at) \
            VALUES ('{id}','{owner}','thread','active',20,80,{last})")).unwrap();
    }
    for (id, at) in [
        ("cleared", 50),
        ("partial", 200),
        ("live", 50),
        ("queued", 50),
        ("locked", 50),
        ("other", 200),
    ] {
        execute(&format!(
            "INSERT INTO chat_history (user_id,session_id,role,payload,created_time) \
            VALUES ('{}','{id}','user','{{}}',{at})",
            if id == "other" { "user-b" } else { "user-a" }
        ))
        .unwrap();
    }
    execute("INSERT INTO monitor_sessions (session_id,user_id,status,updated_time,payload) VALUES ('live','user-a','running',50,'{}')").unwrap();
    execute("INSERT INTO agent_tasks (task_id,thread_id,user_id,agent_id,session_id,status,request_payload,retry_count,retry_at,created_at,updated_at) VALUES ('task-a','thread-a','user-a','','queued','pending','{}',0,50,50,50)").unwrap();
    execute("INSERT INTO session_locks (session_id,user_id,agent_id,created_time,updated_time,expires_at,suspended) VALUES ('locked','user-a','',50,50,0,1)").unwrap();
    execute("INSERT INTO session_goals (session_id,user_id,goal_id,objective,status,tokens_used,time_used_seconds,created_at,updated_at,source) VALUES ('goal','user-a','goal-a','input','active',0,0,50,50,'user')").unwrap();
}

fn verify(storage: &dyn StorageBackend) {
    let deleted = storage.delete_logs_by_time_range(10.0, 100.0).unwrap();
    assert_eq!(deleted.get("chat_sessions"), Some(&2));
    let (items, _) = storage
        .list_chat_sessions_by_status("user-a", None, None, None, 0, 100)
        .unwrap();
    let mut ids = items.into_iter().map(|s| s.session_id).collect::<Vec<_>>();
    ids.sort();
    assert_eq!(
        ids,
        ["draft", "goal", "live", "locked", "partial", "queued"]
    );
    for id in ["live", "locked", "queued", "partial"] {
        assert_eq!(
            storage.load_chat_history("user-a", id, None).unwrap().len(),
            1
        );
    }
    assert_eq!(
        storage
            .delete_logs_by_time_range(10.0, 100.0)
            .unwrap()
            .get("chat_sessions"),
        Some(&0)
    );
    assert_eq!(storage.delete_chat_sessions_by_user("user-a").unwrap(), 6);
    assert_eq!(
        storage
            .list_chat_sessions("user-a", None, None, 0, 100)
            .unwrap()
            .1,
        0
    );
    assert!(storage
        .get_session_goal("user-a", "goal")
        .unwrap()
        .is_none());
    assert!(storage
        .get_chat_session("user-b", "other")
        .unwrap()
        .is_some());
}

#[test]
fn session_cleanup_sqlite_preserves_drafts_live_partial_and_other_users() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cleanup.db");
    let storage = SqliteStorage::new(path.to_string_lossy().into());
    storage.ensure_initialized().unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    seed(|sql| Ok(conn.execute_batch(sql)?));
    verify(&storage);
}

#[test]
fn session_cleanup_sqlite_rolls_back_logs_when_catalog_delete_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cleanup.db");
    let storage = SqliteStorage::new(path.to_string_lossy().into());
    storage.ensure_initialized().unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    seed(|sql| Ok(conn.execute_batch(sql)?));
    conn.execute_batch("CREATE TRIGGER reject_catalog_delete BEFORE DELETE ON chat_sessions BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").unwrap();
    assert!(storage.delete_logs_by_time_range(10.0, 100.0).is_err());
    assert_eq!(
        storage
            .load_chat_history("user-a", "cleared", None)
            .unwrap()
            .len(),
        1
    );
    assert!(storage
        .get_chat_session("user-a", "cleared")
        .unwrap()
        .is_some());
}

#[cfg(feature = "postgres-storage")]
#[test]
#[ignore = "requires an isolated PostgreSQL database via WUNDER_CLEANUP_TEST_DSN"]
fn session_cleanup_postgres_preserves_drafts_live_partial_and_other_users() {
    let dsn = std::env::var("WUNDER_CLEANUP_TEST_DSN").unwrap();
    let storage = PostgresStorage::new(dsn.clone(), 5, 8).unwrap();
    storage.ensure_initialized().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (client, connection) = tokio_postgres::connect(&dsn, tokio_postgres::NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            connection.await.unwrap();
        });
        let mut statements = Vec::new();
        seed(|sql| {
            statements.push(sql.to_owned());
            Ok(())
        });
        for sql in statements {
            client.batch_execute(&sql).await.unwrap();
        }
    });
    verify(&storage);
}
