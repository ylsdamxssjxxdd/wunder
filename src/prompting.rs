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
use std::sync::{Arc, OnceLock};
use sysinfo::System;
use tokio::sync::{Mutex as TokioMutex, Notify};

const DEFAULT_CACHE_TTL_S: f64 = 10.0;
const DEFAULT_CACHE_MAX_ITEMS: usize = 128;

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
    let resolved = resolve_prompt_path(path);
    let mtime = resolved
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0);
    let cache_key = resolved.to_string_lossy().to_string();
    let cache = prompt_file_cache();
    if let Some((cached_mtime, cached_text)) = cache.lock().get(&cache_key) {
        if *cached_mtime == mtime {
            return cached_text.clone();
        }
    }
    let text = std::fs::read_to_string(&resolved).unwrap_or_default();
    cache.lock().insert(cache_key, (mtime, text.clone()));
    text
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
        let workdir_key = workdir.to_string_lossy();
        let base_key = format!(
            "{user_id}|{config_version}|{workdir_key}|{overrides_key}|{tool_key}|{tool_mode_key}|{user_tool_version}|{shared_tool_version}|{language}"
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
            let include_tools_protocol =
                !allowed_tool_names.is_empty() && tool_call_mode == ToolCallMode::ToolCall;
            let include_ptc = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "ptc");
            let tool_specs = if include_tools_protocol {
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
            let base_prompt = read_prompt_template(Path::new("app/prompts/system.txt"));
            let workdir_display = workspace.display_path(user_id, workdir);
            let mut prompt = build_system_prompt(
                &base_prompt,
                &tool_specs,
                &workdir_display,
                &workspace_tree,
                include_tools_protocol,
                tool_call_mode,
                include_ptc,
            );
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
            let skill_block = build_skill_prompt_block(&workdir_display, &skills_for_prompt);
            if !skill_block.is_empty() {
                prompt = format!("{}\n\n{}", prompt.trim_end(), skill_block.trim());
            }
            if let Some(bindings) = user_tool_bindings {
                let extra = bindings.extra_prompt.trim();
                if !extra.is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), extra);
                }
            }
            if tool_call_mode == ToolCallMode::ToolCall && allowed_tool_names.contains("a2ui") {
                let a2ui_prompt = build_a2ui_prompt();
                if !a2ui_prompt.is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), a2ui_prompt.trim());
                }
            }
            let include_plan_prompt = allowed_tool_names
                .iter()
                .any(|name| resolve_tool_name(name) == "计划面板");
            if include_plan_prompt {
                let plan_prompt = read_prompt_template(Path::new("app/prompts/plan_prompt.txt"));
                if !plan_prompt.trim().is_empty() {
                    prompt = format!("{}\n\n{}", prompt.trim_end(), plan_prompt.trim());
                }
            }

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

fn build_system_prompt(
    base_prompt: &str,
    tools: &[ToolSpec],
    workdir_display: &str,
    workspace_tree: &str,
    include_tools_protocol: bool,
    tool_call_mode: ToolCallMode,
    include_ptc: bool,
) -> String {
    if !include_tools_protocol {
        let engineer_info = build_engineer_info(workdir_display, workspace_tree, include_ptc);
        return format!("{}\n\n{}", base_prompt.trim(), engineer_info.trim());
    }
    let tools_text = tools
        .iter()
        .map(render_tool_spec)
        .collect::<Vec<_>>()
        .join("\n");
    let extra_path = match tool_call_mode {
        ToolCallMode::FunctionCall => Path::new("app/prompts/extra_prompt_function_call.txt"),
        ToolCallMode::ToolCall => Path::new("app/prompts/extra_prompt_template.txt"),
    };
    let extra_template = read_prompt_template(extra_path);
    let extra_prompt = render_template(
        &extra_template,
        &HashMap::from([
            ("available_tools_describe".to_string(), tools_text),
            (
                "engineer_info".to_string(),
                build_engineer_info(workdir_display, workspace_tree, include_ptc),
            ),
        ]),
    );
    format!("{}\n\n{}", base_prompt.trim(), extra_prompt.trim())
}

fn build_engineer_system_info(workdir_display: &str, workspace_tree: &str) -> String {
    let template_path = Path::new("app/prompts/engineer_system_info.txt");
    let template = read_prompt_template(template_path);
    let os_name = system_name();
    let date_str = Local::now().format("%Y-%m-%d").to_string();
    render_template(
        &template,
        &HashMap::from([
            ("OS".to_string(), os_name),
            ("DATE".to_string(), date_str),
            ("DIR".to_string(), workdir_display.to_string()),
            ("WORKSPACE_TREE".to_string(), workspace_tree.to_string()),
        ]),
    )
}

fn build_engineer_info(workdir_display: &str, workspace_tree: &str, include_ptc: bool) -> String {
    let template_path = Path::new("app/prompts/engineer_info.txt");
    let template = read_prompt_template(template_path);
    let ptc_guidance = if include_ptc {
        i18n::t("prompt.engineer.ptc_guidance")
    } else {
        String::new()
    };
    render_template(
        &template,
        &HashMap::from([
            (
                "engineer_system_info".to_string(),
                build_engineer_system_info(workdir_display, workspace_tree),
            ),
            ("PTC_GUIDANCE".to_string(), ptc_guidance),
        ]),
    )
}

fn build_skill_prompt_block(workdir_display: &str, skills: &[SkillSpec]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    lines.push(i18n::t("prompt.skills.header"));
    lines.push(i18n::t("prompt.skills.rule1"));
    lines.push(i18n::t("prompt.skills.rule2"));
    lines.push(i18n::t("prompt.skills.rule3"));
    lines.push(i18n::t("prompt.skills.rule4"));
    lines.push(i18n::t("prompt.skills.rule5"));
    lines.push(i18n::t_with_params(
        "prompt.skills.rule6",
        &HashMap::from([("workdir".to_string(), workdir_display.to_string())]),
    ));
    lines.push(String::new());
    lines.push(i18n::t("prompt.skills.list_header"));
    let mut sorted = skills.to_vec();
    sorted.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
    lines.join("\n")
}

fn build_a2ui_prompt() -> String {
    let prompt_path = Path::new("app/prompts/a2ui_prompt.txt");
    let schema_path = Path::new("app/prompts/a2ui_schema.json");
    let template = read_prompt_template(prompt_path);
    let schema_text = std::fs::read_to_string(schema_path).unwrap_or_else(|_| "{}".to_string());
    render_template(
        &template,
        &HashMap::from([("a2ui_schema".to_string(), schema_text.trim().to_string())]),
    )
}

fn build_overrides_key(overrides: Option<&Value>) -> String {
    let Some(value) = overrides else {
        return String::new();
    };
    serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
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
    let language = i18n::get_language();
    if language.starts_with("en") {
        if let Some(parent) = path.parent() {
            if let Some(name) = path.file_name() {
                let candidate = parent.join("en").join(name);
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    path.to_path_buf()
}

fn absolute_path_str(path: &Path) -> String {
    let resolved = if path.is_absolute() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let joined = cwd.join(path);
        joined.canonicalize().unwrap_or_else(|_| joined)
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

fn prompt_file_cache() -> &'static Mutex<HashMap<String, (f64, String)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (f64, String)>>> = OnceLock::new();
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
