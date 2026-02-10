// 用户自建工具：负责配置存储、别名绑定与共享工具聚合。
use crate::config::{Config, KnowledgeBaseType, McpServerConfig, McpToolSpec};
use crate::i18n;
use crate::path_utils::{normalize_path_for_compare, normalize_target_path};
use crate::schemas::ToolSpec;
use crate::skills::{load_skills, SkillRegistry, SkillSpec};
use crate::vector_knowledge;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const USER_TOOLS_ROOT_ENV: &str = "WUNDER_USER_TOOLS_ROOT";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserMcpServer {
    pub name: String,
    pub endpoint: String,
    #[serde(default)]
    pub allow_tools: Vec<String>,
    #[serde(default)]
    pub shared_tools: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub auth: Option<Value>,
    #[serde(default)]
    pub tool_specs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserSkillConfig {
    #[serde(default)]
    pub enabled: Vec<String>,
    #[serde(default)]
    pub shared: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserKnowledgeBase {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub shared: bool,
    #[serde(default)]
    pub base_type: Option<String>,
    #[serde(default)]
    pub embedding_model: Option<String>,
    #[serde(default)]
    pub chunk_size: Option<usize>,
    #[serde(default)]
    pub chunk_overlap: Option<usize>,
    #[serde(default)]
    pub top_k: Option<usize>,
    #[serde(default)]
    pub score_threshold: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub struct UserToolsPayload {
    pub user_id: String,
    pub mcp_servers: Vec<UserMcpServer>,
    pub skills: UserSkillConfig,
    pub knowledge_bases: Vec<UserKnowledgeBase>,
    pub shared_tools: Vec<String>,
    pub version: f64,
}

#[derive(Debug, Clone)]
pub enum UserToolKind {
    Mcp,
    Skill,
    Knowledge,
}

#[derive(Debug, Clone)]
pub struct UserToolAlias {
    pub kind: UserToolKind,
    pub owner_id: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct UserSkillSource {
    pub root: PathBuf,
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UserToolBindings {
    pub alias_specs: HashMap<String, ToolSpec>,
    pub alias_map: HashMap<String, UserToolAlias>,
    pub skill_specs: Vec<SkillSpec>,
    pub skill_sources: HashMap<String, UserSkillSource>,
    pub mcp_servers: HashMap<String, HashMap<String, McpServerConfig>>,
    pub shared_tools_enabled: HashSet<String>,
    pub user_version: f64,
    pub shared_version: f64,
}

struct UserToolsCacheEntry {
    version: f64,
    payload: UserToolsPayload,
}

struct SharedToolsCache {
    timestamp: f64,
    payloads: Vec<UserToolsPayload>,
}

struct SkillSpecCacheEntry {
    signature: (f64, Vec<String>),
    specs: Vec<SkillSpec>,
}

struct SkillRegistryCacheEntry {
    signature: (f64, Vec<String>),
    registry: SkillRegistry,
}

#[derive(Default)]
struct SkillCache {
    spec_cache: HashMap<String, SkillSpecCacheEntry>,
    registry_cache: HashMap<String, SkillRegistryCacheEntry>,
    order: VecDeque<String>,
}

/// 用户工具存储：读取/写入 data/user_tools 目录下的配置文件。
pub struct UserToolStore {
    root: PathBuf,
    cache: Mutex<HashMap<String, UserToolsCacheEntry>>,
    shared_cache: Mutex<Option<SharedToolsCache>>,
    shared_cache_ttl_s: f64,
    shared_version: Mutex<f64>,
}

impl UserToolStore {
    pub fn new(_config: &Config) -> Result<Self> {
        let base = resolve_user_tools_root();
        std::fs::create_dir_all(&base)?;
        Ok(Self {
            root: base,
            cache: Mutex::new(HashMap::new()),
            shared_cache: Mutex::new(None),
            shared_cache_ttl_s: 5.0,
            shared_version: Mutex::new(now_ts()),
        })
    }

    /// 构造统一的别名格式：user_id@tool_name。
    pub fn build_alias_name(&self, owner_id: &str, tool_name: &str) -> String {
        format!("{}@{}", owner_id, tool_name)
    }

    /// 获取共享工具版本号，用于提示词缓存判断。
    pub fn shared_version(&self) -> f64 {
        *self
            .shared_version
            .lock()
            .unwrap_or_else(|err| err.into_inner())
    }

    /// 读取指定用户的工具配置并做字段清洗。
    pub fn load_user_tools(&self, user_id: &str) -> UserToolsPayload {
        let safe_id = safe_user_id(user_id);
        let path = self.config_path(&safe_id);
        let version = file_modified_ts(&path);
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&safe_id)
        {
            if cached.version == version {
                return cached.payload.clone();
            }
        }
        let mut payload = self.read_payload(&path, user_id);
        payload.version = version;
        self.cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(
                safe_id,
                UserToolsCacheEntry {
                    version,
                    payload: payload.clone(),
                },
            );
        payload
    }

    /// 更新用户 MCP 服务器配置。
    pub fn update_mcp_servers(
        &self,
        user_id: &str,
        servers: Vec<UserMcpServer>,
    ) -> Result<UserToolsPayload> {
        let mut payload = self.load_user_tools(user_id);
        payload.mcp_servers = normalize_mcp_servers(servers);
        self.save_payload(user_id, payload)
    }

    /// 更新用户技能启用与共享列表。
    pub fn update_skills(
        &self,
        user_id: &str,
        enabled: Vec<String>,
        shared: Vec<String>,
    ) -> Result<UserToolsPayload> {
        let mut payload = self.load_user_tools(user_id);
        payload.skills = normalize_skill_config(enabled, shared);
        self.save_payload(user_id, payload)
    }

    /// 更新用户知识库配置，并清理被移除的目录。
    pub fn update_knowledge_bases(
        &self,
        user_id: &str,
        bases: Vec<UserKnowledgeBase>,
    ) -> Result<UserToolsPayload> {
        let mut payload = self.load_user_tools(user_id);
        let previous: HashSet<String> = payload
            .knowledge_bases
            .iter()
            .filter(|base| !base.name.trim().is_empty())
            .map(|base| base.name.clone())
            .collect();
        let normalized = normalize_knowledge_bases(bases);
        let next: HashSet<String> = normalized
            .iter()
            .filter(|base| !base.name.trim().is_empty())
            .map(|base| base.name.clone())
            .collect();
        let removed: HashSet<String> = previous.difference(&next).cloned().collect();
        if !removed.is_empty() {
            self.cleanup_knowledge_dirs(user_id, &removed);
        }
        payload.knowledge_bases = normalized;
        self.save_payload(user_id, payload)
    }

    /// 更新用户共享工具选择列表。
    pub fn update_shared_tools(
        &self,
        user_id: &str,
        shared_tools: Vec<String>,
    ) -> Result<UserToolsPayload> {
        let mut payload = self.load_user_tools(user_id);
        payload.shared_tools = normalize_name_list(shared_tools);
        self.save_payload(user_id, payload)
    }

    /// 列出所有共享配置（排除当前用户）。
    pub fn list_shared_payloads(&self, exclude_user_id: &str) -> Vec<UserToolsPayload> {
        let now = now_ts();
        if let Some(cache) = self
            .shared_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .as_ref()
        {
            if now - cache.timestamp < self.shared_cache_ttl_s {
                return cache
                    .payloads
                    .iter()
                    .filter(|&item| item.user_id != exclude_user_id)
                    .cloned()
                    .collect();
            }
        }
        let payloads = self.scan_shared_payloads();
        *self
            .shared_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(SharedToolsCache {
            timestamp: now,
            payloads: payloads.clone(),
        });
        payloads
            .into_iter()
            .filter(|item| item.user_id != exclude_user_id)
            .collect()
    }

    /// 获取用户工具目录。
    pub fn get_user_dir(&self, user_id: &str) -> PathBuf {
        self.user_dir(&safe_user_id(user_id))
    }

    /// 获取用户技能目录。
    pub fn get_skill_root(&self, user_id: &str) -> PathBuf {
        self.get_user_dir(user_id).join("skills")
    }

    /// 获取用户知识库根目录。
    pub fn get_knowledge_root(&self, user_id: &str) -> PathBuf {
        self.get_user_dir(user_id).join("knowledge")
    }

    /// 解析用户知识库路径，防止路径穿越。
    pub fn resolve_knowledge_base_root(
        &self,
        user_id: &str,
        base_name: &str,
        create: bool,
    ) -> Result<PathBuf> {
        let cleaned = base_name.trim();
        if cleaned.is_empty() {
            return Err(anyhow!(i18n::t("error.knowledge_base_name_required")));
        }
        if cleaned.contains('/') || cleaned.contains('\\') || cleaned.contains("..") {
            return Err(anyhow!(i18n::t("error.knowledge_name_invalid_path")));
        }
        let root = self.get_knowledge_root(user_id);
        let target = root.join(cleaned);
        let normalized_target = normalize_target_path(&target);
        let normalized_root = normalize_target_path(&root);
        let root_compare = normalize_path_for_compare(&normalized_root);
        let target_compare = normalize_path_for_compare(&normalized_target);
        if normalized_target != normalized_root && !target_compare.starts_with(&root_compare) {
            return Err(anyhow!(i18n::t("error.knowledge_path_out_of_bounds")));
        }
        if create {
            std::fs::create_dir_all(&target)?;
        }
        Ok(target)
    }

    pub fn resolve_knowledge_base_root_with_type(
        &self,
        user_id: &str,
        base_name: &str,
        base_type: KnowledgeBaseType,
        create: bool,
    ) -> Result<PathBuf> {
        if base_type == KnowledgeBaseType::Vector {
            return vector_knowledge::resolve_vector_root(Some(user_id), base_name, create);
        }
        self.resolve_knowledge_base_root(user_id, base_name, create)
    }

    fn cleanup_knowledge_dirs(&self, user_id: &str, removed: &HashSet<String>) {
        for name in removed {
            if let Ok(path) = self.resolve_knowledge_base_root(user_id, name, false) {
                if path.exists() && path.is_dir() {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
            if let Ok(path) = vector_knowledge::resolve_vector_root(Some(user_id), name, false) {
                if path.exists() && path.is_dir() {
                    let _ = std::fs::remove_dir_all(path);
                }
            }
        }
    }

    fn scan_shared_payloads(&self) -> Vec<UserToolsPayload> {
        let mut payloads = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let payload = self.read_payload(&path.join("config.json"), "");
                let mut payload = payload;
                if payload.user_id.is_empty() {
                    payload.user_id = entry.file_name().to_string_lossy().to_string();
                }
                payloads.push(payload);
            }
        }
        payloads
    }

    fn read_payload(&self, path: &Path, fallback_user_id: &str) -> UserToolsPayload {
        if !path.exists() {
            return UserToolsPayload {
                user_id: fallback_user_id.to_string(),
                ..UserToolsPayload::default()
            };
        }
        let content = std::fs::read_to_string(path).unwrap_or_default();
        let value: Value = serde_json::from_str(&content).unwrap_or(Value::Null);
        let user_id = value
            .get("user_id")
            .and_then(Value::as_str)
            .unwrap_or(fallback_user_id)
            .trim()
            .to_string();
        let mcp_servers = normalize_mcp_servers(parse_mcp_servers(&value));
        let skills = normalize_skill_config(
            parse_name_list(value.get("skills").and_then(|item| item.get("enabled"))),
            parse_name_list(value.get("skills").and_then(|item| item.get("shared"))),
        );
        let knowledge_bases = normalize_knowledge_bases(parse_knowledge_bases(&value));
        let shared_tools = normalize_name_list(parse_name_list(value.get("shared_tools")));
        UserToolsPayload {
            user_id,
            mcp_servers,
            skills,
            knowledge_bases,
            shared_tools,
            version: 0.0,
        }
    }

    fn save_payload(
        &self,
        user_id: &str,
        mut payload: UserToolsPayload,
    ) -> Result<UserToolsPayload> {
        let safe_id = safe_user_id(user_id);
        let folder = self.user_dir(&safe_id);
        std::fs::create_dir_all(&folder)?;
        let data = json!({
            "user_id": user_id,
            "mcp": { "servers": payload.mcp_servers.iter().map(user_mcp_server_to_value).collect::<Vec<_>>() },
            "skills": { "enabled": payload.skills.enabled, "shared": payload.skills.shared },
            "knowledge": { "bases": payload.knowledge_bases.iter().map(user_knowledge_base_to_value).collect::<Vec<_>>() },
            "shared_tools": payload.shared_tools,
        });
        let path = self.config_path(&safe_id);
        std::fs::write(&path, serde_json::to_string_pretty(&data)?)?;
        let version = file_modified_ts(&path);
        payload.user_id = user_id.to_string();
        payload.version = version;
        self.cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .insert(
                safe_id,
                UserToolsCacheEntry {
                    version,
                    payload: payload.clone(),
                },
            );
        *self
            .shared_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
        *self
            .shared_version
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = now_ts();
        Ok(payload)
    }

    fn user_dir(&self, safe_user_id: &str) -> PathBuf {
        self.root.join(safe_user_id)
    }

    fn config_path(&self, safe_user_id: &str) -> PathBuf {
        self.user_dir(safe_user_id).join("config.json")
    }
}

/// 用户工具管理器：聚合自建工具并生成别名绑定。
pub struct UserToolManager {
    store: Arc<UserToolStore>,
    skill_cache: Mutex<SkillCache>,
    skill_cache_max: usize,
}

impl UserToolManager {
    pub fn new(store: Arc<UserToolStore>) -> Self {
        Self {
            store,
            skill_cache: Mutex::new(SkillCache::default()),
            skill_cache_max: 128,
        }
    }

    pub fn store(&self) -> &UserToolStore {
        self.store.as_ref()
    }

    /// 清理用户技能缓存，确保读取到最新内容。
    pub fn clear_skill_cache(&self, owner_id: Option<&str>) {
        let mut cache = self
            .skill_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if owner_id.is_none() {
            cache.spec_cache.clear();
            cache.registry_cache.clear();
            cache.order.clear();
            return;
        }
        let prefix = format!("{}::", owner_id.unwrap_or_default());
        cache.spec_cache.retain(|key, _| !key.starts_with(&prefix));
        cache
            .registry_cache
            .retain(|key, _| !key.starts_with(&prefix));
        cache.order.retain(|key| !key.starts_with(&prefix));
    }

    /// 构建用户工具别名绑定。
    pub fn build_bindings(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        user_id: &str,
    ) -> UserToolBindings {
        let user_payload = self.store.load_user_tools(user_id);
        let shared_tools_enabled: HashSet<String> = user_payload
            .shared_tools
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        self.build_bindings_with_shared_filter(
            config,
            skills,
            user_id,
            &user_payload,
            Some(&shared_tools_enabled),
        )
    }

    pub fn build_bindings_for_catalog(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        user_id: &str,
    ) -> UserToolBindings {
        let user_payload = self.store.load_user_tools(user_id);
        self.build_bindings_with_shared_filter(config, skills, user_id, &user_payload, None)
    }

    fn build_bindings_with_shared_filter(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        user_id: &str,
        user_payload: &UserToolsPayload,
        shared_tools_filter: Option<&HashSet<String>>,
    ) -> UserToolBindings {
        let shared_tools_enabled: HashSet<String> = user_payload
            .shared_tools
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let shared_payloads = self.store.list_shared_payloads(user_id);

        let builtin_names: HashSet<String> = config
            .tools
            .builtin
            .enabled
            .iter()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();
        let mcp_names = collect_mcp_tool_names(config);
        let skill_names: HashSet<String> = skills
            .list_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect();
        let knowledge_names = collect_knowledge_tool_names(config, &skill_names);

        let mut blocked_names: HashSet<String> = HashSet::new();
        blocked_names.extend(builtin_names);
        blocked_names.extend(mcp_names);
        blocked_names.extend(skill_names);
        blocked_names.extend(knowledge_names);

        let mut alias_specs = HashMap::new();
        let mut alias_map = HashMap::new();
        let mut skill_specs = Vec::new();
        let mut skill_sources = HashMap::new();
        let mut mcp_servers: HashMap<String, HashMap<String, McpServerConfig>> = HashMap::new();
        let knowledge_schema = build_knowledge_schema();

        let owner_id = if user_payload.user_id.trim().is_empty() {
            user_id.to_string()
        } else {
            user_payload.user_id.clone()
        };

        {
            let mut append_alias = |alias: String,
                                    spec: ToolSpec,
                                    kind: UserToolKind,
                                    owner_id: String,
                                    target: String| {
                if blocked_names.contains(&alias) || alias_specs.contains_key(&alias) {
                    return;
                }
                alias_specs.insert(alias.clone(), spec);
                alias_map.insert(
                    alias.clone(),
                    UserToolAlias {
                        kind,
                        owner_id,
                        target,
                    },
                );
                blocked_names.insert(alias);
            };

            let shared_tools_filter = shared_tools_filter.cloned();
            let mut collect_mcp_tools =
                |owner_id: &str, servers: &[UserMcpServer], shared_only: bool| {
                    for server in servers {
                        let server_name = server.name.trim();
                        if server_name.is_empty() || server_name.contains('@') {
                            continue;
                        }
                        if !server.enabled {
                            continue;
                        }
                        if server.tool_specs.is_empty() {
                            continue;
                        }
                        let allow_tools: HashSet<String> = server
                            .allow_tools
                            .iter()
                            .filter(|name| !name.trim().is_empty())
                            .cloned()
                            .collect();
                        let shared_tools: HashSet<String> = server
                            .shared_tools
                            .iter()
                            .filter(|name| !name.trim().is_empty())
                            .cloned()
                            .collect();
                        let tool_pool: HashSet<String> = server
                            .tool_specs
                            .iter()
                            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                            .map(|name| name.trim().to_string())
                            .filter(|name| !name.is_empty())
                            .collect();
                        let mut enabled_names = if allow_tools.is_empty() {
                            tool_pool
                        } else {
                            allow_tools
                        };
                        if shared_only {
                            enabled_names.retain(|name| shared_tools.contains(name));
                        }
                        if enabled_names.is_empty() {
                            continue;
                        }
                        let owner_map = mcp_servers.entry(owner_id.to_string()).or_default();
                        owner_map
                            .entry(server_name.to_string())
                            .or_insert_with(|| user_mcp_to_config(server));
                        for tool in &server.tool_specs {
                            let tool_name = tool.get("name").and_then(Value::as_str).unwrap_or("");
                            let tool_name = tool_name.trim();
                            if tool_name.is_empty() || !enabled_names.contains(tool_name) {
                                continue;
                            }
                            let description = resolve_mcp_description(server, tool);
                            let schema = normalize_mcp_input_schema(tool);
                            let alias_name = self
                                .store
                                .build_alias_name(owner_id, &format!("{server_name}@{tool_name}"));
                            if shared_only {
                                if let Some(filter) = shared_tools_filter.as_ref() {
                                    if !filter.contains(&alias_name) {
                                        continue;
                                    }
                                }
                            }
                            append_alias(
                                alias_name.clone(),
                                ToolSpec {
                                    name: alias_name.clone(),
                                    description,
                                    input_schema: schema,
                                },
                                UserToolKind::Mcp,
                                owner_id.to_string(),
                                format!("{server_name}@{tool_name}"),
                            );
                        }
                    }
                };

            collect_mcp_tools(&owner_id, &user_payload.mcp_servers, false);
            for shared_payload in &shared_payloads {
                let shared_owner = if shared_payload.user_id.trim().is_empty() {
                    user_id.to_string()
                } else {
                    shared_payload.user_id.clone()
                };
                collect_mcp_tools(&shared_owner, &shared_payload.mcp_servers, true);
            }
        }

        {
            let mut register_skill_source = |owner_id: &str, root: PathBuf, names: Vec<String>| {
                if names.is_empty() {
                    return;
                }
                skill_sources
                    .entry(owner_id.to_string())
                    .and_modify(|source: &mut UserSkillSource| {
                        let mut merged: HashSet<String> = source.names.iter().cloned().collect();
                        for name in names.iter() {
                            merged.insert(name.clone());
                        }
                        source.names = merged.into_iter().collect();
                    })
                    .or_insert(UserSkillSource { root, names });
            };

            let shared_tools_filter = shared_tools_filter.cloned();
            let mut collect_skill_tools = |owner_id: &str, names: &[String], shared_only: bool| {
                if names.is_empty() {
                    return;
                }
                let skill_root = self.store.get_skill_root(owner_id);
                if !skill_root.exists() {
                    return;
                }
                let enabled: HashSet<String> = names
                    .iter()
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();
                let specs = self.load_cached_skill_specs(config, owner_id, &skill_root, &enabled);
                if specs.is_empty() {
                    return;
                }
                register_skill_source(
                    owner_id,
                    skill_root.clone(),
                    enabled.iter().cloned().collect(),
                );
                for spec in specs {
                    if shared_only && !enabled.contains(&spec.name) {
                        continue;
                    }
                    let alias_name = self.store.build_alias_name(owner_id, &spec.name);
                    if shared_only {
                        if let Some(filter) = shared_tools_filter.as_ref() {
                            if !filter.contains(&alias_name) {
                                continue;
                            }
                        }
                    }
                    if blocked_names.contains(&alias_name) || alias_map.contains_key(&alias_name) {
                        continue;
                    }
                    blocked_names.insert(alias_name.clone());
                    alias_map.insert(
                        alias_name.clone(),
                        UserToolAlias {
                            kind: UserToolKind::Skill,
                            owner_id: owner_id.to_string(),
                            target: spec.name.clone(),
                        },
                    );
                    skill_specs.push(SkillSpec {
                        name: alias_name,
                        description: spec.description.clone(),
                        path: spec.path.clone(),
                        input_schema: spec.input_schema.clone(),
                        frontmatter: spec.frontmatter.clone(),
                        root: spec.root.clone(),
                        entrypoint: None,
                    });
                }
            };

            collect_skill_tools(&owner_id, &user_payload.skills.enabled, false);
            for shared_payload in &shared_payloads {
                let shared_owner = if shared_payload.user_id.trim().is_empty() {
                    user_id.to_string()
                } else {
                    shared_payload.user_id.clone()
                };
                collect_skill_tools(&shared_owner, &shared_payload.skills.shared, true);
            }
        }

        {
            let mut append_alias = |alias: String,
                                    spec: ToolSpec,
                                    kind: UserToolKind,
                                    owner_id: String,
                                    target: String| {
                if blocked_names.contains(&alias) || alias_specs.contains_key(&alias) {
                    return;
                }
                alias_specs.insert(alias.clone(), spec);
                alias_map.insert(
                    alias.clone(),
                    UserToolAlias {
                        kind,
                        owner_id,
                        target,
                    },
                );
                blocked_names.insert(alias);
            };

            let shared_tools_filter = shared_tools_filter.cloned();
            let mut collect_knowledge_tools =
                |owner_id: &str, bases: &[UserKnowledgeBase], shared_only: bool| {
                    for base in bases {
                        let base_name = base.name.trim();
                        if base_name.is_empty() {
                            continue;
                        }
                        if !base.enabled {
                            continue;
                        }
                        if shared_only && !base.shared {
                            continue;
                        }
                        let description = if base.description.trim().is_empty() {
                            i18n::t_with_params(
                                "knowledge.tool.description",
                                &HashMap::from([("name".to_string(), base_name.to_string())]),
                            )
                        } else {
                            base.description.clone()
                        };
                        let alias_name = self.store.build_alias_name(owner_id, base_name);
                        if shared_only {
                            if let Some(filter) = shared_tools_filter.as_ref() {
                                if !filter.contains(&alias_name) {
                                    continue;
                                }
                            }
                        }
                        append_alias(
                            alias_name.clone(),
                            ToolSpec {
                                name: alias_name.clone(),
                                description,
                                input_schema: knowledge_schema.clone(),
                            },
                            UserToolKind::Knowledge,
                            owner_id.to_string(),
                            base_name.to_string(),
                        );
                    }
                };

            collect_knowledge_tools(&owner_id, &user_payload.knowledge_bases, false);
            for shared_payload in &shared_payloads {
                let shared_owner = if shared_payload.user_id.trim().is_empty() {
                    user_id.to_string()
                } else {
                    shared_payload.user_id.clone()
                };
                collect_knowledge_tools(&shared_owner, &shared_payload.knowledge_bases, true);
            }
        }

        UserToolBindings {
            alias_specs,
            alias_map,
            skill_specs,
            skill_sources,
            mcp_servers,
            shared_tools_enabled,
            user_version: user_payload.version,
            shared_version: self.store.shared_version(),
        }
    }

    /// 获取用户技能注册表，必要时加载入口脚本。
    pub fn get_user_skill_registry(
        &self,
        config: &Config,
        bindings: &UserToolBindings,
        owner_id: &str,
    ) -> Option<SkillRegistry> {
        let source = bindings.skill_sources.get(owner_id)?;
        if source.names.is_empty() {
            return None;
        }
        let signature = build_skill_signature(&source.root, source.names.iter().cloned());
        let key = format!("{owner_id}::{}", source.root.to_string_lossy());
        let mut cache = self
            .skill_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(entry) = cache.registry_cache.get(&key) {
            if entry.signature == signature {
                return Some(entry.registry.clone());
            }
        }
        let mut scan_config = config.clone();
        scan_config.skills.paths = vec![source.root.to_string_lossy().to_string()];
        scan_config.skills.enabled = source.names.clone();
        let registry = load_skills(&scan_config, true, true, false);
        cache.registry_cache.insert(
            key.clone(),
            SkillRegistryCacheEntry {
                signature,
                registry: registry.clone(),
            },
        );
        Some(registry)
    }

    fn load_cached_skill_specs(
        &self,
        config: &Config,
        owner_id: &str,
        root: &Path,
        names: &HashSet<String>,
    ) -> Vec<SkillSpec> {
        if names.is_empty() || !root.exists() {
            return Vec::new();
        }
        let signature = build_skill_signature(root, names.iter().cloned());
        let key = format!("{owner_id}::{}", root.to_string_lossy());
        let mut cache = self
            .skill_cache
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(entry) = cache.spec_cache.get(&key) {
            if entry.signature == signature {
                return entry.specs.clone();
            }
        }
        let mut scan_config = config.clone();
        scan_config.skills.paths = vec![root.to_string_lossy().to_string()];
        scan_config.skills.enabled = names.iter().cloned().collect();
        let registry = load_skills(&scan_config, false, true, false);
        let specs = registry.list_specs();
        cache.spec_cache.insert(
            key.clone(),
            SkillSpecCacheEntry {
                signature,
                specs: specs.clone(),
            },
        );
        cache.order.push_back(key.clone());
        while cache.order.len() > self.skill_cache_max {
            if let Some(old_key) = cache.order.pop_front() {
                cache.spec_cache.remove(&old_key);
                cache.registry_cache.remove(&old_key);
            }
        }
        specs
    }
}

fn build_knowledge_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": i18n::t("knowledge.tool.query.description") },
            "keywords": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": i18n::t("knowledge.tool.keywords.description") },
            "limit": { "type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description") }
        },
        "anyOf": [
            { "required": ["query"] },
            { "required": ["keywords"] }
        ]
    })
}

fn collect_mcp_tool_names(config: &Config) -> HashSet<String> {
    let mut names = HashSet::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        if server.tool_specs.is_empty() {
            continue;
        }
        let allow: HashSet<String> = server.allow_tools.iter().cloned().collect();
        for tool in &server.tool_specs {
            if tool.name.is_empty() {
                continue;
            }
            if !allow.is_empty() && !allow.contains(&tool.name) {
                continue;
            }
            names.insert(format!("{}@{}", server.name, tool.name));
        }
    }
    names
}

fn collect_knowledge_tool_names(config: &Config, skill_names: &HashSet<String>) -> HashSet<String> {
    let mut names = HashSet::new();
    for base in &config.knowledge.bases {
        if !base.enabled {
            continue;
        }
        let name = base.name.trim();
        if name.is_empty() {
            continue;
        }
        if skill_names.contains(name) {
            continue;
        }
        names.insert(name.to_string());
    }
    names
}

fn parse_mcp_servers(value: &Value) -> Vec<UserMcpServer> {
    let Some(servers) = value.get("mcp").and_then(|item| item.get("servers")) else {
        return Vec::new();
    };
    let Some(list) = servers.as_array() else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for item in list {
        if let Some(server) = parse_user_mcp_server(item) {
            output.push(server);
        }
    }
    output
}

fn parse_user_mcp_server(value: &Value) -> Option<UserMcpServer> {
    let obj = value.as_object()?;
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let endpoint = obj
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let allow_tools = parse_name_list(obj.get("allow_tools"));
    let mut shared_tools = parse_name_list(obj.get("shared_tools"));
    if !allow_tools.is_empty() {
        let allow_set: HashSet<String> = allow_tools.iter().cloned().collect();
        shared_tools.retain(|name| allow_set.contains(name));
    }
    let headers = parse_headers(obj.get("headers"));
    let tool_specs = obj
        .get("tool_specs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let enabled = obj.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    Some(UserMcpServer {
        name,
        endpoint,
        allow_tools,
        shared_tools,
        enabled,
        transport: obj
            .get("transport")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        display_name: obj
            .get("display_name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        headers,
        auth: obj.get("auth").cloned(),
        tool_specs,
    })
}

fn parse_knowledge_bases(value: &Value) -> Vec<UserKnowledgeBase> {
    let Some(list) = value
        .get("knowledge")
        .and_then(|item| item.get("bases"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut output = Vec::new();
    for item in list {
        if let Some(base) = parse_user_knowledge_base(item) {
            output.push(base);
        }
    }
    output
}

fn parse_user_knowledge_base(value: &Value) -> Option<UserKnowledgeBase> {
    let obj = value.as_object()?;
    Some(UserKnowledgeBase {
        name: obj
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        enabled: obj.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        shared: obj.get("shared").and_then(Value::as_bool).unwrap_or(false),
        base_type: obj
            .get("base_type")
            .or_else(|| obj.get("type"))
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        embedding_model: obj
            .get("embedding_model")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        chunk_size: parse_optional_usize(obj.get("chunk_size")),
        chunk_overlap: parse_optional_usize(obj.get("chunk_overlap")),
        top_k: parse_optional_usize(obj.get("top_k")),
        score_threshold: parse_optional_f32(obj.get("score_threshold")),
    })
}

fn normalize_name_list(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    output
}

fn normalize_mcp_servers(mut servers: Vec<UserMcpServer>) -> Vec<UserMcpServer> {
    for server in &mut servers {
        server.allow_tools = normalize_name_list(server.allow_tools.clone());
        server.shared_tools = normalize_name_list(server.shared_tools.clone());
        if !server.allow_tools.is_empty() {
            let allow: HashSet<String> = server.allow_tools.iter().cloned().collect();
            server.shared_tools.retain(|name| allow.contains(name));
        }
    }
    servers
}

fn normalize_skill_config(enabled: Vec<String>, shared: Vec<String>) -> UserSkillConfig {
    let enabled = normalize_name_list(enabled);
    let mut shared = normalize_name_list(shared);
    let allow: HashSet<String> = enabled.iter().cloned().collect();
    shared.retain(|name| allow.contains(name));
    UserSkillConfig { enabled, shared }
}

fn normalize_knowledge_bases(bases: Vec<UserKnowledgeBase>) -> Vec<UserKnowledgeBase> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for base in bases {
        let name = base.name.trim().to_string();
        if !name.is_empty() {
            if seen.contains(&name) {
                continue;
            }
            seen.insert(name.clone());
        }
        output.push(base);
    }
    output
}

fn parse_name_list(value: Option<&Value>) -> Vec<String> {
    let mut output = Vec::new();
    let Some(value) = value else {
        return output;
    };
    if let Some(list) = value.as_array() {
        for item in list {
            if let Some(text) = item.as_str() {
                output.push(text.to_string());
            } else {
                output.push(item.to_string());
            }
        }
    }
    output
}

fn parse_headers(value: Option<&Value>) -> HashMap<String, String> {
    let mut output = HashMap::new();
    let Some(Value::Object(map)) = value else {
        return output;
    };
    for (key, value) in map {
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let val = match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        if val.trim().is_empty() {
            continue;
        }
        output.insert(key, val);
    }
    output
}

fn user_mcp_server_to_value(server: &UserMcpServer) -> Value {
    json!({
        "name": server.name,
        "display_name": server.display_name,
        "endpoint": server.endpoint,
        "transport": server.transport,
        "description": server.description,
        "headers": server.headers,
        "auth": server.auth,
        "tool_specs": server.tool_specs,
        "allow_tools": server.allow_tools,
        "shared_tools": server.shared_tools,
        "enabled": server.enabled
    })
}

fn user_knowledge_base_to_value(base: &UserKnowledgeBase) -> Value {
    json!({
        "name": base.name,
        "description": base.description,
        "enabled": base.enabled,
        "shared": base.shared,
        "base_type": base.base_type,
        "embedding_model": base.embedding_model,
        "chunk_size": base.chunk_size,
        "chunk_overlap": base.chunk_overlap,
        "top_k": base.top_k,
        "score_threshold": base.score_threshold
    })
}

fn parse_optional_usize(value: Option<&Value>) -> Option<usize> {
    match value {
        Some(Value::Number(num)) => num.as_u64().map(|value| value as usize),
        Some(Value::String(text)) => text.trim().parse::<usize>().ok(),
        Some(Value::Bool(value)) => Some(if *value { 1 } else { 0 }),
        _ => None,
    }
}

fn parse_optional_f32(value: Option<&Value>) -> Option<f32> {
    match value {
        Some(Value::Number(num)) => num.as_f64().map(|value| value as f32),
        Some(Value::String(text)) => text.trim().parse::<f32>().ok(),
        _ => None,
    }
}

fn normalize_mcp_input_schema(tool: &Value) -> Value {
    if let Some(schema) = tool.get("inputSchema").or_else(|| tool.get("input_schema")) {
        if schema.is_object() {
            return schema.clone();
        }
    }
    json!({"type": "object", "properties": {}})
}

fn resolve_mcp_description(server: &UserMcpServer, tool: &Value) -> String {
    let description = tool
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if !description.is_empty() {
        return description.to_string();
    }
    if !server.description.trim().is_empty() {
        return server.description.clone();
    }
    if !server.display_name.trim().is_empty() {
        return server.display_name.clone();
    }
    "".to_string()
}

fn user_mcp_to_config(server: &UserMcpServer) -> McpServerConfig {
    let mut tool_specs = Vec::new();
    for tool in &server.tool_specs {
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if name.is_empty() {
            continue;
        }
        let description = tool
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        tool_specs.push(McpToolSpec {
            name: name.to_string(),
            description,
            input_schema: serde_yaml::to_value(normalize_mcp_input_schema(tool))
                .unwrap_or(serde_yaml::Value::Null),
        });
    }
    McpServerConfig {
        name: server.name.clone(),
        endpoint: server.endpoint.clone(),
        allow_tools: server.allow_tools.clone(),
        enabled: server.enabled,
        transport: if server.transport.trim().is_empty() {
            None
        } else {
            Some(server.transport.clone())
        },
        description: if server.description.trim().is_empty() {
            None
        } else {
            Some(server.description.clone())
        },
        display_name: if server.display_name.trim().is_empty() {
            None
        } else {
            Some(server.display_name.clone())
        },
        headers: server.headers.clone(),
        auth: server
            .auth
            .as_ref()
            .and_then(|value| serde_yaml::to_value(value).ok()),
        tool_specs,
    }
}

fn file_modified_ts(path: &Path) -> f64 {
    let Ok(meta) = path.metadata() else {
        return 0.0;
    };
    let Ok(modified) = meta.modified() else {
        return 0.0;
    };
    modified
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

fn resolve_user_tools_root() -> PathBuf {
    std::env::var(USER_TOOLS_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data").join("user_tools"))
}

fn safe_user_id(user_id: &str) -> String {
    let cleaned = user_id.trim();
    if cleaned.is_empty() {
        return "anonymous".to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    output
}

fn build_skill_signature(
    root: &Path,
    names: impl IntoIterator<Item = String>,
) -> (f64, Vec<String>) {
    let mtime = file_modified_ts(root);
    let mut list: Vec<String> = names.into_iter().collect();
    list.sort();
    (mtime, list)
}
