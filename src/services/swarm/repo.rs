use crate::services::user_store::UserStore;
use crate::storage::{TeamRunRecord, TeamTaskRecord};
use anyhow::Result;

pub trait SwarmRepo: Send + Sync {
    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()>;
    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>>;
    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>>;
    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()>;
}

impl SwarmRepo for UserStore {
    fn upsert_team_run(&self, record: &TeamRunRecord) -> Result<()> {
        self.upsert_team_run(record)
    }

    fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunRecord>> {
        self.get_team_run(team_run_id)
    }

    fn list_team_tasks(&self, team_run_id: &str) -> Result<Vec<TeamTaskRecord>> {
        self.list_team_tasks(team_run_id)
    }

    fn upsert_team_task(&self, record: &TeamTaskRecord) -> Result<()> {
        self.upsert_team_task(record)
    }
}
