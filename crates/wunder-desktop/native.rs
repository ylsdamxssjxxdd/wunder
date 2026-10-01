//! In-process desktop façade for native UI frontends.
//!
//! This layer deliberately uses the same AppState, ThreadRuntime and
//! Orchestrator as the server. It only translates typed UI requests to runtime
//! calls; it does not duplicate scheduling or permission rules.

use crate::{args::DesktopArgs, runtime::DesktopRuntime};
use anyhow::{anyhow, Result};
use bytes::Bytes;
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::runtime::Runtime;
#[path = "native_stream.rs"]
mod stream;
pub use stream::{NativeChatEvent, NativeStream};
#[path = "native_catalog.rs"]
mod catalog;
#[path = "native_cron.rs"]
mod cron;
#[path = "native_profile.rs"]
mod profile;
#[path = "native_prompts.rs"]
mod prompts;
#[path = "native_settings.rs"]
mod settings;
#[path = "native_workspace.rs"]
mod workspace;
#[path = "native_world.rs"]
mod world;
pub use catalog::{
    AgentImportOutcome, AgentRecord, AgentSettingsEdit, ToolRecord, WORKER_CARD_SCHEMA_VERSION,
};
pub use cron::{CronRecord, CronRunRecord, NativeCronJobEdit};
pub use profile::NativeProfile;
pub use prompts::{PromptPackInfo, PromptSegmentContent};
pub use settings::{
    DesktopSettings, LanPeerRecord, LanSettings, ModelEdit, ModelProbeOutcome, ModelRecord,
};
pub use workspace::{Directory, FileRecord};
pub use world::{
    WorldContact, WorldEventFeed, WorldGroup, WorldGroupDetail, WorldGroupMember, WorldMessage,
    WorldMessageTracker,
};

#[derive(Debug, Clone)]
pub struct NativeChatInput {
    pub session_id: String,
    pub content: String,
    pub client_message_id: Option<String>,
    /// Local UI attachments retain their original data URL until the shared
    /// runtime persists them into the workspace attachment store.
    pub attachments: Vec<NativeChatAttachment>,
}

#[derive(Debug, Clone)]
pub struct NativeChatAttachment {
    pub name: String,
    pub content: String,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub struct NativeSession {
    pub id: String,
    pub title: String,
    pub updated_at: f64,
    pub agent_id: Option<String>,
    pub consumed_tokens: i64,
    pub tool_calls: i64,
    pub model_request_count: i64,
    pub quota_used: i64,
    pub runtime_status: String,
    pub locked: bool,
}

#[derive(Debug, Clone)]
pub struct NativeMessage {
    pub text: String,
    pub mine: bool,
    pub created_at: f64,
    pub state: String,
    pub stats_status: String,
    pub stats_duration: String,
    pub stats_speed: String,
    pub stats_context: String,
    pub stats_quota: String,
    pub stats_tools: String,
    pub stats_credits: String,
}

pub struct NativeDesktop {
    runtime: Arc<Runtime>,
    desktop: DesktopRuntime,
    settings_lock: std::sync::Mutex<()>,
}

impl NativeDesktop {
    pub fn start() -> Result<Self> {
        Self::start_with_args(DesktopArgs::native_defaults())
    }

    pub fn start_with_args(mut args: DesktopArgs) -> Result<Self> {
        wunder_server::rustls_provider::install_process_default_provider();
        args.native_runtime = true;
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(8)
                .enable_all()
                .build()?,
        );
        let desktop = runtime.block_on(DesktopRuntime::init(&args))?;
        Ok(Self {
            runtime,
            desktop,
            settings_lock: std::sync::Mutex::new(()),
        })
    }

    pub fn user_id(&self) -> &str {
        &self.desktop.user_id
    }

    pub fn state(&self) -> &Arc<wunder_server::state::AppState> {
        &self.desktop.state
    }

    pub fn list_sessions(&self) -> Result<Vec<NativeSession>> {
        let (records, _) = self.desktop.state.storage.list_work_chat_sessions(
            &self.desktop.user_id,
            None,
            Some("active"),
            0,
            100,
        )?;
        Ok(records
            .into_iter()
            .map(|record| self.session_with_stats(record))
            .collect())
    }

    fn session_with_stats(
        &self,
        record: wunder_server::storage::ChatSessionRecord,
    ) -> NativeSession {
        let overview = self
            .desktop
            .state
            .monitor
            .get_log_overview(&record.session_id);
        NativeSession {
            id: record.session_id,
            title: record.title,
            updated_at: record.updated_at,
            agent_id: record.agent_id,
            consumed_tokens: overview
                .as_ref()
                .and_then(|v| v.get("consumed_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            tool_calls: overview
                .as_ref()
                .and_then(|v| v.get("tool_calls"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            model_request_count: overview
                .as_ref()
                .and_then(|v| v.get("model_request_count"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            quota_used: overview
                .as_ref()
                .and_then(|v| v.get("quota_used").or_else(|| v.get("model_request_count")))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            runtime_status: overview
                .as_ref()
                .and_then(|v| v.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("done")
                .to_string(),
            locked: false,
        }
    }

    pub fn get_session(&self, session_id: &str) -> Result<(NativeSession, Vec<NativeMessage>)> {
        let cleaned = session_id.trim();
        let record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, cleaned)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        let history =
            self.desktop
                .state
                .workspace
                .load_history(&self.desktop.user_id, cleaned, 100)?;
        let messages = history.into_iter().filter_map(message_from_value).collect();
        Ok((self.session_with_stats(record), messages))
    }

    /// Recover durable changes from change_seq=0 to the latest cursor.
    ///
    /// Bounded replay: reads durable frames in pages of 200, up to 8 pages.
    /// Snapshot guard: if the durable window was trimmed, returns empty vec
    /// so the frontend can reload an atomic snapshot. I6 compliance: bounded
    /// loop, no unbounded reads.
    pub fn recover_session_durable_changes(&self, session_id: &str) -> Result<Vec<Value>> {
        let cleaned = session_id.trim();
        let workspace = self.desktop.state.workspace.clone();
        let target = cleaned.to_string();
        let mut cursor: i64 = 0;
        let mut frames: Vec<Value> = Vec::new();
        const MAX_PAGES: i64 = 8;
        const PAGE_SIZE: i64 = 200;
        for _ in 0..MAX_PAGES {
            let workspace = workspace.clone();
            let target = target.clone();
            let page = self.runtime.block_on(wunder_server::blocking::run_fs(
                "desktop.recover_changes",
                move || workspace.try_load_thread_changes(&target, cursor, PAGE_SIZE),
            ));
            let records = match page {
                Ok(records) => records,
                Err(_) => break,
            };
            if records.is_empty() {
                break;
            }
            if records.iter().any(|record| {
                record.get("event").and_then(Value::as_str) == Some("thread_snapshot_required")
            }) {
                return Ok(Vec::new());
            }
            let mut progressed = false;
            for record in &records {
                let frame_cursor = record
                    .get("data")
                    .and_then(|data| data.get("cursor"))
                    .and_then(Value::as_i64)
                    .unwrap_or(cursor);
                if frame_cursor > cursor {
                    cursor = frame_cursor;
                    progressed = true;
                }
            }
            let page_len = records.len();
            frames.extend(records);
            if page_len < PAGE_SIZE as usize || !progressed {
                break;
            }
        }
        Ok(frames)
    }

    pub fn create_session_for_agent(&self, agent_id: Option<&str>) -> Result<NativeSession> {
        let agent_id = agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "__default__");
        self.runtime
            .block_on(wunder_server::agent_management::owned(
                self.state(),
                self.user_id(),
                agent_id.unwrap_or("__default__"),
            ))?;
        let now = now_ts();
        let id = format!("native_{}", uuid::Uuid::new_v4().simple());
        let record = wunder_server::storage::ChatSessionRecord {
            session_id: id,
            user_id: self.desktop.user_id.clone(),
            title: "新会话".to_string(),
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
            last_message_at: now,
            agent_id: agent_id.map(ToString::to_string),
            tool_overrides: Vec::new(),
            parent_session_id: None,
            parent_message_id: None,
            spawn_label: None,
            spawned_by: None,
        };
        self.desktop.state.user_store.upsert_chat_session(&record)?;
        Ok(self.session_with_stats(record))
    }

    pub fn rename_session(&self, session_id: &str, title: &str) -> Result<()> {
        let title = title.trim();
        if title.is_empty() || title.chars().count() > 120 || title.chars().any(char::is_control) {
            return Err(anyhow!("线程名称无效"));
        }
        self.desktop.state.user_store.update_chat_session_title(
            &self.desktop.user_id,
            session_id.trim(),
            title,
            now_ts(),
        )
    }

    pub fn archive_session(&self, session_id: &str) -> Result<()> {
        let mut record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, session_id.trim())?
            .ok_or_else(|| anyhow!("线程不存在"))?;
        record.status = "archived".into();
        record.updated_at = now_ts();
        self.desktop.state.user_store.upsert_chat_session(&record)
    }

    pub fn session_detail_page(
        &self,
        session_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        self.desktop
            .state
            .monitor
            .get_detail_page(session_id.trim(), offset, limit)
            .ok_or_else(|| anyhow!("线程日志不存在"))
    }

    pub fn cancel_chat(&self, session_id: &str) -> Result<()> {
        self.runtime.block_on(stream::cancel_chat(
            &self.desktop.state,
            &self.desktop.user_id,
            session_id,
        ))
    }

    pub fn send_chat(&self, input: NativeChatInput) -> Result<NativeStream> {
        if input.session_id.trim().is_empty()
            || (input.content.trim().is_empty() && input.attachments.is_empty())
        {
            return Err(anyhow!(
                "chat session and content or attachment are required"
            ));
        }
        Ok(stream::start(
            &self.runtime,
            self.desktop.state.clone(),
            self.desktop.user_id.clone(),
            input,
        ))
    }

    /// Returns whether the desktop runtime has a configured default ASR model.
    /// The check is deliberately synchronous at this façade boundary so UI code
    /// can reject an empty recording before starting device capture.
    pub fn has_asr_model(&self) -> bool {
        let config = self.runtime.block_on(self.state().config_store.get());
        wunder_server::multimodal_models::resolve_asr_model(&config, None).is_some()
    }

    /// Transcribe an in-memory recording through the shared multimodal runtime.
    /// No local HTTP or bridge transport is involved; the model provider call,
    /// when configured remotely, remains inside the existing runtime service.
    pub fn transcribe_audio(
        &self,
        filename: String,
        content_type: String,
        audio_bytes: Vec<u8>,
    ) -> Result<String> {
        if audio_bytes.is_empty() {
            return Err(anyhow!("录音为空"));
        }
        let config = self.runtime.block_on(self.state().config_store.get());
        let response =
            self.runtime
                .block_on(wunder_server::multimodal_models::transcribe_audio(
                    &config,
                    wunder_server::multimodal_models::AudioTranscriptionRequest {
                        audio_bytes: Bytes::from(audio_bytes),
                        filename,
                        content_type,
                        model_name: None,
                        language: None,
                        prompt: None,
                        response_format: None,
                        temperature: None,
                    },
                ))?;
        let text = response.text.trim().to_string();
        if text.is_empty() {
            return Err(anyhow!("语音识别没有返回文字"));
        }
        Ok(text)
    }
}

fn message_from_value(value: Value) -> Option<NativeMessage> {
    if value.pointer("/meta/type").and_then(Value::as_str) == Some("model_context_internal") {
        return None;
    }
    let role = value.get("role").and_then(Value::as_str)?;
    if role != "user" && role != "assistant" {
        return None;
    }
    // Desktop history reads the storage record directly (rather than the web
    // transcript mapper), so accept all persisted locations used by older
    // records and by the current message_stats metadata.
    let stats = value
        .get("stats")
        .or_else(|| value.get("message_stats"))
        .or_else(|| value.pointer("/meta/message_stats"))
        .unwrap_or(&Value::Null);
    Some(NativeMessage {
        text: value.get("content").and_then(Value::as_str)?.to_string(),
        mine: role == "user",
        created_at: value
            .get("created_at")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
        state: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        stats_status: if role == "assistant" {
            value
                .get("status")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("任务完成")
                .to_string()
        } else {
            String::new()
        },
        stats_duration: format_duration(stats_value(
            stats,
            &["interaction_duration_s", "duration_s", "elapsed_s"],
        )),
        stats_speed: format_speed(stats_value(
            stats,
            &["visible_decode_speed_tps", "decode_speed_tps"],
        )),
        stats_context: format_count(stats_value(
            stats,
            &[
                "contextTokens",
                "context_occupancy_tokens",
                "context_tokens",
            ],
        ))
        .unwrap_or_default(),
        stats_quota: format_count(stats_value(
            stats,
            &["request_consumed_tokens", "consumed_tokens"],
        ))
        .or_else(|| {
            format_count(
                stats
                    .get("round_usage")
                    .and_then(|value| stats_value(value, &["total_tokens", "total"])),
            )
        })
        .unwrap_or_default(),
        stats_tools: format_count(stats_value(stats, &["toolCalls", "tool_calls"]))
            .unwrap_or_default(),
        stats_credits: format_count(stats_value(
            stats,
            &["account_credits_consumed", "creditsConsumed"],
        ))
        .unwrap_or_default(),
    })
}

fn stats_value<'a>(stats: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| stats.get(*key))
}

fn format_count(value: Option<&Value>) -> Option<String> {
    let number = value.and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_f64().map(|value| value as i64))
    })?;
    (number >= 0).then(|| {
        if number >= 1_000_000 {
            format!("{:.1}m", number as f64 / 1_000_000.0)
        } else if number >= 1_000 {
            format!("{:.1}k", number as f64 / 1_000.0)
        } else {
            number.to_string()
        }
    })
}

fn format_duration(value: Option<&Value>) -> String {
    let Some(seconds) = value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return String::new();
    };
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m {:.0}s", (seconds / 60.0).floor(), seconds % 60.0)
    }
}

fn format_speed(value: Option<&Value>) -> String {
    let Some(speed) = value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
    else {
        return String::new();
    };
    format!("{speed:.1}/s")
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or_default()
}
