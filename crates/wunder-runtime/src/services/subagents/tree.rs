//! Durable task-tree identity. Ordinary user forks and swarm threads start new scopes.
use crate::storage::{ChatSessionRecord, StorageBackend};
use anyhow::{anyhow, bail, Result};
use std::collections::HashSet;

pub(crate) const MAX_DEPTH: usize = 32;

pub(crate) struct Identity {
    pub root: String,
    pub path: String,
    pub depth: usize,
}

pub(crate) fn is_child(session: &ChatSessionRecord) -> bool {
    matches!(
        session.spawned_by.as_deref(),
        Some("model" | "subagent_control")
    )
}

pub(crate) fn identity(
    storage: &dyn StorageBackend,
    user: &str,
    session: &str,
) -> Result<Identity> {
    let mut cursor = session.to_string();
    let mut seen = HashSet::new();
    let mut segments = Vec::new();
    loop {
        if !seen.insert(cursor.clone()) || segments.len() > MAX_DEPTH {
            bail!("invalid or excessively deep subagent tree");
        }
        let record = storage
            .get_chat_session(user, &cursor)?
            .ok_or_else(|| anyhow!("task thread not found"))?;
        if !is_child(&record) {
            segments.reverse();
            return Ok(Identity {
                root: cursor,
                depth: segments.len(),
                path: if segments.is_empty() {
                    "/root".into()
                } else {
                    format!("/root/{}", segments.join("/"))
                },
            });
        }
        segments.push(cursor);
        cursor = record
            .parent_session_id
            .filter(|id| !id.is_empty())
            .ok_or_else(|| anyhow!("subagent parent is missing"))?;
    }
}

/// All model-facing selectors use this boundary, including read-only selectors.
pub(crate) fn authorize(
    storage: &dyn StorageBackend,
    user: &str,
    caller: &str,
    target: &str,
    allow_root: bool,
) -> Result<Identity> {
    let caller_identity = identity(storage, user, caller)?;
    let target_identity = identity(storage, user, target)?;
    if caller_identity.root != target_identity.root || (!allow_root && target_identity.depth == 0) {
        bail!("subagent target must belong to the current task tree; root is not a worker target");
    }
    Ok(target_identity)
}

pub(crate) fn resolve(
    storage: &dyn StorageBackend,
    user: &str,
    caller: &str,
    target: &str,
) -> Result<String> {
    if !target.starts_with('/') {
        return Ok(target.to_string());
    }
    if target == "/root" {
        return Ok(identity(storage, user, caller)?.root);
    }
    let id = target
        .rsplit('/')
        .next()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("invalid task path"))?;
    let resolved = authorize(storage, user, caller, id, false)?;
    if resolved.path != target {
        bail!("task path does not match persisted ancestry");
    }
    Ok(id.to_string())
}

pub(crate) fn ancestors(
    storage: &dyn StorageBackend,
    user: &str,
    session: &str,
) -> Result<Vec<String>> {
    let identity = identity(storage, user, session)?;
    let mut ids: Vec<String> = identity
        .path
        .split('/')
        .skip(2)
        .map(str::to_string)
        .collect();
    ids.pop();
    if identity.depth > 0 {
        ids.insert(0, identity.root);
    }
    Ok(ids)
}
