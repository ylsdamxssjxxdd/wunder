use crate::storage::TeamRunRecord;

pub struct SwarmRunner;

impl SwarmRunner {
    pub fn next_status_on_start(_status: &str) -> &'static str {
        "running"
    }

    pub fn next_status_on_finish(record: &TeamRunRecord, has_error: bool) -> &'static str {
        if has_error || record.task_failed > 0 {
            "failed"
        } else {
            "success"
        }
    }
}
