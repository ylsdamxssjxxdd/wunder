//! Goal-round driver: autonomously reserves and submits continuation rounds
//! while the goal is active, armed, and under its round cap. Mirrors the
//! deepseek goal-round-driver: no token budgets, no cooldowns; driving is
//! gated on session availability with humans always taking priority.

use super::types::{
    GoalActivation, GoalBlockReason, GoalRef, GoalRoundTag, GoalView, PHASE_ACTIVE,
};
use anyhow::Result;
use futures::future::BoxFuture;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use tokio::sync::Mutex as AsyncMutex;

/// Host facilities the driver needs from the thread runtime.
pub trait GoalDriverHost: Send + Sync {
    /// Whether the session has no running, queued, waiting, or cancelling
    /// activity; humans keep priority and unadmitted rounds never race them.
    fn host_session_available(&self, user_id: &str, session_id: &str) -> BoxFuture<'_, bool>;
    /// Admit one goal-round turn. The host renders the `<goal_round>` prompt,
    /// performs transcript admission, marks `rounds_started`, and queues the
    /// request before returning.
    fn host_submit_goal_round(
        &self,
        user_id: &str,
        session_id: &str,
        tag: GoalRoundTag,
    ) -> BoxFuture<'_, Result<()>>;
    fn host_load_goal_view(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> BoxFuture<'_, Result<Option<GoalView>>>;
    fn host_pause_goal(
        &self,
        user_id: &str,
        session_id: &str,
        reference: GoalRef,
    ) -> BoxFuture<'_, Result<()>>;
    fn host_block_goal(
        &self,
        user_id: &str,
        session_id: &str,
        reference: GoalRef,
        reason: GoalBlockReason,
    ) -> BoxFuture<'_, Result<()>>;
}

/// How the turn that owned the in-flight round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEndOutcome {
    Completed,
    /// Question panel is waiting for human input; do not drive.
    WaitingUserInput,
    /// Turn was cancelled while the round was in flight.
    Aborted,
    /// Turn hit the model context/token ceiling.
    MaxTokens,
    Failed,
}

struct RoundAttempt {
    tag: GoalRoundTag,
    cancelled: bool,
}

#[derive(Default)]
struct DriverState {
    user_id: Mutex<Option<String>>,
    attempt: Mutex<Option<RoundAttempt>>,
    requested: AtomicBool,
    gate: AsyncMutex<()>,
}

pub struct GoalDriver {
    states: Mutex<HashMap<String, Arc<DriverState>>>,
    host: RwLock<Option<Weak<dyn GoalDriverHost>>>,
}

impl Default for GoalDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl GoalDriver {
    pub fn new() -> Self {
        Self {
            states: Mutex::new(HashMap::new()),
            host: RwLock::new(None),
        }
    }

    pub fn set_host(&self, host: Arc<dyn GoalDriverHost>) {
        if let Ok(mut slot) = self.host.write() {
            *slot = Some(Arc::downgrade(&host) as Weak<dyn GoalDriverHost>);
        }
    }

    fn state(&self, session_id: &str) -> Arc<DriverState> {
        let key = session_id.trim().to_string();
        if let Ok(map) = self.states.lock() {
            if let Some(existing) = map.get(&key) {
                return existing.clone();
            }
        }
        let state = Arc::new(DriverState::default());
        if let Ok(mut map) = self.states.lock() {
            if map.len() >= 4096 {
                map.clear();
            }
            map.insert(key, state.clone());
        }
        state
    }

    fn state_user_id(&self, state: &DriverState, user_id: &str) -> String {
        let mut slot = state
            .user_id
            .lock()
            .expect("goal driver user_id lock poisoned");
        if slot.as_deref().map(str::trim).unwrap_or("").is_empty() {
            *slot = Some(user_id.trim().to_string());
        }
        slot.clone().unwrap_or_default()
    }

    fn take_attempt_if(&self, state: &DriverState, predicate: impl Fn(&RoundAttempt) -> bool) {
        if let Ok(mut attempt) = state.attempt.lock() {
            let matches = attempt
                .as_ref()
                .map(|inner| predicate(inner))
                .unwrap_or(false);
            if matches {
                *attempt = None;
            }
        }
    }

    fn mark_attempt_cancelled(&self, state: &DriverState) {
        if let Ok(mut attempt) = state.attempt.lock() {
            if let Some(inner) = attempt.as_mut() {
                inner.cancelled = true;
            }
        }
    }

    fn attempt_tag(&self, state: &DriverState) -> Option<GoalRoundTag> {
        state
            .attempt
            .lock()
            .ok()
            .and_then(|attempt| attempt.as_ref().map(|inner| inner.tag.clone()))
    }

    /// Reset per-session driver bookkeeping (goal cleared or session gone).
    pub fn reset_session(&self, session_id: &str) {
        let key = session_id.trim().to_string();
        if let Ok(mut map) = self.states.lock() {
            map.remove(&key);
        }
    }

    /// A goal snapshot changed: re-evaluate driving when idle.
    pub fn notify_goal_changed(&self, user_id: &str, session_id: &str) {
        let state = self.state(session_id);
        self.state_user_id(&state, user_id);
        self.request_drive(&state, user_id, session_id);
    }

    /// A queued goal-round task was withdrawn in favour of competing input.
    pub fn notify_rounds_withdrawn(&self, _user_id: &str, session_id: &str, tags: &[GoalRoundTag]) {
        if tags.is_empty() {
            return;
        }
        let state = self.state(session_id);
        let Some(tag) = self.attempt_tag(&state) else {
            return;
        };
        if tags.iter().any(|withdrawn| *withdrawn == tag) {
            self.mark_attempt_cancelled(&state);
        }
    }

    /// The user pressed stop: an in-flight round settles into a pause at the
    /// next idle point (same fence as the deepseek idle handler).
    pub fn notify_session_cancelled(&self, session_id: &str) {
        let state = self.state(session_id);
        if self.attempt_tag(&state).is_some() {
            self.mark_attempt_cancelled(&state);
        }
    }

    /// Turn finalization hook: settle the attempt, then re-evaluate driving.
    pub fn notify_turn_ended(
        &self,
        user_id: &str,
        session_id: &str,
        goal_round: Option<&GoalRoundTag>,
        outcome: TurnEndOutcome,
    ) {
        let state = self.state(session_id);
        self.state_user_id(&state, user_id);
        match outcome {
            TurnEndOutcome::WaitingUserInput => return,
            TurnEndOutcome::MaxTokens => return, // disarm handled by the service
            TurnEndOutcome::Aborted => {
                if let Some(tag) = goal_round {
                    if self.attempt_tag(&state).as_ref() == Some(tag) {
                        self.mark_attempt_cancelled(&state);
                    }
                }
            }
            TurnEndOutcome::Completed | TurnEndOutcome::Failed => {}
        }
        self.request_drive(&state, user_id, session_id);
    }

    fn request_drive(&self, state: &Arc<DriverState>, user_id: &str, session_id: &str) {
        let already = state.requested.swap(true, Ordering::SeqCst);
        if already {
            return;
        }
        let state = state.clone();
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        let weak_host = self.host.read().ok().and_then(|slot| slot.clone());
        tokio::spawn(async move {
            let Some(host) = weak_host.and_then(|weak| weak.upgrade()) else {
                state.requested.store(false, Ordering::SeqCst);
                return;
            };
            let _guard = state.gate.lock().await;
            loop {
                state.requested.store(false, Ordering::SeqCst);
                drive_once(&host, &state, &user_id, &session_id).await;
                if state.requested.load(Ordering::SeqCst) {
                    continue;
                }
                break;
            }
        });
    }
}

/// One drive evaluation. Guarded by the per-session gate; re-run while new
/// requests arrive during the run.
async fn drive_once(
    host: &Arc<dyn GoalDriverHost>,
    state: &Arc<DriverState>,
    user_id: &str,
    session_id: &str,
) {
    // 1. Settle a cancelled attempt: pause the goal at this idle point.
    let cancelled_tag = state.attempt.lock().ok().and_then(|attempt| {
        attempt
            .as_ref()
            .filter(|inner| inner.cancelled)
            .map(|inner| inner.tag.clone())
    });
    if let Some(tag) = cancelled_tag {
        if let Err(error) = pause_cancelled_round(host, user_id, session_id, &tag).await {
            tracing::warn!(target: "wunder::goal", %session_id, %error, "settle cancelled goal round failed");
        }
        state.take_attempt_if_predicate(|inner| inner.cancelled && inner.tag == tag);
        return;
    }
    // 2. An admitted round is still in flight; its turn end will re-drive.
    if state
        .attempt
        .lock()
        .ok()
        .map(|attempt| attempt.is_some())
        .unwrap_or(false)
    {
        return;
    }
    // 3. Humans keep priority: only drive a fully available session.
    if !host.host_session_available(user_id, session_id).await {
        return;
    }
    // 4. Reserve the next round if the goal is still driving.
    reserve_and_submit(host, state, user_id, session_id).await;
}

impl DriverState {
    fn set_attempt(&self, tag: GoalRoundTag) {
        if let Ok(mut attempt) = self.attempt.lock() {
            *attempt = Some(RoundAttempt {
                tag,
                cancelled: false,
            });
        }
    }

    fn take_attempt_if_predicate(&self, predicate: impl Fn(&RoundAttempt) -> bool) {
        if let Ok(mut attempt) = self.attempt.lock() {
            let matches = attempt
                .as_ref()
                .map(|inner| predicate(inner))
                .unwrap_or(false);
            if matches {
                *attempt = None;
            }
        }
    }
}

async fn pause_cancelled_round(
    host: &Arc<dyn GoalDriverHost>,
    user_id: &str,
    session_id: &str,
    tag: &GoalRoundTag,
) -> Result<()> {
    let Some(view) = host
        .host_load_goal_view(user_id, session_id)
        .await
        .ok()
        .flatten()
    else {
        return Ok(());
    };
    let driving = view.record.goal_id == tag.goal_id
        && view.record.revision == tag.revision
        && view.record.phase == PHASE_ACTIVE
        && view.activation == GoalActivation::Armed;
    if !driving {
        return Ok(());
    }
    host.host_pause_goal(
        user_id,
        session_id,
        GoalRef {
            goal_id: tag.goal_id.clone(),
            revision: tag.revision,
        },
    )
    .await
}

async fn reserve_and_submit(
    host: &Arc<dyn GoalDriverHost>,
    state: &Arc<DriverState>,
    user_id: &str,
    session_id: &str,
) {
    let Ok(Some(view)) = host.host_load_goal_view(user_id, session_id).await else {
        return;
    };
    if view.record.phase != PHASE_ACTIVE
        || view.activation != GoalActivation::Armed
        || view.record.rounds_started >= view.record.max_goal_rounds
    {
        return;
    }
    let tag = GoalRoundTag {
        goal_id: view.record.goal_id.clone(),
        revision: view.record.revision,
        round: view.record.rounds_started + 1,
    };
    state.set_attempt(tag.clone());
    if let Err(error) = host
        .host_submit_goal_round(user_id, session_id, tag.clone())
        .await
    {
        state.take_attempt_if_predicate(|inner| inner.tag == tag);
        tracing::warn!(target: "wunder::goal", %session_id, %error, "goal round submission failed");
        let reason = GoalBlockReason {
            code: "queue-failed".into(),
            message: format!("goal round could not be queued: {error}"),
        };
        if let Err(block_error) = host
            .host_block_goal(
                user_id,
                session_id,
                GoalRef {
                    goal_id: tag.goal_id,
                    revision: tag.revision,
                },
                reason,
            )
            .await
        {
            tracing::warn!(target: "wunder::goal", %session_id, %block_error, "block goal after queue failure failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::goal::types::PHASE_PAUSED;
    use crate::storage::SessionGoalRecord;

    struct MockHost {
        available: AtomicBool,
        view: Mutex<Option<GoalView>>,
        submit_error: Mutex<Option<String>>,
        submits: Mutex<Vec<GoalRoundTag>>,
        pauses: Mutex<Vec<GoalRef>>,
        blocks: Mutex<Vec<(GoalRef, GoalBlockReason)>>,
    }

    impl MockHost {
        fn with_view(view: GoalView) -> Self {
            Self {
                available: AtomicBool::new(true),
                view: Mutex::new(Some(view)),
                submit_error: Mutex::new(None),
                submits: Mutex::new(Vec::new()),
                pauses: Mutex::new(Vec::new()),
                blocks: Mutex::new(Vec::new()),
            }
        }

        fn submit_fails(&self, message: &str) {
            *self.submit_error.lock().unwrap() = Some(message.to_string());
        }

        fn submits(&self) -> Vec<GoalRoundTag> {
            self.submits.lock().unwrap().clone()
        }

        fn pauses(&self) -> Vec<GoalRef> {
            self.pauses.lock().unwrap().clone()
        }

        fn blocks(&self) -> Vec<(GoalRef, GoalBlockReason)> {
            self.blocks.lock().unwrap().clone()
        }
    }

    impl GoalDriverHost for MockHost {
        fn host_session_available(&self, _user_id: &str, _session_id: &str) -> BoxFuture<'_, bool> {
            let available = self.available.load(Ordering::SeqCst);
            Box::pin(async move { available })
        }

        fn host_submit_goal_round(
            &self,
            _user_id: &str,
            _session_id: &str,
            tag: GoalRoundTag,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            self.submits.lock().unwrap().push(tag);
            let error = self.submit_error.lock().unwrap().clone();
            Box::pin(async move {
                match error {
                    Some(message) => Err(anyhow::anyhow!(message)),
                    None => Ok(()),
                }
            })
        }

        fn host_load_goal_view(
            &self,
            _user_id: &str,
            _session_id: &str,
        ) -> BoxFuture<'_, anyhow::Result<Option<GoalView>>> {
            let view = self.view.lock().unwrap().clone();
            Box::pin(async move { Ok(view) })
        }

        fn host_pause_goal(
            &self,
            _user_id: &str,
            _session_id: &str,
            reference: GoalRef,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            self.pauses.lock().unwrap().push(reference);
            Box::pin(async { Ok(()) })
        }

        fn host_block_goal(
            &self,
            _user_id: &str,
            _session_id: &str,
            reference: GoalRef,
            reason: GoalBlockReason,
        ) -> BoxFuture<'_, anyhow::Result<()>> {
            self.blocks.lock().unwrap().push((reference, reason));
            Box::pin(async { Ok(()) })
        }
    }

    fn view(
        phase: &str,
        rounds_started: i64,
        max_goal_rounds: i64,
        activation: GoalActivation,
    ) -> GoalView {
        GoalView {
            record: SessionGoalRecord {
                goal_id: "goal-1".into(),
                session_id: "session-1".into(),
                user_id: "user-1".into(),
                revision: 4,
                objective: "objective".into(),
                phase: phase.to_string(),
                blocked_code: None,
                blocked_message: None,
                max_goal_rounds,
                rounds_started,
                created_at: 0.0,
                updated_at: 0.0,
            },
            activation,
        }
    }

    fn tag(round: i64) -> GoalRoundTag {
        GoalRoundTag {
            goal_id: "goal-1".into(),
            revision: 4,
            round,
        }
    }

    async fn drive(host: &Arc<MockHost>, state: &Arc<DriverState>) {
        let host: Arc<dyn GoalDriverHost> = host.clone();
        drive_once(&host, state, "user-1", "session-1").await;
    }

    #[tokio::test]
    async fn submits_next_round_when_active_armed_and_under_cap() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_ACTIVE,
            2,
            8,
            GoalActivation::Armed,
        )));
        let state = Arc::new(DriverState::default());
        drive(&host, &state).await;

        assert_eq!(host.submits(), vec![tag(3)]);
        let attempt = state.attempt.lock().unwrap();
        assert_eq!(attempt.as_ref().unwrap().tag, tag(3));
        assert!(!attempt.as_ref().unwrap().cancelled);
    }

    #[tokio::test]
    async fn skips_submit_when_goal_is_not_driving() {
        for host_view in [
            view(PHASE_ACTIVE, 2, 8, GoalActivation::Disarmed),
            view(PHASE_PAUSED, 2, 8, GoalActivation::Armed),
            view(PHASE_ACTIVE, 8, 8, GoalActivation::Armed),
        ] {
            let host = Arc::new(MockHost::with_view(host_view));
            let state = Arc::new(DriverState::default());
            drive(&host, &state).await;
            assert!(host.submits().is_empty());
            assert!(state.attempt.lock().unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn skips_submit_when_session_is_unavailable() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_ACTIVE,
            2,
            8,
            GoalActivation::Armed,
        )));
        host.available.store(false, Ordering::SeqCst);
        let state = Arc::new(DriverState::default());
        drive(&host, &state).await;

        assert!(host.submits().is_empty());
        assert!(state.attempt.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn skips_submit_while_a_round_is_in_flight() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_ACTIVE,
            2,
            8,
            GoalActivation::Armed,
        )));
        let state = Arc::new(DriverState::default());
        state.set_attempt(tag(3));
        drive(&host, &state).await;

        assert!(host.submits().is_empty());
        assert!(state.attempt.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn queue_failure_releases_the_attempt_and_blocks_the_goal() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_ACTIVE,
            2,
            8,
            GoalActivation::Armed,
        )));
        host.submit_fails("queue unavailable");
        let state = Arc::new(DriverState::default());
        drive(&host, &state).await;

        assert_eq!(host.submits(), vec![tag(3)]);
        assert!(state.attempt.lock().unwrap().is_none());
        let blocks = host.blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].0.goal_id, "goal-1");
        assert_eq!(blocks[0].0.revision, 4);
        assert_eq!(blocks[0].1.code, "queue-failed");
    }

    #[tokio::test]
    async fn cancelled_attempt_settles_into_a_pause() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_ACTIVE,
            2,
            8,
            GoalActivation::Armed,
        )));
        let state = Arc::new(DriverState::default());
        state.set_attempt(tag(3));
        state.attempt.lock().unwrap().as_mut().unwrap().cancelled = true;
        drive(&host, &state).await;

        assert!(host.submits().is_empty());
        assert_eq!(
            host.pauses(),
            vec![GoalRef {
                goal_id: "goal-1".into(),
                revision: 4,
            }]
        );
        assert!(state.attempt.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelled_attempt_is_released_without_pause_when_goal_stopped_driving() {
        let host = Arc::new(MockHost::with_view(view(
            PHASE_PAUSED,
            2,
            8,
            GoalActivation::Disarmed,
        )));
        let state = Arc::new(DriverState::default());
        state.set_attempt(tag(3));
        state.attempt.lock().unwrap().as_mut().unwrap().cancelled = true;
        drive(&host, &state).await;

        assert!(host.pauses().is_empty());
        assert!(state.attempt.lock().unwrap().is_none());
    }

    #[test]
    fn withdrawal_marks_only_the_matching_attempt_cancelled() {
        let driver = GoalDriver::new();
        let state = driver.state("session-1");
        state.set_attempt(tag(3));

        driver.notify_rounds_withdrawn("user-1", "session-1", &[tag(9)]);
        assert!(!state.attempt.lock().unwrap().as_ref().unwrap().cancelled);

        driver.notify_rounds_withdrawn("user-1", "session-1", &[tag(3)]);
        assert!(state.attempt.lock().unwrap().as_ref().unwrap().cancelled);
    }

    #[test]
    fn session_cancellation_marks_the_in_flight_attempt() {
        let driver = GoalDriver::new();
        let state = driver.state("session-1");
        state.set_attempt(tag(1));

        driver.notify_session_cancelled("session-1");
        assert!(state.attempt.lock().unwrap().as_ref().unwrap().cancelled);

        // Without an attempt the notification is a no-op.
        driver.reset_session("session-1");
        driver.notify_session_cancelled("session-1");
    }

    #[tokio::test]
    async fn turn_end_notifications_settle_the_attempt() {
        let driver = GoalDriver::new();
        let state = driver.state("session-1");
        state.set_attempt(tag(1));

        // WaitingUserInput and MaxTokens never settle or re-drive.
        for outcome in [TurnEndOutcome::WaitingUserInput, TurnEndOutcome::MaxTokens] {
            driver.notify_turn_ended("user-1", "session-1", Some(&tag(1)), outcome);
            assert!(state.attempt.lock().unwrap().is_some());
        }

        // Aborted settles only the exact admitted round.
        driver.notify_turn_ended(
            "user-1",
            "session-1",
            Some(&tag(9)),
            TurnEndOutcome::Aborted,
        );
        assert!(!state.attempt.lock().unwrap().as_ref().unwrap().cancelled);
        driver.notify_turn_ended(
            "user-1",
            "session-1",
            Some(&tag(1)),
            TurnEndOutcome::Aborted,
        );
        assert!(state.attempt.lock().unwrap().as_ref().unwrap().cancelled);
    }

    #[tokio::test]
    async fn notifications_without_a_host_are_noops() {
        let driver = GoalDriver::new();
        driver.notify_goal_changed("user-1", "session-1");
        driver.notify_turn_ended("user-1", "session-1", None, TurnEndOutcome::Completed);
        // Let the spawned drive task observe the missing host and exit.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}
