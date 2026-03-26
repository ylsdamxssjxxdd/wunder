// 用户自建工具：负责配置存储、别名绑定与共享工具聚合。
use crate::config::{Config, KnowledgeBaseType, McpServerConfig, McpToolSpec};
use crate::core::json_schema::normalize_tool_input_schema;
use crate::i18n;
use crate::path_utils::{normalize_path_for_compare, normalize_target_path};
use crate::schemas::ToolSpec;
use crate::skills::{load_skills, SkillRegistry, SkillSpec};
use crate::storage::USER_PRIVATE_CONTAINER_ID;
use crate::vector_knowledge;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

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

#[derive(Debug, Clone, Serialize, Default)]
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
    workspace: Arc<WorkspaceManager>,
    legacy_root: PathBuf,
    cache: Mutex<HashMap<String, UserToolsCacheEntry>>,
}

impl UserToolStore {
    pub fn new(_config: &Config, workspace: Arc<WorkspaceManager>) -> Result<Self> {
        let legacy_root = resolve_user_tools_root();
        std::fs::create_dir_all(&legacy_root)?;
        Ok(Self {
            workspace,
            legacy_root,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// 构造统一的别名格式：user_id@tool_name。
    pub fn build_alias_name(&self, owner_id: &str, tool_name: &str) -> String {
        format!("{}@{}", owner_id, tool_name)
    }

    pub fn build_user_skill_name(
        &self,
        current_user_id: &str,
        owner_id: &str,
        skill_name: &str,
    ) -> String {
        let normalized_owner_id = owner_id.trim();
        let normalized_current_user_id = current_user_id.trim();
        let normalized_skill_name = skill_name.trim();
        if normalized_owner_id == normalized_current_user_id {
            normalized_skill_name.to_string()
        } else {
            self.build_alias_name(normalized_owner_id, normalized_skill_name)
        }
    }

    /// 获取共享工具版本号，用于提示词缓存判断。
    pub fn shared_version(&self) -> f64 {
        0.0
    }

    /// 读取指定用户的工具配置并做字段清洗。
    pub fn load_user_tools(&self, user_id: &str) -> UserToolsPayload {
        let safe_id = safe_user_id(user_id);
        let path = self.config_path(&safe_id);
        let legacy_path = self.legacy_config_path(&safe_id);
        let config_exists = path.exists();
        let version = if config_exists {
            file_modified_ts(&path)
        } else {
            file_modified_ts(&legacy_path)
        };
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
        let cached_payload = self
            .cache
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .get(&safe_id)
            .map(|entry| entry.payload.clone());
        let mut payload = if config_exists {
            self.read_payload(&path, user_id).unwrap_or_else(|err| {
                tracing::warn!(
                    "failed to parse user tooling config {}; using fallback: {err}",
                    path.display()
                );
                cached_payload.unwrap_or_else(|| UserToolsPayload {
                    user_id: user_id.to_string(),
                    ..UserToolsPayload::default()
                })
            })
        } else if legacy_path.exists() {
            self.read_payload(&legacy_path, user_id)
                .unwrap_or_else(|_| UserToolsPayload {
                    user_id: user_id.to_string(),
                    ..UserToolsPayload::default()
                })
        } else {
            UserToolsPayload {
                user_id: user_id.to_string(),
                ..UserToolsPayload::default()
            }
        };
        if !config_exists && payload.skills.enabled.is_empty() {
            let default_enabled = self.resolve_default_skill_enabled(user_id);
            if !default_enabled.is_empty() {
                payload.skills =
                    normalize_skill_config(default_enabled, payload.skills.shared.clone());
            }
        }
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
        payload
    }

    pub fn ensure_materialized(&self, user_id: &str) -> Result<()> {
        let safe_id = safe_user_id(user_id);
        let path = self.config_path(&safe_id);
        if path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            return Ok(());
        }
        let legacy_path = self.legacy_config_path(&safe_id);
        let payload = if legacy_path.exists() {
            self.read_payload(&legacy_path, user_id)?
        } else {
            self.load_user_tools(user_id)
        };
        let _ = self.save_payload(user_id, payload)?;
        Ok(())
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
        let discovered = self.resolve_default_skill_enabled(user_id);
        let effective_enabled = if discovered.is_empty() {
            enabled
        } else {
            discovered
        };
        payload.skills = normalize_skill_config(effective_enabled, shared);
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
        _shared_tools: Vec<String>,
    ) -> Result<UserToolsPayload> {
        let mut payload = self.load_user_tools(user_id);
        payload.shared_tools.clear();
        self.save_payload(user_id, payload)
    }

    /// 列出所有共享配置（排除当前用户）。
    pub fn list_shared_payloads(&self, exclude_user_id: &str) -> Vec<UserToolsPayload> {
        let _ = exclude_user_id;
        Vec::new()
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

    fn read_payload(&self, path: &Path, fallback_user_id: &str) -> Result<UserToolsPayload> {
        if !path.exists() {
            return Ok(UserToolsPayload {
                user_id: fallback_user_id.to_string(),
                ..UserToolsPayload::default()
            });
        }
        let content = std::fs::read_to_string(path)?;
        let value: Value = serde_json::from_str(&content)?;
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
        Ok(UserToolsPayload {
            user_id,
            mcp_servers,
            skills,
            knowledge_bases,
            shared_tools: Vec::new(),
            version: 0.0,
        })
    }

    fn save_payload(
        &self,
        user_id: &str,
        mut payload: UserToolsPayload,
    ) -> Result<UserToolsPayload> {
        payload.shared_tools.clear();
        let safe_id = safe_user_id(user_id);
        let folder = self
            .config_path(&safe_id)
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("invalid tooling config path"))?;
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
        Ok(payload)
    }

    fn user_dir(&self, safe_user_id: &str) -> PathBuf {
        let scoped_user_id = self
            .workspace
            .scoped_user_id_by_container(safe_user_id, USER_PRIVATE_CONTAINER_ID);
        self.workspace.workspace_root(&scoped_user_id)
    }

    fn config_path(&self, safe_user_id: &str) -> PathBuf {
        self.user_dir(safe_user_id)
            .join("global")
            .join("tooling.json")
    }

    fn legacy_config_path(&self, safe_user_id: &str) -> PathBuf {
        self.legacy_root.join(safe_user_id).join("config.json")
    }

    fn resolve_default_skill_enabled(&self, user_id: &str) -> Vec<String> {
        let skill_root = self.get_skill_root(user_id);
        if !skill_root.exists() || !skill_root.is_dir() {
            return Vec::new();
        }
        let mut scan_config = Config::default();
        scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
        scan_config.skills.enabled = Vec::new();
        let registry = load_skills(&scan_config, false, false, false);
        let mut names = registry
            .list_specs()
            .into_iter()
            .map(|spec| spec.name)
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
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
        blocked_names.extend(skill_names.clone());
        blocked_names.extend(knowledge_names);

        let mut alias_specs = HashMap::new();
        let mut alias_map = HashMap::new();
        let mut skill_specs = Vec::new();
        let mut skill_sources = HashMap::new();
        let mut mcp_servers: HashMap<String, HashMap<String, McpServerConfig>> = HashMap::new();
        let knowledge_schema = build_knowledge_schema();

        let current_owner_id = if user_payload.user_id.trim().is_empty() {
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
                        if server.tool_specs.is_empty() {
                            continue;
                        }
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
                        let enabled_names = if shared_only {
                            tool_pool
                                .into_iter()
                                .filter(|name| shared_tools.contains(name))
                                .collect()
                        } else {
                            tool_pool
                        };
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

            collect_mcp_tools(&current_owner_id, &user_payload.mcp_servers, false);
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
            let mut collect_skill_tools =
                |skill_owner_id: &str, names: &[String], shared_only: bool| {
                    let skill_root = self.store.get_skill_root(skill_owner_id);
                    if !skill_root.exists() {
                        return;
                    }
                    let requested_names: HashSet<String> = names
                        .iter()
                        .map(|name| name.trim().to_string())
                        .filter(|name| !name.is_empty())
                        .collect();
                    let specs = self.load_cached_skill_specs(
                        config,
                        skill_owner_id,
                        &skill_root,
                        &requested_names,
                    );
                    if specs.is_empty() {
                        return;
                    }
                    let mut enabled: HashSet<String> =
                        specs.iter().map(|spec| spec.name.clone()).collect();
                    if shared_only {
                        enabled.retain(|name| requested_names.contains(name));
                    }
                    if enabled.is_empty() {
                        return;
                    }
                    register_skill_source(
                        skill_owner_id,
                        skill_root.clone(),
                        enabled.iter().cloned().collect(),
                    );
                    let allow_bare_name = !shared_only && skill_owner_id == current_owner_id;
                    for spec in specs {
                        if shared_only && !enabled.contains(&spec.name) {
                            continue;
                        }
                        let alias_name = self.store.build_user_skill_name(
                            &current_owner_id,
                            skill_owner_id,
                            &spec.name,
                        );
                        let legacy_alias = allow_bare_name
                            .then(|| self.store.build_alias_name(skill_owner_id, &spec.name))
                            .filter(|legacy_name| legacy_name != &alias_name);
                        if shared_only {
                            if let Some(filter) = shared_tools_filter.as_ref() {
                                if !filter.contains(&alias_name) {
                                    continue;
                                }
                            }
                        }
                        let allow_global_skill_override =
                            allow_bare_name && skill_names.contains(&alias_name);
                        if (blocked_names.contains(&alias_name)
                            || alias_map.contains_key(&alias_name))
                            && !allow_global_skill_override
                        {
                            continue;
                        }
                        let alias_info = UserToolAlias {
                            kind: UserToolKind::Skill,
                            owner_id: skill_owner_id.to_string(),
                            target: spec.name.clone(),
                        };
                        blocked_names.insert(alias_name.clone());
                        alias_map.insert(alias_name.clone(), alias_info.clone());
                        if let Some(legacy_alias) = legacy_alias {
                            alias_map.entry(legacy_alias).or_insert(alias_info.clone());
                        }
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

            collect_skill_tools(&current_owner_id, &user_payload.skills.enabled, false);
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

            collect_knowledge_tools(&current_owner_id, &user_payload.knowledge_bases, false);
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
        if !root.exists() {
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
        server.allow_tools = Vec::new();
        server.shared_tools.clear();
        server.enabled = true;
    }
    servers
}

fn normalize_skill_config(enabled: Vec<String>, shared: Vec<String>) -> UserSkillConfig {
    let enabled = normalize_name_list(enabled);
    let effective_enabled = if enabled.is_empty() {
        normalize_name_list(shared)
    } else {
        enabled
    };
    UserSkillConfig {
        enabled: effective_enabled,
        shared: Vec::new(),
    }
}

fn normalize_knowledge_bases(bases: Vec<UserKnowledgeBase>) -> Vec<UserKnowledgeBase> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for mut base in bases {
        base.enabled = true;
        base.shared = false;
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
    normalize_tool_input_schema(tool.get("inputSchema").or_else(|| tool.get("input_schema")))
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
        allow_tools: Vec::new(),
        enabled: true,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::storage::SqliteStorage;
    use crate::workspace::WorkspaceManager;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn normalize_mcp_servers_removes_self_enable_state() {
        let servers = normalize_mcp_servers(vec![UserMcpServer {
            name: "demo".to_string(),
            endpoint: "http://127.0.0.1:9000/mcp".to_string(),
            allow_tools: vec!["tool-a".to_string()],
            shared_tools: vec!["tool-a".to_string(), "tool-b".to_string()],
            enabled: false,
            tool_specs: vec![json!({ "name": "tool-a" }), json!({ "name": "tool-c" })],
            ..UserMcpServer::default()
        }]);
        assert_eq!(servers.len(), 1);
        assert!(servers[0].enabled);
        assert!(servers[0].allow_tools.is_empty());
        assert!(servers[0].shared_tools.is_empty());
    }

    #[test]
    fn normalize_skill_config_preserves_shared_names_without_enabled_list() {
        let config =
            normalize_skill_config(Vec::new(), vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(
            config.enabled,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert!(config.shared.is_empty());
    }

    #[test]
    fn normalize_knowledge_bases_forces_enabled_true() {
        let bases = normalize_knowledge_bases(vec![UserKnowledgeBase {
            name: "kb".to_string(),
            enabled: false,
            shared: true,
            ..UserKnowledgeBase::default()
        }]);
        assert_eq!(bases.len(), 1);
        assert!(bases[0].enabled);
        assert!(!bases[0].shared);
    }

    #[test]
    fn user_mcp_to_config_ignores_legacy_enable_and_allow_filters() {
        let config = user_mcp_to_config(&UserMcpServer {
            name: "demo".to_string(),
            endpoint: "http://127.0.0.1:9000/mcp".to_string(),
            allow_tools: vec!["tool-a".to_string()],
            enabled: false,
            ..UserMcpServer::default()
        });
        assert!(config.enabled);
        assert!(config.allow_tools.is_empty());
    }

    #[test]
    fn load_user_tools_forces_runtime_user_id() {
        let root = tempdir().expect("tempdir");
        let db_path = root.path().join("user-tools.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = root.path().join("workspaces");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        ));
        let store = UserToolStore::new(&Config::default(), workspace).expect("create store");
        let user_id = "alice";
        let safe_id = safe_user_id(user_id);
        let path = store.config_path(&safe_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create config dir");
        }
        std::fs::write(
            &path,
            r#"{
  "user_id": "legacy-id",
  "skills": { "enabled": [], "shared": [] }
}"#,
        )
        .expect("write config");
        let payload = store.load_user_tools(user_id);
        assert_eq!(payload.user_id, user_id);
    }

    #[test]
    fn build_bindings_for_catalog_keeps_current_user_skill_name_when_name_matches_global_skill() {
        let root = tempdir().expect("tempdir");
        let db_path = root.path().join("user-tools.db");
        let storage = Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
        let workspace_root = root.path().join("workspaces");
        let workspace = Arc::new(WorkspaceManager::new(
            workspace_root.to_string_lossy().as_ref(),
            storage,
            0,
            &HashMap::new(),
        ));
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        let global_root = root.path().join("global-skills");
        let global_skill_dir = global_root.join("builtin-demo");
        std::fs::create_dir_all(&global_skill_dir).expect("create global skill dir");
        std::fs::write(
            global_skill_dir.join("SKILL.md"),
            "---\nname: summary_skill\ndescription: global\n---\n# global\n",
        )
        .expect("write global skill");
        config.skills.paths = vec![global_root.to_string_lossy().to_string()];

        let store = UserToolStore::new(&config, workspace).expect("create store");
        let user_skill_dir = store.get_skill_root("alice").join("upload-demo");
        std::fs::create_dir_all(&user_skill_dir).expect("create user skill dir");
        std::fs::write(
            user_skill_dir.join("SKILL.md"),
            "---\nname: summary_skill\ndescription: custom\n---\n# custom\n",
        )
        .expect("write user skill");
        store
            .update_skills("alice", vec!["summary_skill".to_string()], Vec::new())
            .expect("update user skills");

        let manager = UserToolManager::new(Arc::new(store));
        let global_skills = load_skills(&config, false, false, false);
        let bindings = manager.build_bindings_for_catalog(&config, &global_skills, "alice");

        assert!(
            bindings.alias_map.contains_key("summary_skill"),
            "custom user skills should stay mountable even if they share a name with a global skill"
        );
    }
}
