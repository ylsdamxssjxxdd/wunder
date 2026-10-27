use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::fs::File;
use std::future::Future;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Instant;
use wunder_core::thread_store::{ItemListQuery, ThreadHistoryReader, ThreadId, ThreadStore};
use wunder_server::storage::{
    rollout_compression::{compress_rollout, CompressionOptions},
    thread_store::LocalThreadStore,
    SqliteStorage, StorageLifecycle, StressThreadScenario, StressThreadSpec, ThreadLogStore,
};

const USER: &str = "sample-user";
const THREAD: &str = "sample-history-thread";

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("storage history probe failed: {error}");
        std::process::exit(1);
    }
}

fn distribution(mut timings: Vec<f64>) -> Value {
    timings.sort_by(f64::total_cmp);
    let middle = timings.len() / 2;
    let median = if timings.len().is_multiple_of(2) {
        (timings[middle - 1] + timings[middle]) / 2.0
    } else {
        timings[middle]
    };
    json!({
        "samples": timings.len(),
        "median_ms": median,
        "p95_ms": timings[(timings.len() * 95).div_ceil(100) - 1],
    })
}

async fn measure<Operation, Output, OperationFuture>(
    count: usize,
    mut operation: Operation,
) -> Result<Value>
where
    Operation: FnMut() -> OperationFuture,
    OperationFuture: Future<Output = Result<Output>>,
{
    let mut timings = Vec::with_capacity(count);
    for _ in 0..count {
        let started = Instant::now();
        let output = std::hint::black_box(operation().await?);
        timings.push(started.elapsed().as_secs_f64() * 1000.0);
        drop(output);
    }
    Ok(distribution(timings))
}

fn manufacture_legacy_index(root: &Path) -> Result<()> {
    let catalog = Connection::open(root.join("state_1.sqlite"))?;
    let relative: String = catalog.query_row(
        "SELECT rollout_path FROM canonical_rollouts LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    let mut source = File::open(root.join(relative))?;
    let mut conn = Connection::open(root.join("thread_history_1.sqlite"))?;
    let transaction = conn.transaction()?;
    let index_statements = {
        let mut select = transaction.prepare(
            "SELECT sql FROM sqlite_master WHERE type='index' AND tbl_name='thread_items' AND sql IS NOT NULL",
        )?;
        let statements = select
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        statements
    };
    transaction.execute_batch(
        "CREATE TABLE thread_items_probe_legacy(
            rollout_id TEXT NOT NULL,turn_id TEXT,root_turn_id TEXT,item_id TEXT NOT NULL,
            item_type TEXT NOT NULL,created_at_ms INTEGER NOT NULL,rollout_ordinal INTEGER NOT NULL,
            item_json TEXT NOT NULL,preview_json TEXT NOT NULL,visible INTEGER NOT NULL,
            PRIMARY KEY(rollout_id,item_id));",
    )?;
    {
        let mut select = transaction.prepare(
            "SELECT rollout_id,item_id,record_byte_offset,record_byte_length FROM thread_items",
        )?;
        let mut rows = select.query([])?;
        let mut insert = transaction.prepare(
            "INSERT INTO thread_items_probe_legacy SELECT rollout_id,turn_id,root_turn_id,item_id,
             item_type,created_at_ms,rollout_ordinal,?3,preview_json,visible
             FROM thread_items WHERE rollout_id=?1 AND item_id=?2",
        )?;
        while let Some(row) = rows.next()? {
            let offset: u64 = row.get(2)?;
            let length: usize = row.get(3)?;
            ensure!(
                length <= 8 * 1024 * 1024 + 2,
                "fixture record exceeds budget"
            );
            source.seek(SeekFrom::Start(offset))?;
            let mut bytes = vec![0; length];
            source.read_exact(&mut bytes)?;
            let envelope: Value = serde_json::from_slice(&bytes)?;
            let payload = &envelope["payload"];
            let body = if payload.get("event").is_some() {
                payload
                    .get("payload")
                    .context("generated event body is missing")?
            } else {
                payload
            };
            let item = body.get("item").unwrap_or(body);
            ensure!(
                item.get("item_id").is_some(),
                "generated item identity is missing"
            );
            insert.execute(params![
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                serde_json::to_string(item)?
            ])?;
        }
    }
    transaction.execute_batch(
        "DROP TABLE thread_items;
         ALTER TABLE thread_items_probe_legacy RENAME TO thread_items;
         PRAGMA user_version=0;",
    )?;
    for statement in index_statements {
        transaction.execute_batch(&statement)?;
    }
    transaction.commit()?;
    conn.execute_batch(
        "PRAGMA wal_checkpoint(TRUNCATE); VACUUM; PRAGMA wal_checkpoint(TRUNCATE);",
    )?;
    Ok(())
}

async fn run() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let rounds = arguments
        .next()
        .map(|value| value.parse::<i64>())
        .transpose()?
        .unwrap_or(100);
    let samples = arguments
        .next()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(30);
    ensure!(
        arguments.next().is_none(),
        "usage: storage_history_probe [user_rounds] [samples]"
    );
    ensure!(
        (1..=500).contains(&rounds),
        "user_rounds must be in 1..=500"
    );
    ensure!((10..=200).contains(&samples), "samples must be in 10..=200");
    let temporary = tempfile::tempdir().context("could not create isolated fixture")?;
    let root = temporary.path();
    let thread = ThreadId::new(THREAD)?;
    let store = LocalThreadStore::open(root)?;
    let started = Instant::now();
    let stats = store
        .generate_stress_thread(
            USER,
            &StressThreadSpec {
                session_id: THREAD.into(),
                title: "Synthetic history".into(),
                workspace_id: None,
                user_rounds: rounds,
                model_rounds_per_turn: 4,
                seed: Some(7),
                scenario: StressThreadScenario::Complex,
            },
            |_, _| {},
        )
        .await?;
    store.shutdown(&thread).await?;
    let generation_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(store);
    eprintln!(
        "synthetic fixture ready: turns={}, items={}",
        stats.user_turns, stats.items
    );

    manufacture_legacy_index(root)?;
    let index_path = root.join("thread_history_1.sqlite");
    let legacy_bytes = std::fs::metadata(&index_path)?.len();
    let started = Instant::now();
    let migrated = LocalThreadStore::open(root)?;
    let migration_ms = started.elapsed().as_secs_f64() * 1000.0;
    drop(migrated);
    let compact_bytes = std::fs::metadata(&index_path)?.len();
    let reopen = measure(samples, || async { Ok(LocalThreadStore::open(root)?) }).await?;
    let store = LocalThreadStore::open(root)?;
    let query = ItemListQuery {
        thread_id: thread.clone(),
        turn_id: None,
        cursor: None,
        limit: 20,
    };
    let started = Instant::now();
    let page = store.list_items(query.clone()).await?;
    let first_full_page_ms = started.elapsed().as_secs_f64() * 1000.0;
    ensure!(
        page.entries.len() == 20,
        "fixture must contain twenty completed items"
    );
    let selected = page
        .entries
        .iter()
        .max_by_key(|entry| entry.item.payload.to_string().len())
        .context("missing sample item")?;
    let item = selected.item.item_id.clone();
    let item_bytes = serde_json::to_vec(&selected.item)?.len();
    let full_page = measure(samples, || async {
        Ok(store.list_items(query.clone()).await?)
    })
    .await?;
    let full_item = measure(samples, || async {
        Ok(store.read_item(&thread, &item).await?)
    })
    .await?;

    let backend = SqliteStorage::new(root.join("state_1.sqlite").to_string_lossy().into_owned());
    backend.ensure_initialized()?;
    let preview = backend.thread_view_page(USER, THREAD, "chat", None, 12)?;
    ensure!(!preview.chat.is_empty(), "chat preview must not be empty");
    let chat_preview = measure(samples, || async {
        backend.thread_view_page(USER, THREAD, "chat", None, 12)
    })
    .await?;
    let trajectory_preview = measure(samples, || async {
        let page = backend.thread_view_page(USER, THREAD, "trajectory", None, 80)?;
        ensure!(
            !page.trajectory.is_empty() && !page.degraded,
            "trajectory page is incomplete"
        );
        Ok(page)
    })
    .await?;
    let search = backend.thread_view_search(USER, THREAD, "fixture-no-match-marker")?;
    ensure!(
        search.hits.is_empty(),
        "missing-marker search unexpectedly matched"
    );
    let search_has_more = search.has_more;
    let search_no_match = measure(10, || async {
        backend.thread_view_search(USER, THREAD, "fixture-no-match-marker")
    })
    .await?;

    let catalog = Connection::open(root.join("state_1.sqlite"))?;
    let relative: String = catalog.query_row(
        "SELECT rollout_path FROM canonical_rollouts LIMIT 1",
        [],
        |row| row.get(0),
    )?;
    drop(catalog);
    let rollout_path = root.join(relative);
    let raw_bytes = std::fs::metadata(&rollout_path)?.len();
    let started = Instant::now();
    let compressed_path = compress_rollout(
        &rollout_path,
        &CompressionOptions {
            enabled: true,
            level: 3,
            min_age_days: 0,
        },
    )?
    .context("fixture compression did not run")?;
    let compression_ms = started.elapsed().as_secs_f64() * 1000.0;
    let compressed_bytes = std::fs::metadata(compressed_path)?.len();
    ensure!(
        store.read_item(&thread, &item).await? == *selected,
        "compressed body changed"
    );
    let restored = backend.thread_view_page(USER, THREAD, "chat", None, 12)?;
    ensure!(
        serde_json::to_value(&restored.chat)? == serde_json::to_value(&preview.chat)?,
        "compressed chat preview changed"
    );
    let compressed_item =
        measure(10, || async { Ok(store.read_item(&thread, &item).await?) }).await?;
    let compressed_page =
        measure(10, || async { Ok(store.list_items(query.clone()).await?) }).await?;
    let compressed_preview = measure(samples, || async {
        backend.thread_view_page(USER, THREAD, "chat", None, 12)
    })
    .await?;

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "probe": "canonical_history_latency",
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "cache": "OS cache not cleared; first access is not a cold-disk measurement",
            "scope": "isolated synthetic data; no pre-refactor latency baseline; no model or UI timing",
            "fixture": { "user_turns": stats.user_turns, "model_requests": stats.model_requests,
                "completed_items": stats.items, "rollout_records": stats.rollout_records,
                "seed": stats.seed, "generation_and_shutdown_ms": generation_ms,
                "raw_rollout_bytes": raw_bytes, "compressed_rollout_bytes": compressed_bytes,
                "selected_full_item_bytes": item_bytes },
            "migration": { "legacy_index_bytes": legacy_bytes, "compact_index_bytes": compact_bytes,
                "single_open_verify_and_vacuum_ms": migration_ms,
                "fixture": "legacy body table reconstructed from production-generated canonical records" },
            "reopen_without_migration": reopen,
            "first_full_page_after_reopen_ms": first_full_page_ms,
            "uncompressed": { "full_items_page_20": full_page, "full_item": full_item,
                "chat_preview_12": chat_preview, "trajectory_preview_80": trajectory_preview,
                "search_no_match": search_no_match, "search_has_more": search_has_more },
            "compressed": { "compression_ms": compression_ms, "full_item": compressed_item,
                "full_items_page_20": compressed_page, "chat_preview_12": compressed_preview,
                "reader": "each body operation decodes the full rollout into a temporary file" },
        }))?
    );
    drop(backend);
    drop(store);
    Ok(())
}
