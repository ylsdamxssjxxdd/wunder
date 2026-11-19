use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub(crate) struct SessionListArgs {
    #[serde(default)]
    pub(crate) limit: Option<i64>,
    #[serde(default, rename = "activeMinutes", alias = "active_minutes")]
    pub(crate) active_minutes: Option<f64>,
    #[serde(default, rename = "messageLimit", alias = "message_limit")]
    pub(crate) message_limit: Option<i64>,
    #[serde(
        default,
        alias = "parent_id",
        alias = "parentId",
        alias = "parentSessionId"
    )]
    pub(crate) parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SessionHistoryArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    pub(crate) session_key: Option<String>,
    #[serde(default)]
    pub(crate) limit: Option<i64>,
    #[serde(default, rename = "includeTools", alias = "include_tools")]
    pub(crate) include_tools: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SessionSendArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    pub(crate) session_key: Option<String>,
    pub(crate) message: String,
    #[serde(default, rename = "timeoutSeconds", alias = "timeout_seconds")]
    pub(crate) timeout_seconds: Option<f64>,
    #[serde(
        default,
        rename = "announceParentSessionId",
        alias = "announce_parent_session_id"
    )]
    pub(crate) announce_parent_session_id: Option<String>,
    #[serde(default)]
    pub(crate) label: Option<String>,
    #[serde(
        default,
        rename = "announcePersistHistory",
        alias = "announce_persist_history"
    )]
    pub(crate) announce_persist_history: Option<bool>,
    #[serde(
        default,
        rename = "announceEmitParentEvents",
        alias = "announce_emit_parent_events"
    )]
    pub(crate) announce_emit_parent_events: Option<bool>,
    #[serde(default, rename = "waitForever", alias = "wait_forever")]
    pub(crate) wait_forever: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawSessionSpawnArgs {
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default, rename = "agentName", alias = "agent_name", alias = "name")]
    agent_name: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(
        default,
        rename = "runTimeoutSeconds",
        alias = "run_timeout_seconds",
        alias = "timeoutSeconds",
        alias = "timeout_seconds"
    )]
    run_timeout_seconds: Option<f64>,
    #[serde(default, rename = "runInBackground", alias = "run_in_background")]
    run_in_background: Option<bool>,
    #[serde(default)]
    cleanup: Option<String>,
    #[serde(default, rename = "threadStrategy", alias = "thread_strategy")]
    thread_strategy: Option<String>,
    #[serde(default, rename = "reuseThread", alias = "reuse_thread")]
    reuse_thread: Option<bool>,
}

#[derive(Debug)]
pub(crate) struct SessionSpawnArgs {
    pub(crate) task: String,
    pub(crate) label: Option<String>,
    pub(crate) agent_id: Option<String>,
    pub(crate) agent_name: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) run_timeout_seconds: Option<f64>,
    /// 默认后台：派出后立即返回，完成时自动唤醒父线程。
    /// 只有下一步依赖子任务结果、必须在本轮内拿到时才置 false。
    pub(crate) run_in_background: Option<bool>,
    pub(crate) cleanup: Option<String>,
    pub(crate) thread_strategy: Option<String>,
    pub(crate) reuse_thread: Option<bool>,
}

impl<'de> Deserialize<'de> for SessionSpawnArgs {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawSessionSpawnArgs::deserialize(deserializer)?;
        let task = raw
            .task
            .or(raw.message)
            .or(raw.prompt)
            .ok_or_else(|| serde::de::Error::missing_field("task"))?;
        Ok(Self {
            task,
            label: raw.label,
            agent_id: raw.agent_id,
            agent_name: raw.agent_name,
            model: raw.model,
            run_timeout_seconds: raw.run_timeout_seconds,
            run_in_background: raw.run_in_background,
            cleanup: raw.cleanup,
            thread_strategy: raw.thread_strategy,
            reuse_thread: raw.reuse_thread,
        })
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub(crate) struct SubagentControlArgs {
    pub(crate) action: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn spawn_args_separate_background_from_run_timeout() {
        let args: SessionSpawnArgs = serde_json::from_value(json!({
            "task": "整理目录",
            "run_in_background": false,
            "timeout_seconds": 90.0,
            "fork_turns": 2,
            "context_summary": "已知背景"
        }))
        .unwrap();
        assert_eq!(args.run_in_background, Some(false));
        assert_eq!(args.run_timeout_seconds, Some(90.0));

        let default: SessionSpawnArgs =
            serde_json::from_value(json!({ "task": "整理目录", "runTimeoutSeconds": 30.0 }))
                .unwrap();
        // 默认由 sessions_spawn 解释为后台；这里只确认超时不再暗含等待语义。
        assert_eq!(default.run_in_background, None);
        assert_eq!(default.run_timeout_seconds, Some(30.0));
    }
}
