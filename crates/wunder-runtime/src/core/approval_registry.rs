use crate::core::approval::{ApprovalRequestKind, ApprovalResponse};
use std::collections::HashMap;
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalSource {
    ChatWs,
    Channel,
}

#[derive(Debug)]
pub struct PendingApprovalEntry {
    pub approval_id: String,
    pub source: ApprovalSource,
    pub session_id: String,
    pub request_id: Option<String>,
    pub channel: Option<String>,
    pub account_id: Option<String>,
    pub peer_id: Option<String>,
    pub thread_id: Option<String>,
    pub actor_id: Option<String>,
    pub tool: String,
    pub summary: String,
    pub kind: ApprovalRequestKind,
    pub created_at: f64,
    pub respond_to: oneshot::Sender<ApprovalResponse>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingApprovalSnapshot {
    pub approval_id: String,
    pub source: ApprovalSource,
    pub session_id: String,
    pub request_id: Option<String>,
    pub channel: Option<String>,
    pub account_id: Option<String>,
    pub peer_id: Option<String>,
    pub thread_id: Option<String>,
    pub actor_id: Option<String>,
    pub tool: String,
    pub summary: String,
    pub kind: ApprovalRequestKind,
    pub created_at: f64,
}

impl PendingApprovalEntry {
    pub fn snapshot(&self) -> PendingApprovalSnapshot {
        PendingApprovalSnapshot {
            approval_id: self.approval_id.clone(),
            source: self.source,
            session_id: self.session_id.clone(),
            request_id: self.request_id.clone(),
            channel: self.channel.clone(),
            account_id: self.account_id.clone(),
            peer_id: self.peer_id.clone(),
            thread_id: self.thread_id.clone(),
            actor_id: self.actor_id.clone(),
            tool: self.tool.clone(),
            summary: self.summary.clone(),
            kind: self.kind,
            created_at: self.created_at,
        }
    }
}

#[derive(Default)]
pub struct PendingApprovalRegistry {
    inner: Mutex<HashMap<String, PendingApprovalEntry>>,
}

impl PendingApprovalRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn upsert(&self, entry: PendingApprovalEntry) -> Option<PendingApprovalEntry> {
        let approval_id = entry.approval_id.clone();
        let mut guard = self.inner.lock().await;
        guard.insert(approval_id, entry)
    }

    pub async fn remove(&self, approval_id: &str) -> Option<PendingApprovalEntry> {
        let mut guard = self.inner.lock().await;
        guard.remove(approval_id)
    }

    pub async fn get_snapshot(&self, approval_id: &str) -> Option<PendingApprovalSnapshot> {
        let guard = self.inner.lock().await;
        guard.get(approval_id).map(PendingApprovalEntry::snapshot)
    }

    pub async fn find_snapshots<F>(&self, predicate: F) -> Vec<PendingApprovalSnapshot>
    where
        F: Fn(&PendingApprovalSnapshot) -> bool,
    {
        let guard = self.inner.lock().await;
        guard
            .values()
            .map(PendingApprovalEntry::snapshot)
            .filter(predicate)
            .collect()
    }

    pub async fn remove_matching<F>(&self, predicate: F) -> Vec<PendingApprovalEntry>
    where
        F: Fn(&PendingApprovalSnapshot) -> bool,
    {
        let mut guard = self.inner.lock().await;
        let ids = guard
            .values()
            .map(PendingApprovalEntry::snapshot)
            .filter(predicate)
            .map(|entry| entry.approval_id)
            .collect::<Vec<_>>();
        let mut removed = Vec::with_capacity(ids.len());
        for approval_id in ids {
            if let Some(entry) = guard.remove(&approval_id) {
                removed.push(entry);
            }
        }
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_entry(approval_id: &str) -> PendingApprovalEntry {
        let (tx, _rx) = oneshot::channel();
        PendingApprovalEntry {
            approval_id: approval_id.to_string(),
            source: ApprovalSource::ChatWs,
            session_id: "sess_1".to_string(),
            request_id: Some("req_1".to_string()),
            channel: None,
            account_id: None,
            peer_id: None,
            thread_id: None,
            actor_id: None,
            tool: "execute_command".to_string(),
            summary: "run command".to_string(),
            kind: ApprovalRequestKind::Exec,
            created_at: 1.0,
            respond_to: tx,
        }
    }

    #[tokio::test]
    async fn registry_upsert_and_find_snapshot() {
        let registry = PendingApprovalRegistry::new();
        registry.upsert(build_entry("appr_1")).await;
        let snapshot = registry.get_snapshot("appr_1").await.expect("snapshot");
        assert_eq!(snapshot.approval_id, "appr_1");
        assert_eq!(snapshot.request_id.as_deref(), Some("req_1"));
        assert_eq!(snapshot.source, ApprovalSource::ChatWs);
    }

    #[tokio::test]
    async fn registry_remove_matching_filters_by_source() {
        let registry = PendingApprovalRegistry::new();
        registry.upsert(build_entry("appr_ws")).await;
        let (tx, _rx) = oneshot::channel();
        registry
            .upsert(PendingApprovalEntry {
                approval_id: "appr_channel".to_string(),
                source: ApprovalSource::Channel,
                session_id: "sess_1".to_string(),
                request_id: None,
                channel: Some("xmpp".to_string()),
                account_id: Some("acc_1".to_string()),
                peer_id: Some("peer_1".to_string()),
                thread_id: None,
                actor_id: None,
                tool: "apply_patch".to_string(),
                summary: "patch file".to_string(),
                kind: ApprovalRequestKind::Patch,
                created_at: 2.0,
                respond_to: tx,
            })
            .await;

        let removed = registry
            .remove_matching(|entry| entry.source == ApprovalSource::ChatWs)
            .await;
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].approval_id, "appr_ws");

        let remaining = registry
            .get_snapshot("appr_channel")
            .await
            .expect("channel approval remains");
        assert_eq!(remaining.source, ApprovalSource::Channel);
    }
}
