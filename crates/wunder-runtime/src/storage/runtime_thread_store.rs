#[cfg(feature = "postgres-storage")]
use crate::storage::postgres::PostgresAdmissionStore;
#[cfg(feature = "postgres-storage")]
use crate::storage::postgres::PostgresThreadStore;
use crate::storage::thread_store::LocalThreadStore;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use wunder_core::thread_store::*;

#[derive(Clone)]
pub struct RuntimeThreadStore {
    inner: Backend,
}

#[derive(Clone)]
enum Backend {
    Local(Arc<LocalThreadStore>),
    #[cfg(feature = "postgres-storage")]
    Postgres {
        store: Arc<PostgresThreadStore>,
        admissions: PostgresAdmissionStore,
    },
}
impl RuntimeThreadStore {
    pub(crate) fn local(s: Arc<LocalThreadStore>) -> Self {
        Self {
            inner: Backend::Local(s),
        }
    }
    #[cfg(feature = "postgres-storage")]
    pub(crate) fn postgres(s: Arc<PostgresThreadStore>) -> Self {
        Self {
            inner: Backend::Postgres {
                store: s.clone(),
                admissions: PostgresAdmissionStore::new(s.storage_clone()),
            },
        }
    }
    pub async fn session_meta(&self, t: &ThreadId) -> ThreadStoreResult<SessionMeta> {
        match &self.inner {
            Backend::Local(s) => s.session_meta(t).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => {
                let r = store.storage_session_meta(t).await?;
                Ok(r)
            }
        }
    }
    pub async fn history_position(&self, t: &ThreadId) -> ThreadStoreResult<HistoryPosition> {
        match &self.inner {
            Backend::Local(s) => s.history_position(t).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.history_position_public(t).await,
        }
    }
    pub async fn model_messages(
        &self,
        t: &ThreadId,
        e: Option<&TurnId>,
    ) -> ThreadStoreResult<Vec<Value>> {
        match &self.inner {
            Backend::Local(s) => s.model_messages(t, e).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.model_messages(t, e).await,
        }
    }
    pub async fn begin_turn(
        &self,
        user: &str,
        t: &ThreadId,
        turn: &TurnId,
        model: &str,
    ) -> ThreadStoreResult<()> {
        match &self.inner {
            Backend::Local(s) => s.begin_turn(user, t, turn, model).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.begin_turn(user, t, turn, model).await,
        }
    }
    pub(crate) async fn materialize_terminal_turn(
        &self,
        user: &str,
        thread: &ThreadId,
        turn: &TurnId,
    ) -> ThreadStoreResult<()> {
        match &self.inner {
            Backend::Local(store) => store.materialize_turn(user, thread, turn, "", false).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { .. } => Err(ThreadStoreError::Conflict {
                message: "service terminal materialization requires its lifecycle transaction"
                    .into(),
            }),
        }
    }
    /// Service lifecycle state and canonical records share a database
    /// transaction. `None` retains the local rollout-first commit boundary.
    pub(crate) async fn update_service_turn(
        &self,
        user: &str,
        session: &str,
        turn: &str,
        status: &str,
        summary: &str,
        payload: &Value,
    ) -> Result<Option<bool>> {
        #[cfg(not(feature = "postgres-storage"))]
        let _ = (user, session, turn, status, summary, payload);
        match &self.inner {
            Backend::Local(_) => Ok(None),
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => {
                use crate::storage::ThreadLogStore;
                let storage = store.storage_clone();
                let user = user.to_owned();
                let session = session.to_owned();
                let turn = turn.to_owned();
                let status = status.to_owned();
                let summary = summary.to_owned();
                let payload = payload.clone();
                crate::core::blocking::run_db("postgres.thread_store.update_turn", move || {
                    storage
                        .update_thread_turn(&user, &session, &turn, &status, &summary, &payload)
                        .map(Some)
                })
                .await
            }
        }
    }
    pub async fn append_canonical(
        &self,
        t: &ThreadId,
        r: Vec<CanonicalRecord>,
    ) -> ThreadStoreResult<AppendOutcome> {
        match &self.inner {
            Backend::Local(s) => s.append_canonical(t, r).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.append_canonical(t, r).await,
        }
    }

    pub async fn append_model_message(
        &self,
        thread: &ThreadId,
        turn: &TurnId,
        payload: Value,
    ) -> ThreadStoreResult<AppendOutcome> {
        match &self.inner {
            Backend::Local(s) => s.append_model_message(thread, turn, payload).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => {
                store.append_model_message(thread, turn, payload).await
            }
        }
    }

    pub async fn append_chat_payload(&self, user: &str, payload: &Value) -> Result<()> {
        match &self.inner {
            Backend::Local(s) => s.append_chat_payload(user, payload).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store
                .append_chat_payload(user, payload)
                .await
                .map_err(Into::into),
        }
    }
    pub async fn generate_stress_thread(
        &self,
        user: &str,
        spec: &crate::storage::stress_model::StressThreadSpec,
        progress: impl FnMut(i64, i64) + Send,
    ) -> Result<crate::storage::stress_model::StressThreadStats> {
        match &self.inner {
            Backend::Local(store) => store.generate_stress_thread(user, spec, progress).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => {
                store.generate_stress_thread(user, spec, progress).await
            }
        }
    }
    pub async fn create_thread(&self, r: CreateThreadRequest) -> ThreadStoreResult<ThreadMetadata> {
        match &self.inner {
            Backend::Local(s) => s.create_thread(r).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.create_thread(r).await,
        }
    }
    pub async fn get_thread(&self, t: &ThreadId) -> ThreadStoreResult<ThreadMetadata> {
        match &self.inner {
            Backend::Local(s) => s.get_thread(t).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.get_thread(t).await,
        }
    }
    pub async fn read_item(&self, t: &ThreadId, i: &ItemId) -> ThreadStoreResult<ItemSummary> {
        match &self.inner {
            Backend::Local(s) => s.read_item(t, i).await,
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { store, .. } => store.read_item(t, i).await,
        }
    }
    pub fn root(&self) -> std::path::PathBuf {
        match &self.inner {
            Backend::Local(s) => s.root().to_path_buf(),
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { .. } => std::path::PathBuf::new(),
        }
    }
    pub(crate) fn admission_store(&self) -> Result<RuntimeAdmissions> {
        match &self.inner {
            Backend::Local(s) => Ok(RuntimeAdmissions::Local(s.admission_store()?)),
            #[cfg(feature = "postgres-storage")]
            Backend::Postgres { admissions, .. } => {
                Ok(RuntimeAdmissions::Postgres(admissions.clone()))
            }
        }
    }
}
pub(crate) enum RuntimeAdmissions {
    Local(crate::storage::thread_admission::TurnAdmissions),
    #[cfg(feature = "postgres-storage")]
    Postgres(PostgresAdmissionStore),
}
impl RuntimeAdmissions {
    pub fn check_owner(&self, u: &str, t: &str) -> Result<()> {
        match self {
            Self::Local(a) => a.check_owner(u, t),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.check_owner(u, t),
        }
    }
    pub fn register_owner(&self, u: &str, t: &str) -> Result<bool> {
        match self {
            Self::Local(a) => a.register_owner(u, t),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.register_owner(u, t),
        }
    }
    pub fn accept(&self, u: &str, t: &str, input: &Value) -> Result<Value> {
        match self {
            Self::Local(a) => a.accept(u, t, input),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.accept(u, t, input),
        }
    }
    pub fn turn(&self, t: &str, id: &str) -> Result<Option<Value>> {
        match self {
            Self::Local(a) => a.turn(t, id),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.turn(t, id),
        }
    }
    pub fn pending(&self, t: &str, id: &str) -> Result<Option<Value>> {
        match self {
            Self::Local(a) => a.pending(t, id),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.pending(t, id),
        }
    }
    pub fn update_with_control(
        &self,
        t: &str,
        id: &str,
        status: &str,
        committed: bool,
        payload: &Value,
    ) -> Result<bool> {
        match self {
            Self::Local(a) => a.update_with_control(t, id, status, committed, payload),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.update_with_control(t, id, status, committed, payload),
        }
    }
    pub fn ids_by_status(&self, t: &str, status: &str, limit: usize) -> Result<Vec<String>> {
        match self {
            Self::Local(a) => a.ids_by_status(t, status, limit),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.ids_by_status(t, status, limit),
        }
    }
    pub fn settle_orphaned_turns(&self, t: &str) -> Result<usize> {
        match self {
            Self::Local(a) => a.settle_orphaned_turns(t),
            #[cfg(feature = "postgres-storage")]
            Self::Postgres(a) => a.settle_orphaned_turns(t),
        }
    }
}
