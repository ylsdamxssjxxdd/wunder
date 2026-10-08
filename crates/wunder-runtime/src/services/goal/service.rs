//! Same-session goal domain: durable snapshot with CAS-on-revision
//! mutations, process-local activation, and continuation driving.

use super::types::*;
use crate::core::blocking;
use crate::services::stream_events::StreamEventService;
use crate::storage::{SessionGoalRecord, StorageBackend};
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub const EVENT_GOAL_UPDATED: &str = "goal_updated";

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

async fn run_goal_db<T, F>(label: &'static str, task: F) -> anyhow::Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    blocking::run_db(label, task).await
}

/// Goal domain handle: activation registry + continuation driver. One per
/// process; sessions absent from the registry are disarmed, so a process
/// restart always starts disarmed and only humans re-arm.
pub struct GoalService {
    activations: Mutex<HashMap<String, GoalActivation>>,
    driver: super::driver::GoalDriver,
}

impl Default for GoalService {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalService {
    pub fn new() -> Self {
        Self {
            activations: Mutex::new(HashMap::new()),
            driver: super::driver::GoalDriver::new(),
        }
    }

    /// Late-bound host (thread runtime) that admits and submits goal rounds.
    pub fn set_driver_host(&self, host: Arc<dyn super::driver::GoalDriverHost>) {
        self.driver.set_host(host);
    }

    pub fn activation(&self, session_id: &str) -> GoalActivation {
        self.activations
            .lock()
            .ok()
            .and_then(|map| map.get(session_id.trim()).copied())
            .unwrap_or(GoalActivation::Disarmed)
    }

    fn set_activation(&self, session_id: &str, activation: GoalActivation) {
        if let Ok(mut map) = self.activations.lock() {
            if activation == GoalActivation::Disarmed && !map.contains_key(session_id) {
                // Stay absent: unarmed is the default and keeps the map small.
                return;
            }
            map.insert(session_id.trim().to_string(), activation);
        }
    }

    /// Drop tracking for a session whose goal is gone.
    pub fn forget_session(&self, session_id: &str) {
        if let Ok(mut map) = self.activations.lock() {
            map.remove(session_id.trim());
        }
        self.driver.reset_session(session_id);
    }

    fn view(&self, record: SessionGoalRecord) -> GoalView {
        let activation = self.activation(&record.session_id);
        GoalView { record, activation }
    }

    async fn load(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionGoalRecord>> {
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        let storage = storage.clone();
        run_goal_db("goal.get", move || {
            storage.get_session_goal(&user_id, &session_id)
        })
        .await
    }

    pub async fn get_view(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
    ) -> Result<Option<GoalView>> {
        match self.load(storage, user_id, session_id).await? {
            Some(record) => Ok(Some(self.view(record))),
            None => Ok(None),
        }
    }

    /// Batched projection for session lists: one storage query, activation
    /// merged per record.
    pub async fn list_views(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_ids: &[String],
    ) -> Result<Vec<GoalView>> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let user_id = user_id.trim().to_string();
        let ids: Vec<String> = session_ids
            .iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();
        let storage = storage.clone();
        let records = blocking::run_db("goal.list", move || {
            storage.list_session_goals(&user_id, &ids)
        })
        .await?;
        Ok(records
            .into_iter()
            .map(|record| self.view(record))
            .collect())
    }

    async fn emit_event(
        &self,
        storage: &Arc<dyn StorageBackend>,
        operation: &str,
        view: &GoalView,
    ) {
        let service = StreamEventService::new(storage.clone());
        let session_id = view.record.session_id.clone();
        let user_id = view.record.user_id.clone();
        let payload = json!({
            "type": EVENT_GOAL_UPDATED,
            "operation": operation,
            "goal": goal_payload(view),
        });
        if let Err(error) = service.append_event(&session_id, &user_id, payload).await {
            tracing::warn!(target: "wunder::goal", session_id = %session_id, %error, "emit goal event failed");
        }
    }

    fn notify_changed(&self, user_id: &str, session_id: &str) {
        self.driver
            .notify_goal_changed(user_id.trim(), session_id.trim());
    }

    pub async fn create(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        objective: &str,
        max_goal_rounds: Option<i64>,
    ) -> Result<GoalView> {
        let objective = validate_objective(objective)?;
        let max_goal_rounds = resolve_max_goal_rounds(max_goal_rounds)?;
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        if let Some(current) = self.load(&storage, &user_id, &session_id).await? {
            if current.phase != PHASE_COMPLETE {
                return Err(goal_error(
                    "session already has an active, paused, or blocked goal; edit, resume, or clear it first",
                    ERR_GOAL_ALREADY_EXISTS,
                )
                .into());
            }
            // Prototype hard cutover: a completed goal is replaced outright.
            run_goal_db("goal.delete", {
                let storage = storage.clone();
                let user_id = user_id.clone();
                let session_id = session_id.clone();
                move || storage.delete_session_goal(&user_id, &session_id)
            })
            .await?;
        }
        let record = SessionGoalRecord {
            goal_id: Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            user_id: user_id.clone(),
            revision: 1,
            objective,
            phase: PHASE_ACTIVE.to_string(),
            blocked_code: None,
            blocked_message: None,
            max_goal_rounds,
            rounds_started: 0,
            created_at: now_ts(),
            updated_at: now_ts(),
        };
        let record_for_write = record.clone();
        let storage_for_write = storage.clone();
        run_goal_db("goal.upsert", move || {
            storage_for_write.upsert_session_goal(&record_for_write)
        })
        .await?;
        self.set_activation(&session_id, GoalActivation::Armed);
        let view = self.view(record);
        self.emit_event(&storage, "created", &view).await;
        self.notify_changed(&user_id, &session_id);
        Ok(view)
    }

    pub async fn edit(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
        objective: &str,
        max_goal_rounds: Option<i64>,
    ) -> Result<GoalView> {
        let objective = validate_objective(objective)?;
        let mut record = self
            .require_current(&storage, user_id, session_id, reference)
            .await?;
        let expected_revision = record.revision;
        if let Some(max_goal_rounds) = max_goal_rounds {
            let resolved = resolve_max_goal_rounds(Some(max_goal_rounds))?;
            if resolved < record.rounds_started {
                return Err(goal_error(
                    "max_goal_rounds cannot be lower than the rounds already started",
                    ERR_GOAL_INVALID_MAX_ROUNDS,
                )
                .into());
            }
            record.max_goal_rounds = resolved;
        }
        record.revision = expected_revision + 1;
        record.objective = objective;
        record.updated_at = now_ts();
        self.cas_write(&storage, &record, expected_revision).await?;
        let view = self.view(record);
        self.emit_event(&storage, "edited", &view).await;
        self.notify_changed(user_id, session_id);
        Ok(view)
    }

    pub async fn pause(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
    ) -> Result<GoalView> {
        let mut record = self
            .require_current(&storage, user_id, session_id, reference)
            .await?;
        if record.phase == PHASE_PAUSED {
            return Err(goal_error("goal is already paused", ERR_GOAL_INVALID_TRANSITION).into());
        }
        if record.phase == PHASE_COMPLETE {
            return Err(
                goal_error("a complete goal is terminal", ERR_GOAL_INVALID_TRANSITION).into(),
            );
        }
        let expected_revision = record.revision;
        record.revision = expected_revision + 1;
        record.phase = PHASE_PAUSED.to_string();
        record.blocked_code = None;
        record.blocked_message = None;
        record.updated_at = now_ts();
        self.cas_write(&storage, &record, expected_revision).await?;
        self.set_activation(session_id, GoalActivation::Disarmed);
        let view = self.view(record);
        self.emit_event(&storage, "paused", &view).await;
        self.notify_changed(user_id, session_id);
        Ok(view)
    }

    pub async fn resume(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
    ) -> Result<GoalView> {
        let mut record = self
            .require_current(&storage, user_id, session_id, reference)
            .await?;
        if record.phase != PHASE_PAUSED && record.phase != PHASE_BLOCKED {
            return Err(goal_error(
                "only a paused or blocked goal can be resumed",
                ERR_GOAL_INVALID_TRANSITION,
            )
            .into());
        }
        let expected_revision = record.revision;
        record.revision = expected_revision + 1;
        record.phase = PHASE_ACTIVE.to_string();
        record.blocked_code = None;
        record.blocked_message = None;
        record.updated_at = now_ts();
        self.cas_write(&storage, &record, expected_revision).await?;
        self.set_activation(session_id, GoalActivation::Armed);
        let view = self.view(record);
        self.emit_event(&storage, "resumed", &view).await;
        self.notify_changed(user_id, session_id);
        Ok(view)
    }

    pub async fn complete(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
    ) -> Result<GoalView> {
        let mut record = self
            .require_current(&storage, user_id, session_id, reference)
            .await?;
        if record.phase == PHASE_COMPLETE {
            return Err(goal_error("goal is already complete", ERR_GOAL_INVALID_TRANSITION).into());
        }
        let expected_revision = record.revision;
        record.revision = expected_revision + 1;
        record.phase = PHASE_COMPLETE.to_string();
        record.blocked_code = None;
        record.blocked_message = None;
        record.updated_at = now_ts();
        self.cas_write(&storage, &record, expected_revision).await?;
        self.set_activation(session_id, GoalActivation::Disarmed);
        let view = self.view(record);
        self.emit_event(&storage, "completed", &view).await;
        self.notify_changed(user_id, session_id);
        Ok(view)
    }

    /// Unified exit for continuation failures: the goal stops driving until a
    /// human resumes it.
    pub async fn block(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
        reason: GoalBlockReason,
    ) -> Result<GoalView> {
        let mut record = self
            .require_current(&storage, user_id, session_id, reference)
            .await?;
        if record.phase != PHASE_ACTIVE {
            return Err(goal_error(
                "only an active goal can be blocked",
                ERR_GOAL_INVALID_TRANSITION,
            )
            .into());
        }
        let expected_revision = record.revision;
        record.revision = expected_revision + 1;
        record.phase = PHASE_BLOCKED.to_string();
        record.blocked_code = Some(reason.code);
        record.blocked_message = Some(reason.message);
        record.updated_at = now_ts();
        self.cas_write(&storage, &record, expected_revision).await?;
        self.set_activation(session_id, GoalActivation::Disarmed);
        let view = self.view(record);
        self.emit_event(&storage, "blocked", &view).await;
        self.notify_changed(user_id, session_id);
        Ok(view)
    }

    pub async fn clear(
        &self,
        storage: Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
    ) -> Result<()> {
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        let removed = run_goal_db("goal.delete", {
            let storage = storage.clone();
            let user_id = user_id.clone();
            let session_id = session_id.clone();
            move || storage.delete_session_goal(&user_id, &session_id)
        })
        .await?;
        self.forget_session(&session_id);
        if removed > 0 {
            self.emit_cleared_event(&storage, &user_id, &session_id);
        }
        Ok(())
    }

    fn emit_cleared_event(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
    ) {
        let service = StreamEventService::new(storage.clone());
        let payload = json!({
            "type": EVENT_GOAL_UPDATED,
            "operation": "cleared",
            "goal": Value::Null,
        });
        let user_id = user_id.to_string();
        let session_id = session_id.to_string();
        tokio::spawn(async move {
            if let Err(error) = service.append_event(&session_id, &user_id, payload).await {
                tracing::warn!(target: "wunder::goal", session_id = %session_id, %error, "emit goal cleared event failed");
            }
        });
    }

    /// Admission of one reserved goal round: bumps `rounds_started` by CAS so
    /// only the admitted attempt counts toward the cap.
    pub async fn admit_round(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        tag: &GoalRoundTag,
    ) -> Result<bool> {
        let Some(mut record) = self.load(storage, user_id, session_id).await? else {
            return Ok(false);
        };
        if record.goal_id != tag.goal_id || record.revision != tag.revision {
            return Ok(false);
        }
        if record.rounds_started >= tag.round {
            return Ok(true);
        }
        let expected_revision = record.revision;
        record.rounds_started = tag.round;
        record.updated_at = now_ts();
        let written = self
            .cas_write_opt(storage, &record, expected_revision)
            .await?;
        if !written {
            // Losing to a concurrent mutation is fine; the loser re-reads.
            return Ok(false);
        }
        Ok(true)
    }

    /// Disarm only (no durable change): process restart, turn cut off by the
    /// context window, or session teardown.
    pub fn disarm(&self, session_id: &str) {
        self.set_activation(session_id, GoalActivation::Disarmed);
    }

    pub fn driver(&self) -> &super::driver::GoalDriver {
        &self.driver
    }

    async fn require_current(
        &self,
        storage: &Arc<dyn StorageBackend>,
        user_id: &str,
        session_id: &str,
        reference: &GoalRef,
    ) -> Result<SessionGoalRecord> {
        let Some(record) = self.load(storage, user_id, session_id).await? else {
            return Err(goal_error("session has no goal", ERR_GOAL_NOT_FOUND).into());
        };
        if record.goal_id != reference.goal_id {
            return Err(goal_error(
                "goal id does not match the current session goal; read get_goal again",
                ERR_GOAL_NOT_FOUND,
            )
            .into());
        }
        if record.revision != reference.revision {
            return Err(goal_error(
                "goal revision changed since it was read; read get_goal and retry",
                ERR_GOAL_STALE_REVISION,
            )
            .into());
        }
        Ok(record)
    }

    async fn cas_write(
        &self,
        storage: &Arc<dyn StorageBackend>,
        record: &SessionGoalRecord,
        expected_revision: i64,
    ) -> Result<()> {
        if !self
            .cas_write_opt(storage, record, expected_revision)
            .await?
        {
            return Err(goal_error(
                "goal changed concurrently; read get_goal and retry",
                ERR_GOAL_STALE_REVISION,
            )
            .into());
        }
        Ok(())
    }

    async fn cas_write_opt(
        &self,
        storage: &Arc<dyn StorageBackend>,
        record: &SessionGoalRecord,
        expected_revision: i64,
    ) -> Result<bool> {
        let record = record.clone();
        let storage = storage.clone();
        run_goal_db("goal.cas", move || {
            storage.update_session_goal(&record, expected_revision)
        })
        .await
    }
}

/// Validate a shared objective string.
pub fn validate_objective(objective: impl AsRef<str>) -> Result<String> {
    let cleaned = objective.as_ref().trim();
    if cleaned.is_empty() {
        return Err(goal_error("goal objective is required", ERR_GOAL_INVALID_OBJECTIVE).into());
    }
    if cleaned.chars().count() > MAX_OBJECTIVE_CHARS {
        return Err(goal_error("goal objective is too long", ERR_GOAL_INVALID_OBJECTIVE).into());
    }
    Ok(cleaned.to_string())
}

fn resolve_max_goal_rounds(max_goal_rounds: Option<i64>) -> Result<i64> {
    let resolved = max_goal_rounds.unwrap_or(DEFAULT_MAX_GOAL_ROUNDS);
    if resolved < 1 || resolved > 10_000 {
        return Err(goal_error(
            "max_goal_rounds must be between 1 and 10000",
            ERR_GOAL_INVALID_MAX_ROUNDS,
        )
        .into());
    }
    Ok(resolved)
}

/// Compact stable JSON projection used by the API, CLI, and desktop faces.
pub fn goal_payload(view: &GoalView) -> Value {
    json!({
        "goal_id": view.record.goal_id,
        "revision": view.record.revision,
        "objective": view.record.objective,
        "phase": view.record.phase,
        "blocked_code": view.record.blocked_code,
        "blocked_message": view.record.blocked_message,
        "max_goal_rounds": view.record.max_goal_rounds,
        "rounds_started": view.record.rounds_started,
        "created_at": view.record.created_at,
        "updated_at": view.record.updated_at,
        "activation": view.activation.as_str(),
    })
}

/// Exact deepseek-style compact tool output: `{goal: null}` or the goal value
/// plus the activation string.
pub fn goal_tool_value(view: &GoalView) -> Value {
    let mut goal = json!({
        "id": view.record.goal_id,
        "revision": view.record.revision,
        "objective": view.record.objective,
        "phase": view.record.phase,
        "roundsStarted": view.record.rounds_started,
        "maxGoalRounds": view.record.max_goal_rounds,
    });
    if view.phase() == GoalPhase::Blocked {
        if let Some(reason) = view.blocked_reason() {
            goal["blockedReason"] = json!({"code": reason.code, "message": reason.message});
        }
    }
    json!({
        "goal": goal,
        "activation": view.activation.as_str(),
    })
}

/// Compact tool output for "no goal".
pub fn goal_tool_value_none() -> Value {
    json!({ "goal": Value::Null })
}
