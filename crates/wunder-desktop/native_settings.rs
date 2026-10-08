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
    /// Generation and multimodal parameters, projected as editor-ready
    /// strings (empty = unset) so the Slint list model stays plain data.
    pub temperature: String,
    pub timeout_s: String,
    pub max_output: String,
    pub thinking_token_budget: String,
    pub max_rounds: String,
    pub max_context: String,
    pub tts_voice: String,
    pub tts_response_format: String,
    pub tts_speed: String,
    pub tts_instructions: String,
    pub asr_language: String,
    pub asr_response_format: String,
    pub asr_temperature: String,
    pub asr_prompt: String,
    pub image_size: String,
    pub image_output_format: String,
    pub image_steps: String,
    pub image_guidance_scale: String,
    pub image_negative_prompt: String,
    pub video_size: String,
    pub video_seconds: String,
    pub video_fps: String,
    pub video_negative_prompt: String,
}

#[derive(Clone, Debug)]
pub struct DesktopSettings {
    pub workspace_root: String,
    pub language: String,
    pub theme: String,
    pub send_key: String,
    pub font_size: i32,
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

/// Editor payload for one model configuration. Every parameter field is an
/// editor string; blank means "keep unset" (or "keep stored secret" for the
/// access key).
#[derive(Default)]
pub struct ModelEdit<'a> {
    pub key: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model_type: &'a str,
    pub temperature: &'a str,
    pub timeout_s: &'a str,
    pub max_output: &'a str,
    pub thinking_token_budget: &'a str,
    pub max_rounds: &'a str,
    pub max_context: &'a str,
    pub tts_voice: &'a str,
    pub tts_response_format: &'a str,
    pub tts_speed: &'a str,
    pub tts_instructions: &'a str,
    pub asr_language: &'a str,
    pub asr_response_format: &'a str,
    pub asr_temperature: &'a str,
    pub asr_prompt: &'a str,
    pub image_size: &'a str,
    pub image_output_format: &'a str,
    pub image_steps: &'a str,
    pub image_guidance_scale: &'a str,
    pub image_negative_prompt: &'a str,
    pub video_size: &'a str,
    pub video_seconds: &'a str,
    pub video_fps: &'a str,
    pub video_negative_prompt: &'a str,
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

    /// Local roots for every workspace container, ascending by container id.
    /// Container 0 is the user private container; 1-10 are working directories.
    pub fn container_roots(&self) -> Result<Vec<(i32, String)>> {
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let settings = load_desktop_settings(&self.desktop.settings_path)?;
        let mut roots: Vec<(i32, String)> = settings
            .container_roots
            .iter()
            .map(|(container_id, root)| (*container_id, root.clone()))
            .collect();
        roots.sort_by_key(|(container_id, _)| *container_id);
        Ok(roots)
    }

    pub fn get_desktop_settings(&self) -> Result<DesktopSettings> {
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let config = self.runtime.block_on(self.state().config_store.get());
        let lan = self.read_lan_settings();
        let persisted = load_desktop_settings(&self.desktop.settings_path).unwrap_or_default();
        Ok(self.ordered_project(
            &config,
            lan,
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
        )?)
    }

    /// Persisted UI preferences: accent palette, composer send key and chat
    /// font size. The Slint shell renders every accepted value live.
    pub fn save_preferences(
        &self,
        theme: &str,
        send_key: &str,
        font_size: i32,
    ) -> Result<DesktopSettings> {
        let theme = theme.trim();
        let send_key = send_key.trim();
        if !matches!(
            theme,
            "light" | "eva-orange" | "hula-green" | "minimal" | "tech-blue"
        ) {
            bail!("暂不支持该主题");
        }
        if !matches!(send_key, "enter" | "ctrl_enter" | "none") {
            bail!("发送键设置无效");
        }
        if !(12..=20).contains(&font_size) {
            bail!("字体大小必须在 12 到 20 之间");
        }
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let mut settings = load_desktop_settings(&self.desktop.settings_path)?;
        settings.theme = theme.to_string();
        settings.send_key = send_key.to_string();
        settings.font_size = font_size;
        settings.updated_at = super::now_ts();
        save_desktop_settings(&self.desktop.settings_path, &settings)?;
        // Release the settings lock before re-reading through the façade:
        // get_desktop_settings takes the same non-reentrant mutex.
        drop(_guard);
        self.get_desktop_settings()
    }

    /// Persist the model list display order. Keys missing from the config are
    /// ignored at read time, so a stale order never hides or breaks entries.
    pub fn save_model_order(&self, keys: &[String]) -> Result<()> {
        let mut order = self.navigation_order()?;
        order.models = keys.to_vec();
        self.save_navigation_order(&order)
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
        let sessions = self
            .list_sessions(None)
            .map(|items| items.len())
            .unwrap_or(0);
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
            "font_size": settings.font_size,
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
        Ok(self.ordered_project(
            &config,
            self.read_lan_settings(),
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
        )?)
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

    /// Provider presets for the model editor dropdown: normalized key plus
    /// its default base URL (empty for the generic compatible provider).
    pub fn provider_presets(&self) -> Vec<(String, String)> {
        wunder_server::llm::PROVIDER_PRESETS
            .iter()
            .map(|(name, url)| ((*name).to_string(), (*url).to_string()))
            .collect()
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
        let temperature = parse_f32(edit.temperature, "temperature", 0.0, 2.0)?;
        let timeout_s = parse_u64(edit.timeout_s, "超时秒数", 1, u64::MAX)?;
        let max_output = parse_u32(edit.max_output, "最大输出", 1)?;
        let thinking_token_budget = parse_u32(edit.thinking_token_budget, "思考预算", 0)?;
        let max_rounds = parse_u32(edit.max_rounds, "最大轮次", 1)?;
        let max_context = parse_u32(edit.max_context, "上下文长度", 1)?;
        let tts_voice = optional_text(edit.tts_voice, 256)?;
        let tts_response_format =
            optional_choice(edit.tts_response_format, "音频格式", &TTS_FORMATS)?;
        let tts_speed = parse_f32(edit.tts_speed, "语速", 0.25, 4.0)?;
        let tts_instructions = optional_text(edit.tts_instructions, 4096)?;
        let asr_language = optional_text(edit.asr_language, 64)?;
        let asr_response_format =
            optional_choice(edit.asr_response_format, "返回格式", &ASR_FORMATS)?;
        let asr_temperature = parse_f32(edit.asr_temperature, "识别 temperature", 0.0, 2.0)?;
        let asr_prompt = optional_text(edit.asr_prompt, 4096)?;
        let image_size = optional_text(edit.image_size, 64)?;
        let image_output_format =
            optional_choice(edit.image_output_format, "输出格式", &IMAGE_FORMATS)?;
        let image_steps = parse_u32_range(edit.image_steps, "采样步数", 1, 200)?;
        let image_guidance_scale = parse_f32(edit.image_guidance_scale, "引导系数", 0.0, 20.0)?;
        let image_negative_prompt = optional_text(edit.image_negative_prompt, 4096)?;
        let video_size = optional_text(edit.video_size, 64)?;
        let video_seconds = parse_f32(edit.video_seconds, "时长", 0.5, f32::MAX)?;
        let video_fps = parse_u32(edit.video_fps, "帧率", 1)?;
        let video_negative_prompt = optional_text(edit.video_negative_prompt, 4096)?;
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
            entry.temperature = temperature;
            entry.timeout_s = timeout_s;
            entry.max_output = max_output;
            entry.thinking_token_budget = thinking_token_budget;
            entry.max_rounds = max_rounds;
            entry.max_context = max_context;
            entry.tts_voice = tts_voice.clone();
            entry.tts_response_format = tts_response_format.clone();
            entry.tts_speed = tts_speed;
            entry.tts_instructions = tts_instructions.clone();
            entry.asr_language = asr_language.clone();
            entry.asr_response_format = asr_response_format.clone();
            entry.asr_temperature = asr_temperature;
            entry.asr_prompt = asr_prompt.clone();
            entry.image_size = image_size.clone();
            entry.image_output_format = image_output_format.clone();
            entry.image_num_inference_steps = image_steps;
            entry.image_guidance_scale = image_guidance_scale;
            entry.image_negative_prompt = image_negative_prompt.clone();
            entry.video_size = video_size.clone();
            entry.video_seconds = video_seconds;
            entry.video_fps = video_fps;
            entry.video_negative_prompt = video_negative_prompt.clone();
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

    /// Remove one model configuration. A default pointing at the removed key
    /// is cleared so the runtime never resolves a dangling default.
    pub fn delete_model(&self, key: &str) -> Result<DesktopSettings> {
        let key = key.trim();
        if key.is_empty() {
            bail!("模型标识为空");
        }
        self.update_settings(|config, _| {
            let entry = config
                .llm
                .models
                .remove(key)
                .ok_or_else(|| anyhow!("模型不存在"))?;
            let kind = entry.model_type.as_deref().unwrap_or("llm");
            if default_for(&config.llm, kind) == key {
                clear_default(&mut config.llm, kind);
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
        Ok(self.ordered_project(
            &config,
            lan,
            (persisted.theme.as_str(), persisted.send_key.as_str()),
            &persisted,
        )?)
    }

    /// Project the config into editor settings with the user's saved model
    /// display order applied. Keys without a saved slot keep their key-sorted
    /// position at the end, so new entries always appear instead of vanishing.
    fn ordered_project(
        &self,
        config: &Config,
        lan: LanSettings,
        preferences: (&str, &str),
        persisted: &crate::runtime::DesktopSettings,
    ) -> Result<DesktopSettings> {
        let mut settings = project(config, lan, preferences, persisted, &self.desktop.app_dir);
        let order = self
            .navigation_order()
            .map(|order| order.models)
            .unwrap_or_default();
        if !order.is_empty() {
            let mut ordered: Vec<ModelRecord> = Vec::with_capacity(settings.models.len());
            for key in &order {
                if let Some(position) = settings.models.iter().position(|model| &model.key == key) {
                    ordered.push(settings.models.remove(position));
                }
            }
            ordered.extend(settings.models);
            settings.models = ordered;
        }
        Ok(settings)
    }
}

fn validate_text(text: &str, limit: usize) -> Result<()> {
    if text.trim().is_empty() || text.chars().count() > limit || text.chars().any(char::is_control)
    {
        bail!("配置字段为空、过长或含控制字符");
    }
    Ok(())
}

const TTS_FORMATS: &[&str] = &["wav", "mp3", "flac", "aac", "opus", "pcm"];
const ASR_FORMATS: &[&str] = &["json", "text", "verbose_json", "srt", "vtt"];
const IMAGE_FORMATS: &[&str] = &["png", "jpeg", "webp"];

/// Parse one optional numeric editor field: blank means unset, otherwise the
/// value must lie inside the admin-side range.
fn parse_f32(raw: &str, name: &str, min: f32, max: f32) -> Result<Option<f32>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let value: f32 = raw.parse().map_err(|_| anyhow!("{name} 必须是数字"))?;
    if !value.is_finite() || value < min || value > max {
        bail!("{name} 必须在 {min} 到 {max} 之间");
    }
    Ok(Some(value))
}

fn parse_u32(raw: &str, name: &str, min: u32) -> Result<Option<u32>> {
    parse_u32_range(raw, name, min, u32::MAX)
}

fn parse_u32_range(raw: &str, name: &str, min: u32, max: u32) -> Result<Option<u32>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let value: u32 = raw.parse().map_err(|_| anyhow!("{name} 必须是整数"))?;
    if value < min || value > max {
        bail!("{name} 必须在 {min} 到 {max} 之间");
    }
    Ok(Some(value))
}

fn parse_u64(raw: &str, name: &str, min: u64, max: u64) -> Result<Option<u64>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let value: u64 = raw.parse().map_err(|_| anyhow!("{name} 必须是整数"))?;
    if value < min || value > max {
        bail!("{name} 必须在 {min} 到 {max} 之间");
    }
    Ok(Some(value))
}

/// Optional free-text field: blank clears the stored value.
fn optional_text(raw: &str, limit: usize) -> Result<Option<String>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.chars().count() > limit || raw.chars().any(char::is_control) {
        bail!("配置字段过长或含控制字符");
    }
    Ok(Some(raw.to_string()))
}

fn optional_choice(raw: &str, name: &str, allowed: &[&str]) -> Result<Option<String>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if !allowed.contains(&raw) {
        bail!("{name} 无效");
    }
    Ok(Some(raw.to_string()))
}

/// Render a stored number as an editor-friendly string without trailing zeros.
fn format_number(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn optional_string(value: Option<&str>) -> String {
    value.unwrap_or_default().to_string()
}

fn clear_default(llm: &mut LlmConfig, kind: &str) {
    match kind {
        "embedding" => llm.default_embedding = None,
        "asr" => llm.default_asr = None,
        "tts" => llm.default_tts = None,
        "image" => llm.default_image = None,
        "video" => llm.default_video = None,
        _ => llm.default = String::new(),
    }
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
                temperature: value.temperature.map(format_number).unwrap_or_default(),
                timeout_s: value.timeout_s.map(|v| v.to_string()).unwrap_or_default(),
                max_output: value.max_output.map(|v| v.to_string()).unwrap_or_default(),
                thinking_token_budget: value
                    .thinking_token_budget
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                max_rounds: value.max_rounds.map(|v| v.to_string()).unwrap_or_default(),
                max_context: value.max_context.map(|v| v.to_string()).unwrap_or_default(),
                tts_voice: optional_string(value.tts_voice.as_deref()),
                tts_response_format: optional_string(value.tts_response_format.as_deref()),
                tts_speed: value.tts_speed.map(format_number).unwrap_or_default(),
                tts_instructions: optional_string(value.tts_instructions.as_deref()),
                asr_language: optional_string(value.asr_language.as_deref()),
                asr_response_format: optional_string(value.asr_response_format.as_deref()),
                asr_temperature: value.asr_temperature.map(format_number).unwrap_or_default(),
                asr_prompt: optional_string(value.asr_prompt.as_deref()),
                image_size: optional_string(value.image_size.as_deref()),
                image_output_format: optional_string(value.image_output_format.as_deref()),
                image_steps: value
                    .image_num_inference_steps
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                image_guidance_scale: value
                    .image_guidance_scale
                    .map(format_number)
                    .unwrap_or_default(),
                image_negative_prompt: optional_string(value.image_negative_prompt.as_deref()),
                video_size: optional_string(value.video_size.as_deref()),
                video_seconds: value.video_seconds.map(format_number).unwrap_or_default(),
                video_fps: value.video_fps.map(|v| v.to_string()).unwrap_or_default(),
                video_negative_prompt: optional_string(value.video_negative_prompt.as_deref()),
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
        font_size: persisted.font_size.clamp(12, 20),
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
