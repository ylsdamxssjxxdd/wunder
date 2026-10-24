//! Test fixtures that create canonical threads through the production store.
//!
//! A test must never fabricate completed history without the immutable session
//! metadata that a real executor freezes before its first append.

use super::thread_store::LocalThreadStore;
use serde_json::Value;
use std::path::Path;
use wunder_core::thread_store::{
    CreateThreadRequest, RolloutId, SessionMeta, ThreadId, ThreadMetadata, ThreadMetadataStore,
    ThreadStatus, TurnId,
};

pub(crate) fn freeze_thread(root: &Path, user: &str, session: &str) {
    let store = LocalThreadStore::open(root).expect("open canonical test thread store");
    store
        .admission_store()
        .expect("open canonical test admissions")
        .register_owner(user, session)
        .expect("register canonical test thread owner");
    let thread = ThreadId::new(session).expect("valid canonical test thread id");
    let rollout = RolloutId::new(format!("fixture-rollout-{session}"))
        .expect("valid canonical test rollout id");
    super::canonical_thread_log::wait(store.create_thread(CreateThreadRequest {
        metadata: ThreadMetadata {
            thread_id: thread.clone(),
            current_rollout_id: rollout.clone(),
            status: ThreadStatus::Active,
            created_at_ms: 1,
            updated_at_ms: 1,
            title: None,
            workspace_id: None,
            pinned: false,
        },
        session_meta: SessionMeta {
            thread_id: thread,
            rollout_id: rollout,
            created_at_ms: 1,
            source: "test_fixture".into(),
            initial_system_prompt: "frozen test instructions".into(),
            initial_memory: None,
            parent_thread_id: None,
            history_base: None,
            capabilities: Value::Null,
        },
    }))
    .expect("run canonical test thread creation")
    .expect("freeze canonical test thread");
}

pub(crate) fn begin_turn(root: &Path, user: &str, session: &str, turn: &str) {
    let store = LocalThreadStore::open(root).expect("open canonical test thread store");
    let thread = ThreadId::new(session).expect("valid canonical test thread id");
    let turn = TurnId::new(turn).expect("valid canonical test turn id");
    super::canonical_thread_log::wait(store.begin_turn(user, &thread, &turn, "fixture-model"))
        .expect("run canonical test turn start")
        .expect("begin canonical test turn");
}
