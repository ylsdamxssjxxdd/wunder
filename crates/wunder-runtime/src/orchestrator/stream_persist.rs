use super::*;
use std::sync::mpsc::{self as std_mpsc, SyncSender, TryRecvError, TrySendError};
use std::thread;

const STREAM_EVENT_WRITE_QUEUE_SIZE: usize = 2048;
const STREAM_EVENT_WRITE_BATCH_SIZE: usize = 128;

struct StreamPersistTask {
    storage: Arc<dyn StorageBackend>,
    session_id: String,
    user_id: String,
    event_id: i64,
    payload: Value,
    event_type: String,
    cleanup_cutoff: Option<f64>,
}

struct StreamPersistQueue {
    sender: SyncSender<StreamPersistTask>,
    fallback_writes: AtomicU64,
}

impl StreamPersistQueue {
    fn new() -> Self {
        let (sender, receiver) = std_mpsc::sync_channel(STREAM_EVENT_WRITE_QUEUE_SIZE);
        if let Err(err) = thread::Builder::new()
            .name("wunder-stream-persist".to_string())
            .spawn(move || Self::run(receiver))
        {
            warn!("failed to spawn stream event writer thread: {err}");
        }
        Self {
            sender,
            fallback_writes: AtomicU64::new(0),
        }
    }

    fn run(receiver: std_mpsc::Receiver<StreamPersistTask>) {
        while let Ok(task) = receiver.recv() {
            let mut batch = Vec::with_capacity(STREAM_EVENT_WRITE_BATCH_SIZE);
            batch.push(task);
            while batch.len() < STREAM_EVENT_WRITE_BATCH_SIZE {
                match receiver.try_recv() {
                    Ok(task) => batch.push(task),
                    Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
                }
            }
            for task in batch {
                Self::apply_task(task);
            }
        }
    }

    fn enqueue(&self, task: StreamPersistTask) {
        match self.sender.try_send(task) {
            Ok(()) => {}
            Err(TrySendError::Full(task)) | Err(TrySendError::Disconnected(task)) => {
                let fallback_writes =
                    self.fallback_writes.fetch_add(1, AtomicOrdering::Relaxed) + 1;
                if fallback_writes == 1 || fallback_writes.is_multiple_of(1000) {
                    warn!(
                        "stream event persist queue saturated, using fallback writes {fallback_writes} times"
                    );
                }
                Self::spawn_fallback(task);
            }
        }
    }

    fn spawn_fallback(task: StreamPersistTask) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn_blocking(move || Self::apply_task(task));
        } else {
            Self::apply_task(task);
        }
    }

    fn apply_task(task: StreamPersistTask) {
        let StreamPersistTask {
            storage,
            session_id,
            user_id,
            event_id,
            payload,
            event_type,
            cleanup_cutoff,
        } = task;
        if let Err(err) = storage.append_stream_event(&session_id, &user_id, event_id, &payload) {
            warn!("failed to persist stream event {event_type} for session {session_id}: {err}");
        }
        if let Some(cutoff) = cleanup_cutoff {
            if let Err(err) = storage.delete_stream_events_before(cutoff) {
                warn!("failed to cleanup stream events before {cutoff} for session {session_id}: {err}");
            }
        }
    }
}

fn stream_persist_queue() -> &'static StreamPersistQueue {
    static STREAM_PERSIST_QUEUE: OnceLock<StreamPersistQueue> = OnceLock::new();
    STREAM_PERSIST_QUEUE.get_or_init(StreamPersistQueue::new)
}

pub(super) fn enqueue_stream_event_persist(
    storage: Arc<dyn StorageBackend>,
    session_id: String,
    user_id: String,
    event_id: i64,
    payload: Value,
    event_type: String,
    cleanup_cutoff: Option<f64>,
) {
    if event_id <= 0 {
        return;
    }
    if session_id.trim().is_empty() || user_id.trim().is_empty() || event_type.trim().is_empty() {
        return;
    }
    stream_persist_queue().enqueue(StreamPersistTask {
        storage,
        session_id,
        user_id,
        event_id,
        payload,
        event_type,
        cleanup_cutoff,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::SqliteStorage;

    fn build_storage() -> Arc<dyn StorageBackend> {
        let db_path = std::env::temp_dir().join(format!(
            "wunder_stream_event_persist_{}.db",
            uuid::Uuid::new_v4().simple()
        ));
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()))
    }

    #[test]
    fn apply_task_appends_stream_event() {
        let storage = build_storage();
        StreamPersistQueue::apply_task(StreamPersistTask {
            storage: storage.clone(),
            session_id: "sess_queue_append".to_string(),
            user_id: "user_queue_append".to_string(),
            event_id: 7,
            payload: json!({
                "event": "progress",
                "data": { "summary": "queued" },
                "timestamp": "2026-03-07T00:00:00+08:00"
            }),
            event_type: "progress".to_string(),
            cleanup_cutoff: None,
        });

        let records = storage
            .load_stream_events("sess_queue_append", 0, 16)
            .expect("load stream events");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["event"], json!("progress"));
        assert_eq!(records[0]["data"]["summary"], json!("queued"));
        assert_eq!(records[0]["event_id"], json!(7));
    }

    #[test]
    fn apply_task_runs_cleanup_cutoff() {
        let storage = build_storage();
        StreamPersistQueue::apply_task(StreamPersistTask {
            storage: storage.clone(),
            session_id: "sess_queue_cleanup".to_string(),
            user_id: "user_queue_cleanup".to_string(),
            event_id: 3,
            payload: json!({
                "event": "progress",
                "data": { "summary": "cleanup" },
                "timestamp": "2026-03-07T00:00:00+08:00"
            }),
            event_type: "progress".to_string(),
            cleanup_cutoff: Some(Utc::now().timestamp_millis() as f64 / 1000.0 + 60.0),
        });

        let records = storage
            .load_stream_events("sess_queue_cleanup", 0, 16)
            .expect("load stream events");
        assert!(records.is_empty());
    }
}
