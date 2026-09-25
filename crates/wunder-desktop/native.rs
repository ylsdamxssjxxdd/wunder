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
#[path = "native_profile.rs"]
mod profile;
#[path = "native_settings.rs"]
mod settings;
#[path = "native_workspace.rs"]
mod workspace;
pub use catalog::{AgentRecord, ToolRecord};
pub use profile::NativeProfile;
pub use settings::{DesktopSettings, LanPeerRecord, LanSettings, ModelEdit, ModelRecord};
pub use workspace::{Directory, FileRecord};

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
}

#[derive(Debug, Clone)]
pub struct NativeMessage {
    pub text: String,
    pub mine: bool,
    pub created_at: f64,
    pub state: String,
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
        Ok(records.into_iter().map(session_from_record).collect())
    }

    pub fn list_subagents(&self, session_id: &str) -> std::result::Result<Value, String> {
        let items = wunder_server::list_parent_subagents(
            self.desktop.state.storage.as_ref(),
            Some(self.desktop.state.monitor.as_ref()),
            &self.desktop.user_id,
            session_id,
            Some(200),
        )
        .map_err(|error| error.to_string())?;
        Ok(serde_json::json!({"data": {"items": items}}))
    }

    pub fn get_session(&self, session_id: &str) -> Result<(NativeSession, Vec<NativeMessage>)> {
        let cleaned = session_id.trim();
        let record = self
            .desktop
            .state
            .user_store
            .get_chat_session(&self.desktop.user_id, cleaned)?
            .ok_or_else(|| anyhow!("chat session not found"))?;
        let history = self.desktop.state.workspace.load_history_page(
            &self.desktop.user_id,
            cleaned,
            None,
            100,
        )?;
        let messages = history.into_iter().filter_map(message_from_value).collect();
        Ok((session_from_record(record), messages))
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
        Ok(session_from_record(record))
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

fn session_from_record(record: wunder_server::storage::ChatSessionRecord) -> NativeSession {
    NativeSession {
        id: record.session_id,
        title: record.title,
        updated_at: record.updated_at,
        agent_id: record.agent_id,
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
    })
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs_f64())
        .unwrap_or_default()
}
