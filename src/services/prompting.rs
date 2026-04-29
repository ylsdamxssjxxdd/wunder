// 系统提示词构建：模板渲染、工具描述拼接与缓存管理。
use crate::config::Config;
use crate::i18n;
use crate::llm::ToolCallMode;
use crate::schemas::ToolSpec;
use crate::services::default_agent_sync::DEFAULT_AGENT_ID_ALIAS;
use crate::services::tools::skill_call::render_skill_markdown_for_model;
use crate::services::user_prompt_templates;
use crate::services::worker_card_files::{
    stable_id_from_worker_card_file_name, worker_card_file_name as canonical_worker_card_file_name,
};
use crate::skills::{SkillRegistry, SkillSpec};
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::tools::{
    builtin_aliases, collect_available_tool_names, collect_prompt_tool_specs,
    render_prompt_tool_spec, resolve_tool_name,
};
use crate::user_tools::UserToolBindings;
use crate::workspace::WorkspaceManager;
use chrono::Local;
use parking_lot::Mutex;
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
#[cfg(not(target_vendor = "win7"))]
use sysinfo::System;
#[cfg(target_vendor = "win7")]
use sysinfo::{System, SystemExt};
use tokio::sync::{Mutex as TokioMutex, Notify};

const DEFAULT_CACHE_TTL_S: f64 = 10.0;
const DEFAULT_CACHE_MAX_ITEMS: usize = 128;
const SYSTEM_PROMPT_ROLE_PATH: &str = "prompts/system/role.txt";
const SYSTEM_PROMPT_ENGINEERING_PATH: &str = "prompts/system/engineering.txt";
const SYSTEM_PROMPT_TOOLS_PROTOCOL_PATH: &str = "prompts/system/tools_protocol.txt";
const SYSTEM_PROMPT_SKILLS_PROTOCOL_PATH: &str = "prompts/system/skills_protocol.txt";
const SYSTEM_PROMPT_INNER_VISIBLE_PROTOCOL_PATH: &str = "prompts/system/inner_visible_protocol.txt";
const SYSTEM_PROMPT_MEMORY_PATH: &str = "prompts/system/memory.txt";
const SYSTEM_PROMPT_EXTRA_PATH: &str = "prompts/system/extra.txt";
pub const SYSTEM_PROMPT_MEMORY_PLACEHOLDER: &str = "<<WUNDER_HISTORY_MEMORY>>";

static SYSTEM_PROMPT_TEMPLATES_REVISION: AtomicU64 = AtomicU64::new(0);

/// Bump the in-memory revision used by the system prompt builder cache.
///
/// Admin prompt template packs are edited via API and stored on disk. Instead of
/// hitting filesystem metadata on every system prompt build (expensive on bind
/// mounts), we bump this revision on writes so the prompt cache is invalidated
/// immediately and cheaply.
pub fn bump_system_prompt_templates_revision() {
    SYSTEM_PROMPT_TEMPLATES_REVISION.fetch_add(1, Ordering::Relaxed);
}

pub fn system_prompt_templates_revision() -> u64 {
    SYSTEM_PROMPT_TEMPLATES_REVISION.load(Ordering::Relaxed)
}

pub struct PromptComposer {
    cache: Mutex<PromptCache>,
    tool_cache: Mutex<ToolSpecCache>,
    ttl_s: f64,
    max_items: usize,
    inflight: TokioMutex<HashMap<String, InflightEntry>>,
}

struct PromptCacheEntry {
    prompt: String,
    timestamp: f64,
}

#[derive(Default)]
struct PromptCache {
    entries: HashMap<String, PromptCacheEntry>,
    order: VecDeque<String>,
}

struct ToolSpecCacheEntry {
    specs: Vec<ToolSpec>,
    timestamp: f64,
}

#[derive(Default)]
struct ToolSpecCache {
    entries: HashMap<String, ToolSpecCacheEntry>,
    order: VecDeque<String>,
}

struct InflightEntry {
    notify: Arc<Notify>,
    waiters: usize,
}

pub fn read_prompt_template(path: &Path) -> String {
    read_prompt_template_with_exists(path, None).0
}

fn read_prompt_template_with_exists(path: &Path, locale_override: Option<&str>) -> (String, bool) {
    let locale = resolve_requested_prompt_locale(locale_override).unwrap_or("default");
    let cache_key = format!("{locale}|{}", path.to_string_lossy());
    let revision = system_prompt_templates_revision();

    let cache = prompt_file_cache();
    {
        let cache = cache.lock();
        if let Some(entry) = cache.get(&cache_key) {
            if entry.revision == revision {
                return (entry.text.clone(), entry.exists);
            }
        }
    }

    // Admin-managed system prompt templates are edited via API and we bump
    // `SYSTEM_PROMPT_TEMPLATES_REVISION` on writes, so we can avoid filesystem
    // metadata checks on hot paths (bind-mount `stat` can be expensive).
    let resolved = resolve_prompt_path(path, locale_override);
    let (text, exists) = match std::fs::read_to_string(&resolved) {
        Ok(text) => (text, true),
        Err(_) => (String::new(), false),
    };
    cache.lock().insert(
        cache_key,
        PromptFileCacheEntry {
            revision,
            text: text.clone(),
            exists,
        },
    );
    (text, exists)
}

pub fn read_prompt_template_from_active_pack(config: &Config, path: &Path) -> String {
    let template_id = resolve_prompt_template_id(config);
    read_prompt_template_from_pack(config, &template_id, path, None)
}

impl PromptComposer {
    pub fn new(ttl_s: f64, max_items: usize) -> Self {
        Self {
            cache: Mutex::new(PromptCache::default()),
            tool_cache: Mutex::new(ToolSpecCache::default()),
            ttl_s: if ttl_s <= 0.0 {
                DEFAULT_CACHE_TTL_S
            } else {
                ttl_s
            },
            max_items: if max_items == 0 {
                DEFAULT_CACHE_MAX_ITEMS
            } else {
                max_items
            },
            inflight: TokioMutex::new(HashMap::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn build_system_prompt_cached(
        &self,
        config: &Config,
        config_version: u64,
        workspace: &WorkspaceManager,
        workspace_id: &str,
        prompt_owner_user_id: &str,
        current_agent_id: Option<&str>,
        workdir: &Path,
        overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        tool_call_mode: ToolCallMode,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        agent_prompt: Option<&str>,
        preview_skill: bool,
    ) -> String {
        let tool_key = build_tool_key(allowed_tool_names);
        let language = i18n::get_language();
        let tool_mode_key = match tool_call_mode {
            ToolCallMode::FunctionCall => "function_call",
            ToolCallMode::ToolCall => "tool_call",
            ToolCallMode::FreeformCall => "freeform_call",
        };
        let user_tool_version = user_tool_bindings
            .map(|item| item.user_version)
            .unwrap_or(0.0);
        let shared_tool_version = user_tool_bindings
            .map(|item| item.shared_version)
            .unwrap_or(0.0);
        let overrides_key = build_overrides_key(overrides);
        let agent_prompt_key = build_prompt_key(agent_prompt);
        let current_agent_key = normalize_inner_visible_agent_id(current_agent_id);
        let prompt_template_scope = resolve_prompt_template_scope(config, prompt_owner_user_id);
        let prompt_template_key = build_prompt_template_cache_key(&prompt_template_scope);
        let templates_revision = system_prompt_templates_revision();
        let workdir_key = workdir.to_string_lossy();
        let preview_skill_key = if preview_skill { "preview" } else { "meta" };
        let base_key = format!(
            "{workspace_id}|{current_agent_key}|{config_version}|{prompt_template_key}|{templates_revision}|{workdir_key}|{overrides_key}|{tool_key}|{tool_mode_key}|{user_tool_version}|{shared_tool_version}|{agent_prompt_key}|{preview_skill_key}|{language}"
        );
        let workspace_version = workspace.get_tree_cache_version(workspace_id);
        let cache_key = format!("{base_key}|{workspace_version}");
        let now = now_ts();
        if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
            return prompt;
        }

        loop {
            let (notify, is_leader) = {
                let mut inflight = self.inflight.lock().await;
                if let Some(entry) = inflight.get_mut(&base_key) {
                    entry.waiters = entry.waiters.saturating_add(1);
                    (entry.notify.clone(), false)
                } else {
                    let notify = Arc::new(Notify::new());
                    inflight.insert(
                        base_key.clone(),
                        InflightEntry {
                            notify: notify.clone(),
                            waiters: 0,
                        },
                    );
                    (notify, true)
                }
            };

            if !is_leader {
                notify.notified().await;
                let workspace_version = workspace.get_tree_cache_version(workspace_id);
                let cache_key = format!("{base_key}|{workspace_version}");
                let now = now_ts();
                if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                    return prompt;
                }
                continue;
            }

            let workspace_version = workspace.get_tree_cache_version(workspace_id);
            let cache_key = format!("{base_key}|{workspace_version}");
            let now = now_ts();
            if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                self.notify_inflight(&base_key).await;
                return prompt;
            }

            let tree_snapshot = workspace.get_workspace_tree_snapshot(workspace_id);
            let workspace_version = tree_snapshot.version;
            let cache_key = format!("{base_key}|{workspace_version}");
            let workspace_tree = tree_snapshot.tree;
            let include_ptc = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "ptc");
            let tool_specs = if tool_call_mode != ToolCallMode::FunctionCall
                && !allowed_tool_names.is_empty()
            {
                let tool_cache_key = format!(
                    "{config_version}|{user_tool_version}|{shared_tool_version}|{language}|{tool_key}"
                );
                let now = now_ts();
                if let Some(specs) = self.get_cached_tool_specs(&tool_cache_key, now) {
                    specs
                } else {
                    let specs = collect_prompt_tool_specs(
                        config,
                        skills,
                        allowed_tool_names,
                        user_tool_bindings,
                    );
                    self.insert_cached_tool_specs(tool_cache_key, specs.clone(), now);
                    specs
                }
            } else {
                Vec::new()
            };
            let tool_specs = if is_local_runtime_mode(&config.server.mode) {
                localize_tool_specs_for_local_runtime(tool_specs, workspace, workspace_id)
            } else {
                tool_specs
            };
            let workdir_display = if is_local_runtime_mode(&config.server.mode) {
                absolute_path_str(workdir)
            } else {
                workspace.display_path(workspace_id, workdir)
            };
            let base_skill_specs = skills.list_specs();
            let builtin_skills_for_prompt =
                filter_skill_specs(&base_skill_specs, allowed_tool_names);
            let mut user_skills_for_prompt = Vec::new();
            if let Some(bindings) = user_tool_bindings {
                if !bindings.skill_specs.is_empty() {
                    let user_skills = filter_skill_specs(&bindings.skill_specs, allowed_tool_names);
                    if !user_skills.is_empty() {
                        user_skills_for_prompt = user_skills;
                    }
                }
            }

            let prompt = build_system_prompt_skeleton(
                config,
                &prompt_template_scope,
                allowed_tool_names,
                tool_call_mode,
                &tool_specs,
                include_ptc,
                &workdir_display,
                &workspace_tree,
                &builtin_skills_for_prompt,
                &user_skills_for_prompt,
                agent_prompt,
                preview_skill,
                &build_inner_visible_prompt_mapping(
                    workspace,
                    prompt_owner_user_id,
                    current_agent_id,
                    is_local_runtime_mode(&config.server.mode),
                ),
            );

            self.insert_cached_prompt(cache_key, prompt.clone(), now_ts());
            self.notify_inflight(&base_key).await;
            return prompt;
        }
    }

    async fn notify_inflight(&self, key: &str) {
        let entry = {
            let mut inflight = self.inflight.lock().await;
            inflight.remove(key)
        };
        if let Some(entry) = entry {
            for _ in 0..entry.waiters {
                entry.notify.notify_one();
            }
        }
    }

    pub fn resolve_allowed_tool_names(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        tool_names: &[String],
        user_tool_bindings: Option<&UserToolBindings>,
    ) -> HashSet<String> {
        let selected = normalize_tool_names(tool_names);
        if selected.is_empty() {
            return HashSet::new();
        }
        let available = collect_available_tool_names(config, skills, user_tool_bindings);
        selected
            .into_iter()
            .filter(|name| available.contains(name))
            .collect()
    }

    fn get_cached_prompt(&self, key: &str, now: f64) -> Option<String> {
        let cache = self.cache.lock();
        let entry = cache.entries.get(key)?;
        if now - entry.timestamp > self.ttl_s {
            return None;
        }
        Some(entry.prompt.clone())
    }

    fn insert_cached_prompt(&self, key: String, prompt: String, now: f64) {
        let mut cache = self.cache.lock();
        cache.entries.insert(
            key.clone(),
            PromptCacheEntry {
                prompt,
                timestamp: now,
            },
        );
        cache.order.push_back(key);
        while cache.order.len() > self.max_items {
            if let Some(old_key) = cache.order.pop_front() {
                cache.entries.remove(&old_key);
            }
        }
    }

    fn get_cached_tool_specs(&self, key: &str, now: f64) -> Option<Vec<ToolSpec>> {
        let cache = self.tool_cache.lock();
        let entry = cache.entries.get(key)?;
        if now - entry.timestamp > self.ttl_s {
            return None;
        }
        Some(entry.specs.clone())
    }

    fn insert_cached_tool_specs(&self, key: String, specs: Vec<ToolSpec>, now: f64) {
        let mut cache = self.tool_cache.lock();
        cache.entries.insert(
            key.clone(),
            ToolSpecCacheEntry {
                specs,
                timestamp: now,
            },
        );
        cache.order.push_back(key);
        while cache.order.len() > self.max_items {
            if let Some(old_key) = cache.order.pop_front() {
                cache.entries.remove(&old_key);
            }
        }
    }
}

#[derive(Debug, Clone)]
struct PromptTemplateScope {
    system_pack_id: String,
    user_pack_id: Option<String>,
    owner_user_id: Option<String>,
    locale_override: Option<String>,
}

fn resolve_prompt_template_id(config: &Config) -> String {
    let value = config.prompt_templates.active.trim();
    if value.is_empty() {
        "default".to_string()
    } else {
        value.to_string()
    }
}

fn resolve_prompt_template_scope(
    config: &Config,
    prompt_owner_user_id: &str,
) -> PromptTemplateScope {
    let system_pack_id = resolve_prompt_template_id(config);
    let user_pack_id =
        user_prompt_templates::load_user_active_pack_id(config, prompt_owner_user_id);
    if let Some(locale) = user_prompt_templates::builtin_user_pack_locale(&user_pack_id) {
        return PromptTemplateScope {
            system_pack_id,
            user_pack_id: None,
            owner_user_id: None,
            locale_override: Some(locale.to_string()),
        };
    }
    if user_pack_id.eq_ignore_ascii_case(user_prompt_templates::DEFAULT_PACK_ID) {
        return PromptTemplateScope {
            system_pack_id,
            user_pack_id: None,
            owner_user_id: None,
            locale_override: None,
        };
    }
    PromptTemplateScope {
        system_pack_id,
        user_pack_id: Some(user_pack_id),
        owner_user_id: Some(prompt_owner_user_id.trim().to_string()),
        locale_override: None,
    }
}

fn build_prompt_template_cache_key(scope: &PromptTemplateScope) -> String {
    let system_pack = scope.system_pack_id.trim();
    let system_pack = if system_pack.is_empty() {
        user_prompt_templates::DEFAULT_PACK_ID
    } else {
        system_pack
    };
    if let Some(user_pack_id) = scope.user_pack_id.as_deref() {
        let owner = scope
            .owner_user_id
            .as_deref()
            .map(user_prompt_templates::safe_user_prompt_key)
            .unwrap_or_else(|| "anonymous".to_string());
        let locale = scope.locale_override.as_deref().unwrap_or("default");
        return format!("user:{owner}:{user_pack_id}|system:{system_pack}|locale:{locale}");
    }
    let locale = scope.locale_override.as_deref().unwrap_or("default");
    format!("system:{system_pack}|locale:{locale}")
}

fn resolve_prompt_template_root(config: &Config, template_id: &str) -> PathBuf {
    if template_id.trim().eq_ignore_ascii_case("default") {
        return PathBuf::from(".");
    }
    let root = config.prompt_templates.root.trim();
    let root = if root.is_empty() {
        PathBuf::from("./config/data/prompt_templates")
    } else {
        PathBuf::from(root)
    };
    let root = if root.is_absolute() {
        root
    } else {
        user_prompt_templates::resolve_prompts_root().join(root)
    };
    root.join(template_id.trim())
}

fn read_prompt_template_from_pack(
    config: &Config,
    template_id: &str,
    path: &Path,
    locale_override: Option<&str>,
) -> String {
    let template_id = template_id.trim();
    let is_default = template_id.is_empty() || template_id.eq_ignore_ascii_case("default");
    if !is_default {
        let pack_root = resolve_prompt_template_root(config, template_id);
        let candidate = pack_root.join(path);
        let (text, exists) = read_prompt_template_with_exists(&candidate, locale_override);
        if exists {
            return text;
        }
    }
    read_prompt_template_with_exists(path, locale_override).0
}

fn read_prompt_template_from_scope(
    config: &Config,
    scope: &PromptTemplateScope,
    path: &Path,
) -> String {
    if let (Some(user_pack_id), Some(owner_user_id)) = (
        scope.user_pack_id.as_deref(),
        scope.owner_user_id.as_deref(),
    ) {
        let user_pack_root =
            user_prompt_templates::resolve_user_pack_root(config, owner_user_id, user_pack_id);
        let user_candidate = user_pack_root.join(path);
        let (text, exists) =
            read_prompt_template_with_exists(&user_candidate, scope.locale_override.as_deref());
        if exists {
            return text;
        }
    }
    read_prompt_template_from_pack(
        config,
        &scope.system_pack_id,
        path,
        scope.locale_override.as_deref(),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_system_prompt_skeleton(
    config: &Config,
    template_scope: &PromptTemplateScope,
    allowed_tool_names: &HashSet<String>,
    tool_call_mode: ToolCallMode,
    tools: &[ToolSpec],
    include_ptc: bool,
    workdir_display: &str,
    workspace_tree: &str,
    builtin_skills: &[SkillSpec],
    user_skills: &[SkillSpec],
    agent_prompt: Option<&str>,
    preview_skill: bool,
    inner_visible_mapping: &HashMap<String, String>,
) -> String {
    let os_name = system_name();
    let date_str = Local::now().format("%Y-%m-%d").to_string();

    let role =
        read_prompt_template_from_scope(config, template_scope, Path::new(SYSTEM_PROMPT_ROLE_PATH));

    let mut engineering_flags = HashMap::new();
    let is_local = is_local_runtime_mode(&config.server.mode);
    engineering_flags.insert("RUNTIME_LOCAL".to_string(), is_local);
    engineering_flags.insert("RUNTIME_SERVER".to_string(), !is_local);
    engineering_flags.insert("HAS_PTC".to_string(), include_ptc);

    let engineering_template = read_prompt_template_from_scope(
        config,
        template_scope,
        Path::new(SYSTEM_PROMPT_ENGINEERING_PATH),
    );
    let engineering_template = apply_prompt_flags(&engineering_template, &engineering_flags);
    let engineering = render_template(
        &engineering_template,
        &HashMap::from([
            ("OS".to_string(), os_name),
            ("DATE".to_string(), date_str),
            ("DIR".to_string(), workdir_display.to_string()),
            ("WORKSPACE_TREE".to_string(), workspace_tree.to_string()),
        ]),
    );

    let inner_visible_template = read_prompt_template_from_scope(
        config,
        template_scope,
        Path::new(SYSTEM_PROMPT_INNER_VISIBLE_PROTOCOL_PATH),
    );
    let inner_visible_block = render_template(&inner_visible_template, inner_visible_mapping);

    // When `tool_call_mode=function_call`, tool specs and invocation protocol are
    // provided by the LLM API itself, so we omit the entire tools-protocol block
    // to keep the system prompt minimal.
    let tools_block = if tool_call_mode != ToolCallMode::FunctionCall {
        let tools_text = if !tools.is_empty() {
            tools
                .iter()
                .map(|spec| render_tool_spec(spec, tool_call_mode == ToolCallMode::FreeformCall))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        let mut tools_flags = HashMap::new();
        tools_flags.insert(
            "TOOL_CALL_MODE_TOOL_CALL".to_string(),
            tool_call_mode == ToolCallMode::ToolCall,
        );
        tools_flags.insert(
            "TOOL_CALL_MODE_FUNCTION_CALL".to_string(),
            tool_call_mode == ToolCallMode::FunctionCall,
        );
        tools_flags.insert(
            "TOOL_CALL_MODE_FREEFORM_CALL".to_string(),
            tool_call_mode == ToolCallMode::FreeformCall,
        );
        let has_a2ui = allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "a2ui");
        let has_plan = allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "计划面板");
        let has_question_panel = allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "问询面板");
        let has_subagent_control = allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "子智能体控制");
        let has_sessions_yield = allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "会话让出");
        tools_flags.insert("HAS_A2UI_TOOL".to_string(), has_a2ui);
        tools_flags.insert("HAS_PLAN_TOOL".to_string(), has_plan);
        tools_flags.insert("HAS_QUESTION_PANEL_TOOL".to_string(), has_question_panel);
        tools_flags.insert(
            "HAS_SUBAGENT_CONTROL_TOOL".to_string(),
            has_subagent_control,
        );
        tools_flags.insert("HAS_SESSIONS_YIELD_TOOL".to_string(), has_sessions_yield);
        let tools_template = read_prompt_template_from_scope(
            config,
            template_scope,
            Path::new(SYSTEM_PROMPT_TOOLS_PROTOCOL_PATH),
        );
        let tools_template = apply_prompt_flags(&tools_template, &tools_flags);
        render_template(
            &tools_template,
            &HashMap::from([("available_tools_describe".to_string(), tools_text)]),
        )
    } else {
        String::new()
    };

    let skills_block = if builtin_skills.is_empty() && user_skills.is_empty() {
        String::new()
    } else {
        let builtin_skills_list = if preview_skill {
            render_preview_skill_list_or_placeholder(builtin_skills)
        } else {
            render_skill_list_or_placeholder(builtin_skills)
        };
        let user_skills_list = if preview_skill {
            render_preview_skill_list_or_placeholder(user_skills)
        } else {
            render_skill_list_or_placeholder(user_skills)
        };
        let skills_template = read_prompt_template_from_scope(
            config,
            template_scope,
            Path::new(SYSTEM_PROMPT_SKILLS_PROTOCOL_PATH),
        );
        render_template(
            &skills_template,
            &HashMap::from([
                ("WORKDIR".to_string(), workdir_display.to_string()),
                ("BUILTIN_SKILLS_LIST".to_string(), builtin_skills_list),
                ("USER_SKILLS_LIST".to_string(), user_skills_list),
            ]),
        )
    };

    let memory_template = read_prompt_template_from_scope(
        config,
        template_scope,
        Path::new(SYSTEM_PROMPT_MEMORY_PATH),
    );
    let memory_block = render_template(
        &memory_template,
        &HashMap::from([(
            "HISTORY_MEMORY".to_string(),
            SYSTEM_PROMPT_MEMORY_PLACEHOLDER.to_string(),
        )]),
    );

    let mut blocks = vec![
        role,
        engineering,
        inner_visible_block,
        tools_block,
        skills_block,
        memory_block,
    ];
    blocks.retain(|value| !value.trim().is_empty());
    if !blocks
        .iter()
        .any(|value| value.contains(SYSTEM_PROMPT_MEMORY_PLACEHOLDER))
    {
        blocks.push(SYSTEM_PROMPT_MEMORY_PLACEHOLDER.to_string());
    }

    if let Some(extra) = agent_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let extra_template = read_prompt_template_from_scope(
            config,
            template_scope,
            Path::new(SYSTEM_PROMPT_EXTRA_PATH),
        );
        let extra_block = render_template(
            &extra_template,
            &HashMap::from([("EXTRA_PROMPT".to_string(), extra.to_string())]),
        );
        if !extra_block.trim().is_empty() {
            blocks.push(extra_block);
        }
    }

    blocks.join("\n\n")
}

fn build_inner_visible_prompt_mapping(
    workspace: &WorkspaceManager,
    prompt_owner_user_id: &str,
    current_agent_id: Option<&str>,
    is_local_runtime: bool,
) -> HashMap<String, String> {
    let private_workspace_id =
        workspace.scoped_user_id_by_container(prompt_owner_user_id, USER_PRIVATE_CONTAINER_ID);
    let private_root = workspace.workspace_root(&private_workspace_id);
    let global_dir = private_root.join("global");
    let skills_dir = private_root.join("skills");
    let knowledge_dir = private_root.join("knowledge");
    let agents_dir = private_root.join("agents");
    let global_tooling_file = global_dir.join("tooling.json");
    let global_defaults_card = global_dir.join("defaults.worker-card.json");
    let current_agent_id = normalize_inner_visible_agent_id(current_agent_id);
    let current_agent_card = resolve_inner_visible_agent_card_path(&agents_dir, &current_agent_id);
    let default_agent_card =
        resolve_inner_visible_agent_card_path(&agents_dir, DEFAULT_AGENT_ID_ALIAS);
    let default_agent_note_path = default_agent_card
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!("agents/{name}"))
        .unwrap_or_else(|| "agents/__default__.worker-card.json".to_string());
    let default_agent_only_note = if current_agent_id == DEFAULT_AGENT_ID_ALIAS {
        format!("当前为默认智能体 / default agent in use: {default_agent_note_path}")
    } else {
        format!(
            "当前为普通智能体 / non-default agent: do not edit {default_agent_note_path} unless user explicitly asks"
        )
    };

    let display = |path: &Path| -> String {
        resolve_inner_visible_display_path(workspace, &private_workspace_id, path, is_local_runtime)
    };

    HashMap::from([
        ("INNER_VISIBLE_ROOT".to_string(), display(&private_root)),
        ("INNER_VISIBLE_GLOBAL_DIR".to_string(), display(&global_dir)),
        ("INNER_VISIBLE_SKILLS_DIR".to_string(), display(&skills_dir)),
        (
            "INNER_VISIBLE_KNOWLEDGE_DIR".to_string(),
            display(&knowledge_dir),
        ),
        ("INNER_VISIBLE_AGENTS_DIR".to_string(), display(&agents_dir)),
        (
            "INNER_VISIBLE_GLOBAL_TOOLING_FILE".to_string(),
            display(&global_tooling_file),
        ),
        (
            "INNER_VISIBLE_GLOBAL_DEFAULTS_CARD".to_string(),
            display(&global_defaults_card),
        ),
        (
            "INNER_VISIBLE_CURRENT_AGENT_ID".to_string(),
            current_agent_id,
        ),
        (
            "INNER_VISIBLE_CURRENT_AGENT_CARD".to_string(),
            display(&current_agent_card),
        ),
        (
            "INNER_VISIBLE_DEFAULT_AGENT_ONLY_NOTE".to_string(),
            default_agent_only_note,
        ),
    ])
}

fn normalize_inner_visible_agent_id(agent_id: Option<&str>) -> String {
    let cleaned = agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_AGENT_ID_ALIAS);
    if cleaned.eq_ignore_ascii_case("default") {
        DEFAULT_AGENT_ID_ALIAS.to_string()
    } else {
        cleaned.to_string()
    }
}

fn resolve_inner_visible_agent_card_path(agents_dir: &Path, agent_id: &str) -> PathBuf {
    if let Ok(entries) = std::fs::read_dir(agents_dir) {
        let mut matched = entries
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                if !file_type.is_file() {
                    return None;
                }
                let file_name = entry.file_name().to_string_lossy().to_string();
                let stable_id = stable_id_from_worker_card_file_name(&file_name)?;
                (stable_id == agent_id).then(|| entry.path())
            })
            .collect::<Vec<_>>();
        matched.sort();
        if let Some(path) = matched.into_iter().next_back() {
            return path;
        }
    }
    agents_dir.join(canonical_worker_card_file_name(None, Some(agent_id)))
}

fn resolve_inner_visible_display_path(
    workspace: &WorkspaceManager,
    private_workspace_id: &str,
    path: &Path,
    is_local_runtime: bool,
) -> String {
    let raw = if is_local_runtime {
        absolute_path_str(path)
    } else {
        workspace.display_path(private_workspace_id, path)
    };
    raw.replace('\\', "/")
}

fn is_local_runtime_mode(server_mode: &str) -> bool {
    matches!(
        server_mode.trim().to_ascii_lowercase().as_str(),
        "cli" | "desktop"
    )
}

fn render_skill_list(skills: &[SkillSpec]) -> String {
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let mut lines = Vec::new();
    for spec in sorted {
        lines.push(String::new());
        lines.push(format!("- {}", spec.name));
        lines.push(format!(
            "  SKILL.md: {}",
            absolute_path_str_from_text(&spec.path)
        ));
        if !spec.frontmatter.trim().is_empty() {
            lines.push("  Frontmatter:".to_string());
            for raw_line in spec.frontmatter.lines() {
                let line = raw_line.trim();
                lines.push(format!("    {line}"));
            }
        }
    }
    lines.join("\n").trim().to_string()
}

fn render_skill_list_or_placeholder(skills: &[SkillSpec]) -> String {
    if skills.is_empty() {
        "- (none)".to_string()
    } else {
        render_skill_list(skills)
    }
}

fn render_preview_skill_list(skills: &[SkillSpec]) -> String {
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    let mut blocks = Vec::new();
    for spec in sorted {
        let raw = match std::fs::read_to_string(&spec.path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let skill_path = absolute_path_str_from_text(&spec.path);
        let skill_root = absolute_path_str(&spec.root);
        let rendered = render_skill_markdown_for_model(&raw, &skill_root);
        blocks.push(format!(
            "## {}\nSKILL.md: {}\n\n{}",
            spec.name,
            skill_path,
            rendered.trim()
        ));
    }
    blocks.join("\n\n").trim().to_string()
}

fn render_preview_skill_list_or_placeholder(skills: &[SkillSpec]) -> String {
    if skills.is_empty() {
        "- (none)".to_string()
    } else {
        render_preview_skill_list(skills)
    }
}

fn apply_prompt_flags(template: &str, flags: &HashMap<String, bool>) -> String {
    if template.trim().is_empty() {
        return String::new();
    }
    let mut stack: Vec<bool> = Vec::new();
    let mut include = true;
    let mut output = String::with_capacity(template.len());
    for raw_line in template.lines() {
        let trimmed = raw_line.trim();
        if let Some(tag) = trimmed
            .strip_prefix("[[")
            .and_then(|rest| rest.strip_suffix("]]"))
        {
            let tag = tag.trim();
            if let Some(_tag) = tag.strip_prefix('/') {
                if !stack.is_empty() {
                    stack.pop();
                }
                include = stack.iter().all(|value| *value);
            } else {
                let normalized = tag.to_ascii_uppercase();
                let enabled = flags.get(&normalized).copied().unwrap_or(true);
                stack.push(enabled);
                include = include && enabled;
            }
            continue;
        }
        if include {
            output.push_str(raw_line);
            output.push('\n');
        }
    }
    output.trim_end().to_string()
}

fn build_overrides_key(overrides: Option<&Value>) -> String {
    let Some(value) = overrides else {
        return String::new();
    };
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
}

fn build_prompt_key(prompt: Option<&str>) -> String {
    let text = prompt.unwrap_or("").trim();
    if text.is_empty() {
        return String::new();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    text.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn render_tool_spec(spec: &ToolSpec, freeform_mode: bool) -> String {
    // serde_json 默认会按 key 排序输出，这里手动控制字段顺序，确保 name 在最前面便于模型检索。
    let name = serde_json::to_string(&spec.name).unwrap_or_else(|_| "\"\"".to_string());
    let description =
        serde_json::to_string(&spec.description).unwrap_or_else(|_| "\"\"".to_string());
    let rendered = render_prompt_tool_spec(spec, freeform_mode);
    if let Some(format_value) = rendered.get("format") {
        let format_value =
            serde_json::to_string(format_value).unwrap_or_else(|_| "null".to_string());
        return format!(
            "{{\"name\":{name},\"description\":{description},\"format\":{format_value}}}"
        );
    }
    let arguments = rendered.get("arguments").unwrap_or(&spec.input_schema);
    let arguments = serde_json::to_string(arguments).unwrap_or_else(|_| "null".to_string());
    format!("{{\"name\":{name},\"description\":{description},\"arguments\":{arguments}}}")
}

fn render_template(template: &str, mapping: &HashMap<String, String>) -> String {
    let mut rendered = template.to_string();
    for (key, value) in mapping {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

fn localize_tool_specs_for_local_runtime(
    specs: Vec<ToolSpec>,
    workspace: &WorkspaceManager,
    workspace_id: &str,
) -> Vec<ToolSpec> {
    if specs.is_empty() {
        return specs;
    }
    let public_root = workspace
        .public_root(workspace_id)
        .to_string_lossy()
        .replace('\\', "/");
    let local_root = absolute_path_str(&workspace.workspace_root(workspace_id)).replace('\\', "/");
    specs
        .into_iter()
        .map(|mut spec| {
            spec.description = rewrite_workspace_paths_for_local_text(
                &spec.description,
                &public_root,
                &local_root,
            );
            rewrite_workspace_paths_in_json(&mut spec.input_schema, &public_root, &local_root);
            spec
        })
        .collect()
}

fn rewrite_workspace_paths_in_json(value: &mut Value, public_root: &str, local_root: &str) {
    match value {
        Value::String(text) => {
            *text = rewrite_workspace_paths_for_local_text(text, public_root, local_root);
        }
        Value::Array(items) => {
            for item in items {
                rewrite_workspace_paths_in_json(item, public_root, local_root);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                rewrite_workspace_paths_in_json(item, public_root, local_root);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn rewrite_workspace_paths_for_local_text(
    text: &str,
    public_root: &str,
    local_root: &str,
) -> String {
    if text.trim().is_empty() {
        return text.to_string();
    }
    let replaced_placeholder = text.replace("/workspaces/{user_id}", public_root);
    if public_root.is_empty() || local_root.is_empty() || public_root == local_root {
        return replaced_placeholder;
    }
    replaced_placeholder.replace(public_root, local_root)
}

fn resolve_prompt_path(path: &Path, locale_override: Option<&str>) -> PathBuf {
    let mut resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        user_prompt_templates::resolve_default_prompt_pack_root().join(path)
    };
    let locale = resolve_requested_prompt_locale(locale_override);
    if let Some(locale) = locale {
        if let Some(candidate) = insert_locale_after_prompts(&resolved, locale) {
            if candidate.exists() {
                return candidate;
            }
        }
        if let (Some(parent), Some(name)) = (resolved.parent(), resolved.file_name()) {
            let localized_parent = parent
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.eq_ignore_ascii_case("en") || value.eq_ignore_ascii_case("zh"))
                .unwrap_or(false);
            if !localized_parent {
                let candidate = parent.join(locale).join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    if !resolved.exists() && !path.is_absolute() {
        resolved = path.to_path_buf();
    }
    resolved
}

fn resolve_requested_prompt_locale(locale_override: Option<&str>) -> Option<&'static str> {
    let raw = locale_override.unwrap_or("").trim().to_ascii_lowercase();
    if raw.starts_with("en") {
        return Some("en");
    }
    if raw.starts_with("zh") {
        return Some("zh");
    }
    match i18n::get_language().to_ascii_lowercase() {
        language if language.starts_with("en") => Some("en"),
        language if language.starts_with("zh") => Some("zh"),
        _ => None,
    }
}

fn insert_locale_after_prompts(path: &Path, locale: &str) -> Option<PathBuf> {
    let mut prefix = PathBuf::new();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut found = false;
    for component in path.components() {
        if found {
            rest.push(component.as_os_str().to_os_string());
            continue;
        }
        prefix.push(component.as_os_str());
        if component
            .as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case("prompts")
        {
            found = true;
        }
    }
    if !found {
        return None;
    }
    if rest
        .first()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("en") || value.eq_ignore_ascii_case("zh"))
    {
        return None;
    }
    let mut candidate = prefix.join(locale);
    for component in rest {
        candidate.push(component);
    }
    Some(candidate)
}

fn absolute_path_str(path: &Path) -> String {
    let resolved = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let joined = cwd.join(path);
        joined.canonicalize().unwrap_or(joined)
    };
    let mut text = resolved.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            text = stripped.to_string();
        }
    }
    text
}

fn absolute_path_str_from_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    let path = PathBuf::from(raw);
    absolute_path_str(&path)
}

#[derive(Clone)]
struct PromptFileCacheEntry {
    revision: u64,
    exists: bool,
    text: String,
}

fn prompt_file_cache() -> &'static Mutex<HashMap<String, PromptFileCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, PromptFileCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn build_tool_key(allowed_tool_names: &HashSet<String>) -> String {
    let mut list = allowed_tool_names.iter().cloned().collect::<Vec<_>>();
    list.sort();
    list.join(",")
}

fn normalize_tool_names(tool_names: &[String]) -> Vec<String> {
    if tool_names.is_empty() {
        return Vec::new();
    }
    let alias_map = builtin_aliases();
    let mut aliases_by_name: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in &alias_map {
        aliases_by_name
            .entry(canonical.clone())
            .or_default()
            .push(alias.clone());
    }
    let mut normalized = Vec::new();
    let mut seen = HashSet::new();
    for raw in tool_names {
        let name = raw.trim();
        if name.is_empty() || seen.contains(name) {
            continue;
        }
        if let Some(canonical) = alias_map.get(name) {
            push_unique(&mut normalized, &mut seen, canonical);
            if let Some(aliases) = aliases_by_name.get(canonical) {
                for alias in aliases {
                    push_unique(&mut normalized, &mut seen, alias);
                }
            }
            push_unique(&mut normalized, &mut seen, name);
            continue;
        }
        if let Some(aliases) = aliases_by_name.get(name) {
            push_unique(&mut normalized, &mut seen, name);
            for alias in aliases {
                push_unique(&mut normalized, &mut seen, alias);
            }
            continue;
        }
        push_unique(&mut normalized, &mut seen, name);
    }
    normalized
}

fn push_unique(output: &mut Vec<String>, seen: &mut HashSet<String>, value: &str) {
    if seen.insert(value.to_string()) {
        output.push(value.to_string());
    }
}

fn filter_skill_specs(
    skills: &[SkillSpec],
    allowed_tool_names: &HashSet<String>,
) -> Vec<SkillSpec> {
    if allowed_tool_names.is_empty() {
        return Vec::new();
    }
    skills
        .iter()
        .filter(|spec| allowed_tool_names.contains(&spec.name))
        .cloned()
        .collect()
}

fn system_name() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE.get_or_init(resolve_system_name).clone()
}

#[cfg(target_vendor = "win7")]
fn resolve_system_name() -> String {
    let system = System::new();
    let name = system
        .name()
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    let version = system.os_version().unwrap_or_default();
    if version.is_empty() {
        name
    } else {
        format!("{name} {version}")
    }
}

#[cfg(not(target_vendor = "win7"))]
fn resolve_system_name() -> String {
    let name = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
    let version = System::os_version().unwrap_or_default();
    if version.is_empty() {
        name
    } else {
        format!("{name} {version}")
    }
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrite_workspace_paths_for_local_text_handles_placeholder() {
        let output = rewrite_workspace_paths_for_local_text(
            "![Report](/workspaces/{user_id}/temp_dir/report.md)",
            "/workspaces",
            "C:/Users/test/Desktop/workspace",
        );
        assert_eq!(
            output,
            "![Report](C:/Users/test/Desktop/workspace/temp_dir/report.md)"
        );
    }

    #[test]
    fn rewrite_workspace_paths_for_local_text_handles_scoped_public_root() {
        let output = rewrite_workspace_paths_for_local_text(
            "Open /workspaces/demo-user/temp_dir/report.md",
            "/workspaces/demo-user",
            "C:/Users/test/Desktop/workspace",
        );
        assert_eq!(
            output,
            "Open C:/Users/test/Desktop/workspace/temp_dir/report.md"
        );
    }

    #[test]
    fn rewrite_workspace_paths_in_json_rewrites_nested_strings() {
        let mut schema = json!({
            "type": "object",
            "description": "Use /workspaces/{user_id}/temp_dir/report.md",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "![x](/workspaces/{user_id}/a.png)"
                }
            }
        });
        rewrite_workspace_paths_in_json(
            &mut schema,
            "/workspaces",
            "C:/Users/test/Desktop/workspace",
        );
        assert_eq!(
            schema["description"].as_str(),
            Some("Use C:/Users/test/Desktop/workspace/temp_dir/report.md")
        );
        assert_eq!(
            schema["properties"]["content"]["description"].as_str(),
            Some("![x](C:/Users/test/Desktop/workspace/a.png)")
        );
    }

    #[test]
    fn build_prompt_template_cache_key_includes_locale_override() {
        let scope = PromptTemplateScope {
            system_pack_id: "pack-a".to_string(),
            user_pack_id: None,
            owner_user_id: None,
            locale_override: Some("en".to_string()),
        };
        assert_eq!(
            build_prompt_template_cache_key(&scope),
            "system:pack-a|locale:en"
        );
    }

    #[test]
    fn resolve_requested_prompt_locale_prefers_supported_override() {
        assert_eq!(resolve_requested_prompt_locale(Some("en-US")), Some("en"));
        assert_eq!(resolve_requested_prompt_locale(Some("zh-Hans")), Some("zh"));
    }

    #[test]
    fn skills_protocol_requires_loading_matched_skill_first() {
        let zh = std::fs::read_to_string("config/prompts/zh/system/skills_protocol.txt")
            .expect("zh skills protocol");
        assert!(zh.contains("首个相关动作必须调用“技能调用”"));
        assert!(zh.contains("Frontmatter 是触发依据"));

        let en = std::fs::read_to_string("config/prompts/en/system/skills_protocol.txt")
            .expect("en skills protocol");
        assert!(en.contains("first relevant action must be to call the skill tool"));
        assert!(en.contains("Skill Frontmatter is the trigger source of truth"));
    }
}
