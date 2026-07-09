use super::*;

#[derive(Debug, Default, Clone)]
struct AgentSwarmRuntime {
    lock_sessions: HashSet<String>,
    running_sessions: HashSet<String>,
}

#[derive(Debug, Clone)]
struct SwarmBatchDispatchTask {
    index: usize,
    agent_id: String,
    agent_name: String,
    session_id: String,
    created_session: bool,
    thread_strategy: &'static str,
    team_task_id: String,
    message: String,
    label: Option<String>,
    tool_names: Vec<String>,
    model_name: Option<String>,
    agent_prompt: Option<String>,
    preview_skill: bool,
}

fn resolve_swarm_batch_tool_names(
    context: &ToolContext<'_>,
    config: &Config,
    skills: &SkillRegistry,
    allowed_tools: &HashSet<String>,
    user_id: &str,
    session: &ChatSessionRecord,
    agent: &UserAgentRecord,
) -> Vec<String> {
    let frozen_tool_overrides = context
        .workspace
        .load_session_frozen_tool_overrides(user_id, &session.session_id);
    let overrides =
        resolve_session_tool_overrides(session, frozen_tool_overrides.as_deref(), Some(agent));
    let filtered = apply_tool_overrides(allowed_tools.clone(), &overrides, config, skills);
    finalize_tool_names(filtered)
}

async fn dispatch_swarm_batch_task(
    context: &ToolContext<'_>,
    task: SwarmBatchDispatchTask,
) -> Result<Value> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let model_name = task.model_name.clone();
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: task.message,
        client_message_id: None,
        tool_names: task.tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(task.session_id.clone()),
        agent_id: Some(task.agent_id.clone()),
        workspace_container_id: None,
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt: task.agent_prompt,
        preview_skill: task.preview_skill,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        enforce_runtime_queue: false,
        approval_tx: None,
    };

    // Swarm tasks are reported via team_* realtime/timeline events.
    // Also emit subagent_dispatch_item_update so the frontend sub-agent panel refreshes.
    // Emit subagent_dispatch_item_update so the frontend sub-agent panel refreshes
    // without polluting the queen bee's chat transcript.
    let _task_label = task.label.as_deref();
    let announce = Some(AnnounceConfig {
        parent_session_id: context.session_id.to_string(),
        label: task.label.clone(),
        dispatch_id: None,
        strategy: None,
        completion_mode: None,
        remaining_action: None,
        parent_turn_ref: None,
        parent_user_round: None,
        parent_model_round: None,
        emit_parent_events: true,
        auto_wake: false,
        persist_history_message: false,
    });

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let _receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(context.session_id.to_string()),
        Some(task.agent_id.clone()),
        model_name,
        SessionRunMeta {
            run_kind: Some("swarm".to_string()),
            requested_by: Some("agent_swarm".to_string()),
            team_task_id: Some(task.team_task_id.clone()),
            ..SessionRunMeta::default()
        },
        announce,
        SessionCleanup::Keep,
        None,
    )
    .await?;

    Ok(build_model_tool_success(
        "spawn",
        "accepted",
        format!("Spawned swarm task {}.", task.team_task_id),
        json!({
            "task_id": task.team_task_id,
            "run_id": run_id,
            "session_id": task.session_id,
            "agent_id": task.agent_id,
            "agent_name": task.agent_name,
            "created_session": task.created_session,
            "thread_strategy": task.thread_strategy,
        }),
    ))
}

#[allow(dead_code)]
fn build_swarm_timeout_monitoring_payload(
    run_id: Option<&str>,
    team_run_id: &str,
    target_session_id: &str,
) -> Value {
    let run_id = run_id.map(str::trim).filter(|value| !value.is_empty());
    let mut suggestions = Vec::new();
    if let Some(run_id) = run_id {
        suggestions.push(json!({
            "purpose": "continue waiting for 60 seconds",
            "args": {
                "action": "wait",
                "run_ids": [run_id],
                "wait_seconds": 60
            }
        }));
        suggestions.push(json!({
            "purpose": "inspect current snapshot",
            "args": {
                "action": "wait",
                "run_ids": [run_id],
                "wait_seconds": 0
            }
        }));
    }
    let session_key = target_session_id.trim();
    if !session_key.is_empty() {
        suggestions.push(json!({
            "purpose": "inspect worker session history",
            "args": {
                "action": "history",
                "session_id": session_key
            }
        }));
    }
    json!({
        "note": "This wait timed out; the worker may still be running.",
        "team_run_id": team_run_id,
        "run_id": run_id,
        "session_id": if session_key.is_empty() { Value::Null } else { json!(session_key) },
        "suggested_calls": suggestions
    })
}

#[allow(dead_code)]
fn build_swarm_wait_monitoring_payload(run_ids: &[String]) -> Value {
    let run_ids = dedupe_non_empty_strings(run_ids.to_vec());
    json!({
        "note": "Some tasks are not finished; continue waiting or inspect a snapshot.",
        "run_ids": run_ids,
        "suggested_calls": [
            {
                "purpose": "continue waiting for 60 seconds",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 60
                }
            },
            {
                "purpose": "inspect current snapshot",
                "args": {
                    "action": "wait",
                    "run_ids": run_ids,
                    "wait_seconds": 0
                }
            }
        ]
    })
}

pub(crate) async fn agent_swarm(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmControlArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "unknown",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm arguments are invalid: {err}"),
                "Provide action and use one of list/status/send/history/spawn/batch_send/wait.",
                &["action"],
                json!({ "action": "list" }),
                args,
                json!({
                    "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
                }),
            ));
        }
    };
    let action = payload.action.trim();
    if action.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "unknown",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm action cannot be empty",
            "Provide action and use one of list/status/send/history/spawn/batch_send/wait.",
            &["action"],
            json!({ "action": "list" }),
            args,
            json!({
                "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
            }),
        ));
    }
    let action_lower = action.to_lowercase();
    match action_lower.as_str() {
        "list" | "agents_list" | "agent_list" | "swarm_list" => {
            agent_swarm_list(context, args).await
        }
        "status" | "agent_status" | "agents_status" | "swarm_status" => {
            agent_swarm_status(context, args).await
        }
        "send" | "agent_send" | "agents_send" | "swarm_send" => {
            agent_swarm_send(context, args).await
        }
        "batch_send" | "swarm_batch_send" | "agents_batch_send" | "batch" | "fanout"
        | "dispatch" => agent_swarm_batch_send(context, args).await,
        "wait" | "join" | "collect" | "swarm_wait" => agent_swarm_wait(context, args).await,
        "history" | "agent_history" | "agents_history" | "swarm_history" => {
            agent_swarm_history(context, args).await
        }
        "spawn" | "agent_spawn" | "agents_spawn" | "swarm_spawn" => {
            agent_swarm_spawn(context, args).await
        }
        _ => Ok(build_agent_swarm_args_failure(
            action,
            "TOOL_ARGS_INVALID",
            format!("unknown agent_swarm action: {action}"),
            "Use one of list/status/send/history/spawn/batch_send/wait with the required fields.",
            &["action"],
            json!({ "action": "list" }),
            args,
            json!({
                "allowed_actions": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
            }),
        )),
    }
}

async fn agent_swarm_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let cutoff = payload
        .active_minutes
        .filter(|value| *value > 0.0)
        .map(|value| now_ts() - value * 60.0);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let mut items = Vec::new();
    for agent in collect_swarm_agents(context, user_id, include_current, &swarm_hive_id)? {
        let runtime = runtime_map.get(&agent.agent_id);
        let (sessions, session_total) =
            context
                .storage
                .list_chat_sessions(user_id, Some(&agent.agent_id), None, 0, 1)?;
        let latest = sessions.first();
        if let Some(cutoff) = cutoff {
            let latest_updated = latest.map(|record| record.updated_at).unwrap_or(0.0);
            let active_count = merge_swarm_active_sessions(runtime).len();
            if latest_updated < cutoff && active_count == 0 {
                continue;
            }
        }
        let active_session_ids = merge_swarm_active_sessions(runtime);
        let last_status =
            latest.and_then(|record| monitor_session_status(context, &record.session_id));
        items.push(json!({
            "agent_id": agent.agent_id,
            "hive_id": normalize_hive_id(&agent.hive_id),
            "name": agent.name,
            "description": agent.description,
            "status": agent.status,
            "is_shared": agent.is_shared,
            "access_level": agent.access_level,
            "updated_at": format_ts(agent.updated_at),
            "session_total": session_total,
            "active_session_total": active_session_ids.len(),
            "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
            "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
            "active_session_ids": active_session_ids,
            "last_session_id": latest.map(|record| record.session_id.clone()),
            "last_message_at": latest.map(|record| format_ts(record.last_message_at)),
            "last_session_status": last_status,
        }));
        if items.len() as i64 >= limit {
            break;
        }
    }
    Ok(build_model_tool_success(
        "list",
        "completed",
        format!("Listed {} swarm workers.", items.len()),
        json!({ "total": items.len(), "items": items }),
    ))
}

async fn agent_swarm_status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let requested_agent_id = normalize_optional_string(payload.agent_id);
    let requested_agent_name = normalize_optional_string(payload.agent_name);
    if requested_agent_id.is_none() && requested_agent_name.is_none() {
        return agent_swarm_list(context, args).await;
    }
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    let Some(agent) = resolve_swarm_agent_record(
        context.storage.as_ref(),
        user_id,
        current_agent_id.as_deref(),
        include_current,
        &swarm_hive_id,
        requested_agent_id.as_deref(),
        requested_agent_name.as_deref(),
    )?
    else {
        return agent_swarm_list(context, args).await;
    };
    let agent_id = agent.agent_id.clone();
    let limit = clamp_limit(payload.limit, 20, MAX_SESSION_LIST_ITEMS);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let runtime = runtime_map.get(&agent_id);
    let active_session_ids = merge_swarm_active_sessions(runtime);
    let active_set: HashSet<String> = active_session_ids.iter().cloned().collect();
    let (sessions, session_total) =
        context
            .storage
            .list_chat_sessions(user_id, Some(&agent_id), None, 0, limit)?;
    let mut recent_sessions = Vec::with_capacity(sessions.len());
    for record in sessions {
        let status = monitor_session_status(context, &record.session_id);
        recent_sessions.push(json!({
            "session_id": record.session_id,
            "title": record.title,
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "parent_session_id": record.parent_session_id,
            "status": status,
            "active": active_set.contains(&record.session_id),
        }));
    }
    Ok(build_model_tool_success(
        "status",
        "completed",
        format!("Loaded swarm status for {}.", agent.name),
        json!({
            "agent": {
                "agent_id": agent.agent_id,
                "hive_id": normalize_hive_id(&agent.hive_id),
                "name": agent.name,
                "description": agent.description,
                "status": agent.status,
                "is_shared": agent.is_shared,
                "access_level": agent.access_level,
                "updated_at": format_ts(agent.updated_at),
            },
            "session_total": session_total,
            "active_session_total": active_session_ids.len(),
            "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
            "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
            "active_session_ids": active_session_ids,
            "recent_sessions": recent_sessions,
        }),
    ))
}

pub(crate) async fn agent_swarm_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmSendArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm send arguments are invalid: {err}"),
                "Provide action=\"send\", a non-empty message, and one of agent_name/agent_id/session_id.",
                &["message", "agent_name|agent_id|session_id"],
                agent_swarm_send_example(),
                args,
                json!({}),
            ));
        }
    };
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm send requires non-empty message",
            "Provide a non-empty message and identify the target with agent_name, agent_id, or session_id.",
            &["message"],
            agent_swarm_send_example(),
            args,
            json!({}),
        ));
    }
    let current_agent_id = current_agent_id(context);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let requested_agent_id = normalize_optional_string(payload.agent_id);
    let requested_agent_name = normalize_optional_string(payload.agent_name);
    let thread_strategy = match parse_swarm_worker_thread_strategy(
        payload.thread_strategy.as_deref(),
        payload.reuse_main_thread,
    ) {
        Ok(strategy) => strategy,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm send thread strategy is invalid: {err}"),
                "threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
                &[],
                agent_swarm_send_example(),
                args,
                json!({
                    "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                }),
            ));
        }
    };
    let has_session_key = payload
        .session_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if requested_agent_id.is_none() && requested_agent_name.is_none() && !has_session_key {
        return Ok(build_agent_swarm_args_failure(
            "send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm send requires agent_id/agent_name or session_id",
            "Provide at least one target field: agent_name, agent_id, or session_id, then send message.",
            &["agent_name|agent_id|session_id"],
            agent_swarm_send_example(),
            args,
            json!({}),
        ));
    }
    let (target_agent, target_session_id, created_session) = if let Some(session_key) =
        payload.session_key
    {
        let session_id = resolve_session_key(Some(session_key))?;
        let record = context
            .storage
            .get_chat_session(user_id, &session_id)?
            .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
        let resolved_agent_id = normalize_optional_string(record.agent_id.clone())
            .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
        if !include_current {
            ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
        }
        if let Some(requested) = requested_agent_id.as_ref() {
            if requested != &resolved_agent_id {
                return Err(anyhow!(
                    "agent_swarm send agent_id does not match target session"
                ));
            }
        }
        if let Some(requested) = requested_agent_name.as_deref() {
            let agent = load_agent_record(
                context.storage.as_ref(),
                user_id,
                Some(&resolved_agent_id),
                false,
            )?
            .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
            if swarm_tool_hint::normalize_swarm_agent_name_lookup_key(requested)
                != swarm_tool_hint::normalize_swarm_agent_name_lookup_key(&agent.name)
            {
                return Err(anyhow!(
                    "agent_swarm send agent_name does not match target session"
                ));
            }
        }
        let target_agent = load_agent_record(
            context.storage.as_ref(),
            user_id,
            Some(&resolved_agent_id),
            false,
        )?
        .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
        ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
        (target_agent, session_id, false)
    } else {
        let target_agent = resolve_swarm_agent_record(
            context.storage.as_ref(),
            user_id,
            current_agent_id.as_deref(),
            include_current,
            &swarm_hive_id,
            requested_agent_id.as_deref(),
            requested_agent_name.as_deref(),
        )?
        .ok_or_else(|| anyhow!("agent_swarm send requires agent_id/agent_name or session_id"))?;
        match thread_strategy {
            SwarmWorkerThreadStrategy::MainThread => {
                let (main_session, created_main_session) = if let Some((orchestration_state, _)) =
                    active_orchestration_for_agent(
                        context.storage.as_ref(),
                        user_id,
                        &target_agent.agent_id,
                    ) {
                    let (binding, created) = ensure_orchestration_member_session(
                        context.storage.as_ref(),
                        user_id,
                        &orchestration_state,
                        &target_agent,
                    )?;
                    let session = context
                        .storage
                        .get_chat_session(user_id, &binding.session_id)?
                        .ok_or_else(|| anyhow!("orchestration worker session not found"))?;
                    (session, created)
                } else {
                    crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                        context.storage.as_ref(),
                        user_id,
                        &target_agent,
                    )?
                };
                (target_agent, main_session.session_id, created_main_session)
            }
            SwarmWorkerThreadStrategy::FreshMainThread => {
                // Callers can still force a clean worker thread, but this is no longer the
                // default path for swarm workers.
                let dispatch_preview = build_swarm_dispatch_message(
                    context.storage.as_ref(),
                    context.monitor.as_deref(),
                    user_id,
                    &swarm_hive_id,
                    current_agent_id.as_deref(),
                    context.session_id,
                    None,
                    None,
                    &message,
                )?;
                let prepared = prepare_swarm_child_session(
                    context,
                    &dispatch_preview,
                    payload.label.clone(),
                    &target_agent.agent_id,
                )?;
                (target_agent, prepared.child_session_id, true)
            }
        }
    };
    let target_agent_id = target_agent.agent_id.clone();
    let orchestration_context = load_dispatch_context(
        context.storage.as_ref(),
        context.workspace.as_ref(),
        context.workspace_id,
        user_id,
        context.session_id,
    );

    let mother_agent_id = claim_swarm_mother_for_context(context, user_id, &swarm_hive_id)?;
    let mut run_record = create_swarm_team_run_record(
        context,
        user_id,
        &swarm_hive_id,
        mother_agent_id,
        None,
        "direct_send",
        1,
    );
    context.storage.upsert_team_run(&run_record)?;
    emit_swarm_run_started(context, &run_record);
    let mut task_record = create_swarm_team_task_record(
        &run_record,
        &target_agent_id,
        Some(target_session_id.clone()),
        created_session.then_some(target_session_id.clone()),
        0,
    );
    let dispatch_message = build_swarm_dispatch_message(
        context.storage.as_ref(),
        context.monitor.as_deref(),
        user_id,
        &swarm_hive_id,
        current_agent_id.as_deref(),
        context.session_id,
        Some(&run_record.team_run_id),
        Some(&task_record.task_id),
        &message,
    )?;
    let dispatch_message = build_worker_dispatch_message(
        context.config,
        context.workspace.as_ref(),
        &context
            .workspace
            .scoped_user_id_by_container(user_id, target_agent.sandbox_container_id),
        &dispatch_message,
        orchestration_context.as_ref(),
        &target_agent_id,
        &target_agent.name,
        created_session
            || !session_has_visible_history(context.storage.as_ref(), user_id, &target_session_id),
    );
    if let Some(orchestration_context) = orchestration_context.as_ref() {
        persist_session_context(
            context.storage.as_ref(),
            user_id,
            &target_session_id,
            &OrchestrationSessionContext {
                mode: ORCHESTRATION_MODE.to_string(),
                run_id: orchestration_context.run_id.clone(),
                group_id: orchestration_context.group_id.clone(),
                role: "worker".to_string(),
                round_index: orchestration_context.round_index,
                mother_agent_id: orchestration_context.mother_agent_id.clone(),
            },
        )?;
    }
    context.storage.upsert_team_task(&task_record)?;
    emit_swarm_task_dispatched(context, &run_record, &task_record);

    if worker_already_dispatched_in_round(
        context.storage.as_ref(),
        user_id,
        context.session_id,
        &target_session_id,
    )? {
        task_record.status = "success".to_string();
        task_record.result_summary = Some("already_dispatched_this_round".to_string());
        task_record.updated_time = now_ts();
        task_record.finished_time = Some(task_record.updated_time);
        task_record.elapsed_s = Some(0.0);
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_updated(context, &run_record, &task_record);
        let (terminal, failed) =
            sync_swarm_run_summary(context, &mut run_record, std::slice::from_ref(&task_record))?;
        if terminal {
            emit_swarm_run_terminal(context, &run_record, failed);
        }
        return Ok(skipped_swarm_task_result(
            "send",
            &task_record.task_id,
            &target_session_id,
            &target_agent_id,
            &target_agent.name,
            "already_dispatched_this_round",
        ));
    }

    let wait_mode = resolve_swarm_wait_mode(
        payload.timeout_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    let mut send_args = json!({
        "session_id": target_session_id,
        "message": dispatch_message,
    });
    // Notify the parent session (queen bee) so the frontend sub-agent panel refreshes
    send_args["announceParentSessionId"] = json!(context.session_id);
    send_args["announcePersistHistory"] = json!(false);
    send_args["announceEmitParentEvents"] = json!(true);
    send_args["teamTaskId"] = json!(task_record.task_id);
    match wait_mode {
        SwarmWaitMode::Immediate => {
            send_args["timeoutSeconds"] = json!(0.0);
        }
        SwarmWaitMode::Finite(timeout_s) => {
            send_args["timeoutSeconds"] = json!(timeout_s);
        }
        SwarmWaitMode::Infinite => {
            send_args["waitForever"] = json!(true);
        }
    }
    let result = match sessions_send(context, &send_args).await {
        Ok(value) => value,
        Err(err) => {
            task_record.status = "error".to_string();
            task_record.error = Some(truncate_tool_result_text(&err.to_string()));
            task_record.updated_time = now_ts();
            task_record.finished_time = Some(task_record.updated_time);
            context.storage.upsert_team_task(&task_record)?;
            emit_swarm_task_updated(context, &run_record, &task_record);
            let (terminal, failed) = sync_swarm_run_summary(
                context,
                &mut run_record,
                std::slice::from_ref(&task_record),
            )?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
            return Err(err);
        }
    };
    let state = tool_result_field(&result, "state")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            normalize_tool_run_state(
                result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("accepted"),
            )
        });
    let run_id = tool_result_field(&result, "run_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let Some(run_id) = run_id.as_deref() {
        task_record.session_run_id = Some(run_id.to_string());
    }
    let reply = tool_result_field(&result, "reply")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let error_text = tool_result_field(&result, "error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let elapsed_s = tool_result_field(&result, "elapsed_s")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0);
    let should_sync_progress = matches!(state.as_str(), "accepted" | "running" | "timeout");
    if let Some(run_id) = run_id.as_deref() {
        if should_sync_progress {
            if let Some(session_run) = context.storage.get_session_run(run_id)? {
                if !is_swarm_task_terminal_status(&session_run.status) {
                    apply_session_run_to_swarm_task(&mut task_record, &session_run);
                }
            }
            task_record.updated_time = task_record.updated_time.max(now_ts());
            context.storage.upsert_team_task(&task_record)?;
            emit_swarm_task_updated(context, &run_record, &task_record);
            let (terminal, failed) = sync_swarm_run_summary(
                context,
                &mut run_record,
                std::slice::from_ref(&task_record),
            )?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
        }
    } else if state == "error" {
        task_record.status = "error".to_string();
        task_record.error = error_text.as_deref().map(truncate_tool_result_text);
        task_record.updated_time = now_ts();
        task_record.finished_time = Some(task_record.updated_time);
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_updated(context, &run_record, &task_record);
        let (terminal, failed) =
            sync_swarm_run_summary(context, &mut run_record, std::slice::from_ref(&task_record))?;
        if terminal {
            emit_swarm_run_terminal(context, &run_record, failed);
        }
    }
    return Ok(build_agent_swarm_tool_result(
        "send",
        &state,
        run_record.team_run_id,
        Some(task_record.task_id),
        run_id,
        Some(target_session_id),
        Some(target_agent_id),
        Some(target_agent.name),
        Some(created_session),
        reply,
        error_text,
        elapsed_s,
        Some(json!({
            "thread_strategy": if has_session_key {
                "session_key"
            } else {
                thread_strategy.as_tool_value()
            }
        })),
    ));
}

pub(crate) async fn agent_swarm_batch_send(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: AgentSwarmBatchSendArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm batch_send arguments are invalid: {err}"),
                "Provide action=\"batch_send\" and non-empty tasks. Each task needs a target and message.",
                &["tasks"],
                agent_swarm_batch_send_example(),
                args,
                json!({}),
            ));
        }
    };
    if payload.tasks.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "batch_send",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm batch_send requires non-empty tasks",
            "Provide non-empty tasks. Each task needs a target field and a non-empty message.",
            &["tasks"],
            agent_swarm_batch_send_example(),
            args,
            json!({}),
        ));
    }

    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let max_tasks = context
        .config
        .tools
        .swarm
        .max_parallel_tasks_per_team
        .max(1);
    if payload.tasks.len() > max_tasks {
        return Ok(build_agent_swarm_args_failure(
            "batch_send",
            "TOOL_ARGS_INVALID",
            format!(
                "agent_swarm batch_send task count {} exceeds max_parallel_tasks_per_team {}",
                payload.tasks.len(),
                max_tasks
            ),
            format!("Reduce tasks to {max_tasks} or split the batch_send call."),
            &[],
            agent_swarm_batch_send_example(),
            args,
            json!({
                "max_parallel_tasks_per_team": max_tasks
            }),
        ));
    }

    let shared_message = normalize_optional_string(payload.message.clone());
    let shared_label = normalize_optional_string(payload.label.clone());
    let shared_thread_strategy = match parse_swarm_worker_thread_strategy(
        payload.thread_strategy.as_deref(),
        payload.reuse_main_thread,
    ) {
        Ok(strategy) => strategy,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm batch_send thread strategy is invalid: {err}"),
                "threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
                &[],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                }),
            ));
        }
    };
    for (index, task) in payload.tasks.iter().enumerate() {
        let task_message =
            normalize_optional_string(task.message.clone()).or_else(|| shared_message.clone());
        let inferred_agent_name = task_message
            .as_deref()
            .and_then(infer_swarm_agent_name_from_task_message);
        let has_target = normalize_optional_string(task.agent_id.clone()).is_some()
            || normalize_optional_string(task.agent_name.clone()).is_some()
            || inferred_agent_name.is_some()
            || resolve_swarm_batch_session_key(task.session_key.clone())
                .ok()
                .flatten()
                .is_some();
        if !has_target {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!(
                    "agent_swarm batch_send task[{index}] requires agent_id/agent_name or session_id"
                ),
                "Each task needs one of agent_name, agent_id, or session_id.",
                &["tasks[].agent_name|agent_id|session_id"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "Summarize the requested material."
                    }
                }),
            ));
        }
        let has_message = task_message.is_some();
        if !has_message {
            return Ok(build_agent_swarm_args_failure(
                "batch_send",
                "TOOL_ARGS_MISSING_FIELD",
                format!("agent_swarm batch_send task[{index}] requires message"),
                "Provide a non-empty message for each task or a shared top-level message.",
                &["tasks[].message"],
                agent_swarm_batch_send_example(),
                args,
                json!({
                    "task_index": index,
                    "expected_task_shape": {
                        "agent_name": "worker_a",
                        "message": "Summarize the requested material."
                    }
                }),
            ));
        }
    }
    let default_include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let mother_agent_id = claim_swarm_mother_for_context(context, user_id, &swarm_hive_id)?;
    let orchestration_context = load_dispatch_context(
        context.storage.as_ref(),
        context.workspace.as_ref(),
        context.workspace_id,
        user_id,
        context.session_id,
    );
    let allowed_tools = collect_user_allowed_tools(context, user_id)?;

    let agent_access = context.storage.get_user_agent_access(user_id)?;
    let mut agent_map = HashMap::new();
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, agent_access.as_ref(), &agent) {
            continue;
        }
        // Batch send should only validate the concrete requested targets.
        // Unrelated agents from other hives must not poison the local lookup cache.
        if !agent_in_hive(&agent, &swarm_hive_id) {
            continue;
        }
        agent_map.entry(agent.agent_id.clone()).or_insert(agent);
    }

    let (sessions, _) = context
        .storage
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut sessions_by_id = HashMap::with_capacity(sessions.len());
    for session in sessions {
        sessions_by_id.insert(session.session_id.clone(), session.clone());
    }

    let mut dispatch_plan = Vec::with_capacity(payload.tasks.len());
    let mut indexed_items = Vec::new();
    let mut run_ids = Vec::new();
    for (index, task) in payload.tasks.into_iter().enumerate() {
        let message = normalize_optional_string(task.message)
            .or_else(|| shared_message.clone())
            .ok_or_else(|| anyhow!("agent_swarm batch_send task[{index}] requires message"))?;
        let label = normalize_optional_string(task.label).or_else(|| shared_label.clone());
        let include_current = task.include_current.unwrap_or(default_include_current);
        let requested_agent_id = normalize_optional_string(task.agent_id);
        let requested_agent_name = normalize_optional_string(task.agent_name)
            .or_else(|| infer_swarm_agent_name_from_task_message(&message));
        let task_thread_strategy = match if task.thread_strategy.is_some()
            || task.reuse_main_thread.is_some()
        {
            parse_swarm_worker_thread_strategy(
                task.thread_strategy.as_deref(),
                task.reuse_main_thread,
            )
        } else {
            Ok(shared_thread_strategy)
        } {
            Ok(strategy) => strategy,
            Err(err) => {
                return Ok(build_agent_swarm_args_failure(
                    "batch_send",
                    "TOOL_ARGS_INVALID",
                    format!(
                        "agent_swarm batch_send task[{index}] thread strategy is invalid: {err}"
                    ),
                    "Each task threadStrategy must be fresh_main_thread or main_thread; reuseMainThread=true is also supported.",
                    &[],
                    agent_swarm_batch_send_example(),
                    args,
                    json!({
                        "task_index": index,
                        "allowed_thread_strategies": ["fresh_main_thread", "main_thread"]
                    }),
                ));
            }
        };
        let requested_session_id = resolve_swarm_batch_session_key(task.session_key)?;

        let (agent_record, session_record) = if let Some(session_id) = requested_session_id {
            let session_record = if let Some(record) = sessions_by_id.get(&session_id).cloned() {
                record
            } else {
                context
                    .storage
                    .get_chat_session(user_id, &session_id)?
                    .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?
            };

            let resolved_agent_id = normalize_optional_string(session_record.agent_id.clone())
                .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
            if !include_current {
                ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
            }
            if let Some(requested) = requested_agent_id.as_ref() {
                if requested != &resolved_agent_id {
                    return Err(anyhow!(
                        "agent_swarm send agent_id does not match target session"
                    ));
                }
            }
            if let Some(requested) = requested_agent_name.as_deref() {
                let agent_record = if let Some(agent) = agent_map.get(&resolved_agent_id).cloned() {
                    agent
                } else {
                    load_agent_record(
                        context.storage.as_ref(),
                        user_id,
                        Some(&resolved_agent_id),
                        false,
                    )?
                    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
                };
                if swarm_tool_hint::normalize_swarm_agent_name_lookup_key(requested)
                    != swarm_tool_hint::normalize_swarm_agent_name_lookup_key(&agent_record.name)
                {
                    return Err(anyhow!(
                        "agent_swarm send agent_name does not match target session"
                    ));
                }
            }

            let agent_record = if let Some(agent) = agent_map.get(&resolved_agent_id).cloned() {
                agent
            } else {
                load_agent_record(
                    context.storage.as_ref(),
                    user_id,
                    Some(&resolved_agent_id),
                    false,
                )?
                .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
            };
            ensure_swarm_agent_in_hive(&agent_record, &swarm_hive_id)?;

            sessions_by_id.insert(session_record.session_id.clone(), session_record.clone());
            (agent_record, Some(session_record))
        } else {
            let agent_record = resolve_swarm_agent_record(
                context.storage.as_ref(),
                user_id,
                current_agent_id.as_deref(),
                include_current,
                &swarm_hive_id,
                requested_agent_id.as_deref(),
                requested_agent_name.as_deref(),
            )?
            .ok_or_else(|| {
                anyhow!("agent_swarm send requires agent_id/agent_name or session_id")
            })?;
            // Swarm batch dispatch also forces a fresh worker thread when the caller
            // does not explicitly target an existing session_key.
            (agent_record, None)
        };
        let dispatch_message = build_swarm_dispatch_message(
            context.storage.as_ref(),
            context.monitor.as_deref(),
            user_id,
            &swarm_hive_id,
            current_agent_id.as_deref(),
            context.session_id,
            None,
            None,
            &message,
        )?;
        let (
            session_id,
            created_session,
            resolved_thread_strategy,
            tool_names,
            model_name,
            agent_prompt,
            preview_skill,
        ) = if let Some(session_record) = session_record {
            let tool_names = resolve_swarm_batch_tool_names(
                context,
                context.config,
                context.skills,
                &allowed_tools,
                user_id,
                &session_record,
                &agent_record,
            );
            let agent_prompt = {
                let prompt = agent_record.system_prompt.trim();
                if prompt.is_empty() {
                    None
                } else {
                    Some(prompt.to_string())
                }
            };
            let model_name = normalize_optional_string(agent_record.model_name.clone());
            (
                session_record.session_id,
                false,
                "session_key",
                tool_names,
                model_name,
                agent_prompt,
                agent_record.preview_skill,
            )
        } else {
            match task_thread_strategy {
                SwarmWorkerThreadStrategy::MainThread => {
                    let (main_session, created_main_session) =
                        if let Some((orchestration_state, _)) = active_orchestration_for_agent(
                            context.storage.as_ref(),
                            user_id,
                            &agent_record.agent_id,
                        ) {
                            let (binding, created) = ensure_orchestration_member_session(
                                context.storage.as_ref(),
                                user_id,
                                &orchestration_state,
                                &agent_record,
                            )?;
                            let session = context
                                .storage
                                .get_chat_session(user_id, &binding.session_id)?
                                .ok_or_else(|| anyhow!("orchestration worker session not found"))?;
                            (session, created)
                        } else {
                            crate::services::swarm::beeroom::resolve_or_create_agent_main_session(
                                context.storage.as_ref(),
                                user_id,
                                &agent_record,
                            )?
                        };
                    let tool_names = resolve_swarm_batch_tool_names(
                        context,
                        context.config,
                        context.skills,
                        &allowed_tools,
                        user_id,
                        &main_session,
                        &agent_record,
                    );
                    let agent_prompt = {
                        let prompt = agent_record.system_prompt.trim();
                        if prompt.is_empty() {
                            None
                        } else {
                            Some(prompt.to_string())
                        }
                    };
                    let model_name = normalize_optional_string(agent_record.model_name.clone());
                    (
                        main_session.session_id,
                        created_main_session,
                        SwarmWorkerThreadStrategy::MainThread.as_tool_value(),
                        tool_names,
                        model_name,
                        agent_prompt,
                        agent_record.preview_skill,
                    )
                }
                SwarmWorkerThreadStrategy::FreshMainThread => {
                    let prepared = prepare_swarm_child_session(
                        context,
                        &dispatch_message,
                        label.clone(),
                        &agent_record.agent_id,
                    )?;
                    (
                        prepared.child_session_id,
                        true,
                        SwarmWorkerThreadStrategy::FreshMainThread.as_tool_value(),
                        prepared.request.tool_names,
                        prepared.model_name,
                        prepared.request.agent_prompt,
                        prepared.request.preview_skill,
                    )
                }
            }
        };
        let dispatch_message = build_worker_dispatch_message(
            context.config,
            context.workspace.as_ref(),
            &context
                .workspace
                .scoped_user_id_by_container(user_id, agent_record.sandbox_container_id),
            &dispatch_message,
            orchestration_context.as_ref(),
            &agent_record.agent_id,
            &agent_record.name,
            created_session
                || !session_has_visible_history(context.storage.as_ref(), user_id, &session_id),
        );
        if let Some(orchestration_context) = orchestration_context.as_ref() {
            persist_session_context(
                context.storage.as_ref(),
                user_id,
                &session_id,
                &OrchestrationSessionContext {
                    mode: ORCHESTRATION_MODE.to_string(),
                    run_id: orchestration_context.run_id.clone(),
                    group_id: orchestration_context.group_id.clone(),
                    role: "worker".to_string(),
                    round_index: orchestration_context.round_index,
                    mother_agent_id: orchestration_context.mother_agent_id.clone(),
                },
            )?;
        }
        if worker_already_dispatched_in_round(
            context.storage.as_ref(),
            user_id,
            context.session_id,
            &session_id,
        )? {
            let skipped_task_id = format!("task_{}", Uuid::new_v4().simple());
            indexed_items.push((
                index,
                skipped_swarm_task_result(
                    "batch_send",
                    &skipped_task_id,
                    &session_id,
                    &agent_record.agent_id,
                    &agent_record.name,
                    "already_dispatched_this_round",
                ),
            ));
            continue;
        }

        dispatch_plan.push(SwarmBatchDispatchTask {
            index,
            agent_id: agent_record.agent_id,
            agent_name: agent_record.name,
            session_id,
            created_session,
            thread_strategy: resolved_thread_strategy,
            team_task_id: String::new(),
            message: dispatch_message,
            label,
            tool_names,
            model_name,
            agent_prompt,
            preview_skill,
        });
    }

    if dispatch_plan.is_empty() {
        indexed_items.sort_by_key(|(index, _)| *index);
        let items = indexed_items
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>();
        return Ok(build_model_tool_success(
            "batch_send",
            "skipped",
            "All swarm tasks were already dispatched in this round.",
            json!({
                "items": items,
                "task_total": 0,
                "task_success": 0,
                "task_failed": 0,
                "skip_reason": "already_dispatched_this_round",
                "team_run_id": Value::Null,
            }),
        ));
    }

    let mut run_record = create_swarm_team_run_record(
        context,
        user_id,
        &swarm_hive_id,
        mother_agent_id,
        payload.team_run_id.as_deref(),
        "batch_send",
        dispatch_plan.len(),
    );
    context.storage.upsert_team_run(&run_record)?;
    emit_swarm_run_started(context, &run_record);

    let mut task_records_by_index = HashMap::new();
    for task in &mut dispatch_plan {
        let task_record = create_swarm_team_task_record(
            &run_record,
            &task.agent_id,
            Some(task.session_id.clone()),
            task.created_session.then_some(task.session_id.clone()),
            0,
        );
        let dispatch_message = build_swarm_dispatch_message(
            context.storage.as_ref(),
            context.monitor.as_deref(),
            user_id,
            &swarm_hive_id,
            current_agent_id.as_deref(),
            context.session_id,
            Some(&run_record.team_run_id),
            Some(&task_record.task_id),
            &task.message,
        )?;
        task.message = dispatch_message;
        task.team_task_id = task_record.task_id.clone();
        context.storage.upsert_team_task(&task_record)?;
        emit_swarm_task_dispatched(context, &run_record, &task_record);
        task_records_by_index.insert(task.index, task_record.clone());
    }

    let dispatch_targets_by_index = dispatch_plan
        .iter()
        .map(|task| {
            (
                task.index,
                json!({
                    "index": task.index,
                    "agent_id": task.agent_id,
                    "target_agent_id": task.agent_id,
                    "agent_name": task.agent_name,
                    "target_agent_name": task.agent_name,
                    "session_id": task.session_id,
                    "target_session_id": task.session_id,
                    "created_session": task.created_session,
                    "thread_strategy": task.thread_strategy,
                }),
            )
        })
        .collect::<HashMap<_, _>>();

    let dispatch_parallelism = dispatch_plan.len().min(max_tasks).max(1);
    let mut dispatches = stream::iter(dispatch_plan.into_iter().map(|task| async move {
        let index = task.index;
        let result = dispatch_swarm_batch_task(context, task).await;
        (index, result)
    }))
    .buffer_unordered(dispatch_parallelism);

    while let Some((index, result)) = dispatches.next().await {
        match result {
            Ok(result) => {
                let run_id = tool_result_field(&result, "run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                if !run_id.is_empty() {
                    run_ids.push(run_id.clone());
                }
                if let Some(task_record) = task_records_by_index.get_mut(&index) {
                    if !run_id.is_empty() {
                        task_record.session_run_id = Some(run_id.clone());
                        if let Some(session_run) = context.storage.get_session_run(&run_id)? {
                            if !is_swarm_task_terminal_status(&session_run.status) {
                                apply_session_run_to_swarm_task(task_record, &session_run);
                            }
                        }
                    }
                    let status = tool_result_field(&result, "status")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("accepted")
                        .to_ascii_lowercase();
                    if status == "error" && run_id.is_empty() {
                        task_record.status = "error".to_string();
                        task_record.error = tool_result_field(&result, "error")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(truncate_tool_result_text);
                        task_record.elapsed_s = tool_result_field(&result, "elapsed_s")
                            .and_then(Value::as_f64)
                            .filter(|value| value.is_finite() && *value >= 0.0);
                    }
                    task_record.updated_time = now_ts();
                    if matches!(task_record.status.as_str(), "success" | "timeout" | "error") {
                        task_record.finished_time = Some(task_record.updated_time);
                    }
                    context.storage.upsert_team_task(task_record)?;
                    emit_swarm_task_updated(context, &run_record, task_record);
                }
                let task_id = tool_result_field(&result, "task_id")
                    .cloned()
                    .unwrap_or_else(|| {
                        task_records_by_index
                            .get(&index)
                            .map(|item| json!(item.task_id))
                            .unwrap_or(Value::Null)
                    });
                let mut item = json!({
                    "index": index,
                    "status": tool_result_field(&result, "status")
                        .cloned()
                        .unwrap_or_else(|| json!("accepted")),
                    "run_id": if run_id.is_empty() { Value::Null } else { json!(run_id) },
                    "target_agent_id": tool_result_field_or_null(&result, "agent_id"),
                    "agent_name": tool_result_field_or_null(&result, "agent_name"),
                    "target_agent_name": tool_result_field_or_null(&result, "agent_name"),
                    "session_id": tool_result_field_or_null(&result, "session_id"),
                    "target_session_id": tool_result_field_or_null(&result, "session_id"),
                    "task_id": task_id,
                    "created_session": tool_result_field(&result, "created_session")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    "thread_strategy": tool_result_field(&result, "thread_strategy")
                        .cloned()
                        .unwrap_or_else(|| json!(shared_thread_strategy.as_tool_value())),
                });
                if let Some(error) = tool_result_field(&result, "error") {
                    if let Value::Object(ref mut map) = item {
                        map.insert("error".to_string(), error.clone());
                    }
                }
                indexed_items.push((index, item));
            }
            Err(err) => {
                let error_text = truncate_tool_result_text(&err.to_string());
                if let Some(task_record) = task_records_by_index.get_mut(&index) {
                    task_record.status = "error".to_string();
                    task_record.error = Some(error_text.clone());
                    task_record.updated_time = now_ts();
                    task_record.finished_time = Some(task_record.updated_time);
                    context.storage.upsert_team_task(task_record)?;
                    emit_swarm_task_updated(context, &run_record, task_record);
                }
                let mut item = dispatch_targets_by_index
                    .get(&index)
                    .cloned()
                    .unwrap_or_else(|| json!({ "index": index }));
                if let Value::Object(ref mut map) = item {
                    map.insert("status".to_string(), json!("error"));
                    map.insert(
                        "task_id".to_string(),
                        task_records_by_index
                            .get(&index)
                            .map(|item| json!(item.task_id))
                            .unwrap_or(Value::Null),
                    );
                    map.insert("error".to_string(), json!(error_text));
                }
                indexed_items.push((index, item));
            }
        }
    }

    indexed_items.sort_by_key(|(index, _)| *index);
    let items = indexed_items
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    let run_ids = dedupe_non_empty_strings(run_ids);
    let accepted_total = items
        .iter()
        .filter(|item| {
            item.get("run_id")
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    let skipped_total = items
        .iter()
        .filter(|item| {
            item.get("skipped")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let failed_total = items
        .len()
        .saturating_sub(accepted_total)
        .saturating_sub(skipped_total);
    let run_tasks = task_records_by_index.values().cloned().collect::<Vec<_>>();
    let (terminal, failed) = sync_swarm_run_summary(context, &mut run_record, &run_tasks)?;
    if terminal {
        emit_swarm_run_terminal(context, &run_record, failed);
    }

    let mut state = if accepted_total > 0 {
        if failed_total > 0 {
            "partial".to_string()
        } else {
            "accepted".to_string()
        }
    } else if skipped_total > 0 && failed_total == 0 {
        "skipped".to_string()
    } else {
        "error".to_string()
    };
    let mut wait_result_value = Value::Null;

    let wait_mode = resolve_swarm_wait_mode(
        payload.wait_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    if !matches!(wait_mode, SwarmWaitMode::Immediate) {
        let poll_interval_seconds = payload
            .poll_interval_seconds
            .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
        let wait_result = wait_for_swarm_runs(
            context,
            &run_ids,
            swarm_wait_seconds_value(wait_mode),
            poll_interval_seconds,
            true,
        )
        .await?;
        if let Some(wait_items) = tool_result_field(&wait_result, "items").and_then(Value::as_array)
        {
            let mut snapshots_by_run_id: HashMap<String, &Value> = HashMap::new();
            for item in wait_items {
                let Some(run_id) = item
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                snapshots_by_run_id.insert(run_id.to_string(), item);
            }

            for task_record in task_records_by_index.values_mut() {
                let Some(run_id) = task_record
                    .session_run_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(snapshot) = snapshots_by_run_id.get(run_id) else {
                    continue;
                };
                let snapshot_status = snapshot
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_ascii_lowercase();
                task_record.status = match snapshot_status.as_str() {
                    "success" | "ok" => "success".to_string(),
                    "timeout" => "timeout".to_string(),
                    "cancelled" => "cancelled".to_string(),
                    "queued" | "running" => "queued".to_string(),
                    _ => "error".to_string(),
                };
                task_record.started_time = snapshot
                    .get("started_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0);
                task_record.finished_time = snapshot
                    .get("finished_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0);
                task_record.elapsed_s = snapshot
                    .get("elapsed_s")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value >= 0.0);
                task_record.result_summary = snapshot
                    .get("result_preview")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(truncate_tool_result_text);
                task_record.error = snapshot
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(truncate_tool_result_text);
                task_record.updated_time = snapshot
                    .get("updated_time")
                    .and_then(Value::as_f64)
                    .filter(|value| value.is_finite() && *value > 0.0)
                    .unwrap_or_else(now_ts);
                if matches!(
                    task_record.status.as_str(),
                    "success" | "timeout" | "error" | "cancelled"
                ) && task_record.finished_time.is_none()
                {
                    task_record.finished_time = Some(task_record.updated_time);
                }
                context.storage.upsert_team_task(task_record)?;
                emit_swarm_task_updated(context, &run_record, task_record);
            }

            let run_tasks = task_records_by_index.values().cloned().collect::<Vec<_>>();
            let (terminal, failed) = sync_swarm_run_summary(context, &mut run_record, &run_tasks)?;
            if terminal {
                emit_swarm_run_terminal(context, &run_record, failed);
            }
        }
        wait_result_value = wait_result.clone();
        if let Some(wait_state) = wait_result.get("state").and_then(Value::as_str) {
            state = wait_state.to_string();
        }
    }
    Ok(build_agent_swarm_tool_result(
        "batch_send",
        &state,
        run_record.team_run_id,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(json!({
            "run_ids": run_ids,
            "counts": {
                "total": items.len(),
                "accepted": accepted_total,
                "failed": failed_total,
            },
            "items": items,
            "wait": wait_result_value,
        })),
    ))
}

async fn agent_swarm_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmWaitArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "wait",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm wait arguments are invalid: {err}"),
                "Provide action=\"wait\" and run_ids or run_id.",
                &["run_ids|run_id"],
                agent_swarm_wait_example(),
                args,
                json!({}),
            ));
        }
    };
    let mut run_ids = payload.run_ids.unwrap_or_default();
    if let Some(run_id) = payload.run_id {
        run_ids.push(run_id);
    }
    let run_ids = dedupe_non_empty_strings(run_ids);
    if run_ids.is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "wait",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm wait requires runIds",
            "Provide run_ids or run_id. Usually copy run_id from a send/batch_send result first.",
            &["run_ids|run_id"],
            agent_swarm_wait_example(),
            args,
            json!({}),
        ));
    }
    let wait_mode = resolve_swarm_wait_mode(
        payload.wait_seconds,
        context.config.tools.swarm.default_timeout_s,
    );
    let poll_interval_seconds = payload
        .poll_interval_seconds
        .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
    wait_for_swarm_runs(
        context,
        &run_ids,
        swarm_wait_seconds_value(wait_mode),
        poll_interval_seconds,
        true,
    )
    .await
}

async fn agent_swarm_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key.clone())?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let requested_agent_id = args
        .get("agentId")
        .or_else(|| args.get("agent_id"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let target_agent_id = normalize_optional_string(record.agent_id.clone())
        .ok_or_else(|| anyhow!("agent_swarm history target session is missing agent_id"))?;
    let target_agent = load_agent_record(
        context.storage.as_ref(),
        user_id,
        Some(&target_agent_id),
        false,
    )?
    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
    if let Some(requested_agent_id) = requested_agent_id {
        if requested_agent_id != target_agent_id {
            return Err(anyhow!(
                "agent_swarm history agent_id does not match target session"
            ));
        }
    }
    if !include_current {
        ensure_swarm_target_not_current(&target_agent_id, current_agent_id(context).as_deref())?;
    }
    sessions_history(context, args).await
}

async fn agent_swarm_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSpawnArgs = match serde_json::from_value(args.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            return Ok(build_agent_swarm_args_failure(
                "spawn",
                "TOOL_ARGS_INVALID",
                format!("agent_swarm spawn arguments are invalid: {err}"),
                "Provide action=\"spawn\", a non-empty task, and agent_name or agent_id.",
                &["task", "agent_name|agent_id"],
                agent_swarm_spawn_example(),
                args,
                json!({}),
            ));
        }
    };
    let SessionSpawnArgs {
        task,
        label,
        agent_id,
        agent_name,
        model: _,
        run_timeout_seconds,
        cleanup: _,
        thread_strategy,
        reuse_main_thread,
    } = payload;
    if task.trim().is_empty() {
        return Ok(build_agent_swarm_args_failure(
            "spawn",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm spawn requires non-empty task",
            "Provide a non-empty task with clear expected output.",
            &["task"],
            agent_swarm_spawn_example(),
            args,
            json!({}),
        ));
    }
    let requested_agent_id = normalize_optional_string(agent_id);
    let requested_agent_name = normalize_optional_string(agent_name);
    if requested_agent_id.is_none() && requested_agent_name.is_none() {
        return Ok(build_agent_swarm_args_failure(
            "spawn",
            "TOOL_ARGS_MISSING_FIELD",
            "agent_swarm spawn requires agent_id/agent_name",
            "Provide agent_name or agent_id. Use subagent_control.spawn for a temporary child session.",
            &["agent_name|agent_id"],
            agent_swarm_spawn_example(),
            args,
            json!({}),
        ));
    }
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let user_id = context.user_id.trim();
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let target_agent = resolve_swarm_agent_record(
        context.storage.as_ref(),
        user_id,
        current_agent_id(context).as_deref(),
        include_current,
        &swarm_hive_id,
        requested_agent_id.as_deref(),
        requested_agent_name.as_deref(),
    )?
    .ok_or_else(|| anyhow!("agent_swarm spawn target agent not found"))?;
    let mut send_args = json!({
        "agentId": target_agent.agent_id,
        "message": task,
    });
    if let Value::Object(ref mut map) = send_args {
        if let Some(label) = label {
            map.insert("label".to_string(), json!(label));
        }
        if include_current {
            map.insert("includeCurrent".to_string(), Value::Bool(true));
        }
        if let Some(timeout_seconds) = run_timeout_seconds {
            map.insert(
                "timeoutSeconds".to_string(),
                json!(timeout_seconds.max(0.0)),
            );
        }
        if let Some(thread_strategy) = thread_strategy {
            map.insert("threadStrategy".to_string(), json!(thread_strategy));
        }
        if let Some(reuse_main_thread) = reuse_main_thread {
            map.insert("reuseMainThread".to_string(), json!(reuse_main_thread));
        }
        if let Some(hive_id) = swarm_hive_arg(args) {
            map.insert("hiveId".to_string(), json!(hive_id));
        }
    }
    let result = agent_swarm_send(context, &send_args).await?;
    Ok(enrich_agent_swarm_spawn_response(result))
}

pub(crate) fn enrich_agent_swarm_spawn_response(mut response: Value) -> Value {
    let state = response
        .get("state")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("accepted")
        .to_ascii_lowercase();
    let child_session_id = tool_result_field(&response, "session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    if let Some(object) = response.as_object_mut() {
        object.insert("action".to_string(), json!("spawn"));
        if state == "accepted" {
            object.insert(
                "summary".to_string(),
                json!("Worker session was created and the initial task was queued."),
            );
        } else if state == "completed" {
            object.insert(
                "summary".to_string(),
                json!("Worker session completed the initial task."),
            );
        }
    }
    if let Some(data) = response.get_mut("data").and_then(Value::as_object_mut) {
        data.insert("spawned".to_string(), Value::Bool(true));
        if let Some(session_id) = child_session_id {
            data.insert("child_session_id".to_string(), Value::String(session_id));
        }
    }
    response
}

fn collect_swarm_agents(
    context: &ToolContext<'_>,
    user_id: &str,
    include_current: bool,
    hive_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let access = context.storage.get_user_agent_access(user_id)?;
    let current_agent_id = current_agent_id(context);
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !seen.insert(agent.agent_id.clone()) {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, access.as_ref(), &agent) {
            continue;
        }
        if !agent_in_hive(&agent, hive_id) {
            continue;
        }
        if !include_current
            && current_agent_id
                .as_deref()
                .is_some_and(|value| value == agent.agent_id.as_str())
        {
            continue;
        }
        output.push(agent);
    }
    output.sort_by(|a, b| {
        b.updated_at
            .partial_cmp(&a.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    Ok(output)
}

fn collect_swarm_runtime(
    context: &ToolContext<'_>,
    user_id: &str,
) -> Result<HashMap<String, AgentSwarmRuntime>> {
    let mut output = HashMap::new();
    for lock in context.storage.list_session_locks_by_user(user_id)? {
        let agent_id = lock.agent_id.trim();
        let session_id = lock.session_id.trim();
        if agent_id.is_empty() || session_id.is_empty() {
            continue;
        }
        output
            .entry(agent_id.to_string())
            .or_insert_with(AgentSwarmRuntime::default)
            .lock_sessions
            .insert(session_id.to_string());
    }
    if let Some(monitor) = context.monitor.as_ref() {
        for session in monitor.list_sessions(true) {
            let session_user_id = session.get("user_id").and_then(Value::as_str).unwrap_or("");
            if session_user_id.trim() != user_id {
                continue;
            }
            let agent_id = session
                .get("agent_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let session_id = session
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if agent_id.is_empty() || session_id.is_empty() {
                continue;
            }
            output
                .entry(agent_id.to_string())
                .or_insert_with(AgentSwarmRuntime::default)
                .running_sessions
                .insert(session_id.to_string());
        }
    }
    Ok(output)
}

fn merge_swarm_active_sessions(runtime: Option<&AgentSwarmRuntime>) -> Vec<String> {
    let Some(runtime) = runtime else {
        return Vec::new();
    };
    let mut sessions = runtime.lock_sessions.clone();
    sessions.extend(runtime.running_sessions.clone());
    let mut output = sessions.into_iter().collect::<Vec<_>>();
    output.sort();
    output
}

fn monitor_session_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|entry| {
            entry
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
}

fn ensure_swarm_target_not_current(
    target_agent_id: &str,
    current_agent_id: Option<&str>,
) -> Result<()> {
    if current_agent_id.is_some_and(|value| value == target_agent_id) {
        return Err(anyhow!(
            "agent_swarm only manages agents other than the current agent"
        ));
    }
    Ok(())
}

fn swarm_hive_arg(args: &Value) -> Option<&str> {
    args.get("hiveId")
        .or_else(|| args.get("hive_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn resolve_swarm_hive_id(
    context: &ToolContext<'_>,
    user_id: &str,
    requested_hive_id: Option<&str>,
) -> Result<String> {
    resolve_swarm_hive_scope(
        context.storage.as_ref(),
        user_id,
        current_agent_id(context).as_deref(),
        requested_hive_id,
    )
}

fn ensure_swarm_agent_in_hive(agent: &UserAgentRecord, hive_id: &str) -> Result<()> {
    ensure_swarm_agent_in_beeroom(agent, hive_id)
}

pub(crate) fn current_agent_id(context: &ToolContext<'_>) -> Option<String> {
    context
        .agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn infer_swarm_agent_name_from_task_message(message: &str) -> Option<String> {
    let normalized = message.replace('\r', "\n");
    for marker in ["你的角色：", "你的角色:", "角色：", "角色:", "role:"] {
        if let Some((_, tail)) = normalized.split_once(marker) {
            let candidate = tail
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '“' | '”' | '。' | '，' | ','));
            if !candidate.is_empty() && candidate.chars().count() <= 64 {
                return Some(candidate.to_string());
            }
        }
    }
    None
}
