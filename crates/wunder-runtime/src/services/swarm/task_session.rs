use crate::services::orchestration_context::{
    ensure_orchestration_member_session, load_hive_state, load_session_context,
};
use crate::storage::{ChatSessionRecord, StorageBackend, UserAgentRecord};
use anyhow::{anyhow, Result};
use uuid::Uuid;

/// Reuse workers only within the calling task (or its explicit orchestration).
pub fn resolve_or_create_agent_task_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent: &UserAgentRecord,
    parent_session_id: &str,
) -> Result<(ChatSessionRecord, bool)> {
    let parent = storage
        .get_chat_session(user_id, parent_session_id)?
        .filter(|record| record.status != "archived")
        .ok_or_else(|| anyhow!("parent task session not found"))?;
    if let Some(context) = load_session_context(storage, user_id, &parent.session_id) {
        if let Some(state) = load_hive_state(storage, user_id, &context.group_id)
            .filter(|state| state.active && state.run_id == context.run_id)
        {
            let (binding, created) =
                ensure_orchestration_member_session(storage, user_id, &state, agent)?;
            let session = storage
                .get_chat_session(user_id, &binding.session_id)?
                .ok_or_else(|| anyhow!("orchestration task session not found"))?;
            return Ok((session, created));
        }
    }
    let scope = serde_json::to_string(&(
        "worker",
        user_id.trim(),
        &parent.session_id,
        &agent.agent_id,
    ))?;
    let mut session_id = format!(
        "sess_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_OID, scope.as_bytes()).simple()
    );
    // Archived generations remain immutable. The deterministic successor and atomic insert
    // make concurrent dispatchers converge without overwriting titles, prompts or history.
    for _ in 0..256 {
        if let Some(record) = storage.get_chat_session(user_id, &session_id)? {
            if record.agent_id.as_deref() != Some(agent.agent_id.as_str())
                || record.parent_session_id.as_deref() != Some(parent.session_id.as_str())
            {
                return Err(anyhow!("worker task session scope mismatch"));
            }
            if record.status != "archived" {
                return Ok((record, false));
            }
            session_id = format!(
                "sess_{}",
                Uuid::new_v5(&Uuid::NAMESPACE_OID, session_id.as_bytes()).simple()
            );
            continue;
        }
        let now = chrono::Utc::now().timestamp_millis() as f64 / 1000.0;
        let record = ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: user_id.to_string(),
            title: agent.name.clone(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: Some(agent.agent_id.clone()),
            tool_overrides: Vec::new(),
            parent_session_id: Some(parent.session_id.clone()),
            parent_message_id: None,
            spawn_label: None,
            spawned_by: Some("agent_swarm".to_string()),
        };
        if storage.insert_chat_session_if_absent(&record)? {
            return Ok((record, true));
        }
    }
    Err(anyhow!(
        "worker task session generation limit reached; create a new task"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SqliteStorage, StorageLifecycle};
    use wunder_core::storage_backend::ChatSessionStore;
    #[test]
    fn task_scope_reuses_workers_without_replacing_archived_history() {
        let dir = tempfile::tempdir().unwrap();
        let storage = SqliteStorage::new(dir.path().join("tasks.db").to_string_lossy().to_string());
        storage.ensure_initialized().unwrap();
        let agent = UserAgentRecord {
            agent_id: "agent-shared".to_string(),
            user_id: "user-a".to_string(),
            hive_id: "hive-a".to_string(),
            name: "Agent".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            visible_unit_ids: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: "full_auto".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: 0,
            created_at: 1.0,
            updated_at: 1.0,
            preset_binding: None,
            silent: false,
            prefer_mother: true,
        };
        let parent = crate::services::orchestration_context::build_chat_session_with_title(
            "user-a", &agent, "Task",
        );
        storage.upsert_chat_session(&parent).unwrap();
        let (mut worker, created) =
            resolve_or_create_agent_task_session(&storage, "user-a", &agent, &parent.session_id)
                .unwrap();
        assert!(created);
        worker.title = "Updated".to_string();
        storage.upsert_chat_session(&worker).unwrap();
        let (reused, created) =
            resolve_or_create_agent_task_session(&storage, "user-a", &agent, &parent.session_id)
                .unwrap();
        assert_eq!(
            (reused.session_id, reused.title, created),
            (worker.session_id.clone(), worker.title.clone(), false)
        );
        let other = crate::services::orchestration_context::build_chat_session_with_title(
            "user-a", &agent, "Task",
        );
        storage.upsert_chat_session(&other).unwrap();
        let (isolated, _) =
            resolve_or_create_agent_task_session(&storage, "user-a", &agent, &other.session_id)
                .unwrap();
        assert_ne!(worker.session_id, isolated.session_id);
        worker.status = "archived".to_string();
        storage.upsert_chat_session(&worker).unwrap();
        let (next, created) =
            resolve_or_create_agent_task_session(&storage, "user-a", &agent, &parent.session_id)
                .unwrap();
        assert!(created);
        assert_ne!(next.session_id, worker.session_id);
        assert_eq!(
            storage
                .get_chat_session("user-a", &worker.session_id)
                .unwrap()
                .unwrap()
                .status,
            "archived"
        );
        let (_, created) =
            resolve_or_create_agent_task_session(&storage, "user-a", &agent, &parent.session_id)
                .unwrap();
        assert!(!created);
        assert!(!storage.insert_chat_session_if_absent(&worker).unwrap());
    }
}
