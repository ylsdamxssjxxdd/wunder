use super::execute_support::*;
use super::limiter::RequestLimiter;
use super::thread_runtime::{
    thread_closed_payload, thread_not_loaded_payload, thread_status_payload, ThreadRuntimeStatus,
    ThreadRuntimeUpdate,
};
use super::tool_parallel::tool_call_supports_parallel;
use super::*;
use crate::core::approval::{
    ApprovalRequest, ApprovalRequestKind, ApprovalRequestTx, ApprovalResponse,
};
use crate::services::chat_cancel_marker::persist_user_cancelled_turn_marker;
use crate::services::goal;
use crate::tools::ToolContext;
use crate::user_store::UserStore;
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::warn;
use uuid::Uuid;

impl Orchestrator {
    pub(super) async fn finish_request_resources(
        emitter: &EventEmitter,
        limiter: &RequestLimiter,
        session_id: &str,
        acquired: bool,
        heartbeat_task: &mut Option<JoinHandle<()>>,
    ) {
        emitter.finish().await;
        if acquired {
            limiter.release(session_id).await;
        }
        if let Some(handle) = heartbeat_task.take() {
            handle.abort();
        }
    }

    pub(super) async fn emit_and_persist_round_usage(
        &self,
        user_id: &str,
        session_id: &str,
        emitter: &EventEmitter,
        request_round: RoundInfo,
        round_usage: &TokenUsage,
        round_context_tokens: i64,
    ) {
        let usage_payload =
            build_round_usage_payload(round_usage, round_context_tokens, request_round);
        emitter.emit("round_usage", usage_payload).await;
        if round_context_tokens > 0 {
            self.workspace
                .save_session_context_tokens_async(user_id, session_id, round_context_tokens)
                .await;
        }
    }

    pub(super) async fn finish_request_error(
        &self,
        user_id: &str,
        session_id: &str,
        emitter: &EventEmitter,
        active_turn_id: Option<&str>,
        active_turn_round: RoundInfo,
        err: &OrchestratorError,
    ) {
        emitter.emit("error", err.to_payload()).await;
        emit_turn_terminal_event(
            emitter,
            active_turn_round,
            TurnTerminalEvent {
                status: turn_terminal_status_for_error(err),
                stop_reason: Some(err.code()),
                round_usage: None,
                error: Some(err),
                waiting_for_user_input: false,
                stop_meta: None,
            },
        )
        .await;
        if let Some(turn_id) = active_turn_id {
            self.finish_active_turn(
                session_id,
                turn_id,
                emitter,
                active_turn_round,
                ThreadRuntimeStatus::Idle,
            )
            .await;
        }
        if !matches!(err.code(), "USER_BUSY" | "CANCELLED") {
            self.append_chat(
                user_id,
                session_id,
                "assistant",
                Some(&json!(err.message())),
                None,
                None,
                None,
                None,
                None,
            );
        }
        if err.code() == "CANCELLED" {
            let cancel_source = err
                .detail()
                .and_then(|detail| detail.get("cancel_source"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if let Some(cancel_source) = cancel_source {
                self.monitor
                    .mark_cancelled_with_source(session_id, cancel_source);
            } else {
                self.monitor.mark_cancelled(session_id);
            }
            if let Err(marker_err) = persist_user_cancelled_turn_marker(
                self.workspace.clone(),
                Arc::new(UserStore::new(self.storage.clone())),
                user_id,
                session_id,
                cancel_source.unwrap_or("orchestrator_cancel"),
            )
            .await
            {
                warn!(
                    "persist cancelled turn marker failed for session {session_id}: {marker_err}"
                );
            }
        } else if err.code() != "USER_BUSY" {
            self.monitor.mark_error(session_id, err.message());
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn finish_request_success(
        &self,
        user_id: &str,
        session_id: &str,
        agent_id: Option<&str>,
        user_round_id: &str,
        display_question: &str,
        answer: &str,
        emitter: &EventEmitter,
        last_round_info: RoundInfo,
        active_turn_id: Option<&str>,
        goal_continuation_turn: bool,
        goal_turn_started_at: Instant,
        waiting_question_panel: bool,
        has_round_usage: bool,
        round_usage: &TokenUsage,
        stop_reason: &str,
        stop_meta: Option<&Value>,
        skip_auto_memory_extract: bool,
        llm_config: LlmModelConfig,
    ) {
        emit_turn_terminal_event(
            emitter,
            last_round_info,
            TurnTerminalEvent {
                status: "completed",
                stop_reason: Some(stop_reason),
                round_usage: has_round_usage.then_some(round_usage),
                error: None,
                waiting_for_user_input: waiting_question_panel,
                stop_meta,
            },
        )
        .await;
        let goal_usage_record = if goal_continuation_turn {
            let elapsed_seconds = goal_turn_started_at.elapsed().as_secs().max(1) as i64;
            goal::account_turn_usage(
                self.storage.clone(),
                user_id,
                session_id,
                round_usage.total,
                elapsed_seconds,
            )
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        let goal_record = match goal_usage_record {
            Some(record) => Some(record),
            None => goal::get_goal(self.storage.clone(), user_id, session_id)
                .await
                .ok()
                .flatten(),
        };
        if let Some(record) = goal_record.as_ref() {
            if goal::should_continue_goal(record, waiting_question_panel) {
                emitter
                    .emit(
                        "goal_continuation_ready",
                        json!({ "goal": goal::goal_payload(record) }),
                    )
                    .await;
            }
        }
        if let Some(turn_id) = active_turn_id {
            self.finish_active_turn(
                session_id,
                turn_id,
                emitter,
                last_round_info,
                if waiting_question_panel {
                    ThreadRuntimeStatus::WaitingUserInput
                } else {
                    ThreadRuntimeStatus::Idle
                },
            )
            .await;
        }
        if !waiting_question_panel && !answer.trim().is_empty() && !skip_auto_memory_extract {
            self.spawn_auto_memory_extraction(
                user_id,
                agent_id,
                session_id,
                Some(user_round_id),
                display_question,
                answer,
                llm_config,
            );
        }
        if waiting_question_panel {
            self.monitor.mark_question_panel(session_id);
        } else {
            self.monitor.mark_finished(session_id);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn execute_tool_calls_parallel(
        &self,
        calls: Vec<PlannedToolCall>,
        tool_context: &ToolContext<'_>,
        allowed_tool_names: &HashSet<String>,
        session_id: &str,
        turn_id: &str,
        emitter: &EventEmitter,
        approval_tx: Option<ApprovalRequestTx>,
        round_info: RoundInfo,
    ) -> Result<Vec<ToolExecutionOutcome>, OrchestratorError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }
        let parallelism = resolve_tool_parallelism(calls.len());
        let execution_lock = Arc::new(tokio::sync::RwLock::new(()));
        let mut stream = futures::stream::iter(calls.into_iter().map(|planned| {
            let orchestrator = self;
            let approval_tx = approval_tx.clone();
            let emitter = emitter.clone();
            let execution_lock = Arc::clone(&execution_lock);
            async move {
                let PlannedToolCall {
                    mut call,
                    name,
                    function_name,
                } = planned;
                let event_tool_call_id = call
                    .id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
                let tool_display_name =
                    crate::tools::resolve_runtime_tool_display_name(tool_context.config, &name);
                let scoped_tool_context = tool_context.with_event_emitter(
                    tool_context.event_emitter.as_ref().map(|event_emitter| {
                        let mut scoped = event_emitter
                            .with_field("tool_runtime_name", Value::String(name.clone()))
                            .with_field(
                                "tool_display_name",
                                Value::String(tool_display_name.clone()),
                            )
                            .with_field(
                                "tool_function_name",
                                Value::String(function_name.clone()),
                            );
                        if let Some(tool_call_id) = event_tool_call_id.as_ref() {
                            scoped = scoped.with_field(
                                "tool_call_id",
                                Value::String(tool_call_id.clone()),
                            );
                        }
                        scoped
                    }),
                );
                let recovered_args =
                    crate::core::tool_args::recover_tool_args_value_with_meta(&call.arguments);
                call.arguments = recovered_args.value.clone();
                let args = call.arguments.clone();
                let args_repair = recovered_args.repair.clone();
                let workspace_version_before = scoped_tool_context
                    .workspace
                    .get_tree_version(scoped_tool_context.workspace_id);
                let policy_decision = crate::exec_policy::evaluate_tool_call(
                    scoped_tool_context.config,
                    &name,
                    &args,
                    Some(scoped_tool_context.session_id),
                    Some(scoped_tool_context.user_id),
                );
                let policy_meta = policy_decision.as_ref().map(|decision| decision.to_value());
                let started_at = Instant::now();
                let tool_timeout =
                    orchestrator.resolve_tool_timeout(scoped_tool_context.config, &name, &args);
                let supports_parallel_execution = tool_call_supports_parallel(&name, &args);
                let mut result = if !allowed_tool_names.contains(&name) {
                    ToolResultPayload::error(
                        i18n::t("error.tool_disabled_or_unavailable"),
                        json!({ "tool": name.clone() }),
                    )
                } else if let Some(decision) = policy_decision.as_ref() {
                    if !decision.allowed {
                        let mut approved = None;
                        let mut approval_id = None::<String>;
                        let mut approval_kind = None::<ApprovalRequestKind>;
                        let mut approval_summary = None::<String>;
                        if decision.requires_approval {
                            if let Some(tx) = approval_tx.clone() {
                                let (respond_to, response_rx) = tokio::sync::oneshot::channel();
                                let kind = approval_kind_for_tool(&name);
                                let summary = approval_summary_for_tool(&name, &args, kind);
                                let request_id = Uuid::new_v4().simple().to_string();
                                let detail = json!({
                                    "policy": policy_meta.clone().unwrap_or(Value::Null),
                                    "reason": decision.reason.clone(),
                                });
                                let request = ApprovalRequest {
                                    id: request_id.clone(),
                                    kind,
                                    tool: name.clone(),
                                    args: args.clone(),
                                    summary: summary.clone(),
                                    detail: detail.clone(),
                                    respond_to,
                                };
                                if tx.send(request).is_ok() {
                                    approval_id = Some(request_id.clone());
                                    approval_kind = Some(kind);
                                    approval_summary = Some(summary.clone());
                                    orchestrator
                                        .monitor
                                        .mark_approval_pending(session_id, Some(summary.as_str()));
                                    let mut event_payload = json!({
                                        "approval_id": request_id,
                                        "kind": kind,
                                        "tool": name.clone(),
                                        "summary": summary.clone(),
                                        "args": args.clone(),
                                        "detail": detail,
                                    });
                                    if let Value::Object(ref mut map) = event_payload {
                                        if let Some(meta) = policy_meta.clone() {
                                            map.insert("policy".to_string(), meta);
                                        }
                                        if let Some(tool_call_id) = event_tool_call_id.as_ref() {
                                            map.insert(
                                                "tool_call_id".to_string(),
                                                Value::String(tool_call_id.clone()),
                                            );
                                        }
                                        round_info.insert_into(map);
                                    }
                                    emitter.emit("approval_request", event_payload).await;
                                    let _ = orchestrator.active_turns.add_pending_approval(
                                        session_id,
                                        turn_id,
                                        &request_id,
                                    );
                                    orchestrator
                                        .emit_thread_runtime_update(
                                            &emitter,
                                            round_info,
                                            orchestrator.thread_runtime.set_status(
                                                session_id,
                                                turn_id,
                                                ThreadRuntimeStatus::WaitingApproval,
                                            ),
                                        )
                                        .await;
                                    approved = tokio::select! {
                                        res = response_rx => res.ok(),
                                        err = orchestrator.wait_for_cancelled(session_id) => {
                                            if let Some(id) = approval_id.as_deref() {
                                                let _ = orchestrator.active_turns.resolve_pending_approval(
                                                    session_id,
                                                    turn_id,
                                                    id,
                                                );
                                                emit_approval_resolved_event(
                                                    &emitter,
                                                    round_info,
                                                    ApprovalResolvedEvent {
                                                        approval_id: id,
                                                        status: "cancelled",
                                                        scope: "none",
                                                        kind: approval_kind,
                                                        tool_name: &name,
                                                        summary: approval_summary.as_deref(),
                                                        resolved_by: Some("session_cancelled"),
                                                    },
                                                )
                                                .await;
                                            }
                                            return Err(err);
                                        }
                                    };
                                    orchestrator.monitor.mark_running(session_id, None);
                                }
                            }
                        }

                        let approval_response = approved.unwrap_or(ApprovalResponse::Deny);
                        let approval_snapshot = approval_id.as_deref().and_then(|id| {
                            orchestrator
                                .active_turns
                                .resolve_pending_approval(session_id, turn_id, id)
                        });
                        if let Some(id) = approval_id {
                            let (status, scope) =
                                approval_resolution_status_and_scope(approval_response);
                            let mut event_payload = json!({
                                "approval_id": id.clone(),
                                "status": status,
                                "scope": scope,
                                "kind": approval_kind,
                                "tool": name.clone(),
                                "summary": approval_summary.clone().unwrap_or_default(),
                            });
                            if let Value::Object(ref mut map) = event_payload {
                                if let Some(tool_call_id) = event_tool_call_id.as_ref() {
                                    map.insert(
                                        "tool_call_id".to_string(),
                                        Value::String(tool_call_id.clone()),
                                    );
                                }
                                round_info.insert_into(map);
                            }
                            emitter.emit("approval_result", event_payload).await;
                            emit_approval_resolved_event(
                                &emitter,
                                round_info,
                                ApprovalResolvedEvent {
                                    approval_id: &id,
                                    status,
                                    scope,
                                    kind: approval_kind,
                                    tool_name: &name,
                                    summary: approval_summary.as_deref(),
                                    resolved_by: Some("approval_response"),
                                },
                            )
                            .await;
                        }
                        if let Some(snapshot) = approval_snapshot {
                            if snapshot.pending_approval_ids.is_empty()
                                && !snapshot.waiting_for_user_input
                            {
                                orchestrator
                                    .emit_thread_runtime_update(
                                        &emitter,
                                        round_info,
                                        orchestrator.thread_runtime.set_status(
                                            session_id,
                                            turn_id,
                                            ThreadRuntimeStatus::Running,
                                        ),
                                    )
                                    .await;
                            }
                        }

                        let approved = match approval_response {
                            ApprovalResponse::ApproveOnce => Some(ApprovalResponse::ApproveOnce),
                            ApprovalResponse::ApproveSession => {
                                let args_approved = args_with_approved_flag(&args);
                                let _ = crate::exec_policy::evaluate_tool_call(
                                    scoped_tool_context.config,
                                    &name,
                                    &args_approved,
                                    Some(scoped_tool_context.session_id),
                                    Some(scoped_tool_context.user_id),
                                );
                                Some(ApprovalResponse::ApproveSession)
                            }
                            ApprovalResponse::Deny => None,
                        };

                        if let Some(approval_choice) = approved {
                            let result = tokio::select! {
                                res = orchestrator.execute_tool_with_parallel_guard(
                                    Arc::clone(&execution_lock),
                                    &scoped_tool_context,
                                    &name,
                                    &args,
                                    tool_timeout,
                                    supports_parallel_execution,
                                ) => res,
                                err = orchestrator.wait_for_cancelled(session_id) => {
                                    return Err(err);
                                }
                            };
                            let mut executed = match result {
                                Ok(value) => ToolResultPayload::from_value(value),
                                Err(err) => {
                                    if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                        build_tool_timeout_result(&name, tool_timeout)
                                    } else {
                                        ToolResultPayload::error(
                                            err.to_string(),
                                            json!({ "tool": name.clone() }),
                                        )
                                    }
                                }
                            };
                            if let Some(meta) = policy_meta.clone() {
                                executed.insert_meta("policy", meta);
                            }
                            executed.insert_meta(
                                "approval",
                                json!({
                                    "status": "approved",
                                    "scope": if approval_choice == ApprovalResponse::ApproveSession {
                                        "session"
                                    } else {
                                        "once"
                                    }
                                }),
                            );
                            executed
                        } else {
                            let mut denied = ToolResultPayload::error(
                                i18n::t("tool.exec.not_allowed"),
                                json!({ "tool": name.clone() }),
                            );
                            if let Some(meta) = policy_meta.clone() {
                                denied.insert_meta("policy", meta);
                            }
                            denied
                        }
                    } else {
                        let result = tokio::select! {
                            res = orchestrator.execute_tool_with_parallel_guard(
                                Arc::clone(&execution_lock),
                                &scoped_tool_context,
                                &name,
                                &args,
                                tool_timeout,
                                supports_parallel_execution,
                            ) => res,
                            err = orchestrator.wait_for_cancelled(session_id) => {
                                return Err(err);
                            }
                        };
                        let mut executed = match result {
                            Ok(value) => ToolResultPayload::from_value(value),
                            Err(err) => {
                                if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                    build_tool_timeout_result(&name, tool_timeout)
                                } else {
                                    ToolResultPayload::error(
                                        err.to_string(),
                                        json!({ "tool": name.clone() }),
                                    )
                                }
                            }
                        };
                        if let Some(meta) = policy_meta.clone() {
                            executed.insert_meta("policy", meta);
                        }
                        executed
                    }
                } else {
                    let result = tokio::select! {
                        res = orchestrator.execute_tool_with_parallel_guard(
                            Arc::clone(&execution_lock),
                            &scoped_tool_context,
                            &name,
                            &args,
                            tool_timeout,
                            supports_parallel_execution,
                        ) => res,
                        err = orchestrator.wait_for_cancelled(session_id) => {
                            return Err(err);
                        }
                    };
                    match result {
                        Ok(value) => ToolResultPayload::from_value(value),
                        Err(err) => {
                            if err.to_string() == tool_exec::TOOL_TIMEOUT_ERROR {
                                build_tool_timeout_result(&name, tool_timeout)
                            } else {
                                ToolResultPayload::error(
                                    err.to_string(),
                                    json!({ "tool": name.clone() }),
                                )
                            }
                        }
                    }
                };
                let workspace_version_after = scoped_tool_context
                    .workspace
                    .get_tree_version(scoped_tool_context.workspace_id);
                if workspace_version_after > workspace_version_before {
                    result.insert_meta("workspace_version", json!(workspace_version_after));
                    result.insert_meta("workspace_changed", Value::Bool(true));
                }
                result.insert_meta(
                    "parallel_execution",
                    json!({
                        "mode": if supports_parallel_execution {
                            "parallel_read"
                        } else {
                            "exclusive_write"
                        },
                    }),
                );
                if let Some(repair) = args_repair.clone() {
                    result.insert_meta("repair", repair);
                }
                result = orchestrator.normalize_tool_result_payload(&name, result);
                result = orchestrator.finalize_tool_result(&name, result, started_at);
                Ok(ToolExecutionOutcome { call, name, result })
            }
        }))
        .buffered(parallelism);

        let mut outcomes = Vec::new();
        while let Some(outcome) = stream.next().await {
            outcomes.push(outcome?);
        }
        Ok(outcomes)
    }

    async fn execute_tool_with_parallel_guard(
        &self,
        execution_lock: Arc<tokio::sync::RwLock<()>>,
        tool_context: &ToolContext<'_>,
        name: &str,
        args: &Value,
        timeout: Option<Duration>,
        supports_parallel_execution: bool,
    ) -> Result<Value, anyhow::Error> {
        if supports_parallel_execution {
            let _guard = execution_lock.read().await;
            self.execute_tool_with_timeout(tool_context, name, args, timeout)
                .await
        } else {
            let _guard = execution_lock.write().await;
            self.execute_tool_with_timeout(tool_context, name, args, timeout)
                .await
        }
    }

    pub(super) async fn finish_active_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        next_status: ThreadRuntimeStatus,
    ) {
        let unresolved_approvals = self
            .active_turns
            .finish_turn(session_id, turn_id)
            .map(|snapshot| {
                snapshot
                    .pending_approval_ids
                    .into_iter()
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        let pending_entries = self
            .approval_registry
            .remove_matching(|entry| entry.session_id == session_id.trim())
            .await;
        for entry in pending_entries {
            let _ = entry.respond_to.send(ApprovalResponse::Deny);
            if unresolved_approvals.contains(&entry.approval_id) {
                emit_approval_resolved_event(
                    emitter,
                    round_info,
                    ApprovalResolvedEvent {
                        approval_id: &entry.approval_id,
                        status: "cancelled",
                        scope: "none",
                        kind: Some(entry.kind),
                        tool_name: &entry.tool,
                        summary: Some(entry.summary.as_str()),
                        resolved_by: Some("turn_cleanup"),
                    },
                )
                .await;
            }
        }
        self.emit_thread_runtime_update(
            emitter,
            round_info,
            self.thread_runtime
                .finish_turn(session_id, turn_id, next_status),
        )
        .await;
    }

    pub(super) async fn emit_thread_runtime_update(
        &self,
        emitter: &EventEmitter,
        round_info: RoundInfo,
        update: ThreadRuntimeUpdate,
    ) {
        if let Some(snapshot) = update.status {
            let mut payload = thread_status_payload(&snapshot);
            if let Value::Object(ref mut map) = payload {
                round_info.insert_into(map);
            }
            emitter.emit("thread_status", payload).await;
        }
        if let Some(closed_event) = update.closed {
            let mut status_payload = thread_not_loaded_payload(&closed_event);
            if let Value::Object(ref mut map) = status_payload {
                round_info.insert_into(map);
            }
            emitter.emit("thread_status", status_payload).await;

            let mut closed_payload = thread_closed_payload(&closed_event);
            if let Value::Object(ref mut map) = closed_payload {
                round_info.insert_into(map);
            }
            emitter.emit("thread_closed", closed_payload).await;
        }
    }
}
