use crate::storage::StorageBackend;
use anyhow::{bail, Result};
use std::collections::HashSet;

pub(super) fn collect(storage: &dyn StorageBackend, user: &str, root: &str) -> Result<Vec<String>> {
    let mut pending = vec![root.to_string()];
    let mut seen = HashSet::from([root.to_string()]);
    let mut descendants = Vec::new();
    while let Some(parent) = pending.pop() {
        let mut offset = 0;
        loop {
            let (children, total) = storage.list_chat_sessions_by_status(
                user,
                None,
                Some(&parent),
                Some("all"),
                offset,
                256,
            )?;
            if children.is_empty() {
                break;
            }
            offset += children.len() as i64;
            for child in children {
                // Explicit user forks remain independent conversations.
                if child.spawned_by.as_deref() == Some("thread_control")
                    || !seen.insert(child.session_id.clone())
                {
                    continue;
                }
                if seen.len() > 65_536 {
                    bail!("child session tree exceeds cancellation traversal limit");
                }
                pending.push(child.session_id.clone());
                descendants.push(child.session_id);
            }
            if offset >= total {
                break;
            }
        }
    }
    Ok(descendants)
}
