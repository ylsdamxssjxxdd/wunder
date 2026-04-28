mod layout;
mod worker_card;
pub use worker_card::{
    build_worker_card, parse_worker_card, WorkerCardDocument, WorkerCardPreset,
    WorkerCardRecordUpdate,
};

use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::atomic_write::atomic_write_text;
use crate::schemas::AbilityKind;
use crate::services::agent_abilities::normalize_ability_items;
use crate::services::default_agent_protocol::{
    default_agent_meta_key, record_from_default_agent_config,
    DefaultAgentConfig as DefaultAgentConfigMirror,
};
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
use crate::services::worker_card_files::worker_card_file_name as canonical_worker_card_file_name;
use crate::skills::{load_skills, SkillRegistry};
use crate::storage::{
    normalize_hive_id, UserAccountRecord, UserAgentRecord, DEFAULT_HIVE_ID,
    DEFAULT_SANDBOX_CONTAINER_ID,
};
use crate::user_store::UserStore;
use crate::user_tools::{UserToolBindings, UserToolKind, UserToolManager, UserToolStore};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, RwLock};
use tracing::warn;

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
    user_sync_locks: Arc<TokioMutex<HashMap<String, Arc<TokioMutex<()>>>>>,
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
            user_sync_locks: Arc::new(TokioMutex::new(HashMap::new())),
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
        remove_matching_worker_card_files(&paths, Some(agent_id), None)?;
        Ok(())
    }

    pub async fn sync_user_state(&self, user_id: &str) -> Result<()> {
        let user_lock = self.ensure_user_sync_lock(user_id).await;
        let _guard = user_lock.lock().await;
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
        self.ensure_declared_skills_enabled_from_worker_cards(user_id, &paths, &config)?;
        let skills = self.skills.read().await.clone();
        let bindings = self
            .user_tool_manager
            .build_bindings(&config, &skills, user_id);
        let allowed_tool_names = self.allowed_tool_names(user_id, &config, &skills, &bindings)?;
        let worker_card_skill_names = collect_worker_card_skill_names(&skills, &bindings);

        self.sync_default_agent(
            user_id,
            &paths,
            &allowed_tool_names,
            &worker_card_skill_names,
            &bindings,
        )?;
        self.sync_regular_agents(
            user_id,
            &paths,
            &allowed_tool_names,
            &worker_card_skill_names,
            &bindings,
        )?;
        Ok(())
    }

    async fn ensure_user_sync_lock(&self, user_id: &str) -> Arc<TokioMutex<()>> {
        let cleaned = user_id.trim();
        let mut locks = self.user_sync_locks.lock().await;
        locks
            .entry(cleaned.to_string())
            .or_insert_with(|| Arc::new(TokioMutex::new(())))
            .clone()
    }

    fn allowed_tool_names(
        &self,
        user_id: &str,
        config: &Config,
        skills: &SkillRegistry,
        bindings: &UserToolBindings,
    ) -> Result<HashSet<String>> {
        let user = self
            .user_store
            .get_user_by_id(user_id)?
            .unwrap_or_else(|| synthetic_user(user_id));
        let context = UserToolContext {
            config: config.clone(),
            skills: skills.clone(),
            bindings: bindings.clone(),
            tool_access: self.user_store.get_user_tool_access(user_id)?,
        };
        Ok(compute_allowed_tool_names(&user, &context))
    }

    fn ensure_declared_skills_enabled_from_worker_cards(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        config: &Config,
    ) -> Result<()> {
        let declared_skill_names = collect_declared_skill_names_from_worker_cards(paths)?;
        if declared_skill_names.is_empty() {
            return Ok(());
        }

        let payload = self.user_tool_store.load_user_tools(user_id);
        let mut enabled_set: HashSet<String> = payload.skills.enabled.iter().cloned().collect();
        let candidates = declared_skill_names
            .into_iter()
            .filter_map(|name| resolve_local_declared_skill_name(user_id, &name))
            .filter(|name| !enabled_set.contains(name))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(());
        }

        let skill_root = self.user_tool_store.get_skill_root(user_id);
        if !skill_root.exists() || !skill_root.is_dir() {
            return Ok(());
        }
        let mut scan_config = config.clone();
        scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
        scan_config.skills.enabled = Vec::new();
        let registry = load_skills(&scan_config, false, false, false);
        let available: HashSet<String> = registry
            .list_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        if available.is_empty() {
            return Ok(());
        }

        let mut changed = false;
        for name in candidates {
            if available.contains(&name) {
                changed |= enabled_set.insert(name);
            }
        }
        if !changed {
            return Ok(());
        }

        let mut next_enabled = enabled_set.into_iter().collect::<Vec<_>>();
        next_enabled.sort();
        self.user_tool_store
            .update_skills(user_id, next_enabled, payload.skills.shared.clone())?;
        self.user_tool_manager.clear_skill_cache(Some(user_id));
        Ok(())
    }

    fn sync_default_agent(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        allowed_tool_names: &HashSet<String>,
        worker_card_skill_names: &HashSet<String>,
        bindings: &UserToolBindings,
    ) -> Result<()> {
        let mut config = self.load_default_agent_config(user_id, allowed_tool_names)?;
        let has_persisted_state = self.has_persisted_default_agent_state(user_id)?;
        let worker_card_file = resolve_latest_worker_card_file(paths, Some(DEFAULT_AGENT_ID_ALIAS));
        let latest_file_mtime = worker_card_file
            .as_ref()
            .map(|path| file_modified_ts(path))
            .unwrap_or(0.0);

        // File changes win only when they are strictly newer than the runtime snapshot.
        if latest_file_mtime + FILE_TIME_EPSILON_S >= config.updated_at
            || (!has_persisted_state && latest_file_mtime > 0.0)
        {
            match self.apply_default_agent_file(
                user_id,
                worker_card_file
                    .as_deref()
                    .ok_or_else(|| anyhow!("default agent worker-card path missing during sync"))?,
                allowed_tool_names,
                &config,
                bindings,
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

    fn has_persisted_default_agent_state(&self, user_id: &str) -> Result<bool> {
        if self
            .user_store
            .get_meta(&default_agent_meta_key(user_id))?
            .is_some_and(|raw| !raw.trim().is_empty())
        {
            return Ok(true);
        }
        Ok(self
            .user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)?
            .is_some())
    }

    fn sync_regular_agents(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        allowed_tool_names: &HashSet<String>,
        worker_card_skill_names: &HashSet<String>,
        bindings: &UserToolBindings,
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
                .filter(|_| worker_card_mtime + FILE_TIME_EPSILON_S >= record_updated_at)
            {
                match self.apply_agent_files(
                    user_id,
                    &agent_id,
                    record.as_ref(),
                    worker_card_file,
                    allowed_tool_names,
                    bindings,
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
        bindings: &UserToolBindings,
    ) -> Result<UserAgentRecord> {
        let document = load_worker_card_document(worker_card_file)?;
        let mut parsed = parse_worker_card(document, None);
        let (resolved_declared_skill_names, renamed_skills) = resolve_runtime_declared_skill_names(
            user_id,
            &parsed.declared_skill_names,
            allowed_tool_names,
            bindings,
        );
        parsed.declared_skill_names = resolved_declared_skill_names.clone();
        if !renamed_skills.is_empty() {
            parsed.ability_items = remap_skill_ability_items(parsed.ability_items, &renamed_skills);
        }
        let mut runtime_tool_names = parsed.declared_tool_names.clone();
        runtime_tool_names.extend(resolved_declared_skill_names.iter().cloned());
        if runtime_tool_names.is_empty() {
            runtime_tool_names = parsed.tool_names.clone();
        }
        let now = now_ts();
        let mut record = existing.cloned().unwrap_or(UserAgentRecord {
            agent_id: agent_id.trim().to_string(),
            user_id: user_id.trim().to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: agent_id.trim().to_string(),
            description: String::new(),
            system_prompt: String::new(),
            preview_skill: false,
            model_name: None,
            ability_items: Vec::new(),
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
            silent: false,
            prefer_mother: false,
        });

        record.user_id = user_id.trim().to_string();
        record.agent_id = agent_id.trim().to_string();
        if !parsed.name.is_empty() {
            record.name = parsed.name;
        }
        record.description = parsed.description;
        record.system_prompt = parsed.system_prompt;
        record.preview_skill = parsed.preview_skill;
        record.model_name = parsed.model_name;
        record.ability_items = parsed.ability_items;
        record.declared_tool_names = parsed.declared_tool_names;
        record.declared_skill_names = resolved_declared_skill_names;
        record.tool_names = filter_allowed_tools(&runtime_tool_names, allowed_tool_names);
        record.preset_questions = normalize_preset_questions(parsed.preset_questions);
        record.approval_mode = normalize_agent_approval_mode(Some(&parsed.approval_mode));
        record.is_shared = parsed.is_shared;
        record.status = normalize_agent_status(Some(&record.status));
        record.icon = parsed.icon;
        record.sandbox_container_id = parsed.sandbox_container_id;
        record.silent = parsed.silent;
        record.prefer_mother = parsed.prefer_mother;
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
        let worker_card_file =
            worker_card_path(paths, Some(record.name.as_str()), Some(&record.agent_id));
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document).context("serialize worker card failed")?,
        )?;
        remove_matching_worker_card_files(paths, Some(&record.agent_id), Some(&worker_card_file))?;
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
                preview_skill: record.preview_skill,
                ability_items: record.ability_items,
                tool_names: record.tool_names,
                declared_tool_names: record.declared_tool_names,
                declared_skill_names: record.declared_skill_names,
                preset_questions: record.preset_questions,
                approval_mode: record.approval_mode,
                status: record.status,
                icon: record.icon,
                sandbox_container_id: record.sandbox_container_id,
                silent: record.silent,
                prefer_mother: record.prefer_mother,
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
            preview_skill: false,
            ability_items: Vec::new(),
            tool_names: curated_default_tool_names(allowed_tool_names),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            approval_mode: DEFAULT_AGENT_APPROVAL_MODE.to_string(),
            status: DEFAULT_AGENT_STATUS.to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            silent: false,
            prefer_mother: false,
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
        bindings: &UserToolBindings,
    ) -> Result<DefaultAgentConfigMirror> {
        let document = load_worker_card_document(worker_card_file)?;
        let mut parsed = parse_worker_card(document, None);
        let (resolved_declared_skill_names, renamed_skills) = resolve_runtime_declared_skill_names(
            user_id,
            &parsed.declared_skill_names,
            allowed_tool_names,
            bindings,
        );
        parsed.declared_skill_names = resolved_declared_skill_names.clone();
        if !renamed_skills.is_empty() {
            parsed.ability_items = remap_skill_ability_items(parsed.ability_items, &renamed_skills);
        }
        let mut runtime_tool_names = parsed.declared_tool_names.clone();
        runtime_tool_names.extend(resolved_declared_skill_names.iter().cloned());
        if runtime_tool_names.is_empty() {
            runtime_tool_names = parsed.tool_names.clone();
        }
        let now = now_ts();
        let mut config = current.clone();
        if !parsed.name.is_empty() {
            config.name = parsed.name;
        }
        config.description = parsed.description;
        config.system_prompt = parsed.system_prompt;
        config.preview_skill = parsed.preview_skill;
        config.ability_items = parsed.ability_items;
        config.tool_names = filter_allowed_tools(&runtime_tool_names, allowed_tool_names);
        config.declared_tool_names = parsed.declared_tool_names;
        config.declared_skill_names = resolved_declared_skill_names;
        config.preset_questions = normalize_preset_questions(parsed.preset_questions);
        config.approval_mode = normalize_agent_approval_mode(Some(&parsed.approval_mode));
        config.status = normalize_agent_status(Some(DEFAULT_AGENT_STATUS));
        config.icon = parsed.icon;
        config.sandbox_container_id = parsed.sandbox_container_id;
        config.silent = parsed.silent;
        config.prefer_mother = parsed.prefer_mother;
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
    config.ability_items = normalize_ability_items(std::mem::take(&mut config.ability_items));
    config.declared_tool_names =
        normalize_tool_list(std::mem::take(&mut config.declared_tool_names));
    config.declared_skill_names =
        normalize_tool_list(std::mem::take(&mut config.declared_skill_names));
    let mut selected_tool_names = normalize_tool_list(std::mem::take(&mut config.tool_names));
    selected_tool_names.extend(
        config
            .declared_tool_names
            .iter()
            .filter(|name| allowed_tool_names.contains(*name))
            .cloned(),
    );
    selected_tool_names.extend(
        config
            .declared_skill_names
            .iter()
            .filter(|name| allowed_tool_names.contains(*name))
            .cloned(),
    );
    config.tool_names = filter_allowed_tools(
        &normalize_tool_list(selected_tool_names),
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
    record_from_default_agent_config(DEFAULT_AGENT_ID_ALIAS, user_id, "A", config)
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

fn matching_worker_card_files(
    paths: &InnerVisiblePaths,
    agent_id: Option<&str>,
) -> Result<Vec<std::path::PathBuf>> {
    let mut output = Vec::new();
    let normalized_agent_id = normalize_agent_file_stem(agent_id);
    if paths.agents_dir.exists() {
        for entry in fs::read_dir(&paths.agents_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let file_name = entry.file_name().to_string_lossy().to_string();
            if agent_id_from_worker_card_file_name(&file_name)
                .as_deref()
                .is_some_and(|parsed| parsed == normalized_agent_id)
            {
                output.push(entry.path());
            }
        }
    }
    let legacy = legacy_worker_card_path(paths, Some(&normalized_agent_id));
    if legacy.exists() {
        output.push(legacy);
    }
    output.sort();
    output.dedup();
    Ok(output)
}

fn remove_matching_worker_card_files(
    paths: &InnerVisiblePaths,
    agent_id: Option<&str>,
    keep_path: Option<&Path>,
) -> Result<()> {
    for candidate in matching_worker_card_files(paths, agent_id)? {
        if keep_path.is_some_and(|path| path == candidate.as_path()) {
            continue;
        }
        remove_path_if_exists(&candidate)?;
    }
    remove_path_if_exists(&legacy_worker_card_dir(paths, agent_id))?;
    Ok(())
}

fn resolve_latest_worker_card_file(
    paths: &InnerVisiblePaths,
    agent_id: Option<&str>,
) -> Option<std::path::PathBuf> {
    let mut candidates = matching_worker_card_files(paths, agent_id).ok()?;
    if candidates.is_empty() {
        let fallback = paths.agents_dir.join(canonical_worker_card_file_name(
            None,
            Some(&normalize_agent_file_stem(agent_id)),
        ));
        if fallback.exists() {
            candidates.push(fallback);
        }
    }
    candidates
        .into_iter()
        .max_by(|left, right| file_modified_ts(left).total_cmp(&file_modified_ts(right)))
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

fn collect_declared_skill_names_from_worker_cards(
    paths: &InnerVisiblePaths,
) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let default_worker_card = defaults_worker_card_path(paths);
    if default_worker_card.exists() {
        files.push(default_worker_card);
    }
    if let Some(path) = resolve_latest_worker_card_file(paths, Some(DEFAULT_AGENT_ID_ALIAS)) {
        files.push(path);
    }
    for agent_id in discover_agent_ids(&paths.agents_dir)? {
        if let Some(path) = resolve_latest_worker_card_file(paths, Some(&agent_id)) {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();

    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for file in files {
        match load_worker_card_document(&file) {
            Ok(document) => {
                let parsed = parse_worker_card(document, None);
                for raw_name in parsed.declared_skill_names {
                    let cleaned = raw_name.trim();
                    if cleaned.is_empty() {
                        continue;
                    }
                    let owned = cleaned.to_string();
                    if seen.insert(owned.clone()) {
                        output.push(owned);
                    }
                }
            }
            Err(err) => {
                warn!(
                    "skip worker-card skill extraction due to parse error {}: {err}",
                    file.display()
                );
            }
        }
    }
    Ok(output)
}

fn resolve_local_declared_skill_name(user_id: &str, skill_name: &str) -> Option<String> {
    let cleaned = skill_name.trim();
    if cleaned.is_empty() {
        return None;
    }
    let owned_prefix = format!("{}@", user_id.trim());
    if let Some(target) = cleaned.strip_prefix(&owned_prefix) {
        let normalized_target = target.trim();
        return (!normalized_target.is_empty()).then(|| normalized_target.to_string());
    }
    if cleaned.contains('@') {
        return None;
    }
    Some(cleaned.to_string())
}

fn resolve_runtime_declared_skill_names(
    user_id: &str,
    declared_skill_names: &[String],
    allowed_tool_names: &HashSet<String>,
    bindings: &UserToolBindings,
) -> (Vec<String>, HashMap<String, String>) {
    let normalized_user_id = user_id.trim();
    let mut owned_alias_by_target = HashMap::new();
    let mut aliases_by_target: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, info) in &bindings.alias_map {
        if !matches!(info.kind, UserToolKind::Skill) {
            continue;
        }
        let alias_name = alias.trim();
        if alias_name.is_empty() || !allowed_tool_names.contains(alias_name) {
            continue;
        }
        let target_name = info.target.trim();
        if target_name.is_empty() {
            continue;
        }
        aliases_by_target
            .entry(target_name.to_string())
            .or_default()
            .push(alias_name.to_string());
        if info.owner_id.trim() == normalized_user_id {
            owned_alias_by_target
                .entry(target_name.to_string())
                .or_insert_with(|| alias_name.to_string());
        }
    }

    let mut seen = HashSet::new();
    let mut resolved = Vec::new();
    let mut renamed = HashMap::new();
    for raw_name in declared_skill_names {
        let cleaned = raw_name.trim();
        if cleaned.is_empty() {
            continue;
        }
        let mut runtime_name = cleaned.to_string();
        if !allowed_tool_names.contains(cleaned) {
            if let Some(alias) = owned_alias_by_target.get(cleaned) {
                runtime_name = alias.clone();
            } else if let Some(candidates) = aliases_by_target.get(cleaned) {
                let mut unique_candidates = candidates.clone();
                unique_candidates.sort();
                unique_candidates.dedup();
                if unique_candidates.len() == 1 {
                    runtime_name = unique_candidates[0].clone();
                }
            }
        }
        if seen.insert(runtime_name.clone()) {
            resolved.push(runtime_name.clone());
        }
        if runtime_name != cleaned {
            renamed.insert(cleaned.to_string(), runtime_name);
        }
    }
    (resolved, renamed)
}

fn remap_skill_ability_items(
    items: Vec<crate::schemas::AbilityDescriptor>,
    renamed_skill_names: &HashMap<String, String>,
) -> Vec<crate::schemas::AbilityDescriptor> {
    if renamed_skill_names.is_empty() {
        return items;
    }
    let mut output = Vec::with_capacity(items.len());
    for mut item in items {
        if item.kind != AbilityKind::Skill {
            output.push(item);
            continue;
        }
        let runtime_name = item.runtime_name.trim().to_string();
        let fallback_name = item.name.trim().to_string();
        let resolved_name = if runtime_name.is_empty() {
            fallback_name.clone()
        } else {
            runtime_name.clone()
        };
        let replacement = renamed_skill_names
            .get(&resolved_name)
            .or_else(|| renamed_skill_names.get(&runtime_name))
            .or_else(|| renamed_skill_names.get(&fallback_name))
            .cloned();
        let Some(replacement) = replacement else {
            output.push(item);
            continue;
        };
        item.runtime_name = replacement.clone();
        if fallback_name.is_empty() || fallback_name == resolved_name {
            item.name = replacement.clone();
        }
        if item.display_name.trim().is_empty() || item.display_name.trim() == resolved_name {
            item.display_name = replacement.clone();
        }
        if item.id.trim().is_empty()
            || item.id.starts_with("skill:")
            || item.id.starts_with("user_skill:")
        {
            item.id = format!("skill:{replacement}");
        }
        output.push(item);
    }
    normalize_ability_items(output)
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
        token_balance: 0,
        token_granted_total: 0,
        token_used_total: 0,
        last_token_grant_date: None,
        experience_total: 0,
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

    async fn build_service() -> (
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
        let config_store = ConfigStore::new(temp.path().join("override.yaml"));
        let workspace_root_value = workspace_root.to_string_lossy().to_string();
        let config = config_store
            .update(|config| {
                config.workspace.root = workspace_root_value.clone();
                config.tools.builtin.enabled = vec!["读取文件".to_string()];
            })
            .await
            .expect("configure config store");
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

    fn find_worker_card_file(private_root: &Path, agent_id: &str) -> std::path::PathBuf {
        let agents_dir = private_root.join("agents");
        if let Ok(entries) = fs::read_dir(&agents_dir) {
            let mut matched = entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    entry.file_type().ok().and_then(|file_type| {
                        if !file_type.is_file() {
                            return None;
                        }
                        agent_id_from_worker_card_file_name(&file_name)
                            .as_deref()
                            .is_some_and(|parsed| parsed == agent_id)
                            .then(|| entry.path())
                    })
                })
                .collect::<Vec<_>>();
            matched.sort();
            if let Some(path) = matched.into_iter().next_back() {
                return path;
            }
        }
        agents_dir.join(format!("{agent_id}.worker-card.json"))
    }

    #[tokio::test]
    async fn sync_user_state_materializes_and_applies_agent_files() {
        let (_temp, user_store, service, workspace) = build_service().await;
        let now = now_ts();
        let record = UserAgentRecord {
            agent_id: "agent_demo".to_string(),
            user_id: "alice".to_string(),
            hive_id: DEFAULT_HIVE_ID.to_string(),
            name: "Demo".to_string(),
            description: "desc".to_string(),
            system_prompt: "initial prompt".to_string(),
            preview_skill: false,
            model_name: Some("gpt-5".to_string()),
            ability_items: Vec::new(),
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
            silent: false,
            prefer_mother: false,
        };
        user_store.upsert_user_agent(&record).expect("upsert");

        service
            .sync_user_state("alice")
            .await
            .expect("sync to files");
        let private_root = service.private_root("alice");
        let worker_card_file = find_worker_card_file(&private_root, "agent_demo");
        assert!(worker_card_file.exists());

        std::thread::sleep(Duration::from_millis(20));
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read card"))
                .expect("parse card");
        document.metadata.name = "Edited".to_string();
        document.prompt.system_prompt = None;
        document.extra_prompt = Some("edited prompt".to_string());
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
        let (_temp, _user_store, service, _workspace) = build_service().await;

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
        let (_temp, user_store, service, _workspace) = build_service().await;
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
                preview_skill: false,
                model_name: None,
                ability_items: Vec::new(),
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
                silent: false,
                prefer_mother: false,
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
        let bindings = service
            .user_tool_manager
            .build_bindings(&config, &skills, user_id);
        let allowed = service
            .allowed_tool_names(user_id, &config, &skills, &bindings)
            .expect("allowed tools");
        let selected_tool = pick_stable_allowed_tool(&allowed, Some(&skill_alias));

        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync");
        let worker_card_file = find_worker_card_file(&private_root, agent_id);
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read worker card"))
                .expect("parse worker card");
        std::thread::sleep(Duration::from_millis(30));
        document.metadata.name = "Self Updated".to_string();
        document.prompt.system_prompt = None;
        document.extra_prompt = Some("updated prompt by self".to_string());
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
        let (_temp, user_store, service, _workspace) = build_service().await;
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
                preview_skill: false,
                model_name: None,
                ability_items: Vec::new(),
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
                silent: false,
                prefer_mother: false,
            })
            .expect("seed agent");
        service
            .sync_user_state(user_id)
            .await
            .expect("sync to files");

        let private_root = service.private_root(user_id);
        let worker_card_file = find_worker_card_file(&private_root, agent_id);
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
        assert_eq!(repaired.extra_prompt, Some("stable prompt".to_string()));
        assert_eq!(repaired.metadata.name, "Stable");
    }

    #[tokio::test]
    async fn sync_user_state_applies_default_agent_worker_card_updates() {
        let (_temp, user_store, service, _workspace) = build_service().await;
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
                preview_skill: false,
                model_name: None,
                ability_items: Vec::new(),
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
                silent: false,
                prefer_mother: false,
            })
            .expect("seed default agent");

        let private_root = service.private_root(user_id);
        let skill_name = "default_sync_skill";
        let skill_alias = format!("{user_id}@{skill_name}");
        write_test_skill(&private_root.join("skills"), skill_name);
        service
            .user_tool_store
            .update_skills(user_id, vec![skill_name.to_string()], Vec::new())
            .expect("enable test skill");
        service.user_tool_manager.clear_skill_cache(Some(user_id));

        let config = service.config_store.get().await;
        let skills = service.skills.read().await.clone();
        let bindings = service
            .user_tool_manager
            .build_bindings(&config, &skills, user_id);
        let allowed = service
            .allowed_tool_names(user_id, &config, &skills, &bindings)
            .expect("allowed tools");
        let selected_tool = pick_stable_allowed_tool(&allowed, Some(&skill_alias));

        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync");
        let default_card = find_worker_card_file(&private_root, DEFAULT_AGENT_ID_ALIAS);
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&default_card).expect("read default card"))
                .expect("parse default card");
        std::thread::sleep(Duration::from_millis(30));
        document.metadata.name = "Default Updated".to_string();
        document.prompt.system_prompt = None;
        document.extra_prompt = Some("default prompt updated".to_string());
        document.abilities.tool_names = vec![selected_tool.clone()];
        document.abilities.skills = vec![skill_alias.clone()];
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
        let updated_with_skill = user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)
            .expect("query default agent")
            .expect("default agent exists");
        assert_eq!(updated_with_skill.name, "Default Updated");
        assert_eq!(updated_with_skill.system_prompt, "default prompt updated");
        assert_eq!(
            updated_with_skill.declared_tool_names,
            vec![selected_tool.clone()]
        );
        assert_eq!(
            updated_with_skill.declared_skill_names,
            vec![skill_alias.clone()]
        );
        assert!(updated_with_skill.tool_names.contains(&selected_tool));
        if allowed.contains(&skill_alias) {
            assert!(updated_with_skill.tool_names.contains(&skill_alias));
        }
        assert_eq!(updated_with_skill.approval_mode, "auto_edit");
        assert_eq!(updated_with_skill.sandbox_container_id, 3);

        let refreshed_default_card = find_worker_card_file(&private_root, DEFAULT_AGENT_ID_ALIAS);
        let mut document: WorkerCardDocument = serde_json::from_str(
            &fs::read_to_string(&refreshed_default_card).expect("read updated default card"),
        )
        .expect("parse updated default card");
        std::thread::sleep(Duration::from_millis(30));
        document.abilities.skills = Vec::new();
        atomic_write_text(
            &refreshed_default_card,
            &serde_json::to_string_pretty(&document).expect("serialize default card"),
        )
        .expect("rewrite default card without skill");

        service
            .sync_user_state(user_id)
            .await
            .expect("remove default skill update");
        let updated = user_store
            .get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)
            .expect("query default agent after skill removal")
            .expect("default agent exists after skill removal");
        assert_eq!(updated.name, "Default Updated");
        assert_eq!(updated.system_prompt, "default prompt updated");
        assert_eq!(updated.declared_tool_names, vec![selected_tool.clone()]);
        assert!(updated.declared_skill_names.is_empty());
        assert!(updated.tool_names.contains(&selected_tool));
        assert!(!updated.tool_names.contains(&skill_alias));
        assert!(updated
            .ability_items
            .iter()
            .all(|item| item.runtime_name != skill_alias));

        let meta = user_store
            .get_meta(&default_agent_meta_key(user_id))
            .expect("read default meta")
            .expect("meta exists");
        let mirror: DefaultAgentConfigMirror =
            serde_json::from_str(&meta).expect("parse default meta");
        assert_eq!(mirror.name, "Default Updated");
        assert_eq!(mirror.system_prompt, "default prompt updated");
        assert_eq!(mirror.tool_names, vec![selected_tool.clone()]);
        assert_eq!(mirror.declared_tool_names, vec![selected_tool.clone()]);
        assert!(mirror.declared_skill_names.is_empty());
        assert!(mirror
            .ability_items
            .iter()
            .all(|item| item.runtime_name != skill_alias));

        let rewritten_default_card = find_worker_card_file(&private_root, DEFAULT_AGENT_ID_ALIAS);
        let rewritten: WorkerCardDocument = serde_json::from_str(
            &fs::read_to_string(&rewritten_default_card).expect("read rewritten default card"),
        )
        .expect("parse rewritten default card");
        assert_eq!(rewritten.abilities.tool_names, vec![selected_tool]);
        assert!(rewritten.abilities.skills.is_empty());
    }

    #[tokio::test]
    async fn sync_user_state_applies_default_agent_worker_card_updates_without_prior_save() {
        let (_temp, user_store, service, _workspace) = build_service().await;
        let user_id = "carol";

        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync without persisted default agent");

        let private_root = service.private_root(user_id);
        let default_card = find_worker_card_file(&private_root, DEFAULT_AGENT_ID_ALIAS);
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&default_card).expect("read default card"))
                .expect("parse default card");
        std::thread::sleep(Duration::from_millis(30));
        document.metadata.name = "Unsaved Default Updated".to_string();
        document.extra_prompt = Some("prompt from worker card".to_string());
        atomic_write_text(
            &default_card,
            &serde_json::to_string_pretty(&document).expect("serialize default card"),
        )
        .expect("write default card");

        service
            .sync_user_state(user_id)
            .await
            .expect("sync should honor worker card without prior save");

        let meta = user_store
            .get_meta(&default_agent_meta_key(user_id))
            .expect("read default meta")
            .expect("meta should exist");
        let mirror: DefaultAgentConfigMirror =
            serde_json::from_str(&meta).expect("parse default meta");
        assert_eq!(mirror.name, "Unsaved Default Updated");
        assert_eq!(mirror.system_prompt, "prompt from worker card");
    }

    #[tokio::test]
    async fn sync_user_state_auto_enables_declared_local_skill_and_maps_alias() {
        let (_temp, user_store, service, _workspace) = build_service().await;
        let user_id = "dave";
        service
            .sync_user_state(user_id)
            .await
            .expect("initial sync for default files");

        let private_root = service.private_root(user_id);
        let skill_name = "weather";
        let skill_alias = format!("{user_id}@{skill_name}");
        write_test_skill(&private_root.join("skills"), skill_name);

        let default_card = find_worker_card_file(&private_root, DEFAULT_AGENT_ID_ALIAS);
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&default_card).expect("read default card"))
                .expect("parse default card");
        std::thread::sleep(Duration::from_millis(30));
        document.abilities.tool_names = Vec::new();
        document.abilities.skills = vec![skill_name.to_string()];
        atomic_write_text(
            &default_card,
            &serde_json::to_string_pretty(&document).expect("serialize worker card"),
        )
        .expect("write worker card with declared local skill");

        service
            .sync_user_state(user_id)
            .await
            .expect("sync local skill declaration");

        let payload = service.user_tool_store.load_user_tools(user_id);
        assert!(
            payload.skills.enabled.iter().any(|name| name == skill_name),
            "declared local skill should be auto-enabled"
        );

        let meta = user_store
            .get_meta(&default_agent_meta_key(user_id))
            .expect("read default meta")
            .expect("meta should exist");
        let mirror: DefaultAgentConfigMirror =
            serde_json::from_str(&meta).expect("parse default meta");
        assert_eq!(mirror.declared_skill_names, vec![skill_alias.clone()]);
        assert!(mirror.tool_names.contains(&skill_alias));

        let rewritten: WorkerCardDocument = serde_json::from_str(
            &fs::read_to_string(&default_card).expect("read rewritten default card"),
        )
        .expect("parse rewritten default card");
        assert_eq!(rewritten.abilities.skills, vec![skill_alias]);
    }
}
