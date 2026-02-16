// 系统提示词构建：模板渲染、工具描述拼接与缓存管理。
use crate::config::Config;
use crate::i18n;
use crate::llm::ToolCallMode;
use crate::schemas::ToolSpec;
use crate::skills::{SkillRegistry, SkillSpec};
use crate::tools::{
    builtin_aliases, collect_available_tool_names, collect_prompt_tool_specs, resolve_tool_name,
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
use sysinfo::System;
use tokio::sync::{Mutex as TokioMutex, Notify};

const DEFAULT_CACHE_TTL_S: f64 = 10.0;
const DEFAULT_CACHE_MAX_ITEMS: usize = 128;
const PROMPTS_ROOT_ENV: &str = "WUNDER_PROMPTS_ROOT";
const SYSTEM_PROMPT_ROLE_PATH: &str = "prompts/system/role.txt";
const SYSTEM_PROMPT_ENGINEERING_PATH: &str = "prompts/system/engineering.txt";
const SYSTEM_PROMPT_TOOLS_PROTOCOL_PATH: &str = "prompts/system/tools_protocol.txt";
const SYSTEM_PROMPT_SKILLS_PROTOCOL_PATH: &str = "prompts/system/skills_protocol.txt";
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
    read_prompt_template_with_exists(path).0
}

fn read_prompt_template_with_exists(path: &Path) -> (String, bool) {
    let language = i18n::get_language().to_ascii_lowercase();
    let cache_key = format!("{language}|{}", path.to_string_lossy());
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
    let resolved = resolve_prompt_path(path);
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
    read_prompt_template_from_pack(config, &template_id, path)
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
        user_id: &str,
        workdir: &Path,
        overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        tool_call_mode: ToolCallMode,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        agent_prompt: Option<&str>,
    ) -> String {
        let tool_key = build_tool_key(allowed_tool_names);
        let language = i18n::get_language();
        let tool_mode_key = match tool_call_mode {
            ToolCallMode::FunctionCall => "function_call",
            ToolCallMode::ToolCall => "tool_call",
        };
        let user_tool_version = user_tool_bindings
            .map(|item| item.user_version)
            .unwrap_or(0.0);
        let shared_tool_version = user_tool_bindings
            .map(|item| item.shared_version)
            .unwrap_or(0.0);
        let overrides_key = build_overrides_key(overrides);
        let agent_prompt_key = build_prompt_key(agent_prompt);
        let prompt_template_id = resolve_prompt_template_id(config);
        let templates_revision = system_prompt_templates_revision();
        let workdir_key = workdir.to_string_lossy();
        let base_key = format!(
            "{user_id}|{config_version}|{prompt_template_id}|{templates_revision}|{workdir_key}|{overrides_key}|{tool_key}|{tool_mode_key}|{user_tool_version}|{shared_tool_version}|{agent_prompt_key}|{language}"
        );
        let workspace_version = workspace.get_tree_cache_version(user_id);
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
                let workspace_version = workspace.get_tree_cache_version(user_id);
                let cache_key = format!("{base_key}|{workspace_version}");
                let now = now_ts();
                if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                    return prompt;
                }
                continue;
            }

            let workspace_version = workspace.get_tree_cache_version(user_id);
            let cache_key = format!("{base_key}|{workspace_version}");
            let now = now_ts();
            if let Some(prompt) = self.get_cached_prompt(&cache_key, now) {
                self.notify_inflight(&base_key).await;
                return prompt;
            }

            let tree_snapshot = workspace.get_workspace_tree_snapshot(user_id);
            let workspace_version = tree_snapshot.version;
            let cache_key = format!("{base_key}|{workspace_version}");
            let workspace_tree = tree_snapshot.tree;
            let include_ptc = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "ptc");
            let tool_specs = if tool_call_mode == ToolCallMode::ToolCall && !allowed_tool_names.is_empty() {
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
            let workdir_display = workspace.display_path(user_id, workdir);
            let base_skill_specs = skills.list_specs();
            let mut skills_for_prompt = filter_skill_specs(&base_skill_specs, allowed_tool_names);
            if let Some(bindings) = user_tool_bindings {
                if !bindings.skill_specs.is_empty() {
                    let user_skills = filter_skill_specs(&bindings.skill_specs, allowed_tool_names);
                    if !user_skills.is_empty() {
                        skills_for_prompt = merge_skill_specs(skills_for_prompt, user_skills);
                    }
                }
            }

            let prompt = build_system_prompt_skeleton(
                config,
                allowed_tool_names,
                tool_call_mode,
                &tool_specs,
                include_ptc,
                &workdir_display,
                &workspace_tree,
                &skills_for_prompt,
                agent_prompt,
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

fn resolve_prompt_template_id(config: &Config) -> String {
    let value = config.prompt_templates.active.trim();
    if value.is_empty() {
        "default".to_string()
    } else {
        value.to_string()
    }
}

fn resolve_prompt_template_root(config: &Config, template_id: &str) -> PathBuf {
    if template_id.trim().eq_ignore_ascii_case("default") {
        return PathBuf::from(".");
    }
    let root = config.prompt_templates.root.trim();
    let root = if root.is_empty() {
        PathBuf::from("./data/prompt_templates")
    } else {
        PathBuf::from(root)
    };
    let root = if root.is_absolute() {
        root
    } else {
        resolve_prompts_root().join(root)
    };
    root.join(template_id.trim())
}

fn read_prompt_template_from_pack(config: &Config, template_id: &str, path: &Path) -> String {
    let template_id = template_id.trim();
    let is_default = template_id.is_empty() || template_id.eq_ignore_ascii_case("default");
    if !is_default {
        let pack_root = resolve_prompt_template_root(config, template_id);
        let candidate = pack_root.join(path);
        let (text, exists) = read_prompt_template_with_exists(&candidate);
        if exists {
            return text;
        }
    }
    read_prompt_template(path)
}

#[allow(clippy::too_many_arguments)]
fn build_system_prompt_skeleton(
    config: &Config,
    allowed_tool_names: &HashSet<String>,
    tool_call_mode: ToolCallMode,
    tools: &[ToolSpec],
    include_ptc: bool,
    workdir_display: &str,
    workspace_tree: &str,
    skills: &[SkillSpec],
    agent_prompt: Option<&str>,
) -> String {
    let template_id = resolve_prompt_template_id(config);

    let os_name = system_name();
    let date_str = Local::now().format("%Y-%m-%d").to_string();

    let role = read_prompt_template_from_pack(config, &template_id, Path::new(SYSTEM_PROMPT_ROLE_PATH));

    let mut engineering_flags = HashMap::new();
    let is_local = is_local_runtime_mode(&config.server.mode);
    engineering_flags.insert("RUNTIME_LOCAL".to_string(), is_local);
    engineering_flags.insert("RUNTIME_SERVER".to_string(), !is_local);
    engineering_flags.insert("HAS_PTC".to_string(), include_ptc);

    let engineering_template = read_prompt_template_from_pack(
        config,
        &template_id,
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

    let tools_text = if tool_call_mode == ToolCallMode::ToolCall && !tools.is_empty() {
        tools.iter().map(render_tool_spec).collect::<Vec<_>>().join("\n")
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
    let has_a2ui = tool_call_mode == ToolCallMode::ToolCall
        && allowed_tool_names
            .iter()
            .any(|name| resolve_tool_name(name) == "a2ui");
    let has_plan = allowed_tool_names
        .iter()
        .any(|name| resolve_tool_name(name) == "计划面板");
    let has_question_panel = allowed_tool_names
        .iter()
        .any(|name| resolve_tool_name(name) == "问询面板");
    tools_flags.insert("HAS_A2UI_TOOL".to_string(), has_a2ui);
    tools_flags.insert("HAS_PLAN_TOOL".to_string(), has_plan);
    tools_flags.insert("HAS_QUESTION_PANEL_TOOL".to_string(), has_question_panel);
    let tools_template = read_prompt_template_from_pack(
        config,
        &template_id,
        Path::new(SYSTEM_PROMPT_TOOLS_PROTOCOL_PATH),
    );
    let tools_template = apply_prompt_flags(&tools_template, &tools_flags);
    let tools_block = render_template(
        &tools_template,
        &HashMap::from([("available_tools_describe".to_string(), tools_text)]),
    );

    let skills_block = if skills.is_empty() {
        String::new()
    } else {
        let skills_list = render_skill_list(skills);
        let skills_template = read_prompt_template_from_pack(
            config,
            &template_id,
            Path::new(SYSTEM_PROMPT_SKILLS_PROTOCOL_PATH),
        );
        render_template(
            &skills_template,
            &HashMap::from([
                ("WORKDIR".to_string(), workdir_display.to_string()),
                ("SKILLS_LIST".to_string(), skills_list),
            ]),
        )
    };

    let memory_template = read_prompt_template_from_pack(
        config,
        &template_id,
        Path::new(SYSTEM_PROMPT_MEMORY_PATH),
    );
    let memory_block = render_template(
        &memory_template,
        &HashMap::from([(
            "HISTORY_MEMORY".to_string(),
            SYSTEM_PROMPT_MEMORY_PLACEHOLDER.to_string(),
        )]),
    );

    let mut blocks = vec![role, engineering, tools_block, skills_block, memory_block];
    blocks.retain(|value| !value.trim().is_empty());
    if !blocks
        .iter()
        .any(|value| value.contains(SYSTEM_PROMPT_MEMORY_PLACEHOLDER))
    {
        blocks.push(SYSTEM_PROMPT_MEMORY_PLACEHOLDER.to_string());
    }

    if let Some(extra) = agent_prompt.map(str::trim).filter(|value| !value.is_empty()) {
        let extra_template =
            read_prompt_template_from_pack(config, &template_id, Path::new(SYSTEM_PROMPT_EXTRA_PATH));
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

fn apply_prompt_flags(template: &str, flags: &HashMap<String, bool>) -> String {
    if template.trim().is_empty() {
        return String::new();
    }
    let mut stack: Vec<bool> = Vec::new();
    let mut include = true;
    let mut output = String::with_capacity(template.len());
    for raw_line in template.lines() {
        let trimmed = raw_line.trim();
        if let Some(tag) = trimmed.strip_prefix("[[").and_then(|rest| rest.strip_suffix("]]")) {
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

fn render_tool_spec(spec: &ToolSpec) -> String {
    // serde_json 默认会按 key 排序输出，这里手动控制字段顺序，确保 name 在最前面便于模型检索。
    let name = serde_json::to_string(&spec.name).unwrap_or_else(|_| "\"\"".to_string());
    let description =
        serde_json::to_string(&spec.description).unwrap_or_else(|_| "\"\"".to_string());
    let arguments =
        serde_json::to_string(&spec.input_schema).unwrap_or_else(|_| "null".to_string());
    format!("{{\"name\":{name},\"description\":{description},\"arguments\":{arguments}}}")
}

fn render_template(template: &str, mapping: &HashMap<String, String>) -> String {
    let mut rendered = template.to_string();
    for (key, value) in mapping {
        rendered = rendered.replace(&format!("{{{key}}}"), value);
    }
    rendered
}

fn resolve_prompt_path(path: &Path) -> PathBuf {
    let mut resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_prompts_root().join(path)
    };
    let locale = match i18n::get_language().to_ascii_lowercase() {
        language if language.starts_with("en") => Some("en"),
        language if language.starts_with("zh") => Some("zh"),
        _ => None,
    };
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
    if rest.first().and_then(|value| value.to_str()).is_some_and(|value| {
        value.eq_ignore_ascii_case("en") || value.eq_ignore_ascii_case("zh")
    }) {
        return None;
    }
    let mut candidate = prefix.join(locale);
    for component in rest {
        candidate.push(component);
    }
    Some(candidate)
}

fn resolve_prompts_root() -> PathBuf {
    let root = std::env::var(PROMPTS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    normalize_prompts_root(root)
}

fn normalize_prompts_root(root: PathBuf) -> PathBuf {
    if root.join("prompts").is_dir() {
        return root;
    }
    let looks_like_prompts_dir = root
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("prompts"))
        .unwrap_or(false);
    if looks_like_prompts_dir && (root.join("zh").is_dir() || root.join("en").is_dir()) {
        if let Some(parent) = root.parent() {
            return parent.to_path_buf();
        }
    }
    root
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

fn merge_skill_specs(base: Vec<SkillSpec>, extra: Vec<SkillSpec>) -> Vec<SkillSpec> {
    let mut merged = Vec::new();
    let mut seen = HashSet::new();
    for spec in base.into_iter().chain(extra.into_iter()) {
        if seen.insert(spec.name.clone()) {
            merged.push(spec);
        }
    }
    merged
}

fn system_name() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let name = System::name().unwrap_or_else(|| std::env::consts::OS.to_string());
            let version = System::os_version().unwrap_or_default();
            if version.is_empty() {
                name
            } else {
                format!("{name} {version}")
            }
        })
        .clone()
}

fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}
