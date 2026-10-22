//! Explicit control-plane capacity regression against an isolated schema.
use super::*;
use crate::storage::StorageLifecycle;
use serde_json::json;

fn indexes(conn: &mut PgConn<'_>) -> Vec<(String, u32)> {
    conn.query("SELECT c.relname,c.oid FROM pg_class c JOIN pg_index i ON i.indexrelid=c.oid WHERE c.relnamespace=current_schema()::regnamespace ORDER BY c.relname", &[])
        .unwrap().into_iter().map(|row| (row.get(0), row.get(1))).collect()
}

fn samples(dsn: &str) -> Vec<f64> {
    (0..20)
        .map(|_| {
            let storage = PostgresStorage::new(dsn.into(), 5, 2).unwrap();
            let started = Instant::now();
            storage.ensure_initialized_impl().unwrap();
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect()
}

fn p95(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[(sorted.len() * 95).div_ceil(100) - 1]
}

#[test]
#[ignore = "explicit PostgreSQL capacity test; requires WUNDER_TEST_POSTGRES_DSN (key/value format)"]
fn postgres_capacity_startup_preserves_business_rows_without_history_tables() {
    let dsn =
        std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated test database DSN required");
    assert!(
        !dsn.contains("://"),
        "use PostgreSQL key/value DSN for this test"
    );
    let schema = format!("fixture_capacity_{}", std::process::id());
    let admin = PostgresStorage::new(dsn.clone(), 5, 2).unwrap();
    let mut admin_conn = admin.conn().unwrap();
    admin_conn
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .unwrap();
    let dsn = format!("{dsn} options='-c search_path={schema}'");
    let db = PostgresStorage::new(dsn.clone(), 5, 2).unwrap();
    db.ensure_initialized_impl().unwrap();
    let empty = samples(&dsn);
    let mut conn = db.conn().unwrap();
    let rows: i64 = std::env::var("WUNDER_CAPACITY_ROWS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1_000_000);
    assert!(
        rows >= 100_000,
        "capacity fixture must exercise large tables"
    );
    conn.execute("INSERT INTO monitor_sessions(session_id,user_id,status,updated_time,payload) SELECT 'fixture-thread-'||n,'fixture-user','completed',1,'{}' FROM generate_series(1,$1::bigint) n", &[&rows]).unwrap();
    conn.execute("INSERT INTO memory_fragments(memory_id,user_id,agent_id,source_session_id,source_round_id,source_type,category,title_l0,summary_l1,content_l2,fact_key,tags,entities,importance,confidence,tier,status,valid_from,created_at,updated_at) SELECT 'fixture-memory-'||n,'fixture-user','fixture-agent','fixture-thread','fixture-round','fixture-source','fixture-category','fixture-title','','','fixture-key','[]','[]',0.6,0.7,'working','active',1,1,1 FROM generate_series(1,$1::bigint) n", &[&rows]).unwrap();
    let before = indexes(&mut conn);
    let row_version: String = conn
        .query_one(
            "SELECT xmin::text FROM memory_fragments WHERE memory_id='fixture-memory-1'",
            &[],
        )
        .unwrap()
        .get(0);
    let marker_version: String = conn
        .query_one(
            "SELECT xmin::text FROM meta WHERE key=$1",
            &[&SCHEMA_MARKER],
        )
        .unwrap()
        .get(0);
    let large = samples(&dsn);
    assert_eq!(
        indexes(&mut conn),
        before,
        "ordinary boots must preserve index OIDs"
    );
    assert_eq!(
        conn.query_one(
            "SELECT xmin::text FROM memory_fragments WHERE memory_id='fixture-memory-1'",
            &[]
        )
        .unwrap()
        .get::<_, String>(0),
        row_version,
        "ordinary boots must not rewrite business rows"
    );
    assert_eq!(
        conn.query_one("SELECT COUNT(*) FROM monitor_sessions", &[])
            .unwrap()
            .get::<_, i64>(0),
        rows
    );

    // The legacy scheduler hook is deliberately inert, even across a month
    // boundary: rollout storage does not need SQL partitions or marker writes.
    for _ in 0..3 {
        db.ensure_time_partitions_current().unwrap();
    }
    assert_eq!(
        conn.query_one(
            "SELECT xmin::text FROM meta WHERE key=$1",
            &[&SCHEMA_MARKER]
        )
        .unwrap()
        .get::<_, String>(0),
        marker_version
    );
    for table in [
        "chat_history",
        "model_context_entries",
        "thread_logs",
        "thread_turns",
        "thread_items",
        "thread_item_blocks",
        "thread_log_changes",
        "thread_log_metrics",
        "thread_view_heads",
        "thread_chat_rows",
        "thread_trajectory_tiles",
        "thread_body_search",
        "tool_logs",
        "artifact_logs",
        "user_usage_summary",
        "stream_events",
        "deleted_session_log_grace",
    ] {
        assert!(
            !conn
                .query_one("SELECT to_regclass($1) IS NOT NULL", &[&table])
                .unwrap()
                .get::<_, bool>(0),
            "control plane must not create {table}"
        );
    }
    assert!(!conn
        .query_one(
            "SELECT EXISTS(SELECT 1 FROM meta WHERE key='postgres_partition_window_v1')",
            &[]
        )
        .unwrap()
        .get::<_, bool>(0));
    let small_p95 = p95(&empty);
    let large_p95 = p95(&large);
    let report = json!({
        "shape":"server", "version":"fixture-capacity",
        "environment":{"storage":"PostgreSQL","rows_per_business_table":rows,"samples":20},
        "raw":{"empty_startup_ms":empty,"large_startup_ms":large},
        "scenarios":{"startup":{"metrics":{"storage_startup_p95_ms":large_p95,"capacity_added_p95_ms":(large_p95-small_p95).max(0.0)},"gates":{"index_oids_stable":true,"business_rows_unchanged":true,"execution_history_tables_absent":true,"partition_maintenance_inert":true}}}
    });
    if let Ok(path) = std::env::var("WUNDER_CAPACITY_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
    admin_conn
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .unwrap();
    assert!(
        large_p95 < 2000.0,
        "storage startup p95 exceeded 2s: {large_p95}"
    );
    assert!(
        large_p95 - small_p95 < 500.0,
        "capacity growth added more than 500ms"
    );
}
