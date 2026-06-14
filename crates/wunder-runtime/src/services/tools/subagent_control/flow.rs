use super::*;
use std::collections::HashMap;
use std::time::Instant;
use tokio::time::sleep;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WaitCompletionMode {
    All,
    Any,
    FirstSuccess,
}

impl WaitCompletionMode {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Any => "any",
            Self::FirstSuccess => "first_success",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BatchDispatchStrategy {
    ParallelAll,
    FirstSuccess,
    ReviewThenMerge,
}

impl BatchDispatchStrategy {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::ParallelAll => "parallel_all",
            Self::FirstSuccess => "first_success",
            Self::ReviewThenMerge => "review_then_merge",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RemainingBranchAction {
    Keep,
    Interrupt,
    Close,
}

impl RemainingBranchAction {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Interrupt => "interrupt",
            Self::Close => "close",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WaitProgressState {
    pub(super) completion_reached: bool,
    pub(super) all_finished: bool,
    pub(super) matched_total: i64,
    pub(super) matched_success_total: i64,
    pub(super) matched_failed_total: i64,
    pub(super) completed_reason: &'static str,
}

pub(super) async fn wait_for_targets(
    context: &ToolContext<'_>,
    selector: ResolvedTargetSet,
    wait_seconds: f64,
    poll_interval_seconds: f64,
    completion_mode: WaitCompletionMode,
    emit_progress: bool,
) -> Result<Value> {
    let poll_interval = normalize_poll_interval(poll_interval_seconds);
    let started_at = Instant::now();
    let mut status_index = HashMap::new();
    loop {
        let snapshots = collect_snapshots(context, &selector)?;
        if snapshots.is_empty() {
            return Ok(summarize_snapshots(
                &selector,
                snapshots,
                wait_seconds,
                0.0,
                completion_mode,
                WaitProgressState {
                    completion_reached: true,
                    all_finished: true,
                    matched_total: 0,
                    matched_success_total: 0,
                    matched_failed_total: 0,
                    completed_reason: "empty",
                },
                false,
            ));
        }
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let progress_state = evaluate_wait_progress(completion_mode, &snapshots);
        let timed_out =
            wait_seconds > 0.0 && elapsed_s >= wait_seconds && !progress_state.completion_reached;
        emit_wait_updates(context, &selector, &snapshots, &mut status_index);
        if emit_progress {
            emit_wait_progress(context, &selector, &snapshots, elapsed_s);
        }
        if progress_state.completion_reached || timed_out || wait_seconds <= 0.0 {
            return Ok(summarize_snapshots(
                &selector,
                snapshots,
                wait_seconds,
                elapsed_s,
                completion_mode,
                progress_state,
                timed_out,
            ));
        }
        sleep(tokio::time::Duration::from_secs_f64(poll_interval)).await;
    }
}

pub(super) fn summarize_snapshots(
    selector: &ResolvedTargetSet,
    snapshots: Vec<SubagentRunSnapshot>,
    wait_seconds: f64,
    elapsed_s: f64,
    completion_mode: WaitCompletionMode,
    progress_state: WaitProgressState,
    timed_out: bool,
) -> Value {
    let total = snapshots.len() as i64;
    let done_total = snapshots.iter().filter(|item| item.terminal).count() as i64;
    let success_total = snapshots
        .iter()
        .filter(|item| item.status == "success")
        .count() as i64;
    let failed_total = snapshots.iter().filter(|item| item.failed).count() as i64;
    let queued_total = snapshots
        .iter()
        .filter(|item| item.status == "queued")
        .count() as i64;
    let running_total = snapshots
        .iter()
        .filter(|item| matches!(item.status.as_str(), "running" | "waiting" | "cancelling"))
        .count() as i64;
    let selected_items = collect_selected_items(completion_mode, &snapshots);
    let status = summarize_wait_status(
        total,
        failed_total,
        timed_out,
        completion_mode,
        progress_state,
    );
    json!({
        "status": status,
        "dispatch_id": selector.dispatch_id.clone(),
        "parent_id": selector.parent_id.clone(),
        "completion_mode": completion_mode.as_str(),
        "completion_reached": progress_state.completion_reached,
        "completed_reason": progress_state.completed_reason,
        "wait_seconds": wait_seconds,
        "elapsed_s": elapsed_s,
        "all_finished": progress_state.all_finished,
        "total": total,
        "done_total": done_total,
        "success_total": success_total,
        "failed_total": failed_total,
        "queued_total": queued_total,
        "running_total": running_total,
        "selected_total": progress_state.matched_total,
        "selected_success_total": progress_state.matched_success_total,
        "selected_failed_total": progress_state.matched_failed_total,
        "run_ids": selector.run_ids.clone(),
        "session_ids": selector.session_ids.clone(),
        "selected_items": selected_items,
        "items": snapshots.into_iter().map(|item| item.payload).collect::<Vec<_>>(),
    })
}

pub(super) fn merge_wait_result(
    wait_result: Value,
    dispatch_id: &str,
    requested_total: i64,
    accepted_total: i64,
    startup_failed_items: Vec<Value>,
) -> Value {
    let mut result = wait_result;
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    let total = object.get("total").and_then(Value::as_i64).unwrap_or(0);
    let current_status = object
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("running")
        .to_string();
    let failed_total = object
        .get("failed_total")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if let Some(items) = object.get_mut("items").and_then(Value::as_array_mut) {
        items.extend(startup_failed_items.clone());
    }
    let merged_failed_total = failed_total + startup_failed_items.len() as i64;
    object.insert("dispatch_id".to_string(), json!(dispatch_id));
    object.insert("requested_total".to_string(), json!(requested_total));
    object.insert("accepted_total".to_string(), json!(accepted_total));
    object.insert(
        "startup_failed_total".to_string(),
        json!(startup_failed_items.len() as i64),
    );
    object.insert(
        "total".to_string(),
        json!(total + startup_failed_items.len() as i64),
    );
    object.insert("failed_total".to_string(), json!(merged_failed_total));
    if current_status == "ok" && !startup_failed_items.is_empty() {
        object.insert("status".to_string(), json!("partial"));
    }
    result
}

pub(super) fn emit_dispatch_start(
    context: &ToolContext<'_>,
    dispatch_id: &str,
    total: i64,
    label: Option<&str>,
    strategy: BatchDispatchStrategy,
    remaining_action: RemainingBranchAction,
) {
    emit_control_event(
        context,
        "subagent_dispatch_start",
        &json!({
            "dispatch_id": dispatch_id,
            "parent_session_id": context.session_id,
            "total": total,
            "label": label,
            "strategy": strategy.as_str(),
            "completion_mode": completion_mode_from_strategy(strategy).as_str(),
            "remaining_action": remaining_action.as_str(),
        }),
    );
}

pub(super) fn emit_control_event(context: &ToolContext<'_>, event_type: &str, payload: &Value) {
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(event_type, payload.clone());
    }
}

pub(super) fn emit_wait_progress(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    snapshots: &[SubagentRunSnapshot],
    elapsed_s: f64,
) {
    emit_control_event(
        context,
        "progress",
        &json!({
            "stage": "subagent_wait",
            "summary": i18n::t("monitor.summary.subagent_wait"),
            "dispatch_id": selector.dispatch_id.clone(),
            "parent_id": selector.parent_id.clone(),
            "total": snapshots.len(),
            "done_total": snapshots.iter().filter(|item| item.terminal).count(),
            "failed_total": snapshots.iter().filter(|item| item.failed).count(),
            "elapsed_s": elapsed_s,
        }),
    );
}

pub(super) fn emit_wait_updates(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    snapshots: &[SubagentRunSnapshot],
    status_index: &mut HashMap<String, String>,
) {
    for snapshot in snapshots {
        if status_index.get(&snapshot.key) == Some(&snapshot.status) {
            continue;
        }
        status_index.insert(snapshot.key.clone(), snapshot.status.clone());
        let mut payload = snapshot.payload.clone();
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "dispatch_id".to_string(),
                json!(selector.dispatch_id.clone()),
            );
        }
        emit_control_event(context, "subagent_dispatch_item_update", &payload);
    }
}

pub(super) fn normalize_poll_interval(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return SUBAGENT_WAIT_DEFAULT_POLL_S;
    }
    value.clamp(SUBAGENT_WAIT_MIN_POLL_S, SUBAGENT_WAIT_MAX_POLL_S)
}

pub(super) fn parse_wait_completion_mode(value: Option<&str>) -> WaitCompletionMode {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "any" | "one" | "first" | "first_terminal" => WaitCompletionMode::Any,
        "first_success" | "success" => WaitCompletionMode::FirstSuccess,
        _ => WaitCompletionMode::All,
    }
}

pub(super) fn parse_batch_dispatch_strategy(value: Option<&str>) -> BatchDispatchStrategy {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "first_success" | "success" => BatchDispatchStrategy::FirstSuccess,
        "review_then_merge" | "merge" | "collect" => BatchDispatchStrategy::ReviewThenMerge,
        _ => BatchDispatchStrategy::ParallelAll,
    }
}

pub(super) fn parse_remaining_branch_action(value: Option<&str>) -> Option<RemainingBranchAction> {
    match value.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "" => None,
        "keep" | "none" => Some(RemainingBranchAction::Keep),
        "interrupt" | "cancel" | "stop" => Some(RemainingBranchAction::Interrupt),
        "close" | "shutdown" => Some(RemainingBranchAction::Close),
        _ => None,
    }
}

pub(super) fn default_remaining_branch_action_for_strategy(
    strategy: BatchDispatchStrategy,
) -> RemainingBranchAction {
    match strategy {
        BatchDispatchStrategy::ParallelAll | BatchDispatchStrategy::ReviewThenMerge => {
            RemainingBranchAction::Keep
        }
        BatchDispatchStrategy::FirstSuccess => RemainingBranchAction::Interrupt,
    }
}

pub(super) fn completion_mode_from_strategy(strategy: BatchDispatchStrategy) -> WaitCompletionMode {
    match strategy {
        BatchDispatchStrategy::ParallelAll | BatchDispatchStrategy::ReviewThenMerge => {
            WaitCompletionMode::All
        }
        BatchDispatchStrategy::FirstSuccess => WaitCompletionMode::FirstSuccess,
    }
}

pub(super) fn evaluate_wait_progress(
    completion_mode: WaitCompletionMode,
    snapshots: &[SubagentRunSnapshot],
) -> WaitProgressState {
    let all_finished = snapshots.iter().all(|item| item.terminal);
    let terminal_total = snapshots.iter().filter(|item| item.terminal).count() as i64;
    let success_total = snapshots
        .iter()
        .filter(|item| item.status == "success")
        .count() as i64;
    let failed_total = snapshots
        .iter()
        .filter(|item| item.terminal && item.failed)
        .count() as i64;
    match completion_mode {
        WaitCompletionMode::All => WaitProgressState {
            completion_reached: all_finished,
            all_finished,
            matched_total: terminal_total,
            matched_success_total: success_total,
            matched_failed_total: failed_total,
            completed_reason: if all_finished {
                "all_finished"
            } else {
                "pending"
            },
        },
        WaitCompletionMode::Any => WaitProgressState {
            completion_reached: terminal_total > 0,
            all_finished,
            matched_total: terminal_total,
            matched_success_total: success_total,
            matched_failed_total: failed_total,
            completed_reason: if terminal_total > 0 {
                "first_terminal"
            } else {
                "pending"
            },
        },
        WaitCompletionMode::FirstSuccess => WaitProgressState {
            completion_reached: success_total > 0 || all_finished,
            all_finished,
            matched_total: if success_total > 0 {
                success_total
            } else {
                terminal_total
            },
            matched_success_total: success_total,
            matched_failed_total: if success_total > 0 { 0 } else { failed_total },
            completed_reason: if success_total > 0 {
                "first_success"
            } else if all_finished {
                "all_finished_without_success"
            } else {
                "pending"
            },
        },
    }
}

pub(super) fn collect_selected_items(
    completion_mode: WaitCompletionMode,
    snapshots: &[SubagentRunSnapshot],
) -> Vec<Value> {
    match completion_mode {
        WaitCompletionMode::All | WaitCompletionMode::Any => snapshots
            .iter()
            .filter(|item| item.terminal)
            .map(|item| item.payload.clone())
            .collect(),
        WaitCompletionMode::FirstSuccess => {
            let selected = snapshots
                .iter()
                .filter(|item| item.status == "success")
                .map(|item| item.payload.clone())
                .collect::<Vec<_>>();
            if selected.is_empty() {
                snapshots
                    .iter()
                    .filter(|item| item.terminal)
                    .map(|item| item.payload.clone())
                    .collect()
            } else {
                selected
            }
        }
    }
}

pub(super) fn summarize_wait_status(
    total: i64,
    failed_total: i64,
    timed_out: bool,
    completion_mode: WaitCompletionMode,
    progress_state: WaitProgressState,
) -> &'static str {
    if total == 0 {
        return "empty";
    }
    if timed_out {
        return "timeout";
    }
    match completion_mode {
        WaitCompletionMode::All => {
            if progress_state.all_finished {
                if failed_total == 0 {
                    "ok"
                } else {
                    "partial"
                }
            } else {
                "running"
            }
        }
        WaitCompletionMode::Any => {
            if !progress_state.completion_reached {
                "running"
            } else if progress_state.matched_success_total > 0
                && progress_state.matched_failed_total == 0
            {
                "ok"
            } else {
                "partial"
            }
        }
        WaitCompletionMode::FirstSuccess => {
            if progress_state.matched_success_total > 0 {
                "ok"
            } else if progress_state.all_finished {
                "partial"
            } else {
                "running"
            }
        }
    }
}

pub(super) fn apply_remaining_settlement(
    context: &ToolContext<'_>,
    result: &mut Value,
    action: RemainingBranchAction,
) {
    let Some(object) = result.as_object_mut() else {
        return;
    };
    object.insert("remaining_action".to_string(), json!(action.as_str()));
    let completed_reason = object
        .get("completed_reason")
        .and_then(Value::as_str)
        .unwrap_or("");
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let pending_items = collect_pending_settlement_items(completed_reason, &items);
    object.insert(
        "remaining_active_total".to_string(),
        json!(pending_items.len() as i64),
    );
    if pending_items.is_empty() || action == RemainingBranchAction::Keep {
        object.insert("remaining_action_applied".to_string(), json!(false));
        object.insert("settled_total".to_string(), json!(0));
        object.insert("settled_items".to_string(), json!(Vec::<Value>::new()));
        return;
    }

    let settled_items = pending_items
        .iter()
        .map(|item| match action {
            RemainingBranchAction::Keep => {
                json!({ "status": "noop", "updated": false, "action": action.as_str() })
            }
            RemainingBranchAction::Interrupt => interrupt_remaining_session(context, item),
            RemainingBranchAction::Close => close_remaining_session(context, item),
        })
        .collect::<Vec<_>>();
    object.insert("remaining_action_applied".to_string(), json!(true));
    object.insert(
        "settled_total".to_string(),
        json!(settled_items.len() as i64),
    );
    object.insert("settled_items".to_string(), json!(settled_items));
}

pub(super) fn collect_pending_settlement_items(
    completed_reason: &str,
    items: &[Value],
) -> Vec<Value> {
    if !matches!(completed_reason, "first_success" | "first_terminal") {
        return Vec::new();
    }
    items
        .iter()
        .filter(|item| {
            !item
                .get("terminal")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter(|item| {
            item.get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
        })
        .cloned()
        .collect()
}

pub(super) fn interrupt_remaining_session(context: &ToolContext<'_>, item: &Value) -> Value {
    let session_id = item
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if session_id.is_empty() {
        return json!({
            "action": RemainingBranchAction::Interrupt.as_str(),
            "status": "error",
            "updated": false,
            "error": "session_id is required",
        });
    }
    let previous_status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let Some(monitor) = context.monitor.as_ref() else {
        return json!({
            "session_id": session_id,
            "action": RemainingBranchAction::Interrupt.as_str(),
            "status": "error",
            "updated": false,
            "previous_status": previous_status,
            "error": "monitor unavailable",
        });
    };
    let updated = monitor.cancel(session_id);
    let payload = json!({
        "session_id": session_id,
        "run_id": item.get("run_id").cloned().unwrap_or(Value::Null),
        "dispatch_id": item.get("dispatch_id").cloned().unwrap_or(Value::Null),
        "action": RemainingBranchAction::Interrupt.as_str(),
        "status": if updated { "cancelling" } else { "unchanged" },
        "updated": updated,
        "previous_status": previous_status,
    });
    emit_control_event(context, "subagent_interrupt", &payload);
    payload
}

pub(super) fn close_remaining_session(context: &ToolContext<'_>, item: &Value) -> Value {
    let session_id = item
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if session_id.is_empty() {
        return json!({
            "action": RemainingBranchAction::Close.as_str(),
            "status": "error",
            "updated": false,
            "error": "session_id is required",
        });
    }
    let previous_status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    match update_session_status(context, session_id, "closed", true) {
        Ok(mut payload) => {
            if let Some(object) = payload.as_object_mut() {
                object.insert(
                    "action".to_string(),
                    json!(RemainingBranchAction::Close.as_str()),
                );
                object.insert("previous_status".to_string(), json!(previous_status));
                object.insert(
                    "run_id".to_string(),
                    item.get("run_id").cloned().unwrap_or(Value::Null),
                );
                object.insert(
                    "dispatch_id".to_string(),
                    item.get("dispatch_id").cloned().unwrap_or(Value::Null),
                );
            }
            emit_control_event(context, "subagent_close", &payload);
            payload
        }
        Err(err) => json!({
            "session_id": session_id,
            "action": RemainingBranchAction::Close.as_str(),
            "status": "error",
            "updated": false,
            "previous_status": previous_status,
            "error": err.to_string(),
        }),
    }
}

pub(super) fn decorate_dispatch_result(
    mut result: Value,
    strategy: BatchDispatchStrategy,
    label: Option<&str>,
    remaining_action: RemainingBranchAction,
) -> Value {
    let Some(object) = result.as_object_mut() else {
        return result;
    };
    object.insert("strategy".to_string(), json!(strategy.as_str()));
    object.insert(
        "completion_mode".to_string(),
        json!(completion_mode_from_strategy(strategy).as_str()),
    );
    object.insert("label".to_string(), json!(label));
    object.insert(
        "remaining_action".to_string(),
        json!(remaining_action.as_str()),
    );
    let selected_items = object
        .get("selected_items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(summary) = build_dispatch_summary(strategy, &items, &selected_items) {
        object.insert("summary".to_string(), json!(summary));
    }
    if strategy == BatchDispatchStrategy::FirstSuccess {
        if let Some(selected_item) = select_preferred_dispatch_item(&selected_items, true)
            .or_else(|| select_preferred_dispatch_item(&items, true))
        {
            object.insert("winner_item".to_string(), selected_item.clone());
            object.insert("selected_item".to_string(), selected_item);
        }
    }
    result
}

pub(super) fn build_dispatch_summary(
    strategy: BatchDispatchStrategy,
    items: &[Value],
    selected_items: &[Value],
) -> Option<String> {
    match strategy {
        BatchDispatchStrategy::ParallelAll => None,
        BatchDispatchStrategy::FirstSuccess => {
            let preferred = select_preferred_dispatch_item(selected_items, true)
                .or_else(|| select_preferred_dispatch_item(items, true))?;
            build_dispatch_item_summary_line(&preferred)
        }
        BatchDispatchStrategy::ReviewThenMerge => {
            let lines = items
                .iter()
                .filter_map(build_dispatch_item_summary_line)
                .collect::<Vec<_>>();
            if lines.is_empty() {
                None
            } else {
                Some(lines.join("\n"))
            }
        }
    }
}

pub(super) fn select_preferred_dispatch_item(
    items: &[Value],
    prefer_success: bool,
) -> Option<Value> {
    let preferred = items
        .iter()
        .filter(|item| {
            !prefer_success || item.get("status").and_then(Value::as_str) == Some("success")
        })
        .min_by(|left, right| dispatch_item_sort_key(left).cmp(&dispatch_item_sort_key(right)))
        .cloned();
    if preferred.is_some() || prefer_success {
        preferred
    } else {
        items
            .iter()
            .min_by(|left, right| dispatch_item_sort_key(left).cmp(&dispatch_item_sort_key(right)))
            .cloned()
    }
}

pub(super) fn dispatch_item_sort_key(item: &Value) -> (i64, String) {
    let index = item
        .get("index")
        .and_then(Value::as_i64)
        .unwrap_or(i64::MAX);
    let key = item
        .get("run_id")
        .or_else(|| item.get("session_id"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    (index, key)
}

pub(super) fn build_dispatch_item_summary_line(item: &Value) -> Option<String> {
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let label = item
        .get("label")
        .or_else(|| item.get("spawn_label"))
        .or_else(|| item.get("title"))
        .or_else(|| item.get("session_id"))
        .or_else(|| item.get("run_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("subagent");
    let detail = item
        .get("agent_state")
        .and_then(Value::as_object)
        .and_then(|state| state.get("message"))
        .and_then(Value::as_str)
        .or_else(|| item.get("result").and_then(Value::as_str))
        .or_else(|| item.get("error").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_text(value, SUBAGENT_SUMMARY_MAX_CHARS))
        .unwrap_or_else(|| status.to_string());
    Some(format!("[{label}][{status}] {detail}"))
}
