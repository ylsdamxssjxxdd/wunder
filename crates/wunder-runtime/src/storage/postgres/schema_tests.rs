//! Explicit capacity regression against an isolated PostgreSQL schema.
use super::*;
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
fn postgres_capacity_startup_preserves_indexes_and_rows() {
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
    let threads: i64 = std::env::var("WUNDER_CAPACITY_THREADS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(100_000);
    assert!(
        rows >= 100_000 && threads >= 130,
        "capacity fixture must exercise large tables"
    );
    conn.execute("INSERT INTO tool_logs(user_id,session_id,tool,payload,created_time) SELECT 'fixture-user','fixture-thread','fixture-tool','{}',0 FROM generate_series(1,$1::bigint)", &[&rows]).unwrap();
    conn.execute("INSERT INTO artifact_logs(user_id,session_id,kind,payload,created_time) SELECT 'fixture-user','fixture-thread','fixture-kind','{}',0 FROM generate_series(1,$1::bigint)", &[&rows]).unwrap();
    conn.execute("INSERT INTO thread_logs(session_id,user_id,latest_change_seq,created_time,updated_time) SELECT 'fixture-'||lpad(n::text,9,'0'),'fixture-user',1,0,0 FROM generate_series(1,$1::bigint) n", &[&threads]).unwrap();
    conn.execute("INSERT INTO thread_items(session_id,item_id,turn_id,root_turn_id,user_id,item_index,kind,payload,created_time,updated_time,created_seq) SELECT 'fixture-000000001','fixture-item-'||n,'fixture-turn','fixture-turn','fixture-user',n,'tool_call','{\"content\":\"fixture content\",\"args\":{\"value\":\"fixture\"}}',0,0,n FROM generate_series(1,$1::bigint) n", &[&rows]).unwrap();
    conn.execute("INSERT INTO memory_fragments(memory_id,user_id,agent_id,source_session_id,source_round_id,source_type,category,title_l0,summary_l1,content_l2,fact_key,tags,entities,importance,confidence,tier,status,valid_from,created_at,updated_at) SELECT 'fixture-memory-'||n,'fixture-user','fixture-agent','fixture-thread','fixture-round','fixture-source','fixture-category','fixture-title','','','fixture-key','[]','[]',0.6,0.7,'working','active',1,1,1 FROM generate_series(1,$1::bigint) n", &[&rows]).unwrap();
    let before = indexes(&mut conn);
    let version: String = conn
        .query_one(
            "SELECT xmin::text FROM memory_fragments WHERE memory_id='fixture-memory-1'",
            &[],
        )
        .unwrap()
        .get(0);
    let large = samples(&dsn);
    assert_eq!(
        indexes(&mut conn),
        before,
        "ordinary boots must preserve every index OID"
    );
    assert_eq!(
        conn.query_one(
            "SELECT xmin::text FROM memory_fragments WHERE memory_id='fixture-memory-1'",
            &[]
        )
        .unwrap()
        .get::<_, String>(0),
        version,
        "ordinary boots must not rewrite memory data"
    );
    assert_eq!(
        conn.query_one("SELECT COUNT(*) FROM thread_view_heads", &[])
            .unwrap()
            .get::<_, i64>(0),
        0,
        "startup must not discover historical threads"
    );

    // An old BRIN gets one replacement, then an unchanged fingerprint skips it.
    conn.batch_execute("DROP INDEX idx_tool_logs_time; CREATE INDEX idx_tool_logs_time ON tool_logs USING BRIN(created_time); DELETE FROM meta WHERE key='postgres_schema_fingerprint_v1'").unwrap();
    let started = Instant::now();
    PostgresStorage::new(dsn.clone(), 5, 2)
        .unwrap()
        .ensure_initialized_impl()
        .unwrap();
    let upgrade_ms = started.elapsed().as_secs_f64() * 1000.0;
    let migrated = indexes(&mut conn);
    PostgresStorage::new(dsn.clone(), 5, 2)
        .unwrap()
        .ensure_initialized_impl()
        .unwrap();
    assert_eq!(indexes(&mut conn), migrated);
    assert_eq!(conn.query_one("SELECT am.amname FROM pg_class c JOIN pg_am am ON am.oid=c.relam WHERE c.oid='idx_tool_logs_time'::regclass", &[]).unwrap().get::<_,String>(0), "btree");

    // Discovery is bounded independently of the total number of users/threads.
    let first = db.view_pending().unwrap();
    let resumed = PostgresStorage::new(dsn.clone(), 5, 2).unwrap();
    let second = resumed.view_pending().unwrap();
    assert!(
        first.iter().all(|a| second.iter().all(|b| a.1 != b.1)),
        "pending page must rotate"
    );
    assert_eq!(
        conn.query_one("SELECT COUNT(*) FROM thread_view_heads", &[])
            .unwrap()
            .get::<_, i64>(0),
        128
    );
    // An idle poll with discovery completed leaves the transaction read-only.
    conn.batch_execute("UPDATE thread_view_heads SET projection_state='ready'; UPDATE meta SET value='{\"done\":true}' WHERE key='thread_view_discovery_v2'").unwrap();
    let marker: String = conn
        .query_one(
            "SELECT xmin::text FROM meta WHERE key='thread_view_pending_cursor_v1'",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(db.view_pending().unwrap().is_empty());
    assert_eq!(
        conn.query_one(
            "SELECT xmin::text FROM meta WHERE key='thread_view_pending_cursor_v1'",
            &[]
        )
        .unwrap()
        .get::<_, String>(0),
        marker
    );
    // Exercise the real asynchronous maintenance loop against a million-item
    // pending thread, including body/search projection rather than empty pages.
    conn.batch_execute("INSERT INTO thread_turns(session_id,turn_id,user_id,root_turn_id,trigger_kind,user_turn_index,status,payload,created_time,updated_time) VALUES('fixture-000000001','fixture-turn','fixture-user','fixture-turn','user',1,'completed','{}',0,0); UPDATE thread_view_heads SET projection_state='rebuilding' WHERE session_id='fixture-000000001'").unwrap();
    drop(conn);
    let storage = std::sync::Arc::new(db);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (maintenance_duty, batches) = runtime.block_on(async {
        crate::services::thread_view_rebuild::start(storage.clone());
        tokio::time::sleep(Duration::from_secs(12)).await;
        let before = crate::services::thread_view_rebuild::snapshot();
        tokio::time::sleep(Duration::from_secs(60)).await;
        let after = crate::services::thread_view_rebuild::snapshot();
        let duty = (after["work_ms"].as_f64().unwrap() - before["work_ms"].as_f64().unwrap())
            / (after["elapsed_ms"].as_f64().unwrap() - before["elapsed_ms"].as_f64().unwrap());
        (
            duty,
            after["batches"].as_u64().unwrap() - before["batches"].as_u64().unwrap(),
        )
    });
    runtime.shutdown_timeout(Duration::from_secs(5));
    let mut conn = storage.conn().unwrap();
    assert!(
        conn.query_one(
            "SELECT rebuild_after FROM thread_view_heads WHERE session_id='fixture-000000001'",
            &[]
        )
        .unwrap()
        .get::<_, i64>(0)
            > 0,
        "maintenance must make progress"
    );
    assert!(
        conn.query_one("SELECT COUNT(*) FROM thread_body_search", &[])
            .unwrap()
            .get::<_, i64>(0)
            > 0,
        "fixture must exercise search projection"
    );
    let small_p95 = p95(&empty);
    let large_p95 = p95(&large);
    let report = json!({
        "shape":"server", "version":"fixture-capacity",
        "environment":{"storage":"PostgreSQL 16","build":"debug","rows_per_large_table":rows,"historical_threads":threads,"samples":20},
        "raw":{"empty_startup_ms":empty,"large_startup_ms":large,"one_time_upgrade_ms":upgrade_ms,"maintenance_window_s":60,"maintenance_batches":batches},
        "scenarios":{
            "S1":{"metrics":{"storage_startup_p95_ms":large_p95,"capacity_added_p95_ms":(large_p95-small_p95).max(0.0)},"gates":{"index_oids_stable":true,"no_history_scan":true,"no_row_rewrite":true}},
            "S3":{"metrics":{"maintenance_duty_ratio":maintenance_duty},"gates":{"discovery_page_bounded":true,"pending_rotates":true,"durable_resume":true}}
        }
    });
    if let Ok(path) = std::env::var("WUNDER_CAPACITY_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
    // Leave the isolated fixture available after failures for diagnosis.
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
    assert!(
        maintenance_duty <= 0.02,
        "maintenance exceeded its window budget: {maintenance_duty}"
    );
}
