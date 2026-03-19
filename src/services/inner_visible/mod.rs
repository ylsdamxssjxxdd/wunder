mod layout;
mod worker_card;

use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::atomic_write::atomic_write_text;
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, DEFAULT_AGENT_NAME};
use crate::services::default_tool_profile::curated_default_tool_names;
use crate::services::inner_visible::layout::{
    agent_id_from_worker_card_file_name, defaults_worker_card_path, normalize_agent_file_stem,
    tooling_path, user_paths, worker_card_path, InnerVisiblePaths,
};
use crate::services::user_access::{compute_allowed_tool_names, UserToolContext};
use crate::services::user_agent_presets::{
    filter_allowed_tools, normalize_agent_approval_mode, normalize_agent_status,
    normalize_preset_questions, normalize_tool_list,
};
use crate::skills::SkillRegistry;
use crate::storage::{
    normalize_hive_id, UserAccountRecord, UserAgentRecord, DEFAULT_HIVE_ID,
    DEFAULT_SANDBOX_CONTAINER_ID,
};
use crate::user_store::UserStore;
use crate::user_tools::{UserToolBindings, UserToolKind, UserToolManager, UserToolStore};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::warn;

use self::worker_card::{build_worker_card, parse_worker_card, WorkerCardDocument};

const DEFAULT_AGENT_META_PREFIX: &str = "default_agent:";
const DEFAULT_AGENT_APPROVAL_MODE: &str = "full_auto";
const DEFAULT_AGENT_STATUS: &str = "active";
const FILE_TIME_EPSILON_S: f64 = 0.001;

#[derive(Clone)]
pub struct InnerVisibleService {
    config_store: ConfigStore,
    workspace: Arc<WorkspaceManager>,
    skills: Arc<RwLock<SkillRegistry>>,
    user_tool_store: Arc<UserToolStore>,
    user_tool_manager: Arc<UserToolManager>,
    user_store: Arc<UserStore>,
}

impl InnerVisibleService {
    pub fn new(
        config_store: ConfigStore,
        workspace: Arc<WorkspaceManager>,
        skills: Arc<RwLock<SkillRegistry>>,
        user_tool_store: Arc<UserToolStore>,
        user_tool_manager: Arc<UserToolManager>,
        user_store: Arc<UserStore>,
    ) -> Self {
        Self {
            config_store,
            workspace,
            skills,
            user_tool_store,
            user_tool_manager,
            user_store,
        }
    }

    pub fn private_root(&self, user_id: &str) -> std::path::PathBuf {
        user_paths(self.workspace.as_ref(), user_id).private_root
    }

    pub fn ensure_scaffold(&self, user_id: &str) -> Result<InnerVisiblePaths> {
        let paths = user_paths(self.workspace.as_ref(), user_id);
        for dir in [
            &paths.private_root,
            &paths.global_dir,
            &paths.agents_dir,
            &paths.skills_dir,
            &paths.knowledge_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        if let Err(err) = remove_path_if_exists(&paths.legacy_inner_visible_dir) {
            warn!(
                "failed to cleanup legacy inner-visible dir {}: {err}",
                paths.legacy_inner_visible_dir.display()
            );
        }
        Ok(paths)
    }

    pub fn remove_agent_files(&self, user_id: &str, agent_id: &str) -> Result<()> {
        let paths = self.ensure_scaffold(user_id)?;
        remove_path_if_exists(&worker_card_path(&paths, Some(agent_id)))?;
        remove_path_if_exists(&legacy_worker_card_dir(&paths, Some(agent_id)))?;
        Ok(())
    }

    pub async fn sync_user_state(&self, user_id: &str) -> Result<()> {
        let paths = self.ensure_scaffold(user_id)?;
        self.user_store.ensure_default_hive(user_id)?;
        self.user_tool_store.ensure_materialized(user_id)?;
        self.validate_json_file(
            tooling_path(&paths),
            "failed to parse user global tooling config",
        );
        self.validate_worker_card_file(
            defaults_worker_card_path(&paths),
            "failed to parse global defaults worker-card",
        );

        let config = self.config_store.get().await;
        let skills = self.skills.read().await.clone();
        let allowed_tool_names = self.allowed_tool_names(user_id, &config, &skills)?;
        let worker_card_skill_names = collect_worker_card_skill_names(
            &skills,
            &self
                .user_tool_manager
                .build_bindings(&config, &skills, user_id),
        );

        self.sync_default_agent(
            user_id,
            &paths,
            &allowed_tool_names,
            &worker_card_skill_names,
        )?;
        self.sync_regular_agents(
            user_id,
            &paths,
            &allowed_tool_names,
            &worker_card_skill_names,
        )?;
        Ok(())
    }

    fn allowed_tool_names(
        &self,
        user_id: &str,
        config: &Config,
        skills: &SkillRegistry,
    ) -> Result<HashSet<String>> {
        let user = self
            .user_store
            .get_user_by_id(user_id)?
            .unwrap_or_else(|| synthetic_user(user_id));
        let bindings = self
            .user_tool_manager
            .build_bindings(config, skills, user_id);
        let context = UserToolContext {
            config: config.clone(),
            skills: skills.clone(),
            bindings,
            tool_access: self.user_store.get_user_tool_access(user_id)?,
        };
        Ok(compute_allowed_tool_names(&user, &context))
    }

    fn sync_default_agent(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        allowed_tool_names: &HashSet<String>,
        worker_card_skill_names: &HashSet<String>,
    ) -> Result<()> {
        let mut config = self.load_default_agent_config(user_id, allowed_tool_names)?;
        let worker_card_file = resolve_latest_worker_card_file(paths, Some(DEFAULT_AGENT_ID_ALIAS));
        let latest_file_mtime = worker_card_file
            .as_ref()
            .map(|path| file_modified_ts(path))
            .unwrap_or(0.0);

        // File changes win only when they are strictly newer than the runtime snapshot.
        if latest_file_mtime > config.updated_at + FILE_TIME_EPSILON_S {
            match self.apply_default_agent_file(
                user_id,
                worker_card_file
                    .as_deref()
                    .ok_or_else(|| anyhow!("default agent worker-card path missing during sync"))?,
                allowed_tool_names,
                &config,
            ) {
                Ok(updated) => {
                    config = updated;
                }
                Err(err) => {
                    warn!("default agent inner-visible sync failed for {user_id}: {err}");
                }
            }
        }

        self.write_default_agent_files(user_id, paths, &config, worker_card_skill_names)?;
        Ok(())
    }

    fn sync_regular_agents(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        allowed_tool_names: &HashSet<String>,
        worker_card_skill_names: &HashSet<String>,
    ) -> Result<()> {
        let mut by_id: HashMap<String, UserAgentRecord> = self
            .user_store
            .list_user_agents(user_id)?
            .into_iter()
            .filter(|record| record.agent_id != DEFAULT_AGENT_ID_ALIAS)
            .map(|record| (record.agent_id.clone(), record))
            .collect();
        let mut agent_ids = BTreeSet::new();
        agent_ids.extend(by_id.keys().cloned());
        agent_ids.extend(discover_agent_ids(&paths.agents_dir)?);

        for agent_id in agent_ids {
            if agent_id == DEFAULT_AGENT_ID_ALIAS {
                continue;
            }
            let record = by_id.remove(&agent_id);
            let worker_card_file = resolve_latest_worker_card_file(paths, Some(&agent_id));
            let worker_card_mtime = worker_card_file
                .as_ref()
                .map(|path| file_modified_ts(path))
                .unwrap_or(0.0);
            let record_updated_at = record.as_ref().map(|item| item.updated_at).unwrap_or(0.0);

            let final_record = if let Some(worker_card_file) = worker_card_file
                .as_ref()
                .filter(|_| worker_card_mtime > record_updated_at + FILE_TIME_EPSILON_S)
            {
                match self.apply_agent_files(
                    user_id,
                    &agent_id,
                    record.as_ref(),
                    worker_card_file,
                    allowed_tool_names,
                ) {
                    Ok(updated) => updated,
                    Err(err) => {
                        warn!("agent inner-visible sync failed for {user_id}/{agent_id}: {err}");
                        if let Some(existing) = record {
                            existing
                        } else {
                            continue;
                        }
                    }
                }
            } else if let Some(existing) = record {
                existing
            } else {
                continue;
            };

            self.write_agent_files(paths, &final_record, worker_card_skill_names)?;
        }
        Ok(())
    }

    fn apply_agent_files(
        &self,
        user_id: &str,
        agent_id: &str,
        existing: Option<&UserAgentRecord>,
        worker_card_file: &Path,
        allowed_tool_names: &HashSet<String>,
    ) -> Result<UserAgentRecord> {
        let document = load_worker_card_document(worker_card_file)?;
        let parsed = parse_worker_card(document, None);
        let now = now_ts();
        let mut record = existing.cloned().unwrap_or(UserAgentRecord {
            agent_id: agent_id.trim().to_string(),
            user_id: user_id.trim().to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: agent_id.trim().to_string(),
            description: String::new(),
            system_prompt: String::new(),
            model_name: None,
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
            is_shared: false,
            status: DEFAULT_AGENT_STATUS.to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: now,
            updated_at: now,
            preset_binding: None,
        });

        record.user_id = user_id.trim().to_string();
        record.agent_id = agent_id.trim().to_string();
        if !parsed.name.is_empty() {
            record.name = parsed.name;
        }
        record.description = parsed.description;
        record.system_prompt = parsed.system_prompt;
        record.model_name = parsed.model_name;
        record.declared_tool_names = parsed.declared_tool_names;
        record.declared_skill_names = parsed.declared_skill_names;
        record.tool_names = filter_allowed_tools(&parsed.tool_names, allowed_tool_names);
        record.preset_questions = normalize_preset_questions(parsed.preset_questions);
        record.approval_mode = normalize_agent_approval_mode(Some(&parsed.approval_mode));
        record.is_shared = parsed.is_shared;
        record.status = normalize_agent_status(Some(&record.status));
        record.icon = parsed.icon;
        record.sandbox_container_id = parsed.sandbox_container_id;
        record.hive_id = normalize_hive_id(&parsed.hive_id);
        record.updated_at = now;
        if record.created_at <= 0.0 {
            record.created_at = now;
        }

        self.ensure_hive(user_id, &record.hive_id)?;
        self.user_store.upsert_user_agent(&record)?;
        Ok(record)
    }

    fn write_agent_files(
        &self,
        paths: &InnerVisiblePaths,
        record: &UserAgentRecord,
        worker_card_skill_names: &HashSet<String>,
    ) -> Result<()> {
        let hive = self.user_store.get_hive(&record.user_id, &record.hive_id)?;
        let document = build_worker_card(
            record,
            hive.as_ref().map(|item| item.name.as_str()),
            hive.as_ref().map(|item| item.description.as_str()),
            worker_card_skill_names,
        );
        let worker_card_file = worker_card_path(paths, Some(&record.agent_id));
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document).context("serialize worker card failed")?,
        )?;
        remove_path_if_exists(&legacy_worker_card_dir(paths, Some(&record.agent_id)))?;
        Ok(())
    }

    fn load_default_agent_config(
        &self,
        user_id: &str,
        allowed_tool_names: &HashSet<String>,
    ) -> Result<DefaultAgentConfigMirror> {
        let raw = self.user_store.get_meta(&default_agent_meta_key(user_id))?;
        if let Some(raw) = raw.as_deref().filter(|value| !value.trim().is_empty()) {
            if let Ok(mut parsed) = serde_json::from_str::<DefaultAgentConfigMirror>(raw) {
                normalize_default_agent_config(&mut parsed, allowed_tool_names);
                return Ok(parsed);
            }
        }
        if let Some(record) = self
            .user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)?
        {
            let mut config = DefaultAgentConfigMirror {
                name: record.name,
                description: record.description,
                system_prompt: record.system_prompt,
                tool_names: record.tool_names,
                preset_questions: record.preset_questions,
                approval_mode: record.approval_mode,
                status: record.status,
                icon: record.icon,
                sandbox_container_id: record.sandbox_container_id,
                created_at: record.created_at,
                updated_at: record.updated_at,
            };
            normalize_default_agent_config(&mut config, allowed_tool_names);
            return Ok(config);
        }
        let now = now_ts();
        let mut config = DefaultAgentConfigMirror {
            name: DEFAULT_AGENT_NAME.to_string(),
            description: String::new(),
            system_prompt: String::new(),
            tool_names: curated_default_tool_names(allowed_tool_names),
            preset_questions: Vec::new(),
            approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
            status: DEFAULT_AGENT_STATUS.to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: now,
            updated_at: now,
        };
        normalize_default_agent_config(&mut config, allowed_tool_names);
        Ok(config)
    }

    fn apply_default_agent_file(
        &self,
        user_id: &str,
        worker_card_file: &Path,
        allowed_tool_names: &HashSet<String>,
        current: &DefaultAgentConfigMirror,
    ) -> Result<DefaultAgentConfigMirror> {
        let document = load_worker_card_document(worker_card_file)?;
        let parsed = parse_worker_card(document, None);
        let now = now_ts();
        let mut config = current.clone();
        if !parsed.name.is_empty() {
            config.name = parsed.name;
        }
        config.description = parsed.description;
        config.system_prompt = parsed.system_prompt;
        config.tool_names = filter_allowed_tools(&parsed.tool_names, allowed_tool_names);
        config.preset_questions = normalize_preset_questions(parsed.preset_questions);
        config.approval_mode = normalize_agent_approval_mode(Some(&parsed.approval_mode));
        config.status = normalize_agent_status(Some(DEFAULT_AGENT_STATUS));
        config.icon = parsed.icon;
        config.sandbox_container_id = parsed.sandbox_container_id;
        config.updated_at = now;
        if config.created_at <= 0.0 {
            config.created_at = now;
        }
        normalize_default_agent_config(&mut config, allowed_tool_names);
        self.persist_default_agent_config(user_id, &config)?;
        Ok(config)
    }

    fn persist_default_agent_config(
        &self,
        user_id: &str,
        config: &DefaultAgentConfigMirror,
    ) -> Result<()> {
        self.user_store.set_meta(
            &default_agent_meta_key(user_id),
            &serde_json::to_string(config)?,
        )?;
        if self
            .user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)?
            .is_some()
        {
            self.user_store
                .upsert_user_agent(&record_from_default_config(user_id, config))?;
        }
        Ok(())
    }

    fn write_default_agent_files(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        config: &DefaultAgentConfigMirror,
        worker_card_skill_names: &HashSet<String>,
    ) -> Result<()> {
        let record = record_from_default_config(user_id, config);
        self.write_agent_files(paths, &record, worker_card_skill_names)?;
        let defaults_document = build_worker_card(
            &record,
            Some("Default Hive"),
            Some(""),
            worker_card_skill_names,
        );
        let defaults_content = serde_json::to_string_pretty(&defaults_document)?;
        atomic_write_text(&defaults_worker_card_path(paths), &defaults_content)?;
        Ok(())
    }

    fn validate_json_file(&self, path: std::path::PathBuf, label: &str) {
        if !path.exists() {
            return;
        }
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                warn!("{label}: failed to read {}: {err}", path.display());
                return;
            }
        };
        if let Err(err) = serde_json::from_str::<serde_json::Value>(&content) {
            warn!("{label}: failed to parse {}: {err}", path.display());
        }
    }

    fn validate_worker_card_file(&self, path: std::path::PathBuf, label: &str) {
        if !path.exists() {
            return;
        }
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                warn!("{label}: failed to read {}: {err}", path.display());
                return;
            }
        };
        if let Err(err) = serde_json::from_str::<WorkerCardDocument>(&content) {
            warn!("{label}: failed to parse {}: {err}", path.display());
        }
    }

    fn ensure_hive(&self, user_id: &str, hive_id: &str) -> Result<()> {
        let normalized = normalize_hive_id(hive_id);
        if normalized == DEFAULT_HIVE_ID {
            self.user_store.ensure_default_hive(user_id)?;
            return Ok(());
        }
        if self.user_store.get_hive(user_id, &normalized)?.is_none() {
            let now = now_ts();
            self.user_store.upsert_hive(&crate::storage::HiveRecord {
                hive_id: normalized.clone(),
                user_id: user_id.trim().to_string(),
                name: normalized.clone(),
                description: String::new(),
                is_default: false,
                status: "active".to_string(),
                created_time: now,
                updated_time: now,
            })?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DefaultAgentConfigMirror {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_prompt: String,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    preset_questions: Vec<String>,
    #[serde(default)]
    approval_mode: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    sandbox_container_id: i32,
    #[serde(default)]
    created_at: f64,
    #[serde(default)]
    updated_at: f64,
}

fn default_agent_meta_key(user_id: &str) -> String {
    format!("{DEFAULT_AGENT_META_PREFIX}{}", user_id.trim())
}

fn normalize_default_agent_config(
    config: &mut DefaultAgentConfigMirror,
    allowed_tool_names: &HashSet<String>,
) {
    if config.name.trim().is_empty() {
        config.name = DEFAULT_AGENT_NAME.to_string();
    } else {
        config.name = config.name.trim().to_string();
    }
    config.description = config.description.trim().to_string();
    config.system_prompt = config.system_prompt.trim().to_string();
    config.tool_names = filter_allowed_tools(
        &normalize_tool_list(std::mem::take(&mut config.tool_names)),
        allowed_tool_names,
    );
    config.preset_questions =
        normalize_preset_questions(std::mem::take(&mut config.preset_questions));
    config.approval_mode = normalize_agent_approval_mode(Some(&config.approval_mode));
    config.status = normalize_agent_status(Some(&config.status));
    if !(1..=10).contains(&config.sandbox_container_id) {
        config.sandbox_container_id = DEFAULT_SANDBOX_CONTAINER_ID;
    }
    let now = now_ts();
    if config.created_at <= 0.0 {
        config.created_at = now;
    }
    if config.updated_at <= 0.0 {
        config.updated_at = config.created_at;
    }
}

fn record_from_default_config(user_id: &str, config: &DefaultAgentConfigMirror) -> UserAgentRecord {
    UserAgentRecord {
        agent_id: DEFAULT_AGENT_ID_ALIAS.to_string(),
        user_id: user_id.trim().to_string(),
        hive_id: DEFAULT_HIVE_ID.to_string(),
        name: config.name.clone(),
        description: config.description.clone(),
        system_prompt: config.system_prompt.clone(),
        model_name: None,
        tool_names: config.tool_names.clone(),
        declared_tool_names: Vec::new(),
        declared_skill_names: Vec::new(),
        preset_questions: config.preset_questions.clone(),
        access_level: "A".to_string(),
        approval_mode: config.approval_mode.clone(),
        is_shared: false,
        status: config.status.clone(),
        icon: config.icon.clone(),
        sandbox_container_id: config.sandbox_container_id,
        created_at: config.created_at,
        updated_at: config.updated_at,
        preset_binding: None,
    }
}

fn load_worker_card_document(path: &Path) -> Result<WorkerCardDocument> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read worker card failed: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parse worker card failed: {}", path.display()))
}

fn legacy_worker_card_dir(paths: &InnerVisiblePaths, agent_id: Option<&str>) -> std::path::PathBuf {
    paths.agents_dir.join(normalize_agent_file_stem(agent_id))
}

fn legacy_worker_card_path(
    paths: &InnerVisiblePaths,
    agent_id: Option<&str>,
) -> std::path::PathBuf {
    legacy_worker_card_dir(paths, agent_id).join("worker-card.json")
}

fn resolve_latest_worker_card_file(
    paths: &InnerVisiblePaths,
    agent_id: Option<&str>,
) -> Option<std::path::PathBuf> {
    let canonical = worker_card_path(paths, agent_id);
    let legacy = legacy_worker_card_path(paths, agent_id);
    match (canonical.exists(), legacy.exists()) {
        (false, false) => None,
        (true, false) => Some(canonical),
        (false, true) => Some(legacy),
        (true, true) => {
            if file_modified_ts(&canonical) + FILE_TIME_EPSILON_S >= file_modified_ts(&legacy) {
                Some(canonical)
            } else {
                Some(legacy)
            }
        }
    }
}

fn discover_agent_ids(root: &Path) -> Result<Vec<String>> {
    let mut output = Vec::new();
    if !root.exists() {
        return Ok(output);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            if let Some(agent_id) =
                agent_id_from_worker_card_file_name(entry.file_name().to_string_lossy().as_ref())
            {
                output.push(agent_id);
            }
            continue;
        }
        if file_type.is_dir() {
            let legacy_name = entry.file_name().to_string_lossy().trim().to_string();
            if legacy_name.is_empty() {
                continue;
            }
            let legacy_card = entry.path().join("worker-card.json");
            if legacy_card.exists() {
                output.push(legacy_name);
            }
        }
    }
    Ok(output)
}

fn collect_worker_card_skill_names(
    skills: &SkillRegistry,
    bindings: &UserToolBindings,
) -> HashSet<String> {
    let mut output = HashSet::new();
    for spec in skills.list_specs() {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for spec in &bindings.skill_specs {
        let cleaned = spec.name.trim();
        if !cleaned.is_empty() {
            output.insert(cleaned.to_string());
        }
    }
    for (alias, info) in &bindings.alias_map {
        if !matches!(info.kind, UserToolKind::Skill) {
            continue;
        }
        let cleaned_alias = alias.trim();
        if !cleaned_alias.is_empty() {
            output.insert(cleaned_alias.to_string());
        }
        let cleaned_target = info.target.trim();
        if !cleaned_target.is_empty() {
            output.insert(cleaned_target.to_string());
        }
    }
    output
}

fn file_modified_ts(path: &Path) -> f64 {
    let Ok(meta) = path.metadata() else {
        return 0.0;
    };
    let Ok(modified) = meta.modified() else {
        return 0.0;
    };
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn synthetic_user(user_id: &str) -> UserAccountRecord {
    let now = now_ts();
    UserAccountRecord {
        user_id: user_id.trim().to_string(),
        username: user_id.trim().to_string(),
        email: None,
        password_hash: String::new(),
        roles: vec!["user".to_string()],
        status: "active".to_string(),
        access_level: "A".to_string(),
        unit_id: None,
        daily_quota: 0,
        daily_quota_used: 0,
        daily_quota_date: None,
        is_demo: false,
        created_at: now,
        updated_at: now,
        last_login_at: None,
    }
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::user_tools::UserToolManager;
    use crate::skills::load_skills;
    use crate::storage::{SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;

    fn build_service() -> (
        tempfile::TempDir,
        Arc<UserStore>,
        Arc<InnerVisibleService>,
        Arc<WorkspaceManager>,
    ) {
        let temp = tempdir().expect("tempdir");
        let workspace_root = temp.path().join("workspaces");
        let db_path = temp.path().join("state.sqlite3");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        storage.ensure_initialized().expect("init storage");
        let workspace = Arc::new(WorkspaceManager::new(
            &workspace_root.to_string_lossy(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        let mut config = Config::default();
        config.workspace.root = workspace_root.to_string_lossy().to_string();
        let config_store = ConfigStore::new(temp.path().join("override.yaml"));
        let skills = Arc::new(RwLock::new(load_skills(&config, true, true, true)));
        let user_tool_store =
            Arc::new(UserToolStore::new(&config, workspace.clone()).expect("tool store"));
        let user_tool_manager = Arc::new(UserToolManager::new(user_tool_store.clone()));
        let user_store = Arc::new(UserStore::new(storage));
        let service = Arc::new(InnerVisibleService::new(
            config_store,
            workspace.clone(),
            skills,
            user_tool_store,
            user_tool_manager,
            user_store.clone(),
        ));
        (temp, user_store, service, workspace)
    }

    fn write_test_skill(skill_root: &Path, skill_name: &str) {
        let dir = skill_root.join(skill_name);
        fs::create_dir_all(&dir).expect("create skill dir");
        let content =
            format!("---\nname: {skill_name}\ndescription: test skill\n---\n# {skill_name}\n");
        atomic_write_text(&dir.join("SKILL.md"), &content).expect("write SKILL.md");
    }

    fn pick_stable_allowed_tool(allowed: &HashSet<String>, exclude: Option<&str>) -> String {
        let exclude = exclude.unwrap_or_default().trim().to_string();
        let mut candidates = allowed
            .iter()
            .filter(|name| {
                let trimmed = name.trim();
                !trimmed.is_empty()
                    && trimmed != exclude
                    && !trimmed.contains('@')
                    && !trimmed.contains("://")
            })
            .cloned()
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            candidates = allowed
                .iter()
                .filter(|name| {
                    let trimmed = name.trim();
                    !trimmed.is_empty() && trimmed != exclude
                })
                .cloned()
                .collect::<Vec<_>>();
        }
        candidates.sort();
        candidates
            .into_iter()
            .next()
            .expect("at least one allowed tool")
    }

    #[tokio::test]
    async fn sync_user_state_materializes_and_applies_agent_files() {
        let (_temp, user_store, service, workspace) = build_service();
        let now = now_ts();
        let record = UserAgentRecord {
            agent_id: "agent_demo".to_string(),
            user_id: "alice".to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "Demo".to_string(),
            description: "desc".to_string(),
            system_prompt: "initial prompt".to_string(),
            model_name: Some("gpt-5".to_string()),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "A".to_string(),
            approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
            is_shared: false,
            status: DEFAULT_AGENT_STATUS.to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: now,
            updated_at: now,
            preset_binding: None,
        };
        user_store.upsert_user_agent(&record).expect("upsert");

        service
            .sync_user_state("alice")
            .await
            .expect("sync to files");
        let private_root = service.private_root("alice");
        let worker_card_file = private_root
            .join("agents")
            .join("agent_demo.worker-card.json");
        assert!(worker_card_file.exists());

        std::thread::sleep(Duration::from_millis(20));
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read card"))
                .expect("parse card");
        document.metadata.name = "Edited".to_string();
        document.prompt.system_prompt = None;
        document.prompt.extra_prompt = Some("edited prompt".to_string());
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document).expect("serialize card"),
        )
        .expect("write card");

        service
            .sync_user_state("alice")
            .await
            .expect("sync from files");
        let updated = user_store
            .get_user_agent("alice", "agent_demo")
            .expect("get agent")
            .expect("agent exists");
        assert_eq!(updated.name, "Edited".to_string());
        assert_eq!(updated.system_prompt, "edited prompt".to_string());

        let scoped_user_id = workspace.scoped_user_id_by_container("alice", 0);
        assert_eq!(workspace.workspace_root(&scoped_user_id), private_root);
    }

    #[tokio::test]
    async fn sync_user_state_seeds_minimal_inner_visible_dirs() {
        let (_temp, _user_store, service, _workspace) = build_service();

        service.sync_user_state("bob").await.expect("sync");
        let private_root = service.private_root("bob");

        assert!(private_root.join("global").exists());
        assert!(private_root.join("agents").exists());
        assert!(private_root.join("skills").exists());
        assert!(private_root.join("knowledge").exists());
        assert!(!private_root.join(".wunder").exists());
    }

    #[tokio::test]
    async fn sync_user_state_applies_worker_card_self_updates_for_prompt_tools_and_skills() {
        let (_temp, user_store, service, _workspace) = build_service();
        let user_id = "alice";
        let agent_id = "agent_self_update";
        let now = now_ts();
        user_store
            .upsert_user_agent(&UserAgentRecord {
                agent_id: agent_id.to_string(),
                user_id: user_id.to_string(),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                name: "Initial".to_string(),
                description: "desc".to_string(),
                system_prompt: "initial prompt".to_string(),
                model_name: None,
                tool_names: Vec::new(),
                declared_tool_names: Vec::new(),
                declared_skill_names: Vec::new(),
                preset_questions: Vec::new(),
                access_level: "A".to_string(),
                approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
                is_shared: false,
                status: DEFAULT_AGENT_STATUS.to_string(),
                icon: None,
                sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
                created_at: now,
                updated_at: now,
                preset_binding: None,
            })
            .expect("seed agent");

        let private_root = service.private_root(user_id);
        let skill_name = "self_patch";
        let skill_alias = format!("{user_id}@{skill_name}");
        write_test_skill(&private_root.join("skills"), skill_name);
        service
            .user_tool_store
            .update_skills(user_id, vec![skill_name.to_string()], Vec::new())
            .expect("enable test skill");
        service.user_tool_manager.clear_skill_cache(Some(user_id));

        let config = service.config_store.get().await;
        let skills = service.skills.read().await.clone();
        let allowed = service
            .allowed_tool_names(user_id, &config, &skills)
            .expect("allowed tools");
        let selected_tool = pick_stable_allowed_tool(&allowed, Some(&skill_alias));

        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync");
        let worker_card_file = private_root
            .join("agents")
            .join(format!("{agent_id}.worker-card.json"));
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read worker card"))
                .expect("parse worker card");
        std::thread::sleep(Duration::from_millis(30));
        document.metadata.name = "Self Updated".to_string();
        document.prompt.system_prompt = None;
        document.prompt.extra_prompt = Some("updated prompt by self".to_string());
        document.abilities.tool_names = vec![selected_tool.clone()];
        document.abilities.skills = vec![skill_alias.clone()];
        document.interaction.preset_questions = vec!["Q1".to_string(), "Q2".to_string()];
        document.runtime.approval_mode = "suggest".to_string();
        document.runtime.sandbox_container_id = 4;
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document).expect("serialize worker card"),
        )
        .expect("write worker card");

        service
            .sync_user_state(user_id)
            .await
            .expect("sync updates");
        let updated = user_store
            .get_user_agent(user_id, agent_id)
            .expect("query agent")
            .expect("agent exists");
        assert_eq!(updated.name, "Self Updated");
        assert_eq!(updated.system_prompt, "updated prompt by self");
        assert_eq!(updated.declared_tool_names, vec![selected_tool.clone()]);
        assert_eq!(updated.declared_skill_names, vec![skill_alias.clone()]);
        assert!(updated.tool_names.contains(&selected_tool));
        if allowed.contains(&skill_alias) {
            assert!(updated.tool_names.contains(&skill_alias));
        } else {
            assert!(!updated.tool_names.contains(&skill_alias));
        }
        assert_eq!(
            updated.preset_questions,
            vec!["Q1".to_string(), "Q2".to_string()]
        );
        assert_eq!(updated.approval_mode, "suggest");
        assert_eq!(updated.sandbox_container_id, 4);
    }

    #[tokio::test]
    async fn sync_user_state_keeps_last_good_when_worker_card_becomes_invalid() {
        let (_temp, user_store, service, _workspace) = build_service();
        let user_id = "alice";
        let agent_id = "agent_invalid_card";
        let now = now_ts();
        user_store
            .upsert_user_agent(&UserAgentRecord {
                agent_id: agent_id.to_string(),
                user_id: user_id.to_string(),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                name: "Stable".to_string(),
                description: "desc".to_string(),
                system_prompt: "stable prompt".to_string(),
                model_name: None,
                tool_names: Vec::new(),
                declared_tool_names: Vec::new(),
                declared_skill_names: Vec::new(),
                preset_questions: Vec::new(),
                access_level: "A".to_string(),
                approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
                is_shared: false,
                status: DEFAULT_AGENT_STATUS.to_string(),
                icon: None,
                sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
                created_at: now,
                updated_at: now,
                preset_binding: None,
            })
            .expect("seed agent");
        service
            .sync_user_state(user_id)
            .await
            .expect("sync to files");

        let worker_card_file = service
            .private_root(user_id)
            .join("agents")
            .join(format!("{agent_id}.worker-card.json"));
        std::thread::sleep(Duration::from_millis(30));
        atomic_write_text(
            &worker_card_file,
            "{ \"schema_version\": \"wunder/worker-card@1\", \"prompt\": ",
        )
        .expect("write broken worker card");

        service
            .sync_user_state(user_id)
            .await
            .expect("sync should survive invalid file");
        let updated = user_store
            .get_user_agent(user_id, agent_id)
            .expect("query agent")
            .expect("agent exists");
        assert_eq!(updated.name, "Stable");
        assert_eq!(updated.system_prompt, "stable prompt");

        let repaired: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read repaired"))
                .expect("repaired worker card should be valid json");
        assert_eq!(
            repaired.prompt.extra_prompt,
            Some("stable prompt".to_string())
        );
        assert_eq!(repaired.metadata.name, "Stable");
    }

    #[tokio::test]
    async fn sync_user_state_applies_default_agent_worker_card_updates() {
        let (_temp, user_store, service, _workspace) = build_service();
        let user_id = "alice";
        let now = now_ts();
        user_store
            .upsert_user_agent(&UserAgentRecord {
                agent_id: DEFAULT_AGENT_ID_ALIAS.to_string(),
                user_id: user_id.to_string(),
                hive_id: DEFAULT_HIVE_ID.to_string(),
                name: "Default Initial".to_string(),
                description: "default desc".to_string(),
                system_prompt: "default prompt".to_string(),
                model_name: None,
                tool_names: Vec::new(),
                declared_tool_names: Vec::new(),
                declared_skill_names: Vec::new(),
                preset_questions: Vec::new(),
                access_level: "A".to_string(),
                approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
                is_shared: false,
                status: DEFAULT_AGENT_STATUS.to_string(),
                icon: None,
                sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
                created_at: now,
                updated_at: now,
                preset_binding: None,
            })
            .expect("seed default agent");

        let config = service.config_store.get().await;
        let skills = service.skills.read().await.clone();
        let allowed = service
            .allowed_tool_names(user_id, &config, &skills)
            .expect("allowed tools");
        let selected_tool = pick_stable_allowed_tool(&allowed, None);

        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync");
        let private_root = service.private_root(user_id);
        let default_card = private_root
            .join("agents")
            .join("__default__.worker-card.json");
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&default_card).expect("read default card"))
                .expect("parse default card");
        std::thread::sleep(Duration::from_millis(30));
        document.metadata.name = "Default Updated".to_string();
        document.prompt.system_prompt = None;
        document.prompt.extra_prompt = Some("default prompt updated".to_string());
        document.abilities.tool_names = vec![selected_tool.clone()];
        document.runtime.approval_mode = "auto_edit".to_string();
        document.runtime.sandbox_container_id = 3;
        atomic_write_text(
            &default_card,
            &serde_json::to_string_pretty(&document).expect("serialize default card"),
        )
        .expect("write default card");

        service
            .sync_user_state(user_id)
            .await
            .expect("apply default update");
        let updated = user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)
            .expect("query default agent")
            .expect("default agent exists");
        assert_eq!(updated.name, "Default Updated");
        assert_eq!(updated.system_prompt, "default prompt updated");
        assert_eq!(updated.tool_names, vec![selected_tool.clone()]);
        assert_eq!(updated.approval_mode, "auto_edit");
        assert_eq!(updated.sandbox_container_id, 3);

        let meta = user_store
            .get_meta(&default_agent_meta_key(user_id))
            .expect("read default meta")
            .expect("meta exists");
        let mirror: DefaultAgentConfigMirror =
            serde_json::from_str(&meta).expect("parse default meta");
        assert_eq!(mirror.name, "Default Updated");
        assert_eq!(mirror.system_prompt, "default prompt updated");
        assert_eq!(mirror.tool_names, vec![selected_tool]);
    }
}
