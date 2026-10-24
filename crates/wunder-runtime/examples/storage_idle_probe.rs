//! Isolated canonical-storage reopen and idle-file probe.
//!
//! Usage:
//!   cargo run -p wunder-runtime --features sqlite-storage \
//!     --example storage_idle_probe -- [seconds]
//!
//! The default is 60 seconds; pass 600 for a ten-minute file observation.
//! A GUID-named root inside a fresh TempDir contains only a production-generated
//! 2 x 2 synthetic thread. The first store is drained, read and dropped; a new
//! store verifies the same completed items, turns and bounded model context.
//! An empty append recovers and drains its writer without adding records; that
//! writer, the reopened store and Tokio runtime stay alive during measurement.
//! No business calls or storage polls run between the two directory snapshots.
//!
//! The verdict compares relative file sets, lengths, modification times and
//! streamed SHA256 hashes. SQLite SHM and owner lock files are reported separately
//! using file sets, lengths and modification times, without reading locked files.
//! Byte totals distinguish each role's database/WAL, raw/zstd rollout and other
//! files; unused roles report zero. Initialization and final shutdown are outside
//! the measurement. TempDir owns cleanup; no existing data is opened or modified,
//! and neither absolute paths nor retained message bodies are printed.
//!
//! This compares endpoint file stability, not zero SQL reads, CPU usage, the complete
//! application's background services or target-platform performance. Synthetic
//! initialization freezes a profile-specific system prompt and initial-memory
//! snapshot once, matching the immutable thread-initialization contract.

use anyhow::{anyhow, ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;
use wunder_core::thread_store::{
    CanonicalItemKind, CanonicalRecord, ItemListQuery, ItemSummary, ModelContext,
    ModelContextRequest, ThreadHistoryReader, ThreadId, ThreadStore, TurnCompletionStatus,
    TurnListQuery, TurnSummary,
};
use wunder_server::storage::{
    thread_store::LocalThreadStore, StressThreadScenario, StressThreadSpec, StressThreadStats,
};

const MAX_PAGES: usize = 32;
const MAX_RETAINED_ITEMS: usize = 64;
const MAX_SNAPSHOT_FILES: usize = 256;
const BYTE_CATEGORIES: [&str; 17] = [
    "state_db",
    "state_wal",
    "thread_history_db",
    "thread_history_wal",
    "queue_db",
    "queue_wal",
    "goals_db",
    "goals_wal",
    "memories_db",
    "memories_wal",
    "logs_db",
    "logs_wal",
    "rollout_raw",
    "rollout_zstd",
    "other",
    "auxiliary_shm",
    "auxiliary_lock",
];

#[derive(Debug, PartialEq)]
struct RetainedHistory {
    items: Vec<ItemSummary>,
    turns: Vec<TurnSummary>,
    context: ModelContext,
}

#[derive(PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: SystemTime,
    sha256: Option<[u8; 32]>,
}

struct Snapshot {
    tracked: BTreeMap<PathBuf, Fingerprint>,
    auxiliary: BTreeMap<PathBuf, Fingerprint>,
}

#[derive(Default, Serialize)]
struct CategoryTotal {
    files: usize,
    bytes: u64,
}

#[derive(Serialize)]
struct Changes {
    added: usize,
    removed: usize,
    changed: usize,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if let Err(error) = run().await {
        // Display only the outer contextual message, never filesystem sources.
        eprintln!("storage idle probe failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    ensure!(args.next().is_none(), "usage: storage_idle_probe [seconds]");
    if matches!(first.as_deref(), Some("--help" | "-h")) {
        println!("usage: storage_idle_probe [seconds]; default 60, range 1..=3600");
        return Ok(());
    }
    let seconds = first
        .map(|value| value.parse::<u64>())
        .transpose()
        .context("seconds must be an integer")?
        .unwrap_or(60);
    ensure!((1..=3600).contains(&seconds), "seconds must be in 1..=3600");

    let temporary = tempfile::tempdir().context("could not create isolated temporary directory")?;
    let root = temporary.path().join(Uuid::new_v4().to_string());
    let thread = ThreadId::new("sample-idle-thread")?;
    let spec = StressThreadSpec {
        session_id: thread.to_string(),
        title: "Sample history".into(),
        workspace_id: None,
        user_rounds: 2,
        model_rounds_per_turn: 2,
        seed: Some(7),
        scenario: StressThreadScenario::Complex,
    };

    let store = LocalThreadStore::open(&root).context("could not open isolated storage")?;
    let stats = store
        .generate_stress_thread("sample-user", &spec, |_, _| {})
        .await
        .context("could not generate the canonical sample")?;
    store
        .shutdown(&thread)
        .await
        .context("could not drain the original writer")?;
    let expected = read_history(&store, &thread, &stats).await?;
    drop(store);

    let reopened = LocalThreadStore::open(&root).context("could not reopen isolated storage")?;
    let restored = read_history(&reopened, &thread, &stats).await?;
    ensure!(
        restored == expected,
        "completed history changed after reopen"
    );
    drop(restored);
    drop(expected);

    // ThreadStore::flush closes the active writer. An empty nonterminal append
    // instead awaits its flush barrier and projection while retaining the
    // recovered recorder, so the idle interval exercises a live writer task.
    let drained = reopened
        .append_records(&thread, Vec::new())
        .await
        .context("could not recover and drain the reopened writer")?;
    ensure!(
        !drained.projection_pending
            && drained.projected_through.as_ref() == Some(&drained.persisted_through)
            && drained.persisted_through.end_ordinal_exclusive == stats.rollout_records,
        "reopened writer and projection did not reach the generated boundary"
    );
    let before = take_snapshot(root.clone()).await?;
    println!("idle measurement started: seconds={seconds}, reopened_writer_alive=true");
    let started = Instant::now();
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let elapsed = started.elapsed().as_secs_f64();
    let after = take_snapshot(root).await?;
    let unchanged = before.tracked == after.tracked;
    let tracked_changes = changes(&before.tracked, &after.tracked);
    let auxiliary_changes = changes(&before.auxiliary, &after.auxiliary);

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "probe": "canonical_storage_idle",
            "requested_seconds": seconds,
            "observed_seconds": elapsed,
            "reopen_verified": true,
            "reopened_writer_drained_and_kept_alive": true,
            "generated": {
                "user_turns": stats.user_turns,
                "model_requests": stats.model_requests,
                "model_actions": stats.model_actions,
                "completed_items": stats.items,
                "tool_calls": stats.tool_calls,
                "rollout_records": stats.rollout_records,
                "seed": stats.seed,
                "profile": stats.profile,
                "initial_context_tokens": stats.initial_context_tokens,
                "storage_bytes": stats.storage_bytes,
            },
            "tracked": {
                "unchanged": unchanged,
                "before_files": before.tracked.len(),
                "after_files": after.tracked.len(),
                "changes": tracked_changes,
                "comparison": "file set, length, modification time, SHA256",
            },
            "auxiliary": {
                "excluded_from_verdict": true,
                "before_files": before.auxiliary.len(),
                "after_files": after.auxiliary.len(),
                "changes": auxiliary_changes,
                "comparison": "file set, length, modification time; no hashes",
            },
            "bytes_before": byte_totals(&before),
            "bytes_after": byte_totals(&after),
            "scope": "file stability only; SQL reads, CPU and application background services are not measured",
        }))
        .context("could not serialize the probe report")?
    );

    // Keep the reopened instance alive until both snapshots and the comparison
    // have finished. Final flush/checkpoint/cleanup activity is not measured.
    reopened
        .shutdown(&thread)
        .await
        .context("could not shut down after the measurement")?;
    drop(reopened);
    ensure!(unchanged, "tracked storage files changed during idle");
    Ok(())
}

async fn read_history(
    store: &LocalThreadStore,
    thread: &ThreadId,
    stats: &StressThreadStats,
) -> Result<RetainedHistory> {
    ensure!(
        stats.user_turns == 2 && stats.model_requests == 4,
        "unexpected sample size"
    );
    let expected_items = usize::try_from(stats.items).context("invalid generated item count")?;
    ensure!(
        expected_items <= MAX_RETAINED_ITEMS,
        "sample exceeds its read budget"
    );
    let mut items = Vec::new();
    let mut cursor = None;
    let mut item_ids = HashSet::new();
    for _ in 0..MAX_PAGES {
        let page = store
            .list_items(ItemListQuery {
                thread_id: thread.clone(),
                turn_id: None,
                cursor: cursor.clone(),
                limit: 3,
            })
            .await
            .context("could not read a bounded completed-item page")?;
        ensure!(page.entries.len() <= 3, "item page exceeded its limit");
        for item in page.entries {
            ensure!(
                item_ids.insert(item.item.item_id.clone()),
                "item repeated between pages"
            );
            items.push(item);
        }
        ensure!(
            items.len() <= expected_items,
            "unexpected extra completed items"
        );
        if page.next_cursor.is_none() {
            cursor = None;
            break;
        }
        ensure!(page.next_cursor != cursor, "item cursor did not advance");
        cursor = page.next_cursor;
    }
    ensure!(
        cursor.is_none() && items.len() == expected_items,
        "completed items were not fully retained"
    );
    for (kind, count) in [
        (CanonicalItemKind::UserMessage, stats.user_turns),
        (CanonicalItemKind::AssistantMessage, stats.model_requests),
        (CanonicalItemKind::ToolResult, stats.tool_calls),
    ] {
        ensure!(
            items.iter().filter(|item| item.item.kind == kind).count() as i64 == count,
            "completed item kinds do not match generation"
        );
    }

    let mut turns = Vec::new();
    let mut cursor = None;
    let mut turn_ids = HashSet::new();
    for _ in 0..MAX_PAGES {
        let page = store
            .list_turns(TurnListQuery {
                thread_id: thread.clone(),
                cursor: cursor.clone(),
                limit: 1,
            })
            .await
            .context("could not read a bounded turn page")?;
        ensure!(page.entries.len() <= 1, "turn page exceeded its limit");
        for turn in page.entries {
            ensure!(
                turn_ids.insert(turn.turn_id.clone()),
                "turn repeated between pages"
            );
            ensure!(
                turn.status == TurnCompletionStatus::Completed && turn.completed_at_ms.is_some(),
                "generated turn is not completed"
            );
            turns.push(turn);
        }
        ensure!(turns.len() <= 2, "unexpected extra turns");
        if page.next_cursor.is_none() {
            cursor = None;
            break;
        }
        ensure!(page.next_cursor != cursor, "turn cursor did not advance");
        cursor = page.next_cursor;
    }
    ensure!(
        cursor.is_none() && turns.len() == 2,
        "completed turns were not fully retained"
    );
    ensure!(
        turns
            .iter()
            .map(|turn| u64::from(turn.item_count))
            .sum::<u64>()
            == stats.items as u64,
        "turn item counts do not match completed history"
    );
    ensure!(
        items
            .iter()
            .all(|item| turn_ids.contains(&item.item.turn_id)),
        "completed item belongs to an unknown turn"
    );

    let context = store
        .load_model_context(ModelContextRequest {
            thread_id: thread.clone(),
            position: None,
            max_records: MAX_RETAINED_ITEMS as u32,
            max_bytes: 4 * 1024 * 1024,
        })
        .await
        .context("could not read bounded model context")?;
    ensure!(
        !context.truncated && context.records.len() == expected_items,
        "model context was not fully retained within its budget"
    );
    ensure!(context.records.iter().all(|record| matches!(&record.record, CanonicalRecord::ResponseItem(item) if turn_ids.contains(&item.turn_id))), "model context contains unexpected records");
    Ok(RetainedHistory {
        items,
        turns,
        context,
    })
}

async fn take_snapshot(root: PathBuf) -> Result<Snapshot> {
    tokio::task::spawn_blocking(move || snapshot(&root))
        .await
        .context("snapshot worker stopped")?
        .context("could not snapshot generated storage")
}

fn snapshot(root: &Path) -> Result<Snapshot> {
    let mut result = Snapshot {
        tracked: BTreeMap::new(),
        auxiliary: BTreeMap::new(),
    };
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        ensure!(
            entry.file_type().is_file(),
            "unexpected non-file in probe root"
        );
        ensure!(
            result.tracked.len() + result.auxiliary.len() < MAX_SNAPSHOT_FILES,
            "probe snapshot exceeds its file budget"
        );
        let relative = entry.path().strip_prefix(root)?.to_path_buf();
        let name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("invalid probe filename"))?;
        let auxiliary = name.ends_with("-shm") || name.ends_with(".lock");
        let metadata = entry.metadata()?;
        let modified = metadata.modified()?;
        let sha256 = if auxiliary {
            None
        } else {
            let mut file = File::open(entry.path())?;
            let mut hash = Sha256::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hash.update(&buffer[..read]);
            }
            let after = file.metadata()?;
            ensure!(
                metadata.len() == after.len() && modified == after.modified()?,
                "file changed during its snapshot"
            );
            Some(hash.finalize().into())
        };
        let fingerprint = Fingerprint {
            len: metadata.len(),
            modified,
            sha256,
        };
        if auxiliary {
            result.auxiliary.insert(relative, fingerprint);
        } else {
            result.tracked.insert(relative, fingerprint);
        }
    }
    Ok(result)
}

fn changes(
    before: &BTreeMap<PathBuf, Fingerprint>,
    after: &BTreeMap<PathBuf, Fingerprint>,
) -> Changes {
    Changes {
        added: after
            .keys()
            .filter(|path| !before.contains_key(*path))
            .count(),
        removed: before
            .keys()
            .filter(|path| !after.contains_key(*path))
            .count(),
        changed: before
            .iter()
            .filter(|(path, fingerprint)| {
                after.get(*path).is_some_and(|other| other != *fingerprint)
            })
            .count(),
    }
}

fn byte_totals(snapshot: &Snapshot) -> BTreeMap<&'static str, CategoryTotal> {
    let mut totals = BYTE_CATEGORIES
        .into_iter()
        .map(|category| (category, CategoryTotal::default()))
        .collect::<BTreeMap<_, _>>();
    for (path, file) in snapshot.tracked.iter().chain(&snapshot.auxiliary) {
        let total = totals.get_mut(category(path)).expect("known byte category");
        total.files += 1;
        total.bytes += file.len;
    }
    totals
}

fn category(path: &Path) -> &'static str {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match name {
        "state_1.sqlite" => "state_db",
        "state_1.sqlite-wal" => "state_wal",
        "thread_history_1.sqlite" => "thread_history_db",
        "thread_history_1.sqlite-wal" => "thread_history_wal",
        "queue_1.sqlite" => "queue_db",
        "queue_1.sqlite-wal" => "queue_wal",
        "goals_1.sqlite" => "goals_db",
        "goals_1.sqlite-wal" => "goals_wal",
        "memories_1.sqlite" => "memories_db",
        "memories_1.sqlite-wal" => "memories_wal",
        "logs_1.sqlite" => "logs_db",
        "logs_1.sqlite-wal" => "logs_wal",
        _ if name.ends_with("-shm") => "auxiliary_shm",
        _ if name.ends_with(".lock") => "auxiliary_lock",
        _ if name.ends_with(".jsonl.zst") => "rollout_zstd",
        _ if name.ends_with(".jsonl") => "rollout_raw",
        _ => "other",
    }
}
