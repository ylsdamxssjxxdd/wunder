mod layout;
mod worker_card;

use crate::config::Config;
use crate::config_store::ConfigStore;
use crate::core::atomic_write::atomic_write_text;
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, DEFAULT_AGENT_NAME};
use crate::services::default_tool_profile::curated_default_tool_names;
use crate::services::inner_visible::layout::{
    agent_diagnostics_path, agent_effective_path, defaults_worker_card_path,
    global_diagnostics_path, global_effective_path, last_good_agent_dir, system_prompt_path,
    tooling_path, user_paths, worker_card_path, InnerVisiblePaths, AGENTS_DIR,
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
use crate::user_tools::{UserToolManager, UserToolStore, UserToolsPayload};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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
const GLOBAL_README_TEXT: &str = "# Global Configuration\n\nThis folder stores user-level shared configuration.\n\n- `tooling.json`: user MCP/skill sharing and tooling policy\n- `defaults.worker-card.json`: default worker-card profile for the user\n";
const AGENTS_README_TEXT: &str = "# Agent Configuration\n\nEach subfolder maps to one agent.\n\n- `worker-card.json`: primary declarative profile\n- `system_prompt.md`: editable prompt source\n";
const AGENT_README_TEXT: &str = "# Agent Files\n\n- `worker-card.json`: declarative profile\n- `system_prompt.md`: prompt source\n- `skills/`: agent-private skills (reserved)\n";

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
            &paths.inner_visible_dir,
            &paths.effective_dir,
            &paths.diagnostics_dir,
            &paths.last_good_dir,
            &paths.effective_dir.join(AGENTS_DIR),
            &paths.diagnostics_dir.join(AGENTS_DIR),
            &paths.last_good_dir.join("global"),
            &paths.last_good_dir.join(AGENTS_DIR),
        ] {
            fs::create_dir_all(dir)?;
        }
        ensure_file_if_missing(paths.global_dir.join("README.md"), GLOBAL_README_TEXT)?;
        ensure_file_if_missing(paths.agents_dir.join("README.md"), AGENTS_README_TEXT)?;
        Ok(paths)
    }

    pub fn remove_agent_files(&self, user_id: &str, agent_id: &str) -> Result<()> {
        let paths = self.ensure_scaffold(user_id)?;
        let agent_root = worker_card_path(&paths, Some(agent_id))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("invalid agent worker-card path"))?;
        remove_path_if_exists(&agent_root)?;
        remove_path_if_exists(&agent_effective_path(&paths, Some(agent_id)))?;
        remove_path_if_exists(&agent_diagnostics_path(&paths, Some(agent_id)))?;
        remove_path_if_exists(&last_good_agent_dir(&paths, Some(agent_id)))?;
        Ok(())
    }

    pub async fn sync_user_state(&self, user_id: &str) -> Result<()> {
        let paths = self.ensure_scaffold(user_id)?;
        self.user_store.ensure_default_hive(user_id)?;
        self.user_tool_store.ensure_materialized(user_id)?;
        let tooling_valid = self.validate_tooling_file(&paths)?;
        let defaults_valid = self.validate_defaults_worker_card_file(&paths)?;
        let payload = self.user_tool_store.load_user_tools(user_id);
        self.write_global_effective(&paths, &payload)?;
        if tooling_valid && defaults_valid {
            self.clear_diagnostic(global_diagnostics_path(&paths))?;
        }

        let config = self.config_store.get().await;
        let skills = self.skills.read().await.clone();
        let allowed_tool_names = self.allowed_tool_names(user_id, &config, &skills)?;

        self.sync_default_agent(user_id, &paths, &allowed_tool_names)?;
        self.sync_regular_agents(user_id, &paths, &allowed_tool_names)?;
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
        let bindings = self.user_tool_manager.build_bindings(config, skills, user_id);
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
    ) -> Result<()> {
        let mut config = self.load_default_agent_config(user_id, allowed_tool_names)?;
        let worker_card_file = worker_card_path(paths, Some(DEFAULT_AGENT_ID_ALIAS));
        let prompt_file = system_prompt_path(paths, Some(DEFAULT_AGENT_ID_ALIAS));
        let latest_file_mtime = latest_mtime([worker_card_file.as_path(), prompt_file.as_path()]);

        // File changes win only when they are strictly newer than the runtime snapshot.
        if latest_file_mtime > config.updated_at + FILE_TIME_EPSILON_S {
            match self.apply_default_agent_file(
                user_id,
                &worker_card_file,
                &prompt_file,
                allowed_tool_names,
                &config,
            ) {
                Ok(updated) => {
                    config = updated;
                    self.clear_diagnostic(global_diagnostics_path(paths))?;
                }
                Err(err) => {
                    self.write_diagnostic(
                        global_diagnostics_path(paths),
                        "error",
                        "default agent file sync failed",
                        json!({ "error": err.to_string() }),
                    )?;
                    warn!("default agent inner-visible sync failed for {user_id}: {err}");
                }
            }
        }

        self.write_default_agent_files(user_id, paths, &config)?;
        self.write_effective_snapshot(
            agent_effective_path(paths, Some(DEFAULT_AGENT_ID_ALIAS)),
            &record_from_default_config(user_id, &config),
            "default_agent",
        )?;
        Ok(())
    }

    fn sync_regular_agents(
        &self,
        user_id: &str,
        paths: &InnerVisiblePaths,
        allowed_tool_names: &HashSet<String>,
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
        agent_ids.extend(discover_agent_dirs(&paths.agents_dir)?);

        for agent_id in agent_ids {
            if agent_id == DEFAULT_AGENT_ID_ALIAS {
                continue;
            }
            let record = by_id.remove(&agent_id);
            let worker_card_file = worker_card_path(paths, Some(&agent_id));
            let prompt_file = system_prompt_path(paths, Some(&agent_id));
            let worker_card_mtime = file_modified_ts(&worker_card_file);
            let prompt_mtime = file_modified_ts(&prompt_file);
            let latest_file_mtime = worker_card_mtime.max(prompt_mtime);
            let record_updated_at = record.as_ref().map(|item| item.updated_at).unwrap_or(0.0);

            let final_record = if worker_card_file.exists()
                && latest_file_mtime > record_updated_at + FILE_TIME_EPSILON_S
            {
                match self.apply_agent_files(
                    user_id,
                    &agent_id,
                    record.as_ref(),
                    &worker_card_file,
                    &prompt_file,
                    allowed_tool_names,
                ) {
                    Ok(updated) => {
                        self.clear_diagnostic(agent_diagnostics_path(paths, Some(&agent_id)))?;
                        updated
                    }
                    Err(err) => {
                        self.write_diagnostic(
                            agent_diagnostics_path(paths, Some(&agent_id)),
                            "error",
                            "agent file sync failed",
                            json!({ "agent_id": agent_id, "error": err.to_string() }),
                        )?;
                        warn!("agent inner-visible sync failed for {user_id}/{agent_id}: {err}");
                        if let Some(existing) = record {
                            existing
                        } else {
                            continue;
                        }
                    }
                }
            } else if !worker_card_file.exists()
                && prompt_file.exists()
                && prompt_mtime > record_updated_at + FILE_TIME_EPSILON_S
            {
                match self.apply_prompt_only_update(user_id, &agent_id, record.as_ref(), &prompt_file)
                {
                    Ok(updated) => {
                        self.clear_diagnostic(agent_diagnostics_path(paths, Some(&agent_id)))?;
                        updated
                    }
                    Err(err) => {
                        self.write_diagnostic(
                            agent_diagnostics_path(paths, Some(&agent_id)),
                            "error",
                            "system prompt sync failed",
                            json!({ "agent_id": agent_id, "error": err.to_string() }),
                        )?;
                        warn!("agent prompt-only sync failed for {user_id}/{agent_id}: {err}");
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

            self.write_agent_files(paths, &final_record)?;
            self.write_effective_snapshot(
                agent_effective_path(paths, Some(&agent_id)),
                &final_record,
                "agent",
            )?;
        }
        Ok(())
    }

    fn apply_prompt_only_update(
        &self,
        user_id: &str,
        agent_id: &str,
        existing: Option<&UserAgentRecord>,
        prompt_file: &Path,
    ) -> Result<UserAgentRecord> {
        let mut record = existing
            .cloned()
            .ok_or_else(|| anyhow!("system_prompt.md cannot create agent without worker-card.json"))?;
        record.user_id = user_id.trim().to_string();
        record.agent_id = agent_id.trim().to_string();
        record.system_prompt = fs::read_to_string(prompt_file)
            .with_context(|| format!("read system prompt failed: {}", prompt_file.display()))?
            .trim()
            .to_string();
        record.updated_at = now_ts();
        self.user_store.upsert_user_agent(&record)?;
        Ok(record)
    }

    fn apply_agent_files(
        &self,
        user_id: &str,
        agent_id: &str,
        existing: Option<&UserAgentRecord>,
        worker_card_file: &Path,
        prompt_file: &Path,
        allowed_tool_names: &HashSet<String>,
    ) -> Result<UserAgentRecord> {
        let document = load_worker_card_document(worker_card_file)?;
        let prompt_override = load_prompt_override(worker_card_file, prompt_file)?;
        let parsed = parse_worker_card(document, prompt_override);
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

    fn write_agent_files(&self, paths: &InnerVisiblePaths, record: &UserAgentRecord) -> Result<()> {
        let hive = self.user_store.get_hive(&record.user_id, &record.hive_id)?;
        let document = build_worker_card(
            record,
            hive.as_ref().map(|item| item.name.as_str()),
            hive.as_ref().map(|item| item.description.as_str()),
        );
        let agent_root = worker_card_path(paths, Some(&record.agent_id))
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("invalid agent path"))?;
        fs::create_dir_all(agent_root.join("skills"))?;
        ensure_file_if_missing(agent_root.join("README.md"), AGENT_README_TEXT)?;
        let worker_card_file = worker_card_path(paths, Some(&record.agent_id));
        let prompt_file = system_prompt_path(paths, Some(&record.agent_id));
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document)
                .context("serialize worker card failed")?,
        )?;
        atomic_write_text(&prompt_file, &record.system_prompt)?;
        let backup_dir = last_good_agent_dir(paths, Some(&record.agent_id));
        fs::create_dir_all(&backup_dir)?;
        atomic_write_text(
            &backup_dir.join("worker-card.json"),
            &serde_json::to_string_pretty(&document)?,
        )?;
        atomic_write_text(&backup_dir.join("system_prompt.md"), &record.system_prompt)?;
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
        if let Some(record) = self.user_store.get_user_agent(user_id, DEFAULT_AGENT_ID_ALIAS)? {
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
        prompt_file: &Path,
        allowed_tool_names: &HashSet<String>,
        current: &DefaultAgentConfigMirror,
    ) -> Result<DefaultAgentConfigMirror> {
        let document = load_worker_card_document(worker_card_file)?;
        let prompt_override = load_prompt_override(worker_card_file, prompt_file)?;
        let parsed = parse_worker_card(document, prompt_override);
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
        self.user_store
            .set_meta(&default_agent_meta_key(user_id), &serde_json::to_string(config)?)?;
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
    ) -> Result<()> {
        let record = record_from_default_config(user_id, config);
        self.write_agent_files(paths, &record)?;
        let defaults_document = build_worker_card(&record, Some("Default Hive"), Some(""));
        let defaults_content = serde_json::to_string_pretty(&defaults_document)?;
        atomic_write_text(&defaults_worker_card_path(paths), &defaults_content)?;
        let backup_dir = paths.last_good_dir.join("global");
        fs::create_dir_all(&backup_dir)?;
        atomic_write_text(&backup_dir.join("defaults.worker-card.json"), &defaults_content)?;
        Ok(())
    }

    fn write_global_effective(
        &self,
        paths: &InnerVisiblePaths,
        payload: &UserToolsPayload,
    ) -> Result<()> {
        let effective = json!({
            "status": "ok",
            "updated_at": Utc::now().to_rfc3339(),
            "tooling": payload,
            "paths": {
                "tooling": tooling_path(paths),
                "defaults_worker_card": defaults_worker_card_path(paths),
            }
        });
        atomic_write_text(
            &global_effective_path(paths),
            &serde_json::to_string_pretty(&effective)?,
        )?;
        Ok(())
    }

    fn validate_tooling_file(&self, paths: &InnerVisiblePaths) -> Result<bool> {
        let path = tooling_path(paths);
        if !path.exists() {
            return Ok(true);
        }
        let content = fs::read_to_string(&path)?;
        match serde_json::from_str::<Value>(&content) {
            Ok(_) => {
                let backup_dir = paths.last_good_dir.join("global");
                fs::create_dir_all(&backup_dir)?;
                atomic_write_text(&backup_dir.join("tooling.json"), &content)?;
                Ok(true)
            }
            Err(err) => {
                self.write_diagnostic(
                    global_diagnostics_path(paths),
                    "error",
                    "tooling config parse failed",
                    json!({
                        "path": path,
                        "error": err.to_string()
                    }),
                )?;
                Ok(false)
            }
        }
    }

    fn validate_defaults_worker_card_file(&self, paths: &InnerVisiblePaths) -> Result<bool> {
        let path = defaults_worker_card_path(paths);
        if !path.exists() {
            return Ok(true);
        }
        let content = fs::read_to_string(&path)?;
        match serde_json::from_str::<WorkerCardDocument>(&content) {
            Ok(_) => {
                let backup_dir = paths.last_good_dir.join("global");
                fs::create_dir_all(&backup_dir)?;
                atomic_write_text(&backup_dir.join("defaults.worker-card.json"), &content)?;
                Ok(true)
            }
            Err(err) => {
                self.write_diagnostic(
                    global_diagnostics_path(paths),
                    "error",
                    "global defaults worker-card parse failed",
                    json!({
                        "path": path,
                        "error": err.to_string()
                    }),
                )?;
                Ok(false)
            }
        }
    }

    fn write_effective_snapshot(
        &self,
        path: std::path::PathBuf,
        record: &UserAgentRecord,
        source: &str,
    ) -> Result<()> {
        let payload = json!({
            "status": "ok",
            "source": source,
            "updated_at": Utc::now().to_rfc3339(),
            "record": {
                "agent_id": &record.agent_id,
                "user_id": &record.user_id,
                "hive_id": &record.hive_id,
                "name": &record.name,
                "description": &record.description,
                "system_prompt": &record.system_prompt,
                "model_name": &record.model_name,
                "tool_names": &record.tool_names,
                "declared_tool_names": &record.declared_tool_names,
                "declared_skill_names": &record.declared_skill_names,
                "preset_questions": &record.preset_questions,
                "approval_mode": &record.approval_mode,
                "is_shared": record.is_shared,
                "status": &record.status,
                "icon": &record.icon,
                "sandbox_container_id": record.sandbox_container_id,
                "created_at": record.created_at,
                "updated_at": record.updated_at
            }
        });
        atomic_write_text(&path, &serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    fn write_diagnostic(
        &self,
        path: std::path::PathBuf,
        status: &str,
        message: &str,
        details: Value,
    ) -> Result<()> {
        let payload = json!({
            "status": status,
            "message": message,
            "updated_at": Utc::now().to_rfc3339(),
            "details": details
        });
        atomic_write_text(&path, &serde_json::to_string_pretty(&payload)?)?;
        Ok(())
    }

    fn clear_diagnostic(&self, path: std::path::PathBuf) -> Result<()> {
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
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
    let content =
        fs::read_to_string(path).with_context(|| format!("read worker card failed: {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("parse worker card failed: {}", path.display()))
}

fn load_prompt_override(worker_card_path: &Path, prompt_path: &Path) -> Result<Option<String>> {
    if !prompt_path.exists() {
        return Ok(None);
    }
    if file_modified_ts(prompt_path) + FILE_TIME_EPSILON_S < file_modified_ts(worker_card_path) {
        return Ok(None);
    }
    Ok(Some(
        fs::read_to_string(prompt_path)
            .with_context(|| format!("read system prompt failed: {}", prompt_path.display()))?,
    ))
}

fn discover_agent_dirs(root: &Path) -> Result<Vec<String>> {
    let mut output = Vec::new();
    if !root.exists() {
        return Ok(output);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().trim().to_string();
        if !name.is_empty() {
            output.push(name);
        }
    }
    Ok(output)
}

fn latest_mtime<'a>(paths: impl IntoIterator<Item = &'a Path>) -> f64 {
    let mut latest = 0.0_f64;
    for path in paths {
        latest = latest.max(file_modified_ts(path));
    }
    latest
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

fn ensure_file_if_missing(path: std::path::PathBuf, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_write_text(&path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::user_tools::UserToolManager;
    use crate::skills::load_skills;
    use crate::storage::{SqliteStorage, StorageBackend};
    use crate::workspace::WorkspaceManager;
    use std::collections::HashMap;
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

        service.sync_user_state("alice").await.expect("sync to files");
        let private_root = service.private_root("alice");
        let worker_card_file = private_root
            .join("agents")
            .join("agent_demo")
            .join("worker-card.json");
        let prompt_file = private_root
            .join("agents")
            .join("agent_demo")
            .join("system_prompt.md");
        assert!(worker_card_file.exists());
        assert!(prompt_file.exists());

        std::thread::sleep(Duration::from_millis(20));
        let mut document: WorkerCardDocument =
            serde_json::from_str(&fs::read_to_string(&worker_card_file).expect("read card"))
                .expect("parse card");
        document.metadata.name = "Edited".to_string();
        atomic_write_text(
            &worker_card_file,
            &serde_json::to_string_pretty(&document).expect("serialize card"),
        )
        .expect("write card");
        atomic_write_text(&prompt_file, "edited prompt").expect("write prompt");

        service.sync_user_state("alice").await.expect("sync from files");
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
    async fn sync_user_state_seeds_readme_and_global_last_good_files() {
        let (_temp, _user_store, service, _workspace) = build_service();

        service.sync_user_state("bob").await.expect("sync");
        let private_root = service.private_root("bob");

        assert!(private_root.join("global").join("README.md").exists());
        assert!(private_root.join("agents").join("README.md").exists());
        assert!(
            private_root
                .join(".wunder")
                .join("last_good")
                .join("global")
                .join("tooling.json")
                .exists()
        );
        assert!(
            private_root
                .join(".wunder")
                .join("last_good")
                .join("global")
                .join("defaults.worker-card.json")
                .exists()
        );
    }
}
