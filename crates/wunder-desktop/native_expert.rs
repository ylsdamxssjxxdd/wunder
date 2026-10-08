//! Agent-scoped expert operations; no transport or UI dependencies.
use super::{NativeDesktop, NativeSession};
use anyhow::{bail, Result};
use wunder_server::agent_management;
use wunder_server::memory_fragments::{
    MemoryFragmentInput, MemoryFragmentListOptions, MemoryFragmentStore,
};

#[derive(Clone, Debug)]
pub struct ExpertMemory {
    pub id: String,
    pub title: String,
    pub content: String,
    pub category: String,
    pub source: String,
    pub updated_at: f64,
}

impl NativeDesktop {
    fn ensure_expert_access(&self, agent: &str) -> Result<()> {
        self.runtime
            .block_on(agent_management::owned(self.state(), self.user_id(), agent))?;
        Ok(())
    }

    pub fn expert_memories(
        &self,
        agent: &str,
        query: &str,
        category: &str,
    ) -> Result<Vec<ExpertMemory>> {
        self.ensure_expert_access(agent)?;
        let store = MemoryFragmentStore::new(self.state().storage.clone());
        Ok(store
            .list_fragments(
                self.user_id(),
                Some(agent),
                MemoryFragmentListOptions {
                    query: Some(query),
                    category: Some(category),
                    limit: Some(200),
                    ..Default::default()
                },
            )
            .into_iter()
            .map(|item| ExpertMemory {
                id: item.memory_id,
                title: item.title_l0,
                content: item.content_l2,
                category: item.category,
                source: item.source_type,
                updated_at: item.updated_at,
            })
            .collect())
    }

    pub fn save_expert_memory(
        &self,
        agent: &str,
        id: &str,
        title: &str,
        content: &str,
        category: &str,
    ) -> Result<()> {
        self.ensure_expert_access(agent)?;
        if title.trim().is_empty() || content.trim().is_empty() {
            bail!("标题和内容不能为空");
        }
        let store = MemoryFragmentStore::new(self.state().storage.clone());
        if !id.is_empty()
            && store
                .get_fragment(self.user_id(), Some(agent), id)
                .is_none()
        {
            bail!("记忆不存在");
        }
        store.save_fragment(
            self.user_id(),
            Some(agent),
            MemoryFragmentInput {
                memory_id: (!id.is_empty()).then(|| id.to_owned()),
                title_l0: Some(title.into()),
                content_l2: Some(content.into()),
                category: Some(category.into()),
                source_type: Some("manual".into()),
                confirmed_by_user: Some(true),
                ..Default::default()
            },
        )?;
        Ok(())
    }

    pub fn delete_expert_memory(&self, agent: &str, id: &str) -> Result<()> {
        self.ensure_expert_access(agent)?;
        if !MemoryFragmentStore::new(self.state().storage.clone()).delete_fragment(
            self.user_id(),
            Some(agent),
            id,
        ) {
            bail!("记忆不存在或删除失败");
        }
        Ok(())
    }

    pub fn replicate_expert_memories(&self, agent: &str, target: &str) -> Result<()> {
        self.ensure_expert_access(agent)?;
        self.ensure_expert_access(target)?;
        wunder_server::api::user_memory::replicate_agent_memories(
            self.state(),
            self.user_id(),
            agent,
            target,
            Some(true),
        )
        .map_err(|_| anyhow::anyhow!("记忆复刻失败，请确认目标智能体与来源不同"))?;
        Ok(())
    }

    pub fn expert_archives(&self, agent: &str, offset: i64) -> Result<(Vec<NativeSession>, i64)> {
        self.ensure_expert_access(agent)?;
        let scope = if matches!(agent, "" | "default" | "__default__") {
            ""
        } else {
            agent
        };
        let (rows, total) = self.state().storage.list_chat_sessions_by_status(
            self.user_id(),
            Some(scope),
            None,
            Some("archived"),
            offset.max(0),
            50,
        )?;
        let workspaces = self.workspace_lookup();
        Ok((
            rows.into_iter()
                .map(|row| self.session_with_stats(row, &workspaces))
                .collect(),
            total,
        ))
    }

    pub fn expert_runtime(&self, agent: &str, date: Option<&str>) -> Result<serde_json::Value> {
        self.ensure_expert_access(agent)?;
        self.runtime
            .block_on(wunder_server::api::user_agents::load_agent_runtime_records(
                self.state(),
                self.user_id(),
                agent,
                Some(14),
                date,
            ))
            .map_err(|_| anyhow::anyhow!("无法读取运行记录"))
    }
}
