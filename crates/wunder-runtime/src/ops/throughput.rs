//! Single-request model benchmarks. No users, sessions, prompts or responses are persisted.
use crate::config::{Config, LlmModelConfig};
use crate::llm::{is_llm_configured, is_llm_model, LlmClient};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc, time::Instant};
use tokio::sync::watch;
use uuid::Uuid;

mod history;
#[cfg(test)]
mod tests;

pub const INPUT_PRESETS: [u32; 10] = [
    1024, 2048, 8192, 16384, 32768, 65536, 131072, 262144, 524288, 1048576,
];
pub const OUTPUT_PRESETS: [u32; 4] = [1024, 2048, 4096, 8192];
const HISTORY_LIMIT: usize = 50;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ThroughputConfig {
    pub model_name: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

impl ThroughputConfig {
    pub fn resolve(&self, config: &Config) -> Result<LlmModelConfig, String> {
        if !INPUT_PRESETS.contains(&self.input_tokens)
            || !OUTPUT_PRESETS.contains(&self.output_tokens)
        {
            return Err("请选择支持的输入和输出 Token 档位".into());
        }
        let model = config
            .llm
            .models
            .get(&self.model_name)
            .filter(|model| {
                model.enable != Some(false) && is_llm_model(model) && is_llm_configured(model)
            })
            .ok_or("请选择已启用并配置完整的语言模型")?;
        if model
            .max_context
            .is_some_and(|limit| limit > 0 && self.input_tokens + self.output_tokens > limit)
        {
            return Err("输入与输出 Token 之和超过模型配置的上下文窗口".into());
        }
        Ok(model.clone())
    }
}

pub use crate::llm::benchmark::BenchmarkMetrics as ThroughputMetrics;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThroughputSnapshot {
    pub id: String,
    pub status: String,
    pub config: ThroughputConfig,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_s: f64,
    pub length_control: String,
    #[serde(default)]
    pub simulated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub simulation_speed: Option<wunder_core::virtual_model::VirtualModelSpeed>,
    pub metrics: ThroughputMetrics,
    pub error: Option<String>,
    pub persistence_error: bool,
}

impl ThroughputSnapshot {
    pub fn running(&self) -> bool {
        matches!(self.status.as_str(), "running" | "stopping")
    }
}

#[derive(Clone, Serialize)]
pub struct ThroughputStatusResponse {
    pub schema_version: u8,
    pub active: Option<ThroughputSnapshot>,
    pub history: Vec<ThroughputSnapshot>,
}

pub type ThroughputReport = ThroughputSnapshot;

#[derive(Clone)]
pub struct ThroughputManager {
    inner: Arc<Mutex<ManagerState>>,
    history_path: Arc<PathBuf>,
    http: reqwest::Client,
}

struct ManagerState {
    active: Option<ThroughputSnapshot>,
    started: Option<Instant>,
    cancel: Option<watch::Sender<bool>>,
    history: Vec<ThroughputSnapshot>,
    tickets: Vec<(String, Instant)>,
}

impl Default for ThroughputManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThroughputManager {
    pub fn new() -> Self {
        Self::with_history_path(PathBuf::from("config/data/throughput/scenarios-v2.json"))
    }

    fn with_history_path(path: PathBuf) -> Self {
        crate::rustls_provider::install_process_default_provider();
        Self {
            inner: Arc::new(Mutex::new(ManagerState {
                active: None,
                started: None,
                cancel: None,
                history: history::load(&path),
                tickets: Vec::new(),
            })),
            history_path: Arc::new(path),
            http: reqwest::Client::new(),
        }
    }

    pub async fn start(
        &self,
        config: ThroughputConfig,
        model: LlmModelConfig,
    ) -> Result<ThroughputSnapshot, String> {
        let mut state = self.inner.lock();
        if state.cancel.is_some() {
            return Err("已有运行中或正在保存的测试，请等待完成".into());
        }
        let (cancel, receiver) = watch::channel(false);
        let snapshot = ThroughputSnapshot {
            id: Uuid::new_v4().simple().to_string(),
            status: "running".into(),
            config: config.clone(),
            started_at: Utc::now().to_rfc3339(),
            finished_at: None,
            elapsed_s: 0.0,
            length_control: if crate::llm::benchmark::supports_fixed_output(&model) {
                "fixed"
            } else {
                "best_effort"
            }
            .into(),
            simulated: crate::services::virtual_llm::is_virtual_replay_provider(
                model.provider.as_deref(),
            ),
            simulation_speed: crate::services::virtual_llm::is_virtual_replay_provider(
                model.provider.as_deref(),
            )
            .then(|| model.simulation_speed.unwrap_or_default()),
            metrics: ThroughputMetrics::default(),
            error: None,
            persistence_error: false,
        };
        state.active = Some(snapshot.clone());
        state.started = Some(Instant::now());
        state.cancel = Some(cancel);
        let manager = self.clone();
        tokio::spawn(async move {
            manager.execute(config, model, receiver).await;
        });
        Ok(snapshot)
    }

    pub fn issue_ticket(&self) -> String {
        let mut state = self.inner.lock();
        state
            .tickets
            .retain(|(_, time)| time.elapsed().as_secs() < 30);
        if state.tickets.len() >= 64 {
            state.tickets.remove(0);
        }
        let ticket = Uuid::new_v4().simple().to_string();
        state.tickets.push((ticket.clone(), Instant::now()));
        ticket
    }

    pub fn consume_ticket(&self, ticket: &str) -> bool {
        let mut state = self.inner.lock();
        state
            .tickets
            .retain(|(_, time)| time.elapsed().as_secs() < 30);
        if let Some(index) = state.tickets.iter().position(|(value, _)| value == ticket) {
            state.tickets.swap_remove(index);
            true
        } else {
            false
        }
    }

    pub async fn stop(&self) -> Result<ThroughputSnapshot, String> {
        let mut state = self.inner.lock();
        let Some(cancel) = state.cancel.as_ref() else {
            return Err("当前没有运行中的测试".into());
        };
        let _ = cancel.send(true);
        let active = state.active.as_mut().ok_or("当前没有运行中的测试")?;
        if active.running() {
            active.status = "stopping".into();
        }
        Ok(active.clone())
    }

    pub async fn status(&self) -> ThroughputStatusResponse {
        let state = self.inner.lock();
        let mut active = state.active.clone();
        if let Some(active) = active.as_mut().filter(|item| item.running()) {
            active.elapsed_s = state
                .started
                .map(|time| time.elapsed().as_secs_f64())
                .unwrap_or_default();
        }
        ThroughputStatusResponse {
            schema_version: 2,
            active,
            history: state.history.clone(),
        }
    }

    pub async fn report(&self, id: Option<&str>) -> Result<ThroughputReport, String> {
        let state = self.inner.lock();
        state
            .active
            .as_ref()
            .filter(|item| id.is_none_or(|id| item.id == id))
            .or_else(|| {
                state
                    .history
                    .iter()
                    .rev()
                    .find(|item| id.is_none_or(|id| item.id == id))
            })
            .cloned()
            .ok_or_else(|| "测试记录不存在或已超出保留数量".into())
    }

    async fn execute(
        &self,
        config: ThroughputConfig,
        model: LlmModelConfig,
        mut cancel: watch::Receiver<bool>,
    ) {
        // Build large synthetic inputs off the async executor; never create a thread runtime.
        let input_tokens = config.input_tokens;
        let messages =
            tokio::task::spawn_blocking(move || crate::llm::benchmark::messages(input_tokens))
                .await;
        let started = Instant::now();
        let timeout =
            std::time::Duration::from_secs(model.timeout_s.unwrap_or(1800).clamp(1, 3600));
        let client = LlmClient::new(self.http.clone(), model);
        let result = match messages {
            Ok(messages) => {
                let request = client.benchmark(&messages, config.output_tokens, |metrics| {
                    if let Some(active) = self.inner.lock().active.as_mut() {
                        active.metrics = metrics;
                    }
                });
                tokio::select! {
                    biased;
                    _ = cancel.wait_for(|value| *value) => Err("stopped".to_string()),
                    result = tokio::time::timeout(timeout, request) => result.unwrap_or_else(|_| Err("模型请求超时".into())),
                }
            }
            Err(_) => Err("无法生成测试输入".into()),
        };
        let history = {
            let mut state = self.inner.lock();
            let Some(active) = state.active.as_mut() else {
                return;
            };
            active.elapsed_s = started.elapsed().as_secs_f64();
            active.finished_at = Some(Utc::now().to_rfc3339());
            match result {
                Ok(metrics) => {
                    active.status = if metrics.target_reached == Some(true) {
                        "finished"
                    } else {
                        "incomplete"
                    }
                    .into();
                    active.metrics = metrics;
                }
                Err(error) if error == "stopped" => {
                    active.status = "stopped".into();
                }
                Err(error) => {
                    active.status = "error".into();
                    active.error = Some(error);
                }
            }
            let snapshot = active.clone();
            state.history.push(snapshot);
            let overflow = state.history.len().saturating_sub(HISTORY_LIMIT);
            state.history.drain(..overflow);
            state.history.clone()
        };
        // Keep the start gate until persistence completes so older writes cannot win.
        let failed = history::save(&self.history_path, &history).await.is_err();
        let mut state = self.inner.lock();
        if let Some(active) = state.active.as_mut() {
            active.persistence_error = failed;
        }
        if let Some(last) = state.history.last_mut() {
            last.persistence_error = failed;
        }
        state.cancel = None;
    }
}
