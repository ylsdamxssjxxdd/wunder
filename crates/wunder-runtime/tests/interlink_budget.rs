//! Performance and boundary budget for the interlink plane (互通方案 §13.6 /
//! §10.1 / §10.2): the named ledger indexes exist, a 1000-row ledger stays
//! inside the P95 query budget, remote subscriptions are capped and aggregated,
//! and the shadow projection honours its entry cap inside the local budget.
//!
//! These are budget assertions, not smoke tests: every one names the number the
//! document commits to, so a changed cap or a lost index goes red here instead
//! of surfacing as a slow production UI.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use wunder_server::{
    config::Config,
    interlink::{
        client::shadow::{NodeIdentity, ShadowSources, TreeLimits, gather_workspace},
        remote,
    },
    stable_core::interlink::CAP_SHADOW_FULL,
    state::{AppState, AppStateInitOptions},
    storage::{
        InterlinkAuditRecord, InterlinkCommandRecord, ListInterlinkAuditQuery,
        ListInterlinkCommandsQuery,
    },
};

const DEVICE_COUNT: usize = 50;
const LEDGER_ROWS: usize = 1_000;
const QUERY_SAMPLES: usize = 25;
/// Docs §13.6: ledger and audit listings stay under 500 ms at P95.
const LEDGER_P95_BUDGET: Duration = Duration::from_millis(500);
/// Docs §10.2: a 500-entry full projection is computed locally in < 500 ms.
const SHADOW_BUDGET: Duration = Duration::from_millis(500);
/// Docs §6.1 entry cap.
const SHADOW_MAX_ENTRIES: usize = 500;
/// Docs §10.1: at most 8 remote subscriptions per node.
const MAX_SUBSCRIBERS: usize = 8;
const BASE_TS: f64 = 1_700_000_000.0;

fn app_state(dir: &TempDir, db_name: &str) -> Arc<AppState> {
    let mut config = Config::default();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = dir.path().join(db_name).to_string_lossy().to_string();
    config.workspace.root = dir
        .path()
        .join("workspaces")
        .to_string_lossy()
        .to_string();
    Arc::new(
        AppState::new_with_options(
            wunder_server::config_store::ConfigStore::new(dir.path().join("wunder.yaml")),
            config,
            AppStateInitOptions::cli_default(),
        )
        .expect("app state"),
    )
}

fn percentile(mut samples: Vec<Duration>, percentile: usize) -> Duration {
    samples.sort_unstable();
    let index = ((samples.len() * percentile) / 100).min(samples.len() - 1);
    samples[index]
}

fn index_names(db_path: &std::path::Path) -> Vec<String> {
    let connection = rusqlite::Connection::open(db_path).expect("open ledger db");
    let mut statement = connection
        .prepare("SELECT name FROM sqlite_master WHERE type = 'index'")
        .expect("prepare index listing");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query indexes");
    rows.filter_map(Result::ok).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn interlink_ledger_and_audit_carry_the_indexes_the_budget_relies_on() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = app_state(&dir, "budget-indexes.db");
    // Any read is enough to have the schema built.
    state
        .storage
        .list_interlink_commands(ListInterlinkCommandsQuery {
            user_id: Some("budget_user"),
            device_id: None,
            kind: None,
            status: None,
            direction: None,
            offset: 0,
            limit: 1,
        })
        .expect("empty ledger listing");

    let indexes = index_names(&dir.path().join("budget-indexes.db"));
    // Docs §13.6 names these two access paths.
    assert!(
        indexes
            .iter()
            .any(|name| name == "idx_interlink_commands_user_created"),
        "ledger needs (user_id, created_at): {indexes:?}"
    );
    assert!(
        indexes
            .iter()
            .any(|name| name == "idx_interlink_audit_created"),
        "audit needs (created_at): {indexes:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ledger_and_audit_listings_stay_inside_the_p95_budget_at_1000_rows() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = app_state(&dir, "budget-ledger.db");
    let storage = state.storage.clone();
    let user = "budget_user".to_string();

    tokio::task::spawn_blocking(move || {
        for index in 0..LEDGER_ROWS {
            // Spread direction and kind over the rows of one device (20 each),
            // not over `index`: with coprime strides a fixed filter combo can
            // easily match zero rows for most devices.
            let within = index / DEVICE_COUNT;
            let c2l = within % 2 == 0;
            let listing = within % 3 == 0;
            let device_id = format!("dev-budget-{}", index % DEVICE_COUNT);
            let target = if c2l {
                format!("device:{device_id}")
            } else {
                "cloud".to_string()
            };
            let record = InterlinkCommandRecord {
                command_id: format!("cmd-budget-{index}"),
                direction: if c2l { "c2l" } else { "l2c" }.to_string(),
                actor_user_id: user.clone(),
                from_node: if c2l {
                    "cloud".to_string()
                } else {
                    format!("device:{device_id}")
                },
                to_node: target,
                kind: if listing {
                    "workspace.list"
                } else {
                    "thread.message"
                }
                .to_string(),
                args_digest: None,
                approval_state: if listing { "none" } else { "approved" }.to_string(),
                status: if index % 7 == 0 { "failed" } else { "succeeded" }.to_string(),
                created_at: BASE_TS + index as f64,
                acked_at: Some(BASE_TS + index as f64),
                finished_at: Some(BASE_TS + index as f64 + 0.5),
                error_code: None,
                error_summary: None,
            };
            storage.insert_interlink_command(&record).expect("insert ledger row");
            storage
                .insert_interlink_audit(&InterlinkAuditRecord {
                    seq: 0,
                    command_id: Some(record.command_id.clone()),
                    approval_id: None,
                    actor: user.clone(),
                    from_node: Some(record.from_node.clone()),
                    to_node: Some(record.to_node.clone()),
                    action: if c2l {
                        "command.issue"
                    } else {
                        "command.finish"
                    }
                    .to_string(),
                    detail_digest: Some("{\"result\":\"succeeded\"}".to_string()),
                    created_at: record.created_at,
                })
                .expect("insert audit row");
        }
    })
    .await
    .expect("seed ledger");

    let storage = state.storage.clone();
    let ledger_timings = tokio::task::spawn_blocking(move || {
        let mut timings = Vec::with_capacity(QUERY_SAMPLES);
        for sample in 0..QUERY_SAMPLES {
            let started = Instant::now();
            let (rows, total) = storage
                .list_interlink_commands(ListInterlinkCommandsQuery {
                    user_id: Some("budget_user"),
                    device_id: Some(&format!("device:dev-budget-{}", sample % DEVICE_COUNT)),
                    kind: Some("workspace.list"),
                    status: None,
                    direction: None,
                    offset: (sample as i64 % 2) * 2,
                    limit: 20,
                })
                .expect("ledger listing");
            assert!(rows.len() <= 20, "page size must be honoured");
            assert!(
                !rows.is_empty() && total >= 4,
                "the seeded device/kind window must be visible: {total}"
            );
            timings.push(started.elapsed());
        }
        timings
    })
    .await
    .expect("ledger timings");
    let ledger_p95 = percentile(ledger_timings, 95);
    assert!(
        ledger_p95 < LEDGER_P95_BUDGET,
        "ledger P95 {ledger_p95:?} exceeds the {LEDGER_P95_BUDGET:?} budget"
    );

    let storage = state.storage.clone();
    let audit_timings = tokio::task::spawn_blocking(move || {
        let mut timings = Vec::with_capacity(QUERY_SAMPLES);
        for sample in 0..QUERY_SAMPLES {
            let started = Instant::now();
            let (rows, _total) = storage
                .list_interlink_audit(ListInterlinkAuditQuery {
                    user_id: Some("budget_user"),
                    device_id: None,
                    action: Some("command.issue"),
                    since: Some(BASE_TS + sample as f64),
                    until: None,
                    offset: 0,
                    limit: 50,
                })
                .expect("audit listing");
            assert!(rows.len() <= 50, "audit page size must be honoured");
            timings.push(started.elapsed());
        }
        timings
    })
    .await
    .expect("audit timings");
    let audit_p95 = percentile(audit_timings, 95);
    assert!(
        audit_p95 < LEDGER_P95_BUDGET,
        "audit P95 {audit_p95:?} exceeds the {LEDGER_P95_BUDGET:?} budget"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_subscriptions_are_capped_per_node_and_aggregated_into_one_upstream() {
    let hub = remote::hub();
    let device = "dev-budget-sub".to_string();
    let thread = "thread-budget".to_string();

    let mut ids = Vec::with_capacity(MAX_SUBSCRIBERS);
    let mut receivers = Vec::with_capacity(MAX_SUBSCRIBERS);
    for slot in 0..MAX_SUBSCRIBERS {
        let (subscription, receiver) = hub
            .subscribe(&device, &thread)
            .expect("subscription within the node cap");
        // §7.4: interest in a thread is announced upstream once, not per reader.
        assert_eq!(subscription.first, slot == 0, "only the first watcher attaches");
        ids.push(subscription.id);
        receivers.push(receiver);
    }

    // §10.1: the ninth watcher of one node is refused instead of growing state.
    assert!(
        hub.subscribe(&device, &thread).is_err(),
        "the node subscription cap must refuse the ninth watcher"
    );

    // One upstream frame fans out to all eight readers: the node never sees
    // eight sends for eight readers.
    let delivered = hub.publish(&device, &thread, "{\"kind\":\"delta\"}");
    assert_eq!(delivered, MAX_SUBSCRIBERS, "one frame fans out to the node cap");
    for receiver in receivers.iter_mut() {
        let frame = tokio::time::timeout(Duration::from_secs(2), receiver.recv())
            .await
            .expect("subscriber receives the aggregated frame")
            .expect("channel open");
        assert_eq!(frame, "{\"kind\":\"delta\"}");
    }
    let watched = hub.watch_list();
    let pairs: Vec<&(String, String, usize)> = watched
        .iter()
        .filter(|entry| entry.0 == device && entry.1 == thread)
        .collect();
    assert_eq!(pairs.len(), 1, "one (node, thread) pair stays aggregated");
    assert_eq!(pairs[0].2, MAX_SUBSCRIBERS, "the pair reports its watcher count");

    // `unsubscribe` answers whether the pair lost its last watcher: the
    // aggregated upstream interest must drop exactly there (docs §7.4).
    for (index, id) in ids.iter().enumerate() {
        let last = hub.unsubscribe(&device, &thread, *id);
        assert_eq!(
            last,
            index + 1 == ids.len(),
            "only the final watcher detaches the aggregated pair"
        );
    }
    assert!(hub.watch_list().is_empty(), "no watcher left for the node");

    // Freed slots are reusable rather than leaked, and the pair is first again.
    let (refilled, _receiver) = hub
        .subscribe(&device, &thread)
        .expect("a freed slot is reusable");
    assert!(refilled.first, "a re-subscribe re-announces upstream interest");
    assert!(hub.unsubscribe(&device, &thread, refilled.id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_projection_is_capped_at_500_entries_inside_the_local_budget() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = app_state(&dir, "budget-shadow.db");
    let user = "budget_shadow_user";

    let sources = ShadowSources::resolved(
        NodeIdentity::from_session("desktop", "budget"),
        user.to_string(),
        None,
        TreeLimits::default(),
        &[CAP_SHADOW_FULL.to_string()],
    );
    let scope = wunder_server::interlink::client::shadow::workspace_scope(
        &state,
        &sources.local_user_id,
        sources.workspace_id.as_deref(),
    );
    let root = state.workspace.workspace_root(&scope);
    state.workspace.ensure_user_root(user).expect("user root");
    std::fs::create_dir_all(&root).expect("create projection root");

    // 600 files over 12 directories: above the cap, so the entry budget and the
    // truncation flag are exercised by the same walk.
    let seeded_root = root.clone();
    tokio::task::spawn_blocking(move || {
        for bucket in 0..12 {
            let directory = seeded_root.join(format!("dir-{bucket:02}"));
            std::fs::create_dir_all(&directory).expect("create directory");
            for index in 0..50 {
                std::fs::write(
                    directory.join(format!("file-{index:02}.txt")),
                    b"projection budget sample",
                )
                .expect("write file");
            }
        }
    })
    .await
    .expect("seed workspace");

    let started = Instant::now();
    let (workspace, truncated) = gather_workspace(&state, &sources, &TreeLimits::default()).await;
    let elapsed = started.elapsed();

    let entries = workspace["tree"]
        .as_array()
        .unwrap_or_else(|| panic!("projection carries a tree: {workspace}"));
    assert_eq!(
        entries.len(),
        SHADOW_MAX_ENTRIES,
        "the walk must stop at the entry cap while it walks, not after"
    );
    assert!(truncated, "over-cap trees must be marked truncated");
    assert!(
        elapsed < SHADOW_BUDGET,
        "shadow projection took {elapsed:?}, budget is {SHADOW_BUDGET:?}"
    );
    // §6.1 red line: relative paths only.
    let serialized = workspace.to_string();
    assert!(
        !serialized.contains(&root.to_string_lossy().to_string()),
        "projection must not leak the absolute workspace root"
    );
}

// Idle cost of an open tunnel (docs §13.6: CPU < 1%, RSS delta < 30 MB) needs a
// real tunnel, so it lives with the loopback harness:
// `cargo test -p wunder-runtime --test interlink_loopback --features
// sqlite-storage -- --ignored --nocapture --test-threads=1`, sampled by
// `scripts/interlink-bench/measure-idle.ps1`.
