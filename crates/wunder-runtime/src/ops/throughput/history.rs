use super::{ThroughputSnapshot, HISTORY_LIMIT};
use std::path::Path;

pub(super) fn load(path: &Path) -> Vec<ThroughputSnapshot> {
    if std::fs::metadata(path).map_or(true, |meta| meta.len() > 2 * 1024 * 1024) {
        return Vec::new();
    }
    let mut items: Vec<ThroughputSnapshot> = std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let overflow = items.len().saturating_sub(HISTORY_LIMIT);
    items.drain(..overflow);
    items
}

pub(super) async fn save(path: &Path, items: &[ThroughputSnapshot]) -> Result<(), ()> {
    let parent = path.parent().ok_or(())?;
    tokio::fs::create_dir_all(parent).await.map_err(|_| ())?;
    let bytes = serde_json::to_vec(items).map_err(|_| ())?;
    let staging = path.with_extension("tmp");
    tokio::fs::write(&staging, bytes).await.map_err(|_| ())?;
    tokio::fs::rename(staging, path).await.map_err(|_| ())
}
