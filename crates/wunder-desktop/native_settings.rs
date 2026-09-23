//! Serialized, secret-free projections of desktop configuration.
use super::NativeDesktop;
use crate::runtime::{
    load_desktop_settings, normalize_desktop_container_roots, save_desktop_settings,
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
    pub fn get_desktop_settings(&self) -> Result<DesktopSettings> {
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let config = self.runtime.block_on(self.state().config_store.get());
        let lan = self.read_lan_settings();
        Ok(project(&config, lan))
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
        Ok(project(&config, self.read_lan_settings()))
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

    pub fn save_runtime(&self, workspace: &str, language: &str) -> Result<DesktopSettings> {
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
            Ok(())
        })
    }

    fn update_settings(
        &self,
        edit: impl FnOnce(&mut Config, &crate::runtime::DesktopSettings) -> Result<()>,
    ) -> Result<DesktopSettings> {
        // Serialize read/modify/write, including model secret preservation.
        let _guard = self
            .settings_lock
            .lock()
            .map_err(|_| anyhow!("配置锁不可用"))?;
        let old_settings = load_desktop_settings(&self.desktop.settings_path)?;
        let old_config = self.runtime.block_on(self.state().config_store.get());
        let mut config = old_config.clone();
        edit(&mut config, &old_settings)?;
        let mut settings = old_settings.clone();
        settings.llm = Some(config.llm.clone());
        settings.workspace_root = config.workspace.root.clone();
        settings.container_roots = config.workspace.container_roots.clone();
        settings.language = config.i18n.default_language.clone();
        settings.updated_at = super::now_ts();
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
            let _ = save_desktop_settings(&self.desktop.settings_path, &old_settings);
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
        Ok(project(&config, lan))
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

fn project(config: &Config, lan: LanSettings) -> DesktopSettings {
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
        models,
        lan,
    }
}
