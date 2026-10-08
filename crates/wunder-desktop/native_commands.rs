//! Typed slash commands. These never enter the model's ordinary user-message path.
use super::NativeDesktop;
use anyhow::{anyhow, Result};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeChatCommand {
    Compact,
    GoalShow,
    GoalSet(String),
    GoalResume,
    GoalPause,
    GoalClear,
    Help,
    Stop,
    New,
}
impl NativeChatCommand {
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().strip_prefix('/')?.trim_start_matches('/');
        let split = text.find(char::is_whitespace).unwrap_or(text.len());
        let (name, args) = text.split_at(split);
        let args = args.trim();
        Some(match name.to_ascii_lowercase().as_str() {
            "compact" => Self::Compact,
            "goal" => match args
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "" => Self::GoalShow,
                "resume" => Self::GoalResume,
                "pause" => Self::GoalPause,
                "clear" => Self::GoalClear,
                _ => Self::GoalSet(args.into()),
            },
            "help" | "?" => Self::Help,
            "stop" | "cancel" => Self::Stop,
            "new" | "reset" => Self::New,
            _ => return None,
        })
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeGoal {
    pub objective: String,
    /// active | paused | blocked | complete
    pub phase: String,
}
impl NativeGoal {
    pub fn active(&self) -> bool {
        self.phase == "active"
    }
    pub fn paused(&self) -> bool {
        self.phase == "paused"
    }
}
impl NativeDesktop {
    /// Called on a worker. Acceptance returns quickly; execution is observed through watch_chat.
    pub fn execute_chat_command(
        &self,
        session: &str,
        command: NativeChatCommand,
    ) -> Result<String> {
        self.runtime.block_on(async {
            let store = self.state().user_store.clone();
            let owner = self.user_id().to_owned();
            let user = wunder_server::blocking::run_db("native.command.user", move || {
                store
                    .get_user_by_id(&owner)?
                    .ok_or_else(|| anyhow!("用户不存在"))
            })
            .await?;
            let command = match command {
                NativeChatCommand::Compact => {
                    wunder_server::api::chat::compact_native_session(self.state(), &user, session)
                        .await?;
                    return Ok("正在压缩上下文…".into());
                }
                NativeChatCommand::GoalShow => wunder_server::goal::GoalCommand::Show,
                NativeChatCommand::GoalSet(objective) => {
                    wunder_server::goal::GoalCommand::Create { objective }
                }
                NativeChatCommand::GoalResume => wunder_server::goal::GoalCommand::Resume,
                NativeChatCommand::GoalPause => wunder_server::goal::GoalCommand::Pause,
                NativeChatCommand::GoalClear => wunder_server::goal::GoalCommand::Clear,
                _ => return Err(anyhow!("此命令应由界面处理")),
            };
            let state = self.state();
            let service = state.kernel.orchestrator.goal_handle();
            let (_, goal_value) = wunder_server::goal::execute_goal_command(
                service.as_ref(),
                state.storage.clone(),
                &user.user_id,
                session,
                command,
            )
            .await?;
            Ok(match goal_value {
                Some(goal) => {
                    let phase = goal.get("phase").and_then(Value::as_str).unwrap_or("");
                    let objective = goal.get("objective").and_then(Value::as_str).unwrap_or("");
                    let phase_label = match phase {
                        "active" => "执行中",
                        "complete" => "已完成",
                        "paused" => "已暂停",
                        "blocked" => "已阻塞",
                        _ => phase,
                    };
                    format!("目标：{objective} · {phase_label}")
                }
                None => "当前会话没有目标，输入 /goal 加目标内容即可开始".into(),
            })
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commands_match_web_and_preserve_objective() {
        assert_eq!(
            NativeChatCommand::parse(" /GOAL example objective "),
            Some(NativeChatCommand::GoalSet("example objective".into()))
        );
        assert_eq!(
            NativeChatCommand::parse("/goal resume"),
            Some(NativeChatCommand::GoalResume)
        );
        assert_eq!(
            NativeChatCommand::parse("/goal clear"),
            Some(NativeChatCommand::GoalClear)
        );
        assert_eq!(
            NativeChatCommand::parse("/goal"),
            Some(NativeChatCommand::GoalShow)
        );
        assert_eq!(
            NativeChatCommand::parse("//compact"),
            Some(NativeChatCommand::Compact)
        );
        assert_eq!(NativeChatCommand::parse("/compactness"), None);
        assert_eq!(NativeChatCommand::parse("example /goal"), None);
    }

    #[tokio::test]
    async fn native_commands_enforce_owner_and_persist_command_turns() {
        use std::sync::Arc;
        use wunder_server::{
            config::Config,
            config_store::ConfigStore,
            state::{AppState, AppStateInitOptions},
        };
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = temp
            .path()
            .join("fixture.db")
            .to_string_lossy()
            .into_owned();
        config.workspace.root = temp.path().join("workspace").to_string_lossy().into_owned();
        config.skills.enabled.clear();
        std::fs::write(temp.path().join("fixture.yaml"), "{}\n").unwrap();
        let store = ConfigStore::new(temp.path().join("fixture.yaml"));
        store.update(|value| *value = config.clone()).await.unwrap();
        let state = Arc::new(
            AppState::new_with_options(
                store,
                config,
                AppStateInitOptions::cli_default().with_start_thread_runtime(false),
            )
            .unwrap(),
        );
        let user = wunder_server::storage::UserAccountRecord {
            user_id: "fixture-owner".into(),
            username: "fixture-user".into(),
            email: None,
            password_hash: String::new(),
            roles: vec![],
            status: "active".into(),
            access_level: "all".into(),
            unit_id: None,
            quota_balance: 0,
            quota_granted_total: 0,
            quota_used_total: 0,
            last_quota_grant_date: None,
            experience_total: 0,
            is_demo: false,
            created_at: 0.0,
            updated_at: 0.0,
            last_login_at: None,
        };
        state.storage.upsert_user_account(&user).unwrap();
        wunder_server::goal::ensure_session(
            state.storage.clone(),
            &user.user_id,
            "fixture-thread",
            None,
        )
        .await
        .unwrap();
        let api = wunder_server::api::chat_goal::native_session_goal;
        assert!(api(&state, &user, "missing-thread", GoalCommand::Show)
            .await
            .is_err());
        assert!(api(&state, &user, "fixture-thread", GoalCommand::Show)
            .await
            .unwrap()
            .is_none());
        assert!(api(&state, &user, "fixture-thread", GoalCommand::Resume)
            .await
            .is_err());
        let goal = api(
            &state,
            &user,
            "fixture-thread",
            GoalCommand::Set {
                objective: "Fixture objective".into(),
                token_budget: None,
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(goal.objective, "Fixture objective");
        assert_eq!(goal.approval_mode.as_deref(), Some("full_auto"));
        assert!(goal.user_round.is_some());
        let turns =
            super::super::chat_turns::load_turns(&state, &user.user_id, "fixture-thread").unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user.text, "/goal Fixture objective");
        assert!(api(&state, &user, "fixture-thread", GoalCommand::Clear)
            .await
            .is_err());
        super::super::stream::cancel_chat(&state, &user.user_id, "fixture-thread")
            .await
            .unwrap();
        assert!(api(&state, &user, "fixture-thread", GoalCommand::Show)
            .await
            .unwrap()
            .is_none());
        assert!(
            wunder_server::api::chat::compact_native_session(&state, &user, "missing-thread")
                .await
                .is_err()
        );
        wunder_server::goal::ensure_session(
            state.storage.clone(),
            &user.user_id,
            "fixture-compact",
            None,
        )
        .await
        .unwrap();
        let accepted =
            wunder_server::api::chat::compact_native_session(&state, &user, "fixture-compact")
                .await
                .unwrap();
        assert_eq!(accepted["data"]["accepted"], true);
        let turns =
            super::super::chat_turns::load_turns(&state, &user.user_id, "fixture-compact").unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].user.text, "/compact");
        super::super::stream::cancel_chat(&state, &user.user_id, "fixture-compact")
            .await
            .unwrap();
    }
}
