use super::context::ToolContext;
use crate::monitor::MonitorState;
use crate::services::beeroom_realtime::BeeroomRealtimeService;
use crate::services::swarm::events::{
    TEAM_ERROR, TEAM_FINISH, TEAM_START, TEAM_TASK_DISPATCH, TEAM_TASK_RESULT, TEAM_TASK_UPDATE,
};
use crate::storage::{SessionRunRecord, StorageBackend, TeamRunRecord, TeamTaskRecord};
use anyhow::Result;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;

const TEAM_RUN_SUMMARY_MAX_CHARS: usize = 3000;

pub(crate) fn emit_swarm_run_started(context: &ToolContext<'_>, run: &TeamRunRecord) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_START,
        json!({
            "team_run_id": run.team_run_id,
            "hive_id": run.hive_id,
            "status": run.status,
            "task_total": run.task_total,
            "strategy": run.strategy,
            "updated_time": run.updated_time,
        }),
    );
}

pub(crate) fn emit_swarm_task_dispatched(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    task: &TeamTaskRecord,
) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_TASK_DISPATCH,
        json!({
            "team_run_id": task.team_run_id,
            "task_id": task.task_id,
            "hive_id": task.hive_id,
            "agent_id": task.agent_id,
            "status": task.status,
            "priority": task.priority,
            "target_session_id": task.target_session_id,
            "spawned_session_id": task.spawned_session_id,
            "updated_time": task.updated_time,
        }),
    );
}

pub(crate) fn emit_swarm_task_updated(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    task: &TeamTaskRecord,
) {
    emit_swarm_team_event(
        context,
        run,
        TEAM_TASK_UPDATE,
        swarm_task_state_payload(task),
    );

    if is_terminal_task_status(&task.status) {
        emit_swarm_team_event(
            context,
            run,
            TEAM_TASK_RESULT,
            swarm_task_state_payload(task),
        );
    }
}

pub(crate) fn apply_session_run_to_swarm_task(
    task: &mut TeamTaskRecord,
    session_run: &SessionRunRecord,
) {
    if let Some(run_id) = normalize_optional(Some(session_run.run_id.as_str())) {
        task.session_run_id = Some(run_id);
    }
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
    if let Some(result) = normalize_optional(session_run.result.as_deref()) {
        task.result_summary = Some(result);
    }
    if let Some(error) = normalize_optional(session_run.error.as_deref()) {
        task.error = Some(error);
    }
    task.updated_time = task.updated_time.max(session_run.updated_time);
}

pub(crate) fn reconcile_swarm_task_from_session_run(
    storage: &dyn StorageBackend,
    monitor: Option<Arc<MonitorState>>,
    beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    task_id: &str,
    session_run: &SessionRunRecord,
) -> Result<()> {
    let cleaned_task_id = task_id.trim();
    if cleaned_task_id.is_empty() {
        return Ok(());
    }
    let Some(mut task) = storage.get_team_task(cleaned_task_id)? else {
        return Ok(());
    };
    let Some(mut run) = storage.get_team_run(&task.team_run_id)? else {
        return Ok(());
    };
    apply_session_run_to_swarm_task(&mut task, session_run);
    storage.upsert_team_task(&task)?;
    emit_swarm_task_updated_background(monitor.clone(), beeroom_realtime.clone(), &run, &task);

    let tasks = storage.list_team_tasks(&run.team_run_id)?;
    let (terminal, failed) = sync_swarm_run_summary_storage(storage, &mut run, &tasks)?;
    if terminal {
        emit_swarm_run_terminal_background(monitor, beeroom_realtime, &run, failed);
    }
    Ok(())
}

pub(crate) fn sync_swarm_run_summary_storage(
    storage: &dyn StorageBackend,
    run: &mut TeamRunRecord,
    tasks: &[TeamTaskRecord],
) -> Result<(bool, bool)> {
    // Keep run-level counters/status aligned with task snapshots from agent_swarm paths.
    let mut success_total = 0i64;
    let mut failed_total = 0i64;
    let mut active_total = 0usize;
    let mut all_cancelled = !tasks.is_empty();
    let mut latest_updated = run.updated_time;
    let mut earliest_started = run.started_time;
    let mut latest_finished = run.finished_time;

    for task in tasks {
        let normalized = normalize_status(&task.status);
        if is_success_task_status(&normalized) {
            success_total += 1;
            all_cancelled = false;
        } else if is_failed_task_status(&normalized) {
            failed_total += 1;
            if normalized != "cancelled" {
                all_cancelled = false;
            }
        } else {
            active_total += 1;
            all_cancelled = false;
        }
        latest_updated = latest_updated.max(task.updated_time);
        if let Some(started) = task.started_time {
            earliest_started = Some(
                earliest_started
                    .map(|current| current.min(started))
                    .unwrap_or(started),
            );
        }
        if let Some(finished) = task.finished_time {
            latest_finished = Some(
                latest_finished
                    .map(|current| current.max(finished))
                    .unwrap_or(finished),
            );
        }
    }

    run.task_total = tasks.len() as i64;
    run.task_success = success_total;
    run.task_failed = failed_total;
    run.started_time = earliest_started;
    run.updated_time = latest_updated;

    let terminal = !tasks.is_empty() && active_total == 0;
    let failed = terminal && failed_total > 0;

    if !terminal {
        run.status = if tasks.is_empty() {
            "queued".to_string()
        } else {
            "running".to_string()
        };
        run.finished_time = None;
        run.elapsed_s = None;
        run.error = None;
    } else {
        let finished_at = latest_finished.unwrap_or_else(now_ts);
        run.finished_time = Some(finished_at);
        run.elapsed_s = run
            .started_time
            .map(|started| (finished_at - started).max(0.0));

        if failed {
            run.status = if all_cancelled {
                "cancelled".to_string()
            } else {
                "failed".to_string()
            };
            run.summary = build_tool_managed_summary(tasks);
            run.error = tasks
                .iter()
                .filter_map(|task| normalize_optional(task.error.as_deref()))
                .next()
                .or_else(|| all_cancelled.then_some("cancelled".to_string()));
        } else {
            run.status = "completed".to_string();
            run.summary = build_tool_managed_summary(tasks);
            run.error = None;
        }
    }

    storage.upsert_team_run(run)?;
    Ok((terminal, failed))
}

pub(crate) fn sync_swarm_run_summary(
    context: &ToolContext<'_>,
    run: &mut TeamRunRecord,
    tasks: &[TeamTaskRecord],
) -> Result<(bool, bool)> {
    sync_swarm_run_summary_storage(context.storage.as_ref(), run, tasks)
}

pub(crate) fn emit_swarm_run_terminal(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    failed: bool,
) {
    if failed {
        emit_swarm_team_event(context, run, TEAM_ERROR, swarm_run_error_payload(run));
    }
    emit_swarm_team_event(context, run, TEAM_FINISH, swarm_run_finish_payload(run));
}

pub(crate) fn emit_swarm_team_event(
    context: &ToolContext<'_>,
    run: &TeamRunRecord,
    event_type: &str,
    payload: Value,
) {
    let cleaned_event = event_type.trim();
    if cleaned_event.is_empty() {
        return;
    }

    let normalized_payload = normalize_swarm_team_event_payload(run, payload);

    let streamed = if let Some(emitter) = context
        .event_emitter
        .as_ref()
        .filter(|item| item.stream_enabled())
    {
        emitter.emit(cleaned_event, normalized_payload.clone());
        true
    } else {
        false
    };

    // Streamed tool events already go through the shared event emitter, which
    // records them into monitor detail. Avoid writing the same swarm event into
    // monitor twice when streaming is enabled.
    if !streamed {
        record_swarm_team_event_to_monitor(
            context.monitor.as_ref(),
            run,
            cleaned_event,
            &normalized_payload,
        );
    }
    publish_swarm_team_event_realtime(
        context.beeroom_realtime.as_ref().cloned(),
        run,
        cleaned_event,
        normalized_payload,
    );
}

fn emit_swarm_task_updated_background(
    monitor: Option<Arc<MonitorState>>,
    beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    run: &TeamRunRecord,
    task: &TeamTaskRecord,
) {
    emit_swarm_team_event_background(
        monitor.clone(),
        beeroom_realtime.clone(),
        run,
        TEAM_TASK_UPDATE,
        swarm_task_state_payload(task),
    );

    if is_terminal_task_status(&task.status) {
        emit_swarm_team_event_background(
            monitor,
            beeroom_realtime,
            run,
            TEAM_TASK_RESULT,
            swarm_task_state_payload(task),
        );
    }
}

fn emit_swarm_run_terminal_background(
    monitor: Option<Arc<MonitorState>>,
    beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    run: &TeamRunRecord,
    failed: bool,
) {
    if failed {
        emit_swarm_team_event_background(
            monitor.clone(),
            beeroom_realtime.clone(),
            run,
            TEAM_ERROR,
            swarm_run_error_payload(run),
        );
    }
    emit_swarm_team_event_background(
        monitor,
        beeroom_realtime,
        run,
        TEAM_FINISH,
        swarm_run_finish_payload(run),
    );
}

fn emit_swarm_team_event_background(
    monitor: Option<Arc<MonitorState>>,
    beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    run: &TeamRunRecord,
    event_type: &str,
    payload: Value,
) {
    let cleaned_event = event_type.trim();
    if cleaned_event.is_empty() {
        return;
    }
    let normalized_payload = normalize_swarm_team_event_payload(run, payload);
    record_swarm_team_event_to_monitor(monitor.as_ref(), run, cleaned_event, &normalized_payload);
    publish_swarm_team_event_realtime(beeroom_realtime, run, cleaned_event, normalized_payload);
}

fn normalize_swarm_team_event_payload(run: &TeamRunRecord, mut payload: Value) -> Value {
    if let Value::Object(ref mut map) = payload {
        map.entry("team_run_id".to_string())
            .or_insert_with(|| Value::String(run.team_run_id.clone()));
        map.entry("hive_id".to_string())
            .or_insert_with(|| Value::String(run.hive_id.clone()));
        map.entry("status".to_string())
            .or_insert_with(|| Value::String(run.status.clone()));
        map.entry("updated_time".to_string())
            .or_insert_with(|| json!(run.updated_time));
    }
    payload
}

fn record_swarm_team_event_to_monitor(
    monitor: Option<&Arc<MonitorState>>,
    run: &TeamRunRecord,
    event_type: &str,
    payload: &Value,
) {
    let session_id = run.parent_session_id.trim();
    if session_id.is_empty() {
        return;
    }
    if let Some(monitor) = monitor {
        monitor.record_event(session_id, event_type, payload);
    }
}

fn publish_swarm_team_event_realtime(
    beeroom_realtime: Option<Arc<BeeroomRealtimeService>>,
    run: &TeamRunRecord,
    event_name: &str,
    payload: Value,
) {
    let cleaned_user = run.user_id.trim();
    let cleaned_hive = run.hive_id.trim();
    if cleaned_user.is_empty() || cleaned_hive.is_empty() {
        return;
    }
    let Some(realtime) = beeroom_realtime else {
        return;
    };
    let user_id = cleaned_user.to_string();
    let hive_id = cleaned_hive.to_string();
    let event_name = event_name.to_string();
    tokio::spawn(async move {
        realtime
            .publish_group_event(&user_id, &hive_id, &event_name, payload)
            .await;
    });
}

fn swarm_task_state_payload(task: &TeamTaskRecord) -> Value {
    json!({
        "team_run_id": task.team_run_id,
        "task_id": task.task_id,
        "hive_id": task.hive_id,
        "agent_id": task.agent_id,
        "session_run_id": task.session_run_id,
        "status": task.status,
        "retry_count": task.retry_count,
        "started_time": task.started_time,
        "finished_time": task.finished_time,
        "elapsed_s": task.elapsed_s,
        "result_summary": task.result_summary,
        "error": task.error,
        "updated_time": task.updated_time,
    })
}

fn swarm_run_error_payload(run: &TeamRunRecord) -> Value {
    json!({
        "team_run_id": run.team_run_id,
        "hive_id": run.hive_id,
        "status": run.status,
        "task_total": run.task_total,
        "task_success": run.task_success,
        "task_failed": run.task_failed,
        "summary": run.summary,
        "error": run.error,
        "updated_time": run.updated_time,
    })
}

fn swarm_run_finish_payload(run: &TeamRunRecord) -> Value {
    json!({
        "team_run_id": run.team_run_id,
        "hive_id": run.hive_id,
        "status": run.status,
        "task_total": run.task_total,
        "task_success": run.task_success,
        "task_failed": run.task_failed,
        "started_time": run.started_time,
        "finished_time": run.finished_time,
        "elapsed_s": run.elapsed_s,
        "summary": run.summary,
        "error": run.error,
        "updated_time": run.updated_time,
    })
}

fn normalize_status(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let content = text.trim();
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let mut output = content.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn build_tool_managed_summary(tasks: &[TeamTaskRecord]) -> Option<String> {
    let mut ordered = tasks.to_vec();
    ordered.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.updated_time.total_cmp(&b.updated_time))
            .then_with(|| a.task_id.cmp(&b.task_id))
    });
    if ordered.is_empty() {
        return None;
    }
    if ordered.len() == 1 {
        let task = &ordered[0];
        let summary = normalize_optional(task.result_summary.as_deref())
            .or_else(|| normalize_optional(task.error.as_deref()))
            .unwrap_or_else(|| normalize_status(&task.status));
        return Some(truncate_text(&summary, TEAM_RUN_SUMMARY_MAX_CHARS));
    }
    let mut lines = Vec::with_capacity(ordered.len());
    for task in ordered {
        let status = normalize_status(&task.status);
        let summary = normalize_optional(task.result_summary.as_deref())
            .or_else(|| normalize_optional(task.error.as_deref()))
            .unwrap_or_else(|| status.clone());
        lines.push(format!("[{}][{}] {summary}", task.agent_id, status));
    }
    Some(truncate_text(&lines.join("\n"), TEAM_RUN_SUMMARY_MAX_CHARS))
}

fn is_success_task_status(status: &str) -> bool {
    matches!(status, "success" | "completed")
}

fn is_failed_task_status(status: &str) -> bool {
    matches!(status, "failed" | "error" | "timeout" | "cancelled")
}

fn is_terminal_task_status(status: &str) -> bool {
    let normalized = normalize_status(status);
    is_success_task_status(&normalized) || is_failed_task_status(&normalized)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{
        build_tool_managed_summary, emit_swarm_run_started, reconcile_swarm_task_from_session_run,
    };
    use crate::a2a_store::A2aStore;
    use crate::config::{Config, ObservabilityConfig, SandboxConfig};
    use crate::lsp::LspManager;
    use crate::monitor::MonitorState;
    use crate::services::swarm::events::TEAM_START;
    use crate::services::tools::context::{ToolContext, ToolEventEmitter};
    use crate::skills::SkillRegistry;
    use crate::storage::{
        SessionRunRecord, SqliteStorage, StorageBackend, TeamRunRecord, TeamTaskRecord,
    };
    use crate::workspace::WorkspaceManager;
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tempfile::tempdir;

    fn make_task(
        task_id: &str,
        agent_id: &str,
        status: &str,
        priority: i64,
        updated_time: f64,
        result_summary: Option<&str>,
        error: Option<&str>,
    ) -> TeamTaskRecord {
        TeamTaskRecord {
            task_id: task_id.to_string(),
            team_run_id: "team_demo".to_string(),
            user_id: "u_demo".to_string(),
            hive_id: "default".to_string(),
            agent_id: agent_id.to_string(),
            target_session_id: None,
            spawned_session_id: None,
            session_run_id: None,
            status: status.to_string(),
            retry_count: 0,
            priority,
            started_time: None,
            finished_time: None,
            elapsed_s: None,
            result_summary: result_summary.map(ToString::to_string),
            error: error.map(ToString::to_string),
            updated_time,
        }
    }

    #[test]
    fn tool_managed_summary_single_task_prefers_result() {
        let tasks = vec![make_task(
            "task_1",
            "agent_a",
            "success",
            0,
            1.0,
            Some("alpha result"),
            None,
        )];
        assert_eq!(
            build_tool_managed_summary(&tasks),
            Some("alpha result".to_string())
        );
    }

    #[test]
    fn tool_managed_summary_multiple_tasks_orders_deterministically() {
        let tasks = vec![
            make_task(
                "task_2",
                "agent_b",
                "failed",
                0,
                2.0,
                None,
                Some("beta error"),
            ),
            make_task(
                "task_1",
                "agent_a",
                "success",
                0,
                1.0,
                Some("alpha result"),
                None,
            ),
        ];
        assert_eq!(
            build_tool_managed_summary(&tasks),
            Some("[agent_a][success] alpha result\n[agent_b][failed] beta error".to_string())
        );
    }

    struct TestHarness {
        _dir: tempfile::TempDir,
        config: Config,
        storage: Arc<dyn StorageBackend>,
        workspace: Arc<WorkspaceManager>,
        lsp_manager: Arc<LspManager>,
        monitor: Arc<MonitorState>,
        a2a_store: A2aStore,
        skills: SkillRegistry,
        http: reqwest::Client,
    }

    impl TestHarness {
        fn new() -> Self {
            let dir = tempdir().expect("tempdir");
            let db_path = dir.path().join("swarm-realtime.db");
            let workspace_root = dir.path().join("workspace");
            let storage: Arc<dyn StorageBackend> =
                Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
            storage.ensure_initialized().expect("init storage");
            let config = Config::default();
            let workspace = Arc::new(WorkspaceManager::new(
                workspace_root.to_string_lossy().as_ref(),
                storage.clone(),
                0,
                &HashMap::new(),
            ));
            let lsp_manager = LspManager::new(workspace.clone());
            let monitor = Arc::new(MonitorState::new(
                storage.clone(),
                ObservabilityConfig {
                    log_level: String::new(),
                    monitor_event_limit: 1000,
                    monitor_payload_max_chars: 4000,
                    monitor_drop_event_types: Vec::new(),
                    ..ObservabilityConfig::default()
                },
                SandboxConfig::default(),
                workspace_root.to_string_lossy().to_string(),
            ));
            Self {
                _dir: dir,
                config,
                storage,
                workspace,
                lsp_manager,
                monitor,
                a2a_store: A2aStore::default(),
                skills: SkillRegistry::default(),
                http: reqwest::Client::new(),
            }
        }

        fn context<'a>(
            &'a self,
            user_id: &'a str,
            session_id: &'a str,
            event_emitter: Option<ToolEventEmitter>,
        ) -> ToolContext<'a> {
            ToolContext {
                user_id,
                session_id,
                workspace_id: "workspace-test",
                agent_id: Some("agent_parent"),
                user_round: None,
                model_round: None,
                is_admin: false,
                storage: self.storage.clone(),
                orchestrator: None,
                monitor: Some(self.monitor.clone()),
                beeroom_realtime: None,
                workspace: self.workspace.clone(),
                lsp_manager: self.lsp_manager.clone(),
                config: &self.config,
                a2a_store: &self.a2a_store,
                skills: &self.skills,
                gateway: None,
                user_world: None,
                cron_wake_signal: None,
                user_tool_manager: None,
                user_tool_bindings: None,
                user_tool_store: None,
                request_config_overrides: None,
                allow_roots: None,
                read_roots: None,
                command_sessions: None,
                event_emitter,
                http: &self.http,
            }
        }

        fn make_run(&self, user_id: &str, session_id: &str) -> TeamRunRecord {
            TeamRunRecord {
                team_run_id: "team_demo".to_string(),
                user_id: user_id.to_string(),
                hive_id: "default".to_string(),
                parent_session_id: session_id.to_string(),
                parent_agent_id: Some("agent_parent".to_string()),
                mother_agent_id: Some("agent_mother".to_string()),
                strategy: "batch_send".to_string(),
                status: "queued".to_string(),
                task_total: 2,
                task_success: 0,
                task_failed: 0,
                context_tokens_total: 0,
                context_tokens_peak: 0,
                model_round_total: 0,
                started_time: None,
                finished_time: None,
                elapsed_s: None,
                summary: None,
                error: None,
                updated_time: 1234.0,
            }
        }
    }

    fn team_events<'a>(detail: &'a Value, event_type: &str) -> Vec<&'a Value> {
        detail["events"]
            .as_array()
            .expect("events should be an array")
            .iter()
            .filter(|event| event["type"].as_str() == Some(event_type))
            .collect()
    }

    #[tokio::test]
    async fn swarm_stream_events_use_single_monitor_write_path() {
        let harness = TestHarness::new();
        let session_id = "sess_swarm_stream";
        let user_id = "user_stream";
        harness
            .monitor
            .register(session_id, user_id, "agent_parent", "", false, false);

        let monitor = harness.monitor.clone();
        let session = session_id.to_string();
        let emitter = ToolEventEmitter::new(
            move |event_type, mut data| {
                if let Value::Object(ref mut map) = data {
                    map.insert("user_round".to_string(), json!(4));
                    map.insert("model_round".to_string(), json!(2));
                }
                monitor.record_event(&session, event_type, &data);
            },
            true,
        );
        let context = harness.context(user_id, session_id, Some(emitter));
        let run = harness.make_run(user_id, session_id);

        emit_swarm_run_started(&context, &run);

        let detail = harness
            .monitor
            .get_detail(session_id)
            .expect("detail should exist");
        let team_start = team_events(&detail, TEAM_START);
        assert_eq!(team_start.len(), 1);
        assert_eq!(team_start[0]["data"]["user_round"], json!(4));
        assert_eq!(team_start[0]["data"]["model_round"], json!(2));
    }

    #[tokio::test]
    async fn swarm_non_stream_events_still_write_monitor_directly() {
        let harness = TestHarness::new();
        let session_id = "sess_swarm_non_stream";
        let user_id = "user_non_stream";
        harness
            .monitor
            .register(session_id, user_id, "agent_parent", "", false, false);

        let callback_count = Arc::new(AtomicUsize::new(0));
        let callback_counter = callback_count.clone();
        let emitter = ToolEventEmitter::new(
            move |_event_type, _data| {
                callback_counter.fetch_add(1, Ordering::Relaxed);
            },
            false,
        );
        let context = harness.context(user_id, session_id, Some(emitter));
        let run = harness.make_run(user_id, session_id);

        emit_swarm_run_started(&context, &run);

        assert_eq!(callback_count.load(Ordering::Relaxed), 0);
        let detail = harness
            .monitor
            .get_detail(session_id)
            .expect("detail should exist");
        let team_start = team_events(&detail, TEAM_START);
        assert_eq!(team_start.len(), 1);
        assert!(team_start[0]["data"].get("user_round").is_none());
        assert!(team_start[0]["data"].get("model_round").is_none());
    }

    #[tokio::test]
    async fn reconcile_swarm_task_from_session_run_updates_task_and_run_terminal_state() {
        let harness = TestHarness::new();
        let session_id = "sess_swarm_reconcile";
        let user_id = "user_reconcile";
        harness
            .monitor
            .register(session_id, user_id, "agent_parent", "", false, false);

        let run = harness.make_run(user_id, session_id);
        harness
            .storage
            .upsert_team_run(&run)
            .expect("upsert team run");
        let task = make_task("task_done", "agent_worker", "queued", 0, 10.0, None, None);
        harness
            .storage
            .upsert_team_task(&task)
            .expect("upsert team task");

        let session_run = SessionRunRecord {
            run_id: "run_worker".to_string(),
            session_id: "sess_worker".to_string(),
            parent_session_id: Some(session_id.to_string()),
            user_id: user_id.to_string(),
            dispatch_id: None,
            run_kind: Some("swarm".to_string()),
            requested_by: Some("agent_swarm".to_string()),
            agent_id: Some("agent_worker".to_string()),
            model_name: None,
            status: "success".to_string(),
            queued_time: 100.0,
            started_time: 101.0,
            finished_time: 111.0,
            elapsed_s: 10.0,
            result: Some("done".to_string()),
            error: None,
            updated_time: 111.0,
            metadata: None,
        };

        reconcile_swarm_task_from_session_run(
            harness.storage.as_ref(),
            Some(harness.monitor.clone()),
            None,
            &task.task_id,
            &session_run,
        )
        .expect("reconcile swarm task");

        let updated_task = harness
            .storage
            .get_team_task(&task.task_id)
            .expect("load task")
            .expect("task exists");
        assert_eq!(updated_task.status, "success");
        assert_eq!(updated_task.session_run_id.as_deref(), Some("run_worker"));
        assert_eq!(updated_task.result_summary.as_deref(), Some("done"));

        let updated_run = harness
            .storage
            .get_team_run(&run.team_run_id)
            .expect("load team run")
            .expect("team run exists");
        assert_eq!(updated_run.status, "completed");
        assert_eq!(updated_run.task_success, 1);
        assert_eq!(updated_run.task_failed, 0);
        assert_eq!(updated_run.finished_time, Some(111.0));

        let detail = harness
            .monitor
            .get_detail(session_id)
            .expect("detail should exist");
        assert_eq!(team_events(&detail, "team_task_update").len(), 1);
        assert_eq!(team_events(&detail, "team_task_result").len(), 1);
        assert_eq!(team_events(&detail, "team_finish").len(), 1);
    }
}
