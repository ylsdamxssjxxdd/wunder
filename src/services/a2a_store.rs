// A2A 任务存储：用于 SendMessage/SubscribeToTask 等接口。
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct A2aTask {
    pub id: String,
    pub user_id: String,
    pub status: String,
    pub context_id: Option<String>,
    pub endpoint: Option<String>,
    pub service_name: Option<String>,
    pub method: Option<String>,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
    pub answer: String,
}

#[derive(Default)]
pub struct A2aStore {
    tasks: DashMap<String, A2aTask>,
}

impl A2aStore {
    pub fn new() -> Self {
        Self {
            tasks: DashMap::new(),
        }
    }

    pub fn insert(&self, task: A2aTask) {
        self.tasks.insert(task.id.clone(), task);
    }

    pub fn list_by_user(&self, user_id: &str) -> Vec<A2aTask> {
        self.tasks
            .iter()
            .filter(|entry| entry.user_id == user_id)
            .map(|entry| entry.clone())
            .collect()
    }

    pub fn update(&self, task_id: &str, updater: impl FnOnce(&mut A2aTask)) {
        if let Some(mut entry) = self.tasks.get_mut(task_id) {
            updater(&mut entry);
        }
    }
}
