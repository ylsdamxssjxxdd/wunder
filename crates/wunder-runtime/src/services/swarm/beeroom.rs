use crate::monitor::MonitorState;
use crate::services::user_store::{
    build_default_agent_record_from_storage, list_user_agents_by_hive_with_default,
};
use crate::storage::{
    normalize_hive_id, AgentThreadRecord, ChatSessionRecord, SessionRunRecord, StorageBackend,
    TeamRunRecord, TeamTaskRecord, UserAgentRecord, DEFAULT_HIVE_ID,
};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const BEE_ROOM_MOTHER_META_PREFIX: &str = "beeroom:mother:";
const BEE_ROOM_MOTHER_SESSION_META_PREFIX: &str = "beeroom:mother-session:";

#[derive(Debug, Clone, Deserialize, Serialize)]
struct MotherSessionBinding {
    agent_id: String,
    session_id: String,
    updated_at: f64,
}

#[derive(Debug, Clone, Default)]
pub struct AgentActivitySnapshot {
    pub lock_session_ids: HashSet<String>,
    pub running_session_ids: HashSet<String>,
}

impl AgentActivitySnapshot {
    pub fn active_session_ids(&self) -> Vec<String> {
        let mut items = self.lock_session_ids.clone();
        items.extend(self.running_session_ids.clone());
        let mut output = items.into_iter().collect::<Vec<_>>();
        output.sort();
        output
    }

    pub fn is_idle(&self) -> bool {
        self.lock_session_ids.is_empty() && self.running_session_ids.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct TeamRunSnapshot {
    pub run: TeamRunRecord,
    pub tasks: Vec<TeamTaskRecord>,
    pub completion_status: String,
    pub all_tasks_terminal: bool,
    pub all_agents_idle: bool,
    pub active_agent_ids: Vec<String>,
    pub idle_agent_ids: Vec<String>,
}

pub fn resolve_swarm_hive_id(
    storage: &dyn StorageBackend,
    user_id: &str,
    current_agent_id: Option<&str>,
    requested_hive_id: Option<&str>,
) -> Result<String> {
    let cleaned_user = user_id.trim();
    if cleaned_user.is_empty() {
        return Err(anyhow!("user_id is empty"));
    }

    let requested = requested_hive_id
        .map(normalize_hive_id)
        .filter(|value| !value.trim().is_empty());

    if let Some(agent_id) = current_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let agent = if is_default_agent_alias(agent_id) {
            build_default_agent_record_from_storage(storage, cleaned_user)?
        } else {
            storage
                .get_user_agent(cleaned_user, agent_id)?
                .or_else(|| storage.get_user_agent_by_id(agent_id).ok().flatten())
                .ok_or_else(|| anyhow!("current swarm agent not found"))?
        };
        let resolved = normalize_hive_id(&agent.hive_id);
        if let Some(requested) = requested.as_ref() {
            if requested != &resolved {
                return Err(anyhow!("requested hive is outside current agent hive"));
            }
        }
        return Ok(resolved);
    }

    if let Some(requested) = requested {
        let hive = storage.get_hive(cleaned_user, &requested)?;
        if hive.is_none() {
            return Err(anyhow!("requested hive not found"));
        }
        return Ok(requested);
    }

    Ok(DEFAULT_HIVE_ID.to_string())
}

pub fn ensure_swarm_agent_in_hive(agent: &UserAgentRecord, hive_id: &str) -> Result<()> {
    if normalize_hive_id(&agent.hive_id) == normalize_hive_id(hive_id) {
        return Ok(());
    }
    Err(anyhow!("target is outside current hive"))
}

pub fn agent_in_hive(agent: &UserAgentRecord, hive_id: &str) -> bool {
    normalize_hive_id(&agent.hive_id) == normalize_hive_id(hive_id)
}

pub fn mother_meta_key(user_id: &str, hive_id: &str) -> String {
    format!(
        "{BEE_ROOM_MOTHER_META_PREFIX}{}:{}",
        user_id.trim(),
        normalize_hive_id(hive_id)
    )
}

pub fn mother_session_meta_key(user_id: &str, hive_id: &str) -> String {
    format!(
        "{BEE_ROOM_MOTHER_SESSION_META_PREFIX}{}:{}",
        user_id.trim(),
        normalize_hive_id(hive_id)
    )
}

fn session_belongs_to_agent(record: &ChatSessionRecord, agent_id: &str) -> bool {
    let session_agent_id = record.agent_id.as_deref().map(str::trim).unwrap_or("");
    if is_default_agent_alias(agent_id) {
        session_agent_id.is_empty() || is_default_agent_alias(session_agent_id)
    } else {
        session_agent_id == agent_id.trim()
    }
}

pub fn resolve_bound_hive_mother_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    mother_agent_id: &str,
) -> Result<Option<ChatSessionRecord>> {
    let key = mother_session_meta_key(user_id, hive_id);
    let Some(raw) = storage.get_meta(&key)? else {
        return Ok(None);
    };
    let Ok(binding) = serde_json::from_str::<MotherSessionBinding>(&raw) else {
        return Ok(None);
    };
    if binding.agent_id.trim() != mother_agent_id.trim() || binding.session_id.trim().is_empty() {
        return Ok(None);
    }
    let Some(record) = storage.get_chat_session(user_id.trim(), binding.session_id.trim())? else {
        return Ok(None);
    };
    if record.status.trim().eq_ignore_ascii_case("archived")
        || !session_belongs_to_agent(&record, mother_agent_id)
    {
        return Ok(None);
    }
    Ok(Some(record))
}

fn bind_hive_mother_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    agent_id: &str,
    session_id: &str,
) -> Result<()> {
    let binding = MotherSessionBinding {
        agent_id: agent_id.trim().to_string(),
        session_id: session_id.trim().to_string(),
        updated_at: now_ts(),
    };
    storage.set_meta(
        &mother_session_meta_key(user_id, hive_id),
        &serde_json::to_string(&binding)?,
    )?;
    Ok(())
}

fn session_is_bound_to_another_hive(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    agent_id: &str,
    session_id: &str,
) -> Result<bool> {
    let current_key = mother_session_meta_key(user_id, hive_id);
    let prefix = format!("{BEE_ROOM_MOTHER_SESSION_META_PREFIX}{}:", user_id.trim());
    for (key, raw) in storage.list_meta_prefix(&prefix)? {
        if key == current_key {
            continue;
        }
        let Ok(binding) = serde_json::from_str::<MotherSessionBinding>(&raw) else {
            continue;
        };
        if binding.agent_id.trim() == agent_id.trim()
            && binding.session_id.trim() == session_id.trim()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn resolve_or_create_hive_mother_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    mother_agent: &UserAgentRecord,
) -> Result<(ChatSessionRecord, bool)> {
    let normalized_hive_id = normalize_hive_id(hive_id);
    if !agent_in_hive(mother_agent, &normalized_hive_id) {
        return Err(anyhow!("mother agent is outside current hive"));
    }
    let bound_session = resolve_bound_hive_mother_session(
        storage,
        user_id,
        &normalized_hive_id,
        &mother_agent.agent_id,
    )?;

    // A manually selected/newly created main thread is the user's explicit
    // continuation target. Adopt it for this hive unless another hive already
    // owns that dedicated mother session.
    if let Some(main_session) =
        resolve_agent_main_session(storage, user_id, &mother_agent.agent_id)?
    {
        let differs_from_binding = bound_session
            .as_ref()
            .is_none_or(|bound| bound.session_id != main_session.session_id);
        if differs_from_binding
            && !session_is_bound_to_another_hive(
                storage,
                user_id,
                &normalized_hive_id,
                &mother_agent.agent_id,
                &main_session.session_id,
            )?
        {
            bind_hive_mother_session(
                storage,
                user_id,
                &normalized_hive_id,
                &mother_agent.agent_id,
                &main_session.session_id,
            )?;
            return Ok((main_session, false));
        }
    }

    if let Some(record) = bound_session {
        return Ok((record, false));
    }

    let cleaned_user = user_id.trim();
    let cleaned_agent = mother_agent.agent_id.trim();
    if cleaned_user.is_empty() || cleaned_agent.is_empty() {
        return Err(anyhow!("user_id or agent_id is empty"));
    }
    let now = now_ts();
    let session_scope = format!("{cleaned_user}:{normalized_hive_id}:{cleaned_agent}");
    let stable_session_id = format!(
        "sess_{}",
        Uuid::new_v5(&Uuid::NAMESPACE_OID, session_scope.as_bytes()).simple()
    );
    let existing_stable_session = storage.get_chat_session(cleaned_user, &stable_session_id)?;
    let session_id = if existing_stable_session.as_ref().is_some_and(|record| {
        record.status.trim().eq_ignore_ascii_case("archived")
            || !session_belongs_to_agent(record, cleaned_agent)
    }) {
        format!("sess_{}", Uuid::new_v4().simple())
    } else {
        stable_session_id
    };
    let title = mother_agent
        .name
        .trim()
        .strip_prefix('@')
        .unwrap_or(mother_agent.name.trim())
        .trim();
    let reused_existing = existing_stable_session.as_ref().is_some_and(|record| {
        record.session_id == session_id && session_belongs_to_agent(record, cleaned_agent)
    });
    let record = existing_stable_session
        .filter(|record| {
            record.session_id == session_id && session_belongs_to_agent(record, cleaned_agent)
        })
        .unwrap_or_else(|| ChatSessionRecord {
            session_id: session_id.clone(),
            user_id: cleaned_user.to_string(),
            title: if title.is_empty() {
                cleaned_agent.to_string()
            } else {
                title.to_string()
            },
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: if is_default_agent_alias(cleaned_agent) {
                None
            } else {
                Some(cleaned_agent.to_string())
            },
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        });
    storage.upsert_chat_session(&record)?;
    bind_hive_mother_session(
        storage,
        user_id,
        &normalized_hive_id,
        cleaned_agent,
        &record.session_id,
    )?;
    Ok((record, !reused_existing))
}

pub fn get_mother_agent_id(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
) -> Result<Option<String>> {
    let key = mother_meta_key(user_id, hive_id);
    let Some(raw) = storage.get_meta(&key)? else {
        return Ok(None);
    };
    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(agent_id) = payload.get("agent_id").and_then(serde_json::Value::as_str) {
            let cleaned = agent_id.trim();
            if !cleaned.is_empty() {
                return Ok(Some(cleaned.to_string()));
            }
        }
    }
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    Ok(Some(cleaned.to_string()))
}

pub fn claim_mother_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    candidate_agent_id: &str,
) -> Result<String> {
    let candidate = candidate_agent_id.trim();
    if candidate.is_empty() {
        return Err(anyhow!("candidate mother agent is empty"));
    }

    if let Some(existing) = get_mother_agent_id(storage, user_id, hive_id)? {
        if let Some(agent) = storage.get_user_agent_by_id(&existing)? {
            if agent_in_hive(&agent, hive_id) {
                return Ok(existing);
            }
        }
    }

    let resolved = resolve_preferred_mother_agent_id(storage, user_id, hive_id, Some(candidate))?
        .unwrap_or_else(|| candidate.to_string());
    set_mother_agent(storage, user_id, hive_id, &resolved)
}

pub fn resolve_preferred_mother_agent_id(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    candidate_agent_id: Option<&str>,
) -> Result<Option<String>> {
    let candidate = candidate_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut preferred = storage
        .list_user_agents_by_hive(user_id, hive_id)?
        .into_iter()
        .filter(|agent| agent.prefer_mother)
        .collect::<Vec<_>>();
    if preferred.is_empty() {
        return Ok(candidate);
    }
    preferred.sort_by(|left, right| {
        if let Some(candidate_id) = candidate.as_deref() {
            let left_is_candidate = left.agent_id == candidate_id;
            let right_is_candidate = right.agent_id == candidate_id;
            if left_is_candidate != right_is_candidate {
                return right_is_candidate.cmp(&left_is_candidate);
            }
        }
        right
            .updated_at
            .partial_cmp(&left.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.agent_id.cmp(&right.agent_id))
    });
    Ok(preferred.into_iter().next().map(|agent| agent.agent_id))
}

pub fn set_mother_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    hive_id: &str,
    candidate_agent_id: &str,
) -> Result<String> {
    let candidate = candidate_agent_id.trim();
    if candidate.is_empty() {
        return Err(anyhow!("candidate mother agent is empty"));
    }

    let key = mother_meta_key(user_id, hive_id);
    let payload = json!({
        "agent_id": candidate,
        "claimed_at": now_ts(),
    });
    storage.set_meta(&key, &payload.to_string())?;
    Ok(candidate.to_string())
}

pub fn resolve_agent_main_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
) -> Result<Option<ChatSessionRecord>> {
    let cleaned_user = user_id.trim();
    let cleaned_agent = agent_id.trim();
    if cleaned_user.is_empty() || cleaned_agent.is_empty() {
        return Ok(None);
    }
    let thread_agent_id = if is_default_agent_alias(cleaned_agent) {
        ""
    } else {
        cleaned_agent
    };

    let existing_thread = storage.get_agent_thread(cleaned_user, thread_agent_id)?;
    if let Some(session_id) = existing_thread
        .as_ref()
        .map(|record| record.session_id.trim())
        .filter(|value| !value.is_empty())
    {
        if let Some(record) = storage.get_chat_session(cleaned_user, session_id)? {
            let record_agent_id = record.agent_id.as_deref().map(str::trim).unwrap_or("");
            if (is_default_agent_alias(cleaned_agent) && record_agent_id.is_empty())
                || record_agent_id == cleaned_agent
            {
                return Ok(Some(record));
            }
        }
    }
    // Do not silently promote an arbitrary historical session as the main thread.
    // If the explicit binding is missing or stale, callers should create a fresh
    // main thread through `resolve_or_create_agent_main_session`.
    Ok(None)
}

pub fn resolve_or_create_agent_main_session(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent: &UserAgentRecord,
) -> Result<(ChatSessionRecord, bool)> {
    if let Some(record) = resolve_agent_main_session(storage, user_id, &agent.agent_id)? {
        return Ok((record, false));
    }

    let cleaned_user = user_id.trim();
    let cleaned_agent = agent.agent_id.trim();
    if cleaned_user.is_empty() || cleaned_agent.is_empty() {
        return Err(anyhow!("user_id or agent_id is empty"));
    }

    let now = now_ts();
    let session_id = format!("sess_{}", Uuid::new_v4().simple());
    let title = agent
        .name
        .trim()
        .strip_prefix('@')
        .unwrap_or(agent.name.trim())
        .trim()
        .to_string();
    let record = ChatSessionRecord {
        session_id: session_id.clone(),
        user_id: cleaned_user.to_string(),
        title: if title.is_empty() {
            cleaned_agent.to_string()
        } else {
            title
        },
        status: "active".to_string(),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: Some(cleaned_agent.to_string()),
        tool_overrides: Vec::new(),
        parent_session_id: None,
        parent_message_id: None,
        spawn_label: None,
        spawned_by: None,
    };
    storage.upsert_chat_session(&record)?;
    let existing_thread = storage.get_agent_thread(cleaned_user, cleaned_agent)?;
    bind_agent_main_thread(
        storage,
        cleaned_user,
        cleaned_agent,
        &session_id,
        existing_thread,
    )?;
    Ok((record, true))
}

pub fn collect_agent_activity(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    user_id: &str,
    hive_id: &str,
    agents: &[UserAgentRecord],
) -> Result<HashMap<String, AgentActivitySnapshot>> {
    let normalized_hive_id = normalize_hive_id(hive_id);
    let agent_ids = agents
        .iter()
        .filter(|agent| agent_in_hive(agent, &normalized_hive_id))
        .map(|agent| agent.agent_id.clone())
        .collect::<HashSet<_>>();

    let mut output = HashMap::new();
    for lock in storage.list_session_locks_by_user(user_id)? {
        let agent_id = lock.agent_id.trim();
        let session_id = lock.session_id.trim();
        if agent_id.is_empty() || session_id.is_empty() || !agent_ids.contains(agent_id) {
            continue;
        }
        output
            .entry(agent_id.to_string())
            .or_insert_with(AgentActivitySnapshot::default)
            .lock_session_ids
            .insert(session_id.to_string());
    }

    if let Some(monitor) = monitor {
        for session in monitor.list_sessions(true) {
            let session_user_id = session
                .get("user_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            if session_user_id.trim() != user_id.trim() {
                continue;
            }
            let agent_id = session
                .get("agent_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let session_id = session
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if agent_id.is_empty() || session_id.is_empty() || !agent_ids.contains(agent_id) {
                continue;
            }
            output
                .entry(agent_id.to_string())
                .or_insert_with(AgentActivitySnapshot::default)
                .running_session_ids
                .insert(session_id.to_string());
        }
    }

    Ok(output)
}

#[allow(clippy::too_many_arguments)]
pub fn build_swarm_dispatch_message(
    storage: &dyn StorageBackend,
    _monitor: Option<&MonitorState>,
    user_id: &str,
    hive_id: &str,
    _sender_agent_id: Option<&str>,
    _source_session_id: &str,
    _team_run_id: Option<&str>,
    _task_id: Option<&str>,
    original_message: &str,
) -> Result<String> {
    let hive = storage.get_hive(user_id, hive_id)?;
    let members = list_user_agents_by_hive_with_default(storage, user_id, hive_id)?;
    let mother_agent_id = get_mother_agent_id(storage, user_id, hive_id)?;
    let mother_agent_name = mother_agent_id
        .as_deref()
        .and_then(|mother_id| {
            members
                .iter()
                .find(|agent| agent.agent_id.trim() == mother_id.trim())
        })
        .map(|agent| agent.name.trim().to_string())
        .filter(|name| !name.is_empty());

    // Keep swarm context minimal to reduce prompt noise and chat bubble overflow.
    let payload = json!({
        "swarm": {
            "hive_name": hive.as_ref().map(|item| item.name.clone()),
            "mother_agent_name": mother_agent_name,
        },
    });
    let payload_pretty =
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string());
    Ok(format!(
        "[SWARM_CONTEXT]
{}
[/SWARM_CONTEXT]

任务指令：
{}",
        payload_pretty,
        original_message.trim()
    ))
}

pub fn snapshot_team_run(
    storage: &dyn StorageBackend,
    monitor: Option<&MonitorState>,
    run: &TeamRunRecord,
) -> Result<TeamRunSnapshot> {
    let mut tasks = storage.list_team_tasks(&run.team_run_id)?;
    let agents = list_user_agents_by_hive_with_default(storage, &run.user_id, &run.hive_id)?;
    let activity = collect_agent_activity(storage, monitor, &run.user_id, &run.hive_id, &agents)?;

    let mut success_total = 0i64;
    let mut failed_total = 0i64;
    let mut started_time = run.started_time;
    let mut finished_time = run.finished_time;
    let mut updated_time = run.updated_time;
    let mut result_run = run.clone();
    let mut involved_agents = HashSet::new();

    for task in &mut tasks {
        involved_agents.insert(task.agent_id.clone());
        if let Some(session_run_id) = task
            .session_run_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(session_run) = storage.get_session_run(session_run_id)? {
                apply_session_run_to_task(task, &session_run);
            }
        }
        if is_terminal_status(&task.status) && started_time.is_none() {
            started_time = task.started_time;
        }
        if let Some(task_started) = task.started_time {
            started_time = Some(
                started_time
                    .map(|value| value.min(task_started))
                    .unwrap_or(task_started),
            );
        }
        if let Some(task_finished) = task.finished_time {
            finished_time = Some(
                finished_time
                    .map(|value| value.max(task_finished))
                    .unwrap_or(task_finished),
            );
        }
        updated_time = updated_time.max(task.updated_time);
        match normalize_status(&task.status).as_str() {
            "success" => success_total += 1,
            "error" | "failed" | "timeout" | "cancelled" => failed_total += 1,
            _ => {}
        }
    }

    let all_tasks_terminal = tasks.iter().all(|task| is_terminal_status(&task.status));
    let mut active_agent_ids = involved_agents
        .iter()
        .filter(|agent_id| activity.get(*agent_id).is_some_and(|item| !item.is_idle()))
        .cloned()
        .collect::<Vec<_>>();
    active_agent_ids.sort();
    let mut idle_agent_ids = involved_agents
        .iter()
        .filter(|agent_id| {
            activity
                .get(*agent_id)
                .is_none_or(AgentActivitySnapshot::is_idle)
        })
        .cloned()
        .collect::<Vec<_>>();
    idle_agent_ids.sort();
    let all_agents_idle = active_agent_ids.is_empty();
    let completion_status = if !all_tasks_terminal {
        "running".to_string()
    } else if !all_agents_idle {
        "awaiting_idle".to_string()
    } else if failed_total > 0 {
        if tasks
            .iter()
            .all(|task| normalize_status(&task.status) == "cancelled")
        {
            "cancelled".to_string()
        } else {
            "failed".to_string()
        }
    } else {
        "completed".to_string()
    };

    result_run.task_total = tasks.len() as i64;
    result_run.task_success = success_total;
    result_run.task_failed = failed_total;
    result_run.started_time = started_time;
    result_run.finished_time = if all_tasks_terminal {
        finished_time
    } else {
        None
    };
    result_run.elapsed_s = match (result_run.started_time, result_run.finished_time) {
        (Some(started), Some(finished)) => Some((finished - started).max(0.0)),
        _ => result_run.elapsed_s,
    };
    result_run.updated_time = updated_time;
    result_run.status = completion_status.clone();

    Ok(TeamRunSnapshot {
        run: result_run,
        tasks,
        completion_status,
        all_tasks_terminal,
        all_agents_idle,
        active_agent_ids,
        idle_agent_ids,
    })
}

fn apply_session_run_to_task(task: &mut TeamTaskRecord, session_run: &SessionRunRecord) {
    task.status = normalize_status(&session_run.status);
    if session_run.started_time > 0.0 {
        task.started_time = Some(session_run.started_time);
    }
    if session_run.finished_time > 0.0 {
        task.finished_time = Some(session_run.finished_time);
    }
    if session_run.elapsed_s > 0.0 {
        task.elapsed_s = Some(session_run.elapsed_s);
    }
    if let Some(result) = session_run
        .result
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        task.result_summary = Some(result.to_string());
    }
    if let Some(error) = session_run
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        task.error = Some(error.to_string());
    }
    task.updated_time = task.updated_time.max(session_run.updated_time);
}

fn normalize_status(status: &str) -> String {
    status.trim().to_ascii_lowercase()
}

fn is_terminal_status(status: &str) -> bool {
    matches!(
        normalize_status(status).as_str(),
        "success" | "error" | "failed" | "timeout" | "cancelled"
    )
}

fn bind_agent_main_thread(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: &str,
    session_id: &str,
    existing: Option<AgentThreadRecord>,
) -> Result<()> {
    let now = now_ts();
    let (created_at, status) = if let Some(record) = existing {
        let next_status = if record.status.trim().is_empty() {
            "idle".to_string()
        } else {
            record.status
        };
        (record.created_at, next_status)
    } else {
        (now, "idle".to_string())
    };
    let record = AgentThreadRecord {
        thread_id: format!("thread_{session_id}"),
        user_id: user_id.to_string(),
        agent_id: agent_id.to_string(),
        session_id: session_id.to_string(),
        status,
        created_at,
        updated_at: now,
    };
    storage.upsert_agent_thread(&record)?;
    Ok(())
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn is_default_agent_alias(agent_id: &str) -> bool {
    let cleaned = agent_id.trim();
    cleaned.eq_ignore_ascii_case("__default__") || cleaned.eq_ignore_ascii_case("default")
}

#[cfg(test)]
mod tests {
    use super::{
        bind_agent_main_thread, build_swarm_dispatch_message, resolve_agent_main_session,
        resolve_or_create_agent_main_session, resolve_or_create_hive_mother_session,
        resolve_swarm_hive_id, set_mother_agent,
    };
    use crate::storage::*;
    use serde_json::Value;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn resolve_agent_main_session_requires_explicit_main_thread_binding() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-main-thread.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));

        let session = ChatSessionRecord {
            session_id: "sess_existing".to_string(),
            user_id: "alice".to_string(),
            title: "Intel".to_string(),
            status: "active".to_string(),
            created_at: 10.0,
            updated_at: 12.0,
            last_message_at: 12.0,
            agent_id: Some("agent-intel".to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        storage
            .upsert_chat_session(&session)
            .expect("upsert chat session");

        let resolved = resolve_agent_main_session(storage.as_ref(), "alice", "agent-intel")
            .expect("resolve main session");

        assert!(resolved.is_none());
        assert!(storage
            .get_agent_thread("alice", "agent-intel")
            .expect("get agent thread")
            .is_none());
    }

    #[test]
    fn resolve_or_create_agent_main_session_does_not_promote_arbitrary_existing_session() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-create-main-thread-fresh.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));

        let old_session = ChatSessionRecord {
            session_id: "sess_old".to_string(),
            user_id: "alice".to_string(),
            title: "Legacy".to_string(),
            status: "active".to_string(),
            created_at: 5.0,
            updated_at: 8.0,
            last_message_at: 8.0,
            agent_id: Some("agent-ops".to_string()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        storage
            .upsert_chat_session(&old_session)
            .expect("upsert old chat session");

        let agent = UserAgentRecord {
            agent_id: "agent-ops".to_string(),
            user_id: "alice".to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "Ops Analyst".to_string(),
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
            prefer_mother: false,
        };

        let (session, created) =
            resolve_or_create_agent_main_session(storage.as_ref(), "alice", &agent)
                .expect("resolve or create main session");

        assert!(created);
        assert_ne!(session.session_id, old_session.session_id);
    }

    #[test]
    fn resolve_or_create_agent_main_session_creates_and_binds_when_missing() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-create-main-thread.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));

        let agent = UserAgentRecord {
            agent_id: "agent-ops".to_string(),
            user_id: "alice".to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "Ops Analyst".to_string(),
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
            prefer_mother: false,
        };

        let (session, created) =
            resolve_or_create_agent_main_session(storage.as_ref(), "alice", &agent)
                .expect("resolve or create main session");
        let thread = storage
            .get_agent_thread("alice", "agent-ops")
            .expect("get agent thread")
            .expect("thread record");

        assert!(created);
        assert_eq!(thread.session_id, session.session_id);
        assert_eq!(session.agent_id.as_deref(), Some("agent-ops"));
    }

    #[test]
    fn hive_mother_sessions_are_stable_and_isolated_by_hive() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-hive-mother-session.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let build_agent = |hive_id: &str| UserAgentRecord {
            agent_id: "agent-shared".to_string(),
            user_id: "user-a".to_string(),
            hive_id: hive_id.to_string(),
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

        let (first, first_created) = resolve_or_create_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-a",
            &build_agent("hive-a"),
        )
        .expect("create first hive session");
        let (first_again, first_again_created) = resolve_or_create_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-a",
            &build_agent("hive-a"),
        )
        .expect("reuse first hive session");
        let (second, second_created) = resolve_or_create_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-b",
            &build_agent("hive-b"),
        )
        .expect("create second hive session");

        assert!(first_created);
        assert!(!first_again_created);
        assert!(second_created);
        assert_eq!(first.session_id, first_again.session_id);
        assert_ne!(first.session_id, second.session_id);
    }

    #[test]
    fn hive_mother_session_adopts_a_fresh_explicit_main_thread() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-hive-adopt-main.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let agent = UserAgentRecord {
            agent_id: "agent-mother".to_string(),
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
        let (old_session, _) =
            resolve_or_create_hive_mother_session(storage.as_ref(), "user-a", "hive-a", &agent)
                .expect("create old hive session");
        let fresh_session = ChatSessionRecord {
            session_id: "sess-fresh".to_string(),
            user_id: "user-a".to_string(),
            title: "Fresh".to_string(),
            status: "active".to_string(),
            created_at: 2.0,
            updated_at: 2.0,
            last_message_at: 2.0,
            agent_id: Some(agent.agent_id.clone()),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        storage
            .upsert_chat_session(&fresh_session)
            .expect("upsert fresh session");
        bind_agent_main_thread(
            storage.as_ref(),
            "user-a",
            &agent.agent_id,
            &fresh_session.session_id,
            None,
        )
        .expect("bind fresh main thread");

        let (resolved, created) =
            resolve_or_create_hive_mother_session(storage.as_ref(), "user-a", "hive-a", &agent)
                .expect("adopt fresh main thread");
        let rebound = super::resolve_bound_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-a",
            &agent.agent_id,
        )
        .expect("resolve rebound session")
        .expect("rebound session");

        assert!(!created);
        assert_ne!(resolved.session_id, old_session.session_id);
        assert_eq!(resolved.session_id, fresh_session.session_id);
        assert_eq!(rebound.session_id, fresh_session.session_id);
    }

    #[test]
    fn hive_mother_session_does_not_adopt_another_hives_binding() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-hive-binding-isolation.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let build_agent = |hive_id: &str| UserAgentRecord {
            agent_id: "agent-shared".to_string(),
            user_id: "user-a".to_string(),
            hive_id: hive_id.to_string(),
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
        let first_agent = build_agent("hive-a");
        let (first, _) = resolve_or_create_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-a",
            &first_agent,
        )
        .expect("create first hive session");
        bind_agent_main_thread(
            storage.as_ref(),
            "user-a",
            &first_agent.agent_id,
            &first.session_id,
            None,
        )
        .expect("bind first hive main thread");

        let (second, second_created) = resolve_or_create_hive_mother_session(
            storage.as_ref(),
            "user-a",
            "hive-b",
            &build_agent("hive-b"),
        )
        .expect("create isolated second hive session");

        assert!(second_created);
        assert_ne!(first.session_id, second.session_id);
    }

    #[test]
    fn resolve_swarm_hive_id_accepts_default_agent_alias() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-default-agent-hive.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));

        let resolved = resolve_swarm_hive_id(storage.as_ref(), "alice", Some("__default__"), None)
            .expect("resolve hive id");

        assert_eq!(resolved, DEFAULT_HIVE_ID);
    }

    #[test]
    fn build_swarm_dispatch_message_keeps_only_minimal_swarm_context() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("beeroom-dispatch-message.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));

        let hive = HiveRecord {
            hive_id: "hive_blue".to_string(),
            user_id: "alice".to_string(),
            name: "AI示例蜂群".to_string(),
            description: String::new(),
            is_default: false,
            status: "active".to_string(),
            created_time: 1.0,
            updated_time: 1.0,
        };
        storage.upsert_hive(&hive).expect("upsert hive");

        let mother_agent = UserAgentRecord {
            agent_id: "agent_mother".to_string(),
            user_id: "alice".to_string(),
            hive_id: hive.hive_id.clone(),
            name: "示例母蜂".to_string(),
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
        storage
            .upsert_user_agent(&mother_agent)
            .expect("upsert mother agent");
        set_mother_agent(
            storage.as_ref(),
            "alice",
            &hive.hive_id,
            &mother_agent.agent_id,
        )
        .expect("set mother agent");

        let worker_agent = UserAgentRecord {
            agent_id: "agent_worker".to_string(),
            user_id: "alice".to_string(),
            hive_id: hive.hive_id.clone(),
            name: "侦察工蜂".to_string(),
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
            prefer_mother: false,
        };
        storage
            .upsert_user_agent(&worker_agent)
            .expect("upsert worker agent");

        let message = build_swarm_dispatch_message(
            storage.as_ref(),
            None,
            "alice",
            &hive.hive_id,
            Some(&mother_agent.agent_id),
            "sess_demo",
            None,
            None,
            "执行侦察任务",
        )
        .expect("build dispatch message");

        let start = message.find('{').expect("json start");
        let end = message.find("\n[/SWARM_CONTEXT]").expect("json end");
        let payload: Value =
            serde_json::from_str(&message[start..end]).expect("parse swarm context payload");

        assert_eq!(
            payload,
            serde_json::json!({
                "swarm": {
                    "hive_name": "AI示例蜂群",
                    "mother_agent_name": "示例母蜂"
                }
            })
        );
    }
}
