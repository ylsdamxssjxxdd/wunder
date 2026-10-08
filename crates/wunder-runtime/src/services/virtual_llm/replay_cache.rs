use super::{parse_virtual_log, ParsedVirtualLog, MAX_VIRTUAL_LLM_JSONL_BYTES};
use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

const MAX_ENTRIES: usize = 8;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: SystemTime,
}

struct Entry {
    path: PathBuf,
    stamp: FileStamp,
    parsed: Arc<ParsedVirtualLog>,
    weight: usize,
    loaded: Instant,
}

#[derive(Default)]
pub(super) struct ReplayCache {
    entries: Mutex<Vec<Entry>>,
    // Fixed-size miss gates coalesce concurrent parsing without an unbounded lock map.
    loads: [Mutex<()>; 16],
}

static CACHE: OnceLock<ReplayCache> = OnceLock::new();

pub(super) fn load(path: &Path) -> Result<Arc<ParsedVirtualLog>> {
    CACHE.get_or_init(ReplayCache::default).load(path)
}

pub(super) fn invalidate(path: &Path) {
    if let Some(cache) = CACHE.get() {
        cache.entries.lock().retain(|entry| entry.path != path);
    }
}

impl ReplayCache {
    fn get(&self, path: &Path, stamp: &FileStamp) -> Option<Arc<ParsedVirtualLog>> {
        let mut entries = self.entries.lock();
        entries.retain(|entry| entry.loaded.elapsed() < CACHE_TTL);
        let index = entries
            .iter()
            .position(|entry| entry.path == path && entry.stamp == *stamp)?;
        let entry = entries.remove(index);
        let parsed = Arc::clone(&entry.parsed);
        entries.push(entry);
        Some(parsed)
    }

    pub(super) fn load(&self, path: &Path) -> Result<Arc<ParsedVirtualLog>> {
        if let Some(parsed) = self.get(path, &file_stamp(path)?) {
            return Ok(parsed);
        }
        let mut hasher = DefaultHasher::new();
        path.hash(&mut hasher);
        let _load = self.loads[hasher.finish() as usize % self.loads.len()].lock();
        let stamp = file_stamp(path)?;
        if let Some(parsed) = self.get(path, &stamp) {
            return Ok(parsed);
        }
        // Keep filesystem I/O and parsing outside the shared cache lock.
        let mut text = String::new();
        File::open(path)
            .context("open virtual llm replay log")?
            .take(MAX_VIRTUAL_LLM_JSONL_BYTES + 1)
            .read_to_string(&mut text)
            .context("read virtual llm replay log as UTF-8")?;
        if text.len() as u64 > MAX_VIRTUAL_LLM_JSONL_BYTES {
            return Err(anyhow!("virtual llm jsonl is too large"));
        }
        let parsed = Arc::new(parse_virtual_log(&text, "", "")?);
        if parsed.turns.is_empty() {
            return Err(anyhow!("virtual llm log contains no replay turns"));
        }
        if stamp != file_stamp(path)? {
            return Err(anyhow!(
                "virtual llm replay log changed while loading; retry the request"
            ));
        }
        let weight = parsed_weight(&parsed);
        let mut entries = self.entries.lock();
        entries.retain(|entry| entry.path != path && entry.loaded.elapsed() < CACHE_TTL);
        if weight <= MAX_CACHE_BYTES {
            let mut total: usize = entries.iter().map(|entry| entry.weight).sum();
            while !entries.is_empty()
                && (entries.len() >= MAX_ENTRIES || total + weight > MAX_CACHE_BYTES)
            {
                total -= entries.remove(0).weight;
            }
            entries.push(Entry {
                path: path.to_path_buf(),
                stamp,
                parsed: Arc::clone(&parsed),
                weight,
                loaded: Instant::now(),
            });
        }
        Ok(parsed)
    }
}

fn file_stamp(path: &Path) -> Result<FileStamp> {
    let meta = fs::metadata(path).context("inspect virtual llm replay log")?;
    if !meta.is_file() || meta.len() > MAX_VIRTUAL_LLM_JSONL_BYTES {
        return Err(anyhow!(
            "virtual llm replay log must be a file of at most 32 MiB"
        ));
    }
    Ok(FileStamp {
        size: meta.len(),
        modified: meta.modified().context("read replay modification time")?,
    })
}

fn parsed_weight(parsed: &ParsedVirtualLog) -> usize {
    std::mem::size_of::<ParsedVirtualLog>()
        + parsed.format.capacity()
        + parsed.turns.capacity() * std::mem::size_of::<super::VirtualReplayTurn>()
        + parsed
            .turns
            .iter()
            .map(|turn| {
                turn.content.capacity()
                    + turn.reasoning.capacity()
                    + turn.format.capacity()
                    + turn.tool_calls.as_ref().map_or(0, json_weight)
            })
            .sum::<usize>()
}

fn json_weight(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::String(text) => text.capacity(),
            Value::Array(items) => {
                items.capacity() * std::mem::size_of::<Value>()
                    + items.iter().map(json_weight).sum::<usize>()
            }
            Value::Object(items) => items
                .iter()
                .map(|(key, value)| key.capacity() + 128 + json_weight(value))
                .sum(),
            _ => 0,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_parallel_loads_and_refreshes_changed_or_deleted_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("replay.jsonl");
        fs::write(
            &path,
            "{\"event\":\"llm_output\",\"data\":{\"content\":\"A\"}}",
        )
        .unwrap();
        let cache = ReplayCache::default();
        let loaded = std::thread::scope(|scope| {
            (0..8)
                .map(|_| scope.spawn(|| cache.load(&path).unwrap()))
                .collect::<Vec<_>>()
                .into_iter()
                .map(|task| task.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(loaded.iter().all(|parsed| Arc::ptr_eq(parsed, &loaded[0])));
        fs::write(
            &path,
            "{\"event\":\"llm_output\",\"data\":{\"content\":\"BB\"}}",
        )
        .unwrap();
        let updated = cache.load(&path).unwrap();
        assert_eq!(updated.turns[0].content, "BB");
        assert!(!Arc::ptr_eq(&updated, &loaded[0]));
        fs::remove_file(&path).unwrap();
        assert!(cache.load(&path).is_err());
    }

    #[test]
    fn cache_is_bounded_and_failed_loads_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let cache = ReplayCache::default();
        for index in 0..MAX_ENTRIES + 2 {
            let path = root.path().join(format!("{index}.jsonl"));
            fs::write(&path, "invalid").unwrap();
            assert!(cache.load(&path).is_err());
            fs::write(
                &path,
                "{\"event\":\"llm_output\",\"data\":{\"content\":\"A\"}}",
            )
            .unwrap();
            cache.load(&path).unwrap();
        }
        assert_eq!(cache.entries.lock().len(), MAX_ENTRIES);
    }
}
