use super::*;
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone)]
pub(super) struct ResolvedTargetSet {
    pub(super) run_ids: Vec<String>,
    pub(super) session_ids: Vec<String>,
    pub(super) dispatch_id: Option<String>,
    pub(super) parent_id: Option<String>,
    pub(super) limit: i64,
}

#[derive(Debug, Clone)]
pub(super) struct SubagentRunSnapshot {
    pub(super) key: String,
    pub(super) status: String,
    pub(super) terminal: bool,
    pub(super) failed: bool,
    pub(super) updated_time: f64,
    pub(super) payload: Value,
}

pub(super) fn resolve_subagent_parent_scope(
    explicit_parent_id: Option<String>,
    current_session_id: &str,
) -> Result<String> {
    normalize_optional_string(explicit_parent_id)
        .or_else(|| normalize_optional_string(Some(current_session_id.to_string())))
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))
}

pub(super) fn normalize_status_wait_target(
    context: &ToolContext<'_>,
    target: &SubagentTargetArgs,
    action: &str,
) -> Result<SubagentTargetArgs> {
    if should_autocorrect_single_child_session_target(target) {
        let resolved_session_id = resolve_single_child_session_target(context, target, action)?;
        return Ok(SubagentTargetArgs {
            session_ids: None,
            session_id: Some(resolved_session_id),
            run_ids: None,
            run_id: None,
            ..target.clone()
        });
    }
    if should_autocorrect_single_child_run_target(target) {
        let resolved_run_id = resolve_single_child_run_target(context, target, action)?;
        return Ok(SubagentTargetArgs {
            run_ids: None,
            run_id: Some(resolved_run_id),
            session_ids: None,
            session_id: None,
            ..target.clone()
        });
    }
    Ok(target.clone())
}

pub(super) fn should_autocorrect_single_child_session_target(target: &SubagentTargetArgs) -> bool {
    let has_run_selector =
        !target.run_ids.as_ref().is_none_or(Vec::is_empty) || target.run_id.is_some();
    let has_scope_selector = normalize_optional_string(target.dispatch_id.clone()).is_some()
        || normalize_optional_string(target.parent_id.clone()).is_some();
    if has_run_selector || has_scope_selector {
        return false;
    }
    let mut requested_session_ids = target.session_ids.clone().unwrap_or_default();
    if let Some(session_id) = target.session_id.clone() {
        requested_session_ids.push(session_id);
    }
    dedupe_non_empty_strings(requested_session_ids).len() == 1
}

pub(super) fn should_autocorrect_single_child_run_target(target: &SubagentTargetArgs) -> bool {
    let has_session_selector =
        !target.session_ids.as_ref().is_none_or(Vec::is_empty) || target.session_id.is_some();
    let has_scope_selector = normalize_optional_string(target.dispatch_id.clone()).is_some()
        || normalize_optional_string(target.parent_id.clone()).is_some();
    if has_session_selector || has_scope_selector {
        return false;
    }
    let mut requested_run_ids = target.run_ids.clone().unwrap_or_default();
    if let Some(run_id) = target.run_id.clone() {
        requested_run_ids.push(run_id);
    }
    dedupe_non_empty_strings(requested_run_ids).len() == 1
}

pub(super) fn resolve_single_child_session_target(
    context: &ToolContext<'_>,
    target: &SubagentTargetArgs,
    action: &str,
) -> Result<String> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let current_session_id = context.session_id.trim();
    if current_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let mut resolved_session_ids = Vec::new();
    let mut requested_session_ids = target.session_ids.clone().unwrap_or_default();
    if let Some(session_id) = target.session_id.clone() {
        requested_session_ids.push(session_id);
    }
    let requested_session_ids = dedupe_non_empty_strings(requested_session_ids);
    let first_requested_session_id = requested_session_ids.first().cloned();
    for requested_session_id in requested_session_ids {
        match resolve_direct_child_session_id(
            context,
            user_id,
            current_session_id,
            &requested_session_id,
            action,
        )? {
            Some(session_id) => resolved_session_ids.push(session_id),
            None => {
                return Err(build_child_session_target_error(
                    action,
                    Some(&requested_session_id),
                ));
            }
        }
    }

    let should_resolve_selector = !target.run_ids.as_ref().is_none_or(Vec::is_empty)
        || target.run_id.as_ref().is_some()
        || normalize_optional_string(target.dispatch_id.clone()).is_some()
        || normalize_optional_string(target.parent_id.clone()).is_some();
    if should_resolve_selector {
        let selector = resolve_targets(target, None)?;
        for snapshot in collect_snapshots(context, &selector)? {
            if let Some(session_id) = snapshot
                .payload
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                let session_id = ensure_direct_child_session_id(
                    context,
                    user_id,
                    current_session_id,
                    &session_id,
                    action,
                )?;
                resolved_session_ids.push(session_id);
            }
        }
    }

    let resolved_session_ids = dedupe_non_empty_strings(resolved_session_ids);
    match resolved_session_ids.as_slice() {
        [session_id] => Ok(session_id.clone()),
        [] => {
            if let Some(session_id) =
                find_single_direct_child_session_id(context, user_id, current_session_id)?
            {
                return Ok(session_id);
            }
            Err(build_child_session_target_error(
                action,
                first_requested_session_id.as_deref(),
            ))
        }
        _ => Err(anyhow!(
            "subagent_control {action} requires exactly one child session target; use the exact session_id returned by spawn or a single runId"
        )),
    }
}

pub(super) fn resolve_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_session_id: &str,
    action: &str,
) -> Result<Option<String>> {
    let requested_session_id = requested_session_id.trim();
    if requested_session_id.is_empty() {
        return Ok(None);
    }
    if let Some(record) = context
        .storage
        .get_chat_session(user_id, requested_session_id)?
    {
        if !is_direct_child_session(record.parent_session_id.as_deref(), current_session_id) {
            return Err(anyhow!(
                "subagent_control {action} requires a direct child session of the current session"
            ));
        }
        return Ok(Some(record.session_id));
    }
    let similar =
        find_similar_child_session_id(context, user_id, current_session_id, requested_session_id)?;
    if similar.is_some() {
        return Ok(similar);
    }
    find_single_direct_child_session_id(context, user_id, current_session_id)
}

pub(super) fn resolve_single_child_run_target(
    context: &ToolContext<'_>,
    target: &SubagentTargetArgs,
    action: &str,
) -> Result<String> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let current_session_id = context.session_id.trim();
    if current_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }

    let mut requested_run_ids = target.run_ids.clone().unwrap_or_default();
    if let Some(run_id) = target.run_id.clone() {
        requested_run_ids.push(run_id);
    }
    let requested_run_ids = dedupe_non_empty_strings(requested_run_ids);
    let first_requested_run_id = requested_run_ids.first().cloned();
    let mut resolved_run_ids = Vec::new();
    for requested_run_id in requested_run_ids {
        match resolve_direct_child_run_id(
            context,
            user_id,
            current_session_id,
            &requested_run_id,
            action,
        )? {
            Some(run_id) => resolved_run_ids.push(run_id),
            None => {
                return Err(build_child_run_target_error(
                    action,
                    Some(&requested_run_id),
                ));
            }
        }
    }

    let resolved_run_ids = dedupe_non_empty_strings(resolved_run_ids);
    match resolved_run_ids.as_slice() {
        [run_id] => Ok(run_id.clone()),
        [] => {
            if let Some(run_id) = find_single_direct_child_run_id(context, user_id, current_session_id)? {
                return Ok(run_id);
            }
            Err(build_child_run_target_error(
                action,
                first_requested_run_id.as_deref(),
            ))
        }
        _ => Err(anyhow!(
            "subagent_control {action} requires exactly one child run target; use the exact run_id returned by spawn/list"
        )),
    }
}

pub(super) fn resolve_direct_child_run_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_run_id: &str,
    action: &str,
) -> Result<Option<String>> {
    let requested_run_id = requested_run_id.trim();
    if requested_run_id.is_empty() {
        return Ok(None);
    }
    if let Some(record) = context.storage.get_session_run(requested_run_id)? {
        if !is_direct_child_session(record.parent_session_id.as_deref(), current_session_id) {
            return Err(anyhow!(
                "subagent_control {action} requires a direct child run of the current session"
            ));
        }
        if record.user_id.trim() != user_id {
            return Err(build_child_run_target_error(action, Some(requested_run_id)));
        }
        return Ok(Some(record.run_id));
    }
    let similar =
        find_similar_direct_child_run_id(context, user_id, current_session_id, requested_run_id)?;
    if similar.is_some() {
        return Ok(similar);
    }
    find_single_direct_child_run_id(context, user_id, current_session_id)
}

pub(super) fn ensure_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    session_id: &str,
    action: &str,
) -> Result<String> {
    let Some(record) = context.storage.get_chat_session(user_id, session_id)? else {
        return Err(build_child_session_target_error(action, Some(session_id)));
    };
    if !is_direct_child_session(record.parent_session_id.as_deref(), current_session_id) {
        return Err(anyhow!(
            "subagent_control {action} requires a direct child session of the current session"
        ));
    }
    Ok(record.session_id)
}

pub(super) fn build_child_session_target_error(
    action: &str,
    requested_session_id: Option<&str>,
) -> anyhow::Error {
    let mut message = format!(
        "subagent_control {action} target not found under the current session; use the exact session_id returned by spawn or a runId returned by spawn/list"
    );
    if let Some(requested_session_id) = requested_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        message.push_str(&format!(" (requested: {requested_session_id})"));
    }
    anyhow!(message)
}

pub(super) fn build_child_run_target_error(
    action: &str,
    requested_run_id: Option<&str>,
) -> anyhow::Error {
    let mut message = format!(
        "subagent_control {action} target not found under the current session; use the exact run_id returned by spawn/list"
    );
    if let Some(requested_run_id) = requested_run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        message.push_str(&format!(" (requested: {requested_run_id})"));
    }
    anyhow!(message)
}

pub(super) fn find_single_direct_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
) -> Result<Option<String>> {
    let (sessions, _) = context.storage.list_chat_sessions(
        user_id,
        None,
        Some(current_session_id),
        0,
        MAX_SESSION_LIST_ITEMS,
    )?;
    Ok(select_single_direct_child_session_id(
        sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect(),
    ))
}

pub(super) fn find_single_direct_child_run_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
) -> Result<Option<String>> {
    let runs = context.storage.list_session_runs_by_parent(
        user_id,
        current_session_id,
        MAX_SESSION_LIST_ITEMS,
    )?;
    Ok(select_single_direct_child_run_id(
        runs.into_iter().map(|record| record.run_id).collect(),
    ))
}

pub(super) fn select_single_direct_child_session_id(session_ids: Vec<String>) -> Option<String> {
    let session_ids = dedupe_non_empty_strings(session_ids);
    match session_ids.as_slice() {
        [session_id] => Some(session_id.clone()),
        _ => None,
    }
}

pub(super) fn select_single_direct_child_run_id(run_ids: Vec<String>) -> Option<String> {
    let run_ids = dedupe_non_empty_strings(run_ids);
    match run_ids.as_slice() {
        [run_id] => Some(run_id.clone()),
        _ => None,
    }
}

pub(super) fn find_similar_child_session_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_session_id: &str,
) -> Result<Option<String>> {
    let requested_session_id = requested_session_id.trim();
    if requested_session_id.is_empty() {
        return Ok(None);
    }
    let (sessions, _) = context.storage.list_chat_sessions(
        user_id,
        None,
        Some(current_session_id),
        0,
        MAX_SESSION_LIST_ITEMS,
    )?;
    let mut best_distance: Option<usize> = None;
    let mut matches = Vec::new();
    for session in sessions {
        let candidate = session.session_id.trim();
        let Some(distance) = bounded_edit_distance(requested_session_id, candidate, 2) else {
            continue;
        };
        match best_distance {
            None => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(session.session_id);
            }
            Some(current_best) if distance < current_best => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(session.session_id);
            }
            Some(current_best) if distance == current_best => {
                matches.push(session.session_id);
            }
            Some(_) => {}
        }
    }
    Ok(if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    })
}

pub(super) fn find_similar_direct_child_run_id(
    context: &ToolContext<'_>,
    user_id: &str,
    current_session_id: &str,
    requested_run_id: &str,
) -> Result<Option<String>> {
    let requested_run_id = requested_run_id.trim();
    if requested_run_id.is_empty() {
        return Ok(None);
    }
    let runs = context.storage.list_session_runs_by_parent(
        user_id,
        current_session_id,
        MAX_SESSION_LIST_ITEMS,
    )?;
    let mut best_distance: Option<usize> = None;
    let mut matches = Vec::new();
    for run in runs {
        let candidate = run.run_id.trim();
        let Some(distance) = bounded_edit_distance(requested_run_id, candidate, 2) else {
            continue;
        };
        match best_distance {
            None => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(run.run_id);
            }
            Some(current_best) if distance < current_best => {
                best_distance = Some(distance);
                matches.clear();
                matches.push(run.run_id);
            }
            Some(current_best) if distance == current_best => {
                matches.push(run.run_id);
            }
            Some(_) => {}
        }
    }
    Ok(if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    })
}

pub(super) fn bounded_edit_distance(left: &str, right: &str, max_distance: usize) -> Option<usize> {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let left_len = left_chars.len();
    let right_len = right_chars.len();
    if left_len.abs_diff(right_len) > max_distance {
        return None;
    }
    let mut previous = (0..=right_len).collect::<Vec<_>>();
    for (left_index, left_char) in left_chars.iter().enumerate() {
        let mut current = vec![left_index + 1; right_len + 1];
        let mut row_min = current[0];
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution_cost = usize::from(left_char != right_char);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            let substitution = previous[right_index] + substitution_cost;
            let value = insertion.min(deletion).min(substitution);
            current[right_index + 1] = value;
            row_min = row_min.min(value);
        }
        if row_min > max_distance {
            return None;
        }
        previous = current;
    }
    let distance = previous[right_len];
    (distance <= max_distance).then_some(distance)
}

pub(super) fn is_direct_child_session(
    target_parent_session_id: Option<&str>,
    current_session_id: &str,
) -> bool {
    let current_session_id = current_session_id.trim();
    if current_session_id.is_empty() {
        return false;
    }
    target_parent_session_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| value == current_session_id)
}

pub(super) fn resolve_targets(
    payload: &SubagentTargetArgs,
    default_parent_session_id: Option<&str>,
) -> Result<ResolvedTargetSet> {
    let mut run_ids = payload.run_ids.clone().unwrap_or_default();
    if let Some(run_id) = payload.run_id.clone() {
        run_ids.push(run_id);
    }
    let mut session_ids = payload.session_ids.clone().unwrap_or_default();
    if let Some(session_id) = payload.session_id.clone() {
        session_ids.push(session_id);
    }
    let dispatch_id = normalize_optional_string(payload.dispatch_id.clone());
    let run_ids = dedupe_non_empty_strings(run_ids);
    let session_ids = dedupe_non_empty_strings(session_ids);
    let mut parent_id = normalize_optional_string(payload.parent_id.clone());
    if run_ids.is_empty() && session_ids.is_empty() && dispatch_id.is_none() && parent_id.is_none()
    {
        parent_id = default_parent_session_id
            .and_then(|value| normalize_optional_string(Some(value.to_string())));
    }
    if run_ids.is_empty() && session_ids.is_empty() && dispatch_id.is_none() && parent_id.is_none()
    {
        return Err(anyhow!("subagent target is required"));
    }
    Ok(ResolvedTargetSet {
        run_ids,
        session_ids,
        dispatch_id,
        parent_id,
        limit: clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS),
    })
}

pub(super) fn collect_snapshots(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
) -> Result<Vec<SubagentRunSnapshot>> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let mut run_ids = selector.run_ids.clone();
    if let Some(dispatch_id) = selector.dispatch_id.as_deref() {
        let records =
            context
                .storage
                .list_session_runs_by_dispatch(user_id, dispatch_id, selector.limit)?;
        run_ids.extend(records.into_iter().map(|record| record.run_id));
    }
    if let Some(parent_id) = selector.parent_id.as_deref() {
        let records =
            context
                .storage
                .list_session_runs_by_parent(user_id, parent_id, selector.limit)?;
        let mut seen_sessions = HashSet::new();
        for record in records {
            if seen_sessions.insert(record.session_id.clone()) {
                run_ids.push(record.run_id);
            }
        }
    }
    let run_ids = dedupe_non_empty_strings(run_ids);
    let mut snapshots = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut seen_session_ids = HashSet::new();
    for run_id in run_ids {
        let snapshot = build_run_snapshot(context, &run_id)?;
        if let Some(session_id) = snapshot
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            seen_session_ids.insert(session_id);
        }
        if seen_keys.insert(snapshot.key.clone()) {
            snapshots.push(snapshot);
        }
    }
    for session_id in &selector.session_ids {
        if seen_session_ids.contains(session_id) {
            continue;
        }
        let snapshot = build_session_snapshot(context, session_id)?;
        if seen_keys.insert(snapshot.key.clone()) {
            snapshots.push(snapshot);
        }
    }
    snapshots.sort_by(|left, right| {
        right
            .updated_time
            .partial_cmp(&left.updated_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(snapshots)
}

pub(super) fn build_run_snapshot(
    context: &ToolContext<'_>,
    run_id: &str,
) -> Result<SubagentRunSnapshot> {
    let user_id = context.user_id.trim();
    if let Some(record) = context.storage.get_session_run(run_id)? {
        let session = context
            .storage
            .get_chat_session(user_id, &record.session_id)
            .ok()
            .flatten();
        let runtime_status = runtime_status(context, &record.session_id);
        let session_status = session
            .as_ref()
            .map(|entry| normalize_session_status(&entry.status))
            .unwrap_or_else(|| "missing".to_string());
        let status =
            resolve_effective_status(&record.status, runtime_status.as_deref(), &session_status);
        let terminal = is_terminal_status(&status);
        let failed = is_failed_status(&status);
        let message =
            run_message_for_status(&status, record.result.as_deref(), record.error.as_deref());
        let metadata = record.metadata.clone();
        let (parent_turn_ref, parent_user_round, parent_model_round) =
            crate::services::subagents::parent_turn_payload(
                metadata.as_ref(),
                session
                    .as_ref()
                    .and_then(|entry| entry.parent_message_id.as_deref()),
            );
        let mut payload = json!({
            "run_id": record.run_id,
            "dispatch_id": record.dispatch_id,
            "run_kind": record.run_kind,
            "requested_by": record.requested_by,
            "status": status,
            "run_status": record.status,
            "runtime_status": runtime_status,
            "session_status": session_status,
            "terminal": terminal,
            "failed": failed,
            "session_id": record.session_id,
            "parent_session_id": record.parent_session_id,
            "agent_id": record.agent_id,
            "model_name": record.model_name,
            "queued_time": record.queued_time,
            "started_time": record.started_time,
            "finished_time": record.finished_time,
            "elapsed_s": record.elapsed_s,
            "result": record.result,
            "error": record.error,
            "agent_state": {
                "status": collab_agent_status(&status),
                "message": message,
            },
            "updated_time": record.updated_time,
            "title": session.as_ref().map(|entry| entry.title.clone()),
            "spawn_label": session.as_ref().and_then(|entry| entry.spawn_label.clone()),
            "spawned_by": session.as_ref().and_then(|entry| entry.spawned_by.clone()),
        });
        if let Some(object) = payload.as_object_mut() {
            object.insert(
                "metadata".to_string(),
                metadata.clone().unwrap_or(Value::Null),
            );
            object.insert(
                "controller_session_id".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "controller_session_id",
                ),
            );
            object.insert(
                "depth".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "depth"),
            );
            object.insert(
                "role".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "role"),
            );
            object.insert(
                "control_scope".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "control_scope"),
            );
            object.insert(
                "spawn_mode".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "spawn_mode"),
            );
            object.insert(
                "strategy".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "strategy"),
            );
            object.insert(
                "completion_mode".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "completion_mode",
                ),
            );
            object.insert(
                "remaining_action".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "remaining_action",
                ),
            );
            object.insert(
                "dispatch_label".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_label"),
            );
            object.insert(
                "dispatch_index".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_index"),
            );
            object.insert(
                "dispatch_size".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "dispatch_size"),
            );
            object.insert(
                "cleanup".to_string(),
                crate::services::subagents::run_metadata_field(metadata.as_ref(), "cleanup"),
            );
            object.insert(
                "run_timeout_seconds".to_string(),
                crate::services::subagents::run_metadata_field(
                    metadata.as_ref(),
                    "run_timeout_seconds",
                ),
            );
            object.insert("parent_turn_ref".to_string(), parent_turn_ref);
            object.insert("parent_user_round".to_string(), parent_user_round);
            object.insert("parent_model_round".to_string(), parent_model_round);
        }
        Ok(SubagentRunSnapshot {
            key: record.run_id.clone(),
            status: status.clone(),
            terminal,
            failed,
            updated_time: record.updated_time,
            payload,
        })
    } else {
        Ok(SubagentRunSnapshot {
            key: run_id.trim().to_string(),
            status: "not_found".to_string(),
            terminal: true,
            failed: true,
            updated_time: 0.0,
            payload: json!({
                "run_id": run_id,
                "status": "not_found",
                "terminal": true,
                "failed": true,
                "agent_state": {
                    "status": "not_found",
                    "message": "run not found",
                },
                "error": "run not found",
            }),
        })
    }
}

pub(super) fn build_session_snapshot(
    context: &ToolContext<'_>,
    session_id: &str,
) -> Result<SubagentRunSnapshot> {
    let user_id = context.user_id.trim();
    let Some(session) = context.storage.get_chat_session(user_id, session_id)? else {
        return Ok(SubagentRunSnapshot {
            key: session_id.trim().to_string(),
            status: "not_found".to_string(),
            terminal: true,
            failed: true,
            updated_time: 0.0,
            payload: json!({
                "session_id": session_id,
                "status": "not_found",
                "terminal": true,
                "failed": true,
                "error": "session not found",
            }),
        });
    };
    if let Some(record) = context
        .storage
        .list_session_runs_by_session(user_id, &session.session_id, 1)?
        .into_iter()
        .next()
    {
        return build_run_snapshot(context, &record.run_id);
    }
    let session_status = normalize_session_status(&session.status);
    let runtime_status = runtime_status(context, &session.session_id);
    let status = resolve_effective_status("", runtime_status.as_deref(), &session_status);
    let terminal = is_terminal_status(&status);
    let failed = is_failed_status(&status);
    let (parent_turn_ref, parent_user_round, parent_model_round) =
        crate::services::subagents::parent_turn_payload(None, session.parent_message_id.as_deref());
    let mut payload = json!({
        "status": status,
        "runtime_status": runtime_status,
        "session_status": session_status,
        "terminal": terminal,
        "failed": failed,
        "agent_state": {
            "status": collab_agent_status(&status),
            "message": serde_json::Value::Null,
        },
        "session_id": session.session_id,
        "parent_session_id": session.parent_session_id,
        "agent_id": session.agent_id,
        "title": session.title,
        "spawn_label": session.spawn_label,
        "spawned_by": session.spawned_by,
        "updated_time": session.updated_at,
    });
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "controller_session_id".to_string(),
            session
                .parent_session_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        object.insert("metadata".to_string(), Value::Null);
        object.insert("depth".to_string(), Value::Null);
        object.insert("role".to_string(), Value::Null);
        object.insert("control_scope".to_string(), Value::Null);
        object.insert("spawn_mode".to_string(), Value::Null);
        object.insert("strategy".to_string(), Value::Null);
        object.insert("completion_mode".to_string(), Value::Null);
        object.insert("remaining_action".to_string(), Value::Null);
        object.insert("dispatch_label".to_string(), Value::Null);
        object.insert("dispatch_index".to_string(), Value::Null);
        object.insert("dispatch_size".to_string(), Value::Null);
        object.insert("cleanup".to_string(), Value::Null);
        object.insert("run_timeout_seconds".to_string(), Value::Null);
        object.insert("parent_turn_ref".to_string(), parent_turn_ref);
        object.insert("parent_user_round".to_string(), parent_user_round);
        object.insert("parent_model_round".to_string(), parent_model_round);
    }
    Ok(SubagentRunSnapshot {
        key: session.session_id.clone(),
        status: status.clone(),
        terminal,
        failed,
        updated_time: session.updated_at,
        payload,
    })
}

pub(super) fn collect_target_session_ids(
    context: &ToolContext<'_>,
    selector: &ResolvedTargetSet,
    cascade: bool,
) -> Result<Vec<String>> {
    let mut session_ids = selector.session_ids.clone();
    for snapshot in collect_snapshots(context, selector)? {
        if let Some(session_id) = snapshot
            .payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            session_ids.push(session_id);
        }
    }
    let mut session_ids = dedupe_non_empty_strings(session_ids);
    if cascade {
        let descendants = collect_descendant_session_ids(context, &session_ids, selector.limit)?;
        session_ids.extend(descendants);
        session_ids = dedupe_non_empty_strings(session_ids);
    }
    Ok(session_ids)
}

pub(super) fn collect_descendant_session_ids(
    context: &ToolContext<'_>,
    root_session_ids: &[String],
    limit: i64,
) -> Result<Vec<String>> {
    let user_id = context.user_id.trim();
    let mut queue: VecDeque<String> = root_session_ids.iter().cloned().collect();
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    while let Some(parent_session_id) = queue.pop_front() {
        let (children, _) = context.storage.list_chat_sessions_by_status(
            user_id,
            None,
            Some(&parent_session_id),
            Some("all"),
            0,
            limit,
        )?;
        for child in children {
            if seen.insert(child.session_id.clone()) {
                queue.push_back(child.session_id.clone());
                output.push(child.session_id);
                if output.len() >= limit as usize {
                    return Ok(output);
                }
            }
        }
    }
    Ok(output)
}

pub(super) fn update_session_status(
    context: &ToolContext<'_>,
    session_id: &str,
    next_status: &str,
    cancel_running: bool,
) -> Result<Value> {
    let user_id = context.user_id.trim();
    let Some(mut record) = context.storage.get_chat_session(user_id, session_id)? else {
        return Ok(json!({ "session_id": session_id, "status": "not_found", "updated": false }));
    };
    let updated = record.status.trim() != next_status;
    if cancel_running {
        if let Some(monitor) = context.monitor.as_ref() {
            let _ = monitor.cancel(session_id);
        }
    }
    if updated {
        record.status = next_status.to_string();
        record.updated_at = now_ts();
        context.storage.upsert_chat_session(&record)?;
    }
    Ok(json!({
        "session_id": session_id,
        "status": next_status,
        "updated": updated,
        "title": record.title,
        "parent_session_id": record.parent_session_id,
    }))
}

pub(super) fn runtime_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|entry| {
            entry
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.trim().to_ascii_lowercase())
        })
        .filter(|value| !value.is_empty())
}

pub(super) fn normalize_session_status(status: &str) -> String {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        "active".to_string()
    } else {
        normalized
    }
}

pub(super) fn resolve_effective_status(
    run_status: &str,
    runtime_status: Option<&str>,
    session_status: &str,
) -> String {
    let run_status = run_status.trim().to_ascii_lowercase();
    if is_terminal_status(&run_status) {
        return run_status;
    }
    if session_status == "closed" {
        return "closed".to_string();
    }
    if let Some(runtime_status) = runtime_status {
        let runtime_status = runtime_status.trim().to_ascii_lowercase();
        if !runtime_status.is_empty() {
            return runtime_status;
        }
    }
    if run_status.is_empty() {
        if session_status == "active" {
            "idle".to_string()
        } else {
            session_status.to_string()
        }
    } else {
        run_status
    }
}

pub(super) fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "closed" | "idle" | "not_found"
    )
}

pub(super) fn is_failed_status(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "closed" | "not_found"
    )
}

pub(super) fn collab_agent_status(status: &str) -> &'static str {
    match status.trim().to_ascii_lowercase().as_str() {
        "queued" | "accepted" | "active" => "pending_init",
        "running" | "waiting" => "running",
        "cancelling" | "cancelled" => "interrupted",
        "success" | "idle" => "completed",
        "error" | "timeout" | "failed" => "errored",
        "closed" => "shutdown",
        "not_found" => "not_found",
        _ => "running",
    }
}

pub(super) fn run_message_for_status(
    status: &str,
    result: Option<&str>,
    error: Option<&str>,
) -> Option<String> {
    let source = if status == "success" {
        result
    } else if is_failed_status(status) {
        error
    } else {
        None
    }?;
    let text = source.trim();
    if text.is_empty() {
        None
    } else {
        Some(truncate_text(text, SUBAGENT_SUMMARY_MAX_CHARS))
    }
}
