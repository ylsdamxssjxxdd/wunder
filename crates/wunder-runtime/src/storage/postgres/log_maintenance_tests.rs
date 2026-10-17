//! Opt-in integration checks; every test owns a separate PostgreSQL schema.
use super::super::*;
use super::{begin, delete_rows_all, lock_usage, PurgeBudget};
use crate::storage::{USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY};
use std::time::Instant;

struct Fixture {
    db: PostgresStorage,
    admin: PostgresStorage,
    schema: String,
}

impl Fixture {
    fn new() -> Self {
        let dsn =
            std::env::var("WUNDER_TEST_POSTGRES_DSN").expect("isolated test database DSN required");
        assert!(!dsn.contains("://"), "use a key/value PostgreSQL DSN");
        let admin = PostgresStorage::new(dsn.clone(), 5, 4).unwrap();
        let schema = format!("fixture_log_maintenance_{}", uuid::Uuid::new_v4().simple());
        admin
            .conn()
            .unwrap()
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .unwrap();
        let db =
            PostgresStorage::new(format!("{dsn} options='-c search_path={schema}'"), 5, 4).unwrap();
        // Minimal fixture, without the application's unrelated schema/bootstrap.
        db.assume_schema_initialized();
        db.conn().unwrap().batch_execute(
            "CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL,updated_time DOUBLE PRECISION NOT NULL);
             CREATE TABLE user_usage_summary(user_id TEXT PRIMARY KEY,chat_records BIGINT NOT NULL DEFAULT 0,
                tool_records BIGINT NOT NULL DEFAULT 0,updated_time DOUBLE PRECISION NOT NULL DEFAULT 0);
             CREATE TABLE thread_logs(session_id TEXT PRIMARY KEY,user_id TEXT NOT NULL);
             CREATE TABLE chat_sessions(session_id TEXT PRIMARY KEY);
             CREATE TABLE artifact_logs(session_id TEXT NOT NULL,user_id TEXT NOT NULL,
                created_time DOUBLE PRECISION NOT NULL);
             CREATE TABLE thread_turns(session_id TEXT NOT NULL,user_id TEXT NOT NULL,
                created_time DOUBLE PRECISION NOT NULL);
             CREATE TABLE thread_item_blocks(session_id TEXT NOT NULL,user_id TEXT NOT NULL,
                created_time DOUBLE PRECISION NOT NULL);
             CREATE TABLE thread_log_changes(session_id TEXT NOT NULL,user_id TEXT NOT NULL,
                created_time DOUBLE PRECISION NOT NULL);
             CREATE TABLE thread_items(session_id TEXT NOT NULL,user_id TEXT NOT NULL,item_id TEXT NOT NULL,
                kind TEXT NOT NULL,visibility TEXT NOT NULL DEFAULT 'user',created_time DOUBLE PRECISION NOT NULL,
                PRIMARY KEY(session_id,item_id,created_time)) PARTITION BY RANGE(created_time);
             CREATE TABLE thread_items_first PARTITION OF thread_items FOR VALUES FROM (0) TO (1000);
             CREATE TABLE thread_items_second PARTITION OF thread_items FOR VALUES FROM (1000) TO (2000);
             CREATE INDEX ON thread_items(user_id,session_id,created_time,item_id);
             CREATE TABLE tool_logs(id BIGINT NOT NULL,user_id TEXT NOT NULL,session_id TEXT NOT NULL,
                created_time DOUBLE PRECISION NOT NULL,excluded BOOLEAN NOT NULL DEFAULT FALSE,
                PRIMARY KEY(id,created_time)) PARTITION BY RANGE(created_time);
             CREATE TABLE tool_logs_first PARTITION OF tool_logs FOR VALUES FROM (0) TO (1000);
             CREATE TABLE tool_logs_second PARTITION OF tool_logs FOR VALUES FROM (1000) TO (2000);
             CREATE INDEX ON tool_logs(user_id,session_id,id);
             CREATE TABLE deleted_session_log_grace(user_id TEXT NOT NULL,session_id TEXT NOT NULL,
                deleted_at DOUBLE PRECISION NOT NULL,PRIMARY KEY(user_id,session_id));"
        ).unwrap();
        for table in [
            "thread_log_metrics",
            "thread_body_search",
            "thread_body_manifests",
            "thread_body_chunks",
            "thread_chat_rows",
            "thread_trajectory_rows",
            "terminal_transcript_blocks",
            "thread_trajectory_tiles",
            "thread_view_heads",
        ] {
            db.conn()
                .unwrap()
                .batch_execute(&format!(
                "CREATE TABLE {table}(session_id TEXT NOT NULL,user_id TEXT NOT NULL,value BIGINT)"
            ))
                .unwrap();
        }
        Self { db, admin, schema }
    }

    fn count(&self, table: &str) -> i64 {
        self.db
            .conn()
            .unwrap()
            .query_one(&format!("SELECT COUNT(*) FROM {table}"), &[])
            .unwrap()
            .get(0)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.admin
            .conn()
            .unwrap()
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .unwrap();
    }
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_resumes_batches_and_preserves_other_sessions() {
    let f = Fixture::new();
    f.db.conn().unwrap().batch_execute(
        "INSERT INTO thread_logs VALUES('fixture-target','fixture-user'),('fixture-keep','fixture-user');
         INSERT INTO thread_items SELECT 'fixture-target','fixture-user','item-'||n,'user_message','user',
            CASE WHEN n%2=0 THEN 100.0 ELSE 1100.0 END FROM generate_series(1,4500) n;
         INSERT INTO thread_items VALUES('fixture-keep','fixture-user','keep','assistant_message','user',1100);
         INSERT INTO tool_logs SELECT n,'fixture-user','fixture-target',
            CASE WHEN n%2=0 THEN 100.0 ELSE 1100.0 END,n%10=0 FROM generate_series(1,4500) n;
         INSERT INTO tool_logs VALUES(5000,'fixture-user','fixture-keep',1100,FALSE);
         INSERT INTO thread_body_chunks SELECT 'fixture-target','fixture-user',n FROM generate_series(1,4500) n;
         INSERT INTO thread_body_chunks VALUES('fixture-keep','fixture-user',9999);
         INSERT INTO deleted_session_log_grace VALUES('fixture-user','fixture-target',0);"
    ).unwrap();
    f.db.reconcile_user_usage_impl(true).unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (4501, 4051)
    );
    assert!(f
        .db
        .delete_tool_logs_by_session_impl("fixture-user", "fixture-target")
        .is_err());
    assert!(f.count("tool_logs") > 1);
    f.db.delete_tool_logs_by_session_impl("fixture-user", "fixture-target")
        .unwrap();
    assert_eq!(f.count("tool_logs"), 1);
    // An incremental pass after removal of the source maximum must neither
    // regress watermarks nor count an unchanged surviving row a second time.
    assert!(f
        .db
        .delete_thread_log_by_session_impl("fixture-user", "fixture-target")
        .is_err());
    assert!(f.count("thread_items") > 1);
    for _ in 0..5 {
        if f.db
            .delete_thread_log_by_session_impl("fixture-user", "fixture-target")
            .is_ok()
        {
            break;
        }
    }
    assert_eq!(f.count("thread_items"), 1);
    assert_eq!(f.count("thread_body_chunks"), 1);
    assert_eq!(f.count("thread_logs"), 1);
    f.db.reconcile_user_usage_impl(false).unwrap();
    f.db.reconcile_user_usage_impl(false).unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (1, 1)
    );
    assert_eq!(
        f.count("deleted_session_log_grace"),
        1,
        "storage slices must never clear retry markers"
    );
    f.db.conn().unwrap().batch_execute("INSERT INTO tool_logs VALUES(5001,'fixture-user','fixture-keep',1101,FALSE);
        INSERT INTO thread_items VALUES('fixture-keep','fixture-user','next','user_message','user',1101)").unwrap();
    f.db.reconcile_user_usage_impl(false).unwrap();
    f.db.reconcile_user_usage_impl(false).unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (2, 2)
    );
    assert_eq!(
        f.db.conn()
            .unwrap()
            .query_one("SHOW lock_timeout", &[])
            .unwrap()
            .get::<_, String>(0),
        "0"
    );
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_yields_on_summary_lock_and_reconcile() {
    let f = Fixture::new();
    f.db.conn().unwrap().batch_execute("INSERT INTO thread_logs VALUES('fixture-target','fixture-user');
        INSERT INTO thread_items VALUES('fixture-target','fixture-user','item','user_message','user',100);
        INSERT INTO tool_logs VALUES(1,'fixture-user','fixture-target',100,FALSE);
        INSERT INTO deleted_session_log_grace VALUES('fixture-user','fixture-target',0)").unwrap();
    f.db.reconcile_user_usage_impl(true).unwrap();
    let mut holder = f.db.conn().unwrap();
    let mut locked = holder.transaction().unwrap();
    locked
        .execute(
            "UPDATE user_usage_summary SET updated_time=1 WHERE user_id='fixture-user'",
            &[],
        )
        .unwrap();
    let started = Instant::now();
    assert!(f
        .db
        .delete_tool_logs_by_session_impl("fixture-user", "fixture-target")
        .is_err());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        f.count("tool_logs"),
        1,
        "counter failure must roll back the DELETE"
    );
    assert!(f
        .db
        .delete_thread_log_by_session_impl("fixture-user", "fixture-target")
        .is_err());
    assert_eq!(f.count("thread_items"), 1);
    locked.commit().unwrap();

    let mut held = begin(&mut holder).unwrap();
    lock_usage(&mut held, true).unwrap();
    let sweep = f
        .db
        .cleanup_orphan_session_logs_impl(200.0)
        .expect("orphan cleanup should defer on a busy usage lock");
    assert!(sweep.deferred);
    assert!(!sweep.completed);
    let started = Instant::now();
    assert!(f
        .db
        .delete_tool_logs_by_session_impl("fixture-user", "fixture-target")
        .is_err());
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "advisory coordination must fail without waiting"
    );
    assert_eq!(f.count("tool_logs"), 1);
    held.commit().unwrap();
    let mut held = begin(&mut holder).unwrap();
    lock_usage(&mut held, false).unwrap();
    assert!(f.db.reconcile_user_usage_impl(false).is_err());
    held.commit().unwrap();
    f.db.delete_tool_logs_by_session_impl("fixture-user", "fixture-target")
        .unwrap();
    f.db.delete_thread_log_by_session_impl("fixture-user", "fixture-target")
        .unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (0, 0)
    );
    assert_eq!(f.count("deleted_session_log_grace"), 1);
    // Explicitly exercise the shared watermark lock entry points as well.
    let mut conn = f.db.conn().unwrap();
    let mut tx = begin(&mut conn).unwrap();
    for key in [USAGE_CHAT_WATERMARK_KEY, USAGE_TOOL_WATERMARK_KEY] {
        super::super::log_stats_store::read_meta_f64(&mut tx, key).unwrap();
    }
    tx.commit().unwrap();
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_partition_row_ids_are_scoped() {
    let f = Fixture::new();
    f.db.conn()
        .unwrap()
        .batch_execute(
            "INSERT INTO tool_logs VALUES
        (1,'fixture-user','fixture-target',100,FALSE),
        (2,'fixture-user','fixture-target',1100,FALSE),
        (3,'fixture-user','fixture-keep',1100,FALSE)",
        )
        .unwrap();
    let removed = delete_rows_all(
        &mut f.db.conn().unwrap(),
        "tool_logs",
        "user_id=$1 AND session_id=$2",
        &[&"fixture-user", &"fixture-target"],
        &mut PurgeBudget::new(),
    )
    .unwrap();
    assert_eq!(removed, 2);
    assert_eq!(f.count("tool_logs"), 1);
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_partial_delete_does_not_report_completion() {
    let f = Fixture::new();
    // Simulate a selected row that cannot be removed on this pass. Returning
    // fewer rows than the batch size must never clear the durable retry marker.
    f.db.conn()
        .unwrap()
        .batch_execute(
            "INSERT INTO thread_body_chunks VALUES('fixture-target','fixture-user',1);
         CREATE FUNCTION fixture_keep_row() RETURNS trigger LANGUAGE plpgsql AS
         'BEGIN RETURN NULL; END';
         CREATE TRIGGER fixture_keep BEFORE DELETE ON thread_body_chunks
         FOR EACH ROW EXECUTE FUNCTION fixture_keep_row()",
        )
        .unwrap();
    assert!(delete_rows_all(
        &mut f.db.conn().unwrap(),
        "thread_body_chunks",
        "user_id=$1 AND session_id=$2",
        &[&"fixture-user", &"fixture-target"],
        &mut PurgeBudget::new(),
    )
    .is_err());
    assert_eq!(f.count("thread_body_chunks"), 1);
    f.db.conn()
        .unwrap()
        .batch_execute("DROP TRIGGER fixture_keep ON thread_body_chunks")
        .unwrap();
    assert_eq!(
        delete_rows_all(
            &mut f.db.conn().unwrap(),
            "thread_body_chunks",
            "user_id=$1 AND session_id=$2",
            &[&"fixture-user", &"fixture-target"],
            &mut PurgeBudget::new(),
        )
        .unwrap(),
        1
    );
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_orphan_watermark_waits_for_every_row() {
    let f = Fixture::new();
    f.db.conn().unwrap().batch_execute(
        "INSERT INTO thread_logs SELECT 'fixture-orphan-'||n,'fixture-user' FROM generate_series(1,4500) n;
         INSERT INTO thread_items SELECT 'fixture-orphan-'||n,'fixture-user','item','user_message','user',100
            FROM generate_series(1,4500) n;
         INSERT INTO tool_logs SELECT n,'fixture-user','fixture-orphan-'||n,100,FALSE
            FROM generate_series(1,4500) n;
         INSERT INTO thread_log_changes SELECT 'fixture-orphan-'||n,'fixture-user',100
            FROM generate_series(1,10) n;
         INSERT INTO chat_sessions VALUES('fixture-live');
         INSERT INTO deleted_session_log_grace VALUES('fixture-user','fixture-grace',0);
         INSERT INTO thread_logs VALUES('fixture-live','fixture-user'),('fixture-grace','fixture-user');
         INSERT INTO thread_items VALUES('fixture-live','fixture-user','item','user_message','user',100),
            ('fixture-grace','fixture-user','item','user_message','user',100);
         INSERT INTO tool_logs VALUES(5000,'fixture-user','fixture-live',100,FALSE),
            (5001,'fixture-user','fixture-grace',100,FALSE)"
    ).unwrap();
    // One projection holds its rows back: the interval keeps them, so the
    // watermark must stay put even though other tables are already empty.
    f.db.conn()
        .unwrap()
        .batch_execute(
            "CREATE FUNCTION fixture_keep_row() RETURNS trigger LANGUAGE plpgsql AS
         'BEGIN RETURN NULL; END';
         CREATE TRIGGER fixture_keep BEFORE DELETE ON thread_log_changes
         FOR EACH ROW EXECUTE FUNCTION fixture_keep_row()",
        )
        .unwrap();
    f.db.reconcile_user_usage_impl(true).unwrap();
    let partial = f.db.cleanup_orphan_session_logs_impl(200.0).unwrap();
    assert!(
        !partial.completed,
        "an unfinished interval is progress, not success"
    );
    assert_eq!(
        f.db.conn()
            .unwrap()
            .query_one(
                "SELECT COUNT(*) FROM meta WHERE key='orphan_sweep_time_wm_v2'",
                &[],
            )
            .unwrap()
            .get::<_, i64>(0),
        0,
        "unfinished intervals must not advance"
    );
    f.db.conn()
        .unwrap()
        .batch_execute("DROP TRIGGER fixture_keep ON thread_log_changes")
        .unwrap();
    let mut completed = false;
    for _ in 0..5 {
        if f.db
            .cleanup_orphan_session_logs_impl(200.0)
            .unwrap()
            .completed
        {
            completed = true;
            break;
        }
    }
    assert!(
        completed,
        "sweep must reach the cutoff once rows are deletable"
    );
    assert_eq!(f.count("thread_items"), 2);
    assert_eq!(f.count("tool_logs"), 2);
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (2, 2)
    );
    assert_eq!(
        f.db.conn()
            .unwrap()
            .query_one(
                "SELECT value::double precision FROM meta WHERE key='orphan_sweep_time_wm_v2'",
                &[],
            )
            .unwrap()
            .get::<_, f64>(0),
        200.0
    );
}

#[test]
#[ignore = "requires WUNDER_TEST_POSTGRES_DSN; owns an isolated schema"]
fn postgres_log_maintenance_watermarks_share_the_count_snapshot() {
    let f = Fixture::new();
    // Add rows after source aggregation but before summary writes finish.
    // A later, independent MAX query would incorrectly skip these rows.
    f.db.conn().unwrap().batch_execute(
        "INSERT INTO thread_logs VALUES('fixture-target','fixture-user');
         INSERT INTO thread_items VALUES('fixture-target','fixture-user','first','user_message','user',100);
         INSERT INTO tool_logs VALUES(1,'fixture-user','fixture-target',100,FALSE);
         CREATE FUNCTION fixture_write_during_summary() RETURNS trigger LANGUAGE plpgsql AS
         'BEGIN
            INSERT INTO thread_items VALUES(''fixture-target'',''fixture-user'',''next'',''user_message'',''user'',101);
            INSERT INTO tool_logs VALUES(2,''fixture-user'',''fixture-target'',101,FALSE);
            RETURN NEW;
          END';
         CREATE TRIGGER fixture_summary_write AFTER INSERT ON user_usage_summary
         FOR EACH ROW EXECUTE FUNCTION fixture_write_during_summary()",
    ).unwrap();
    f.db.reconcile_user_usage_impl(true).unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (1, 1)
    );
    f.db.conn()
        .unwrap()
        .batch_execute("DROP TRIGGER fixture_summary_write ON user_usage_summary")
        .unwrap();
    f.db.reconcile_user_usage_impl(false).unwrap();
    f.db.reconcile_user_usage_impl(false).unwrap();
    assert_eq!(
        f.db.get_user_usage_summary_impl().unwrap()["fixture-user"],
        (2, 2)
    );
}
