//! User-facing queue operations: list, interject (promote), withdraw (cancel) and reorder.
//!
//! The engine already parks a submitted turn when the thread is busy; these helpers
//! let the owner of a session act on a *single* parked turn instead of only the
//! session-wide admin promote.

use super::*;

const QUEUE_LIST_LIMIT: i64 = 64;

fn is_parked_queue_status(status: &str) -> bool {
    status == TASK_STATUS_PENDING || status == TASK_STATUS_RETRY
}

/// 排队条目里属于用户自己发出的那一轮：子智能体信箱、目标轮次等内部任务
/// 走同一张队列表，但不能出现在用户可见的排队条里。
fn is_user_visible_queue_task(task: &AgentTaskRecord) -> bool {
    let payload = &task.request_payload;
    if payload
        .get("hidden_internal_user")
        .and_then(Value::as_bool)
        == Some(true)
    {
        return false;
    }
    let overrides = payload.get("config_overrides");
    if overrides
        .and_then(|value| value.get("__agent_message"))
        .is_some()
    {
        return false;
    }
    goal::read_goal_round_tag(overrides).is_none()
}

fn queue_task_priority(task: &AgentTaskRecord) -> i64 {
    task.request_payload
        .get("queue_priority")
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

/// 与派发序一致：优先级高者先，其次 retry_at、created_at、task_id。
fn sort_queue_tasks_by_dispatch_order(tasks: &mut [AgentTaskRecord]) {
    tasks.sort_by(|left, right| {
        queue_task_priority(right)
            .cmp(&queue_task_priority(left))
            .then(
                left.retry_at
                    .partial_cmp(&right.retry_at)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                left.created_at
                    .partial_cmp(&right.created_at)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
}

fn queue_task_view(task: &AgentTaskRecord, position: usize) -> Value {
    let payload = &task.request_payload;
    let content = payload
        .get("question")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let mut view = json!({
        "queue_id": task.task_id,
        "session_id": task.session_id,
        "status": task.status,
        "content": content,
        "position": position,
        "queue_ahead": payload.get("queue_ahead").cloned().unwrap_or(json!(position)),
        "queue_total": payload.get("queue_total").cloned().unwrap_or(Value::Null),
        "wait_ahead": payload.get("wait_ahead").cloned().unwrap_or(Value::Null),
        "priority": queue_task_priority(task),
        "created_at": task.created_at,
    });
    if let Some(client_message_id) = payload.get("client_message_id").cloned() {
        view["client_message_id"] = client_message_id;
    }
    if let Some(attachments) = payload.get("attachments").cloned() {
        if attachments.as_array().map(|items| !items.is_empty()) == Some(true) {
            view["attachments"] = attachments;
        }
    }
    view
}

fn load_parked_queue_tasks(
    store: &UserStore,
    user_id: &str,
    session_id: &str,
) -> Result<Vec<AgentTaskRecord>> {
    let thread_id = format!("thread_{session_id}");
    let mut tasks = store.list_agent_tasks_by_thread(&thread_id, None, QUEUE_LIST_LIMIT)?;
    tasks.retain(|task| {
        task.user_id == user_id && is_parked_queue_status(&task.status) && is_user_visible_queue_task(task)
    });
    sort_queue_tasks_by_dispatch_order(&mut tasks);
    Ok(tasks)
}

impl ThreadRuntime {
    /// 当前会话里等待执行的轮次（用户可见、按派发序）。
    pub async fn list_user_queue_tasks(&self, user_id: &str, session_id: &str) -> Result<Value> {
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        if user_id.is_empty() || session_id.is_empty() {
            return Err(anyhow!(i18n::t("error.session_not_found")));
        }
        let store = self.user_store.clone();
        let tasks = blocking::run_db("queue.user_list", move || {
            load_parked_queue_tasks(&store, &user_id, &session_id)
        })
        .await?;
        let items = tasks
            .iter()
            .enumerate()
            .map(|(index, task)| queue_task_view(task, index))
            .collect::<Vec<_>>();
        Ok(json!({ "items": items, "total": items.len() }))
    }

    async fn load_owned_parked_task(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<AgentTaskRecord> {
        let cleaned_queue = queue_id.trim().to_string();
        if cleaned_queue.is_empty() {
            return Err(anyhow!(i18n::t("error.content_required")));
        }
        let store = self.user_store.clone();
        let user_id = user_id.trim().to_string();
        let session_id = session_id.trim().to_string();
        blocking::run_db("queue.user_ownership", move || {
            let task = store
                .storage_backend()
                .get_agent_task(&cleaned_queue)?
                .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
            if task.user_id != user_id || task.session_id != session_id {
                return Err(anyhow!(i18n::t("error.permission_denied")));
            }
            if !is_parked_queue_status(&task.status) {
                return Err(anyhow!("task is no longer pending"));
            }
            Ok(task)
        })
        .await
    }

    /// 插话：把指定排队轮次提到队首，在当前动作边界优先执行。
    pub async fn prioritize_queued_task(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<Value> {
        let task = self
            .load_owned_parked_task(user_id, session_id, queue_id)
            .await?;
        let store = self.user_store.clone();
        let task_id = task.task_id.clone();
        let now = now_ts();
        let promoted =
            blocking::run_db("queue.user_promote", move || {
                store.storage_backend().promote_agent_task(&task_id, now)
            })
            .await?;
        if !promoted {
            return Err(anyhow!("task is no longer pending"));
        }
        self.emit_queue_event(
            &task.session_id,
            &task.user_id,
            "queue_update",
            json!({
                "queue_id": task.task_id,
                "session_id": task.session_id,
                "queue_priority": 1,
                "reason": "user_interject",
                "client_message_id": task.request_payload.get("client_message_id").cloned().unwrap_or(Value::Null),
            }),
        )
        .await;
        self.wake().await;
        Ok(json!({
            "ok": true,
            "queue_id": task.task_id,
            "priority": 1,
            "resume_policy": "automatic_at_action_boundary",
        }))
    }

    /// 撤下：取消一个还没开跑的排队轮次（编辑前先撤下，再回填输入框）。
    pub async fn cancel_queued_task(
        &self,
        user_id: &str,
        session_id: &str,
        queue_id: &str,
    ) -> Result<Value> {
        let task = self
            .load_owned_parked_task(user_id, session_id, queue_id)
            .await?;
        let store = self.user_store.clone();
        let task_id = task.task_id.clone();
        let retries = task.retry_count;
        let now = now_ts();
        blocking::run_db("queue.user_cancel", move || {
            store.storage_backend().update_agent_task_status(
                UpdateAgentTaskStatusParams {
                    task_id: &task_id,
                    status: TASK_STATUS_CANCELLED,
                    retry_count: retries,
                    retry_at: now,
                    started_at: None,
                    finished_at: Some(now),
                    last_error: Some("user_withdrew_queued_turn"),
                    updated_at: now,
                },
            )
        })
        .await?;
        self.emit_queue_event(
            &task.session_id,
            &task.user_id,
            "queue_cancel",
            json!({
                "queue_id": task.task_id,
                "session_id": task.session_id,
                "queue_status": "cancelled",
                "reason": "user_withdrew",
                "client_message_id": task.request_payload.get("client_message_id").cloned().unwrap_or(Value::Null),
            }),
        )
        .await;
        self.wake().await;
        Ok(json!({"ok": true, "queue_id": task.task_id, "cancelled": true}))
    }

    /// 拖拽排序：按给定的顺序重写派发位次（retry_at），未列出的条目保持原序垫后。
    pub async fn reorder_queued_tasks(
        &self,
        user_id: &str,
        session_id: &str,
        queue_ids: &[String],
    ) -> Result<Value> {
        let cleaned_ids = queue_ids
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if cleaned_ids.is_empty() {
            return Ok(json!({"ok": true, "reordered": 0}));
        }
        let tasks = {
            let store = self.user_store.clone();
            let user_id = user_id.trim().to_string();
            let session_id = session_id.trim().to_string();
            blocking::run_db("queue.user_reorder_list", move || {
                load_parked_queue_tasks(&store, &user_id, &session_id)
            })
            .await?
        };
        let known: HashSet<String> = tasks.iter().map(|task| task.task_id.clone()).collect();
        let mut ordered = cleaned_ids
            .into_iter()
            .filter(|value| known.contains(value))
            .collect::<Vec<_>>();
        // 未列出的条目按原派发序垫后：位次重写必须覆盖整个队列，否则它们会插到中间。
        for task in &tasks {
            if !ordered.contains(&task.task_id) {
                ordered.push(task.task_id.clone());
            }
        }
        if ordered.is_empty() {
            return Ok(json!({"ok": true, "reordered": 0}));
        }
        let store = self.user_store.clone();
        let now = now_ts();
        let reordered = blocking::run_db("queue.user_reorder", move || {
            store
                .storage_backend()
                .reorder_agent_tasks(&ordered, now)
        })
        .await?;
        self.emit_queue_event(
            &tasks[0].session_id,
            &tasks[0].user_id,
            "queue_update",
            json!({
                "queue_id": tasks[0].task_id,
                "session_id": tasks[0].session_id,
                "reason": "user_reorder",
                "reordered": reordered,
            }),
        )
        .await;
        self.wake().await;
        Ok(json!({"ok": true, "reordered": reordered}))
    }
}

#[cfg(all(test, feature = "sqlite-storage"))]
mod tests {
    use super::*;
    use crate::config_store::ConfigStore;
    use crate::state::{AppState, AppStateInitOptions};

    fn park_task(
        state: &AppState,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        content: &str,
        created_at: f64,
        payload_extra: Value,
    ) {
        let mut payload = json!({
            "user_id": user_id,
            "session_id": session_id,
            "question": content,
            "config_overrides": { "__thread_log_turn_id": format!("turn-{task_id}") },
        });
        if let (Some(map), Some(extra)) = (payload.as_object_mut(), payload_extra.as_object()) {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        state
            .storage
            .insert_agent_task(&AgentTaskRecord {
                task_id: task_id.to_string(),
                thread_id: format!("thread_{session_id}"),
                user_id: user_id.to_string(),
                agent_id: "agent-a".to_string(),
                session_id: session_id.to_string(),
                status: TASK_STATUS_PENDING.to_string(),
                request_payload: payload,
                request_id: None,
                retry_count: 0,
                retry_at: created_at,
                created_at,
                updated_at: created_at,
                started_at: None,
                finished_at: None,
                last_error: None,
            })
            .expect("insert task");
    }

    async fn build_state(tag: &str) -> (tempfile::TempDir, Arc<AppState>) {
        let root = tempfile::tempdir().expect("tempdir");
        let mut config = crate::config::Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = root.path().join(format!("{tag}.db")).to_string_lossy().into();
        config.workspace.root = root
            .path()
            .join("workspace")
            .to_string_lossy()
            .into();
        let store = ConfigStore::new(root.path().join("config.yaml"));
        store
            .update(|current| *current = config.clone())
            .await
            .expect("config update");
        let state = AppState::new_with_options(
            store,
            config,
            AppStateInitOptions::cli_default().with_start_thread_runtime(false),
        )
        .expect("state");
        (root, Arc::new(state))
    }

    fn queue_ids(items: &[Value]) -> Vec<String> {
        items
            .iter()
            .map(|item| {
                item.get("queue_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    async fn list_items(state: &Arc<AppState>, user_id: &str, session_id: &str) -> Vec<Value> {
        let data = state
            .kernel
            .thread_runtime
            .list_user_queue_tasks(user_id, session_id)
            .await
            .expect("list queue");
        data.get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn user_queue_lists_own_parked_turns_in_dispatch_order() {
        let (_root, state) = build_state("queue-list").await;
        park_task(&state, "task-a", "user-a", "session-a", "first", 100.0, json!({}));
        park_task(&state, "task-b", "user-a", "session-a", "second", 200.0, json!({}));
        park_task(
            &state,
            "task-other",
            "user-b",
            "session-a",
            "not mine",
            150.0,
            json!({}),
        );
        park_task(
            &state,
            "task-internal",
            "user-a",
            "session-a",
            "mailbox",
            160.0,
            json!({ "config_overrides": { "__agent_message": { "id": "m1", "source": "s" } } }),
        );

        let items = list_items(&state, "user-a", "session-a").await;
        assert_eq!(queue_ids(&items), vec!["task-a", "task-b"]);
        assert_eq!(items[0]["content"], json!("first"));
        assert_eq!(items[0]["position"], json!(0));
        assert_eq!(items[1]["position"], json!(1));
    }

    #[tokio::test]
    async fn interject_moves_one_turn_to_the_front_and_withdraw_drops_it() {
        let (_root, state) = build_state("queue-actions").await;
        park_task(&state, "task-a", "user-a", "session-a", "first", 100.0, json!({}));
        park_task(&state, "task-b", "user-a", "session-a", "second", 200.0, json!({}));

        state
            .kernel
            .thread_runtime
            .prioritize_queued_task("user-a", "session-a", "task-b")
            .await
            .expect("interject");
        let items = list_items(&state, "user-a", "session-a").await;
        assert_eq!(queue_ids(&items), vec!["task-b", "task-a"]);
        assert_eq!(items[0]["priority"], json!(1));

        state
            .kernel
            .thread_runtime
            .cancel_queued_task("user-a", "session-a", "task-a")
            .await
            .expect("withdraw");
        let items = list_items(&state, "user-a", "session-a").await;
        assert_eq!(queue_ids(&items), vec!["task-b"]);
    }

    #[tokio::test]
    async fn queue_actions_reject_other_users_turns() {
        let (_root, state) = build_state("queue-ownership").await;
        park_task(&state, "task-a", "user-b", "session-a", "mine", 100.0, json!({}));
        assert!(state
            .kernel
            .thread_runtime
            .prioritize_queued_task("user-a", "session-a", "task-a")
            .await
            .is_err());
        assert!(state
            .kernel
            .thread_runtime
            .cancel_queued_task("user-a", "session-a", "task-a")
            .await
            .is_err());
        assert!(list_items(&state, "user-a", "session-a").await.is_empty());
    }

    #[tokio::test]
    async fn reorder_rewrites_dispatch_ranks_within_ones_queue() {
        let (_root, state) = build_state("queue-reorder").await;
        park_task(&state, "task-a", "user-a", "session-a", "first", 100.0, json!({}));
        park_task(&state, "task-b", "user-a", "session-a", "second", 200.0, json!({}));
        park_task(&state, "task-c", "user-a", "session-a", "third", 300.0, json!({}));

        let reordered = state
            .kernel
            .thread_runtime
            .reorder_queued_tasks("user-a", "session-a", &["task-c".into(), "task-a".into()])
            .await
            .expect("reorder");
        assert_eq!(reordered["reordered"], json!(2));

        // 重排只改派发位次，created_at 保持不变；列表按派发序返回。
        let items = list_items(&state, "user-a", "session-a").await;
        assert_eq!(queue_ids(&items), vec!["task-c", "task-a", "task-b"]);
        assert_eq!(items[0]["content"], json!("third"));
        assert_eq!(items[2]["content"], json!("second"));
    }
}
