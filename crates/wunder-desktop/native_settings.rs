//! Serialized, secret-free projections of desktop configuration.
use super::NativeDesktop;
use crate::runtime::{
    load_desktop_settings, normalize_desktop_container_roots, runtime_tool_statuses,
    save_desktop_settings, RuntimeToolStatus,
};
use anyhow::{anyhow, bail, Result};
use wunder_server::config::{Config, LlmConfig};

#[derive(Clone, Debug)]
pub struct ModelRecord {
    pub key: String,
    pub provider: String,
    pub model: String,
    pub base_url: String,
    pub model_type: String,
    pub is_default: bool,
}

#[derive(Clone, Debug)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub language: String,
    pub theme: String,
    pub send_key: String,
    pub python_path: String,
    pub git_path: String,
    pub rg_path: String,
    pub tool_status: Vec<RuntimeToolStatus>,
    pub models: Vec<ModelRecord>,
    pub lan: LanSettings,
}

#[derive(Clone, Debug)]
pub struct LanSettings {
    pub enabled: bool,
    pub peer_id: String,
    pub display_name: String,
    pub listen_host: String,
    pub listen_port: u16,
    pub discovery_port: u16,
    pub peer_count: usize,
    pub peers: Vec<LanPeerRecord>,
}

#[derive(Clone, Debug)]
pub struct LanPeerRecord {
    pub peer_id: String,
    pub display_name: String,
    pub lan_ip: String,
    pub listen_port: u16,
}

pub struct ModelEdit<'a> {
    pub key: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model_type: &'a str,
}

impl NativeDesktop {
    /// Switches the selected agent to one of the ten local workspace roots.
    /// A path is accepted directly so desktop users do not need a platform
    /// specific folder picker.
    pub fn save_workspace_binding(
        &self,
        agent_id: &str,
        container_id: i32,
        path: &str,
    ) -> Result<()> {
        let container_id = wunder_server::storage::normalize_sandbox_container_id(container_id);
        let path = std::path::Path::new(path.trim());
        if !path.is_absolute() || path.to_string_lossy().chars().any(char::is_control) {
            bail!("工作目录必须是绝对路径");
        }
        std::fs::create_dir_all(path)?;
        let canonical = path.canonicalize()?;
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let mut settings = load_desktop_settings(&self.desktop.settings_path)?;
        settings
            .container_roots
            .insert(container_id, canonical.to_string_lossy().into_owned());
        save_desktop_settings(&self.desktop.settings_path, &settings)?;
        let mut record = self
            .runtime
            .block_on(wunder_server::agent_management::owned(
                self.state(),
                self.user_id(),
                agent_id,
            ))?;
        record.sandbox_container_id = container_id;
        if record.agent_id == "__default__" {
            let config =
                wunder_server::default_agent_protocol::default_agent_config_from_record(&record);
            self.state().user_store.set_meta(
                &wunder_server::default_agent_protocol::default_agent_meta_key(self.user_id()),
                &serde_json::to_string(&config)?,
            )?;
        } else {
            self.state().user_store.upsert_user_agent(&record)?;
        }
        self.state()
            .workspace
            .set_container_roots(settings.container_roots.clone());
        let roots = settings.container_roots.clone();
        self.runtime
            .block_on(self.state().config_store.update(|config| {
                config.workspace.container_roots = roots.clone();
            }))?;
        Ok(())
    }

    pub fn get_desktop_settings(&self) -> Result<DesktopSettings> {
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let config = self.runtime.block_on(self.state().config_store.get());
        let lan = self.read_lan_settings();
        let persisted = load_desktop_settings(&self.desktop.settings_path).unwrap_or_default();
        Ok(project(
            &config,
            lan,
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
            &self.desktop.app_dir,
        ))
    }

    /// Persisted UI preferences: theme and composer send key. Only "light" is
    /// rendered today; the value is stored so future themes need no migration.
    pub fn save_preferences(&self, theme: &str, send_key: &str) -> Result<DesktopSettings> {
        let theme = theme.trim();
        let send_key = send_key.trim();
        if !matches!(theme, "light") {
            bail!("暂不支持该主题");
        }
        if !matches!(send_key, "enter" | "ctrl_enter") {
            bail!("发送键设置无效");
        }
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let mut settings = load_desktop_settings(&self.desktop.settings_path)?;
        settings.theme = theme.to_string();
        settings.send_key = send_key.to_string();
        settings.updated_at = super::now_ts();
        save_desktop_settings(&self.desktop.settings_path, &settings)?;
        self.get_desktop_settings()
    }

    /// Reset volatile work state (queues, running turns, temporary projections)
    /// through the shared runtime service. Assets, configuration, files and
    /// history are preserved by definition of the service.
    pub fn reset_work_state(&self) -> Result<wunder_server::ResetWorkStateSummary> {
        let user = self.user_id().to_string();
        self.runtime.block_on(async {
            wunder_server::reset_user_work_state(self.state(), &user, "desktop_reset_work_state")
                .await
        })
    }

    /// Export a secret-free diagnostics bundle (settings, counts, versions) into
    /// `directory` and return the file path. Mirrors the secret-free projection:
    /// no API keys, tokens or absolute remote endpoints are included.
    pub fn export_diagnostics(&self, directory: &std::path::Path) -> Result<std::path::PathBuf> {
        let settings = self.get_desktop_settings()?;
        let agents = self.list_agents().map(|items| items.len()).unwrap_or(0);
        let sessions = self.list_sessions().map(|items| items.len()).unwrap_or(0);
        let cron_jobs = self.list_cron_jobs().map(|items| items.len()).unwrap_or(0);
        let models = settings
            .models
            .iter()
            .map(|model| {
                serde_json::json!({
                    "key": model.key,
                    "provider": model.provider,
                    "model": model.model,
                    "model_type": model.model_type,
                    "is_default": model.is_default,
                })
            })
            .collect::<Vec<_>>();
        let bundle = serde_json::json!({
            "kind": "wunder-desktop-diagnostics",
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "language": settings.language,
            "theme": settings.theme,
            "send_key": settings.send_key,
            "counts": {
                "agents": agents,
                "active_sessions": sessions,
                "cron_jobs": cron_jobs,
                "models": models.len(),
            },
            "models": models,
            "lan": {
                "enabled": settings.lan.enabled,
                "peer_id": settings.lan.peer_id,
                "peer_count": settings.lan.peer_count,
            },
        });
        std::fs::create_dir_all(directory)?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = directory.join(format!("diagnostics-{stamp}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&bundle)?)?;
        Ok(path)
    }

    pub fn save_lan(&self, enabled: bool, display_name: &str) -> Result<DesktopSettings> {
        if display_name.chars().any(char::is_control) || display_name.chars().count() > 80 {
            bail!("内网名称无效");
        }
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let mut settings = load_desktop_settings(&self.desktop.settings_path)?;
        settings.lan_mesh.enabled = enabled;
        settings.lan_mesh.display_name = display_name.trim().to_string();
        settings.lan_mesh = settings.lan_mesh.normalized();
        save_desktop_settings(&self.desktop.settings_path, &settings)?;
        self.runtime.block_on(
            wunder_server::desktop_lan::manager().apply_settings(settings.lan_mesh.clone()),
        );
        let config = self.runtime.block_on(self.state().config_store.get());
        let persisted = load_desktop_settings(&self.desktop.settings_path).unwrap_or_default();
        Ok(project(
            &config,
            self.read_lan_settings(),
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
            &self.desktop.app_dir,
        ))
    }

    fn read_lan_settings(&self) -> LanSettings {
        self.runtime.block_on(async {
            let manager = wunder_server::desktop_lan::manager();
            let settings = manager.settings().await;
            let peers = manager
                .list_peers()
                .await
                .into_iter()
                .map(|peer| LanPeerRecord {
                    peer_id: peer.peer_id,
                    display_name: peer.display_name,
                    lan_ip: peer.lan_ip,
                    listen_port: peer.listen_port,
                })
                .collect::<Vec<_>>();
            LanSettings {
                enabled: settings.enabled,
                peer_id: settings.peer_id,
                display_name: settings.display_name,
                listen_host: settings.listen_host,
                listen_port: settings.listen_port,
                discovery_port: settings.discovery_port,
                peer_count: peers.len(),
                peers,
            }
        })
    }

    pub fn save_model(&self, edit: ModelEdit<'_>) -> Result<DesktopSettings> {
        validate_text(edit.key, 96)?;
        validate_text(edit.provider, 128)?;
        validate_text(edit.model, 512)?;
        if edit.base_url.len() > 4096
            || edit.base_url.chars().any(char::is_control)
            || edit.api_key.len() > 8192
            || edit.api_key.chars().any(char::is_control)
        {
            bail!("服务地址或访问密钥无效");
        }
        let kind = edit.model_type.trim();
        if !matches!(
            kind,
            "llm" | "embedding" | "asr" | "tts" | "image" | "video"
        ) {
            bail!("模型类型无效");
        }
        self.update_settings(|config, _| {
            let key = edit.key.trim().to_string();
            if !config.llm.models.contains_key(&key) && config.llm.models.len() >= 200 {
                bail!("模型配置已达到 200 项上限");
            }
            if let Some(previous) = config.llm.models.get(&key) {
                let old_kind = previous.model_type.as_deref().unwrap_or("llm");
                if old_kind != kind && default_for(&config.llm, old_kind) == key {
                    bail!("请先更换该类型的默认模型，再修改模型类型");
                }
            }
            let entry = config.llm.models.entry(key.clone()).or_default();
            entry.provider = Some(edit.provider.trim().into());
            entry.model = Some(edit.model.trim().into());
            entry.base_url = Some(edit.base_url.trim().into());
            entry.model_type = Some(kind.into());
            // An empty editor field preserves a stored secret; secrets never
            // return in DesktopSettings or enter a Slint list model.
            if !edit.api_key.trim().is_empty() {
                entry.api_key = Some(edit.api_key.trim().into());
            }
            if default_for(&config.llm, kind).is_empty() {
                set_default(&mut config.llm, kind, key);
            }
            Ok(())
        })
    }

    pub fn set_default_model(&self, key: &str) -> Result<DesktopSettings> {
        self.update_settings(|config, _| {
            let model = config
                .llm
                .models
                .get(key)
                .ok_or_else(|| anyhow!("模型不存在"))?;
            if model.enable == Some(false) {
                bail!("请先启用模型");
            }
            let kind = model.model_type.clone().unwrap_or_else(|| "llm".into());
            set_default(&mut config.llm, &kind, key.into());
            Ok(())
        })
    }

    pub fn save_runtime(
        &self,
        workspace: &str,
        language: &str,
        python_path: &str,
        git_path: &str,
        rg_path: &str,
    ) -> Result<DesktopSettings> {
        if workspace.trim().is_empty() || !matches!(language, "zh-CN" | "en-US") {
            bail!("请填写工作目录和有效的语言");
        }
        self.update_settings(|config, settings| {
            let path = std::path::Path::new(workspace.trim());
            if !path.is_absolute() {
                bail!("工作目录必须是绝对路径");
            }
            std::fs::create_dir_all(path)?;
            let path = path.canonicalize()?;
            let old_defaults = normalize_desktop_container_roots(
                &std::collections::HashMap::new(),
                std::path::Path::new(&settings.workspace_root),
                &self.desktop.app_dir,
                self.user_id(),
            );
            let mut custom = settings.container_roots.clone();
            custom.retain(|id, root| old_defaults.get(id) != Some(root));
            let roots = normalize_desktop_container_roots(
                &custom,
                &path,
                &self.desktop.app_dir,
                self.user_id(),
            );
            for root in roots.values() {
                std::fs::create_dir_all(root)?;
            }
            config.workspace.root = path.to_string_lossy().into_owned();
            config.workspace.container_roots = roots;
            config.i18n.default_language = language.into();
            // Tool paths stay lenient like the runtime resolvers: an invalid
            // value falls back at spawn time and the status panel reports it.
            settings.python_path = python_path.trim().to_string();
            settings.git_path = git_path.trim().to_string();
            settings.rg_path = rg_path.trim().to_string();
            Ok(())
        })
    }

    fn update_settings(
        &self,
        edit: impl FnOnce(&mut Config, &mut crate::runtime::DesktopSettings) -> Result<()>,
    ) -> Result<DesktopSettings> {
        // Serialize read/modify/write, including model secret preservation.
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let mut old_settings = load_desktop_settings(&self.desktop.settings_path)?;
        let old_config = self.runtime.block_on(self.state().config_store.get());
        let mut config = old_config.clone();
        edit(&mut config, &mut old_settings)?;
        let mut settings = old_settings;
        settings.llm = Some(config.llm.clone());
        settings.workspace_root = config.workspace.root.clone();
        settings.container_roots = config.workspace.container_roots.clone();
        settings.language = config.i18n.default_language.clone();
        settings.updated_at = super::now_ts();
        let restore_settings = settings.clone();
        save_desktop_settings(&self.desktop.settings_path, &settings)?;
        let result = self
            .runtime
            .block_on(self.state().config_store.update(|current| {
                current.llm = config.llm.clone();
                current.workspace.root = config.workspace.root.clone();
                current.workspace.container_roots = config.workspace.container_roots.clone();
                current.i18n.default_language = config.i18n.default_language.clone();
            }));
        if let Err(error) = result {
            // ConfigStore currently mutates memory before persistence. Restore
            // both stores on failure so the UI can safely retry.
            let _ = save_desktop_settings(&self.desktop.settings_path, &restore_settings);
            let _ = self.runtime.block_on(
                self.state()
                    .config_store
                    .update(|current| *current = old_config),
            );
            return Err(error);
        }
        self.state()
            .workspace
            .set_container_roots(config.workspace.container_roots.clone());
        let lan = self.read_lan_settings();
        let persisted = load_desktop_settings(&self.desktop.settings_path).unwrap_or_default();
        Ok(project(
            &config,
            lan,
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
            &self.desktop.app_dir,
        ))
    }
}

fn validate_text(text: &str, limit: usize) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > limit || text.chars().any(char::is_control)
    {
        bail!("配置字段为空、过长或含控制字符");
    }
    Ok(())
}

fn default_for<'a>(llm: &'a LlmConfig, kind: &str) -> &'a str {
    match kind {
        "embedding" => llm.default_embedding.as_deref(),
        "asr" => llm.default_asr.as_deref(),
        "tts" => llm.default_tts.as_deref(),
        "image" => llm.default_image.as_deref(),
        "video" => llm.default_video.as_deref(),
        _ => Some(llm.default.as_str()),
    }
    .unwrap_or_default()
}

fn set_default(llm: &mut LlmConfig, kind: &str, key: String) {
    match kind {
        "embedding" => llm.default_embedding = Some(key),
        "asr" => llm.default_asr = Some(key),
        "tts" => llm.default_tts = Some(key),
        "image" => llm.default_image = Some(key),
        "video" => llm.default_video = Some(key),
        _ => llm.default = key,
    }
}

fn project(
    config: &Config,
    lan: LanSettings,
    preferences: (&str, &str),
    persisted: &crate::runtime::DesktopSettings,
    app_dir: &std::path::Path,
) -> DesktopSettings {
    let mut models = config
        .llm
        .models
        .iter()
        .map(|(key, value)| {
            let kind = value.model_type.as_deref().unwrap_or("llm");
            ModelRecord {
                key: key.clone(),
                provider: value.provider.clone().unwrap_or_default(),
                model: value.model.clone().unwrap_or_default(),
                base_url: value.base_url.clone().unwrap_or_default(),
                model_type: kind.into(),
                is_default: default_for(&config.llm, kind) == key,
            }
        })
        .collect::<Vec<_>>();
    models.sort_by(|a, b| a.key.cmp(&b.key));
    models.truncate(200);
    DesktopSettings {
        workspace_root: config.workspace.root.clone(),
        language: config.i18n.default_language.clone(),
        theme: preferences.0.to_string(),
        send_key: preferences.1.to_string(),
        python_path: persisted.python_path.clone(),
        git_path: persisted.git_path.clone(),
        rg_path: persisted.rg_path.clone(),
        tool_status: runtime_tool_statuses(persisted, app_dir),
        models,
        lan,
    }
}

/// Secret-free probe result: the outcome message is user-facing and the
/// resolved API key never leaves the process boundary.
#[derive(Clone, Debug)]
pub struct ModelProbeOutcome {
    pub max_context: Option<u32>,
    pub message: String,
}

impl NativeDesktop {
    fn resolve_probe_target(
        &self,
        model_key: &str,
        api_key: Option<&str>,
    ) -> Result<(String, String, String, String, String)> {
        let key = model_key.trim();
        if key.is_empty() {
            bail!("模型标识为空");
        }
        let config = self.runtime.block_on(self.state().config_store.get());
        let entry = config
            .llm
            .models
            .get(key)
            .ok_or_else(|| anyhow!("模型配置不存在：{key}"))?;
        let provider = wunder_server::llm::normalize_provider(entry.provider.as_deref());
        let base_url = entry
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                wunder_server::llm::provider_default_base_url(&provider).map(str::to_string)
            })
            .ok_or_else(|| anyhow!("该模型未配置服务地址"))?;
        // An explicit probe key overrides the stored secret (the editor may
        // probe before saving); stored secrets stay inside the process.
        let secret = api_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                entry
                    .api_key
                    .clone()
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_default();
        let model = entry
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("该模型未配置模型名称"))?
            .to_string();
        Ok((
            provider,
            base_url,
            secret,
            model,
            entry.model_type.clone().unwrap_or_default(),
        ))
    }

    /// Probe the context window of one configured model through the shared
    /// OpenAI-compatible resolver used by the desktop API.
    pub fn probe_model_context_window(
        &self,
        model_key: &str,
        api_key: Option<&str>,
    ) -> Result<ModelProbeOutcome> {
        let (provider, base_url, secret, model, _) =
            self.resolve_probe_target(model_key, api_key)?;
        self.probe_model_window(&provider, &model, &base_url, Some(&secret))
    }

    /// Probe with explicit editor fields so an unsaved draft can be probed
    /// too; `model_key` is only used to fall back to the stored secret.
    pub fn probe_model_window(
        &self,
        provider: &str,
        model: &str,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<ModelProbeOutcome> {
        let provider = wunder_server::llm::normalize_provider(Some(provider));
        let base_url = self.effective_probe_base_url(&provider, base_url)?;
        let secret = Self::probe_secret(api_key).unwrap_or_default();
        if !wunder_server::llm::is_openai_compatible_provider(&provider) {
            return Ok(ModelProbeOutcome {
                max_context: None,
                message: "该提供方不支持上下文探测".into(),
            });
        }
        let outcome = self
            .runtime
            .block_on(wunder_server::llm::probe_openai_context_window(
                &base_url,
                &secret,
                model.trim(),
                15,
            ));
        match outcome {
            Ok(Some(value)) => Ok(ModelProbeOutcome {
                max_context: Some(value),
                message: format!("探测成功：上下文 {value}"),
            }),
            Ok(None) => Ok(ModelProbeOutcome {
                max_context: None,
                message: "提供方未返回上下文信息".into(),
            }),
            Err(error) => Ok(ModelProbeOutcome {
                max_context: None,
                message: format!("探测失败：{error}"),
            }),
        }
    }

    /// Probe the voice list of one configured TTS model.
    pub fn probe_model_voices(
        &self,
        model_key: &str,
        api_key: Option<&str>,
    ) -> Result<Vec<String>> {
        let (provider, base_url, secret, model, model_type) =
            self.resolve_probe_target(model_key, api_key)?;
        if model_type != "tts" {
            bail!("只有语音合成模型支持语音列表探测");
        }
        self.probe_model_voice_list(&provider, &model, &base_url, Some(&secret))
    }

    /// Voice-list probe with explicit editor fields (unsaved drafts allowed).
    pub fn probe_model_voice_list(
        &self,
        provider: &str,
        model: &str,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Result<Vec<String>> {
        let provider = wunder_server::llm::normalize_provider(Some(provider));
        if model.trim().is_empty() {
            bail!("模型名称为空");
        }
        let base_url = self.effective_probe_base_url(&provider, base_url)?;
        let secret = Self::probe_secret(api_key).unwrap_or_default();
        if !wunder_server::llm::is_openai_compatible_provider(&provider) {
            bail!("该提供方不支持语音列表探测");
        }
        let voices = self
            .runtime
            .block_on(wunder_server::multimodal_models::probe_tts_voices(
                &base_url,
                &secret,
                model.trim(),
                15,
            ))?;
        Ok(voices)
    }

    fn effective_probe_base_url(&self, provider: &str, base_url: &str) -> Result<String> {
        let trimmed = base_url.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
        wunder_server::llm::provider_default_base_url(provider)
            .map(str::to_string)
            .ok_or_else(|| anyhow!("该模型未配置服务地址"))
    }

    /// Probe secrets: only an explicit editor override reaches the probe;
    /// stored secrets are resolved by the key-based variants. Secrets never
    /// leave the process.
    fn probe_secret(api_key: Option<&str>) -> Option<String> {
        api_key
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }
}
