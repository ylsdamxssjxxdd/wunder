// 内置工具定义与执行入口，保持工具名称与协议一致。
use crate::a2a_store::{A2aStore, A2aTask};
use crate::config::{A2aServiceConfig, Config, KnowledgeBaseConfig};
use crate::i18n;
use crate::knowledge;
use crate::mcp;
use crate::path_utils::is_within_root;
use crate::sandbox;
use crate::schemas::ToolSpec;
use crate::skills::{execute_skill, SkillRegistry};
use crate::user_tools::{
    UserToolAlias, UserToolBindings, UserToolKind, UserToolManager, UserToolStore,
};
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::path::{Component, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::sleep;
use tracing::warn;
use uuid::Uuid;

pub struct ToolContext<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub workspace: Arc<WorkspaceManager>,
    pub config: &'a Config,
    pub a2a_store: &'a A2aStore,
    pub skills: &'a SkillRegistry,
    pub user_tool_manager: Option<&'a UserToolManager>,
    pub user_tool_bindings: Option<&'a UserToolBindings>,
    pub user_tool_store: Option<&'a UserToolStore>,
    pub http: &'a reqwest::Client,
}

pub fn builtin_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            name: "最终回复".to_string(),
            description: i18n::t("tool.spec.final.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": i18n::t("tool.spec.final.args.content")}
                },
                "required": ["content"]
            }),
        },
        ToolSpec {
            name: "a2ui".to_string(),
            description: i18n::t("tool.spec.a2ui.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "uid": {"type": "string", "description": i18n::t("tool.spec.a2ui.args.uid")},
                    "a2ui": {"type": "array", "description": i18n::t("tool.spec.a2ui.args.messages"), "items": {"type": "object"}},
                    "content": {"type": "string", "description": i18n::t("tool.spec.a2ui.args.content")}
                },
                "required": ["uid", "a2ui"]
            }),
        },
        ToolSpec {
            name: "a2a观察".to_string(),
            description: i18n::t("tool.spec.a2a_observe.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_ids": {"type": "array", "items": {"type": "string"}, "description": i18n::t("tool.spec.a2a_observe.args.task_ids")},
                    "tasks": {"type": "array", "items": {"type": "object"}, "description": i18n::t("tool.spec.a2a_observe.args.tasks")},
                    "endpoint": {"type": "string", "description": i18n::t("tool.spec.a2a_observe.args.endpoint")},
                    "service_name": {"type": "string", "description": i18n::t("tool.spec.a2a_observe.args.service_name")},
                    "refresh": {"type": "boolean", "description": i18n::t("tool.spec.a2a_observe.args.refresh")},
                    "timeout_s": {"type": "number", "description": i18n::t("tool.spec.a2a_observe.args.timeout")}
                }
            }),
        },
        ToolSpec {
            name: "a2a等待".to_string(),
            description: i18n::t("tool.spec.a2a_wait.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wait_s": {"type": "number", "description": i18n::t("tool.spec.a2a_wait.args.wait_s")},
                    "poll_interval_s": {"type": "number", "description": i18n::t("tool.spec.a2a_wait.args.poll_interval")},
                    "task_ids": {"type": "array", "items": {"type": "string"}},
                    "tasks": {"type": "array", "items": {"type": "object"}},
                    "endpoint": {"type": "string", "description": i18n::t("tool.spec.a2a_wait.args.endpoint")},
                    "service_name": {"type": "string", "description": i18n::t("tool.spec.a2a_wait.args.service_name")},
                    "refresh": {"type": "boolean", "description": i18n::t("tool.spec.a2a_wait.args.refresh")},
                    "timeout_s": {"type": "number", "description": i18n::t("tool.spec.a2a_wait.args.timeout")}
                }
            }),
        },
        ToolSpec {
            name: "执行命令".to_string(),
            description: i18n::t("tool.spec.exec.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": i18n::t("tool.spec.exec.args.content")},
                    "workdir": {"type": "string", "description": i18n::t("tool.spec.exec.args.workdir")},
                    "timeout_s": {"type": "integer", "description": i18n::t("tool.spec.exec.args.timeout")},
                    "shell": {"type": "boolean", "description": i18n::t("tool.spec.exec.args.shell")}
                },
                "required": ["content"]
            }),
        },
        ToolSpec {
            name: "ptc".to_string(),
            description: i18n::t("tool.spec.ptc.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filename": {"type": "string", "description": i18n::t("tool.spec.ptc.args.filename")},
                    "workdir": {"type": "string", "description": i18n::t("tool.spec.ptc.args.workdir")},
                    "content": {"type": "string", "description": i18n::t("tool.spec.ptc.args.content")}
                },
                "required": ["filename", "workdir", "content"]
            }),
        },
        ToolSpec {
            name: "列出文件".to_string(),
            description: i18n::t("tool.spec.list.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": i18n::t("tool.spec.list.args.path")}
                }
            }),
        },
        ToolSpec {
            name: "搜索内容".to_string(),
            description: i18n::t("tool.spec.search.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": i18n::t("tool.spec.search.args.query")},
                    "path": {"type": "string", "description": i18n::t("tool.spec.search.args.path")}
                },
                "required": ["query"]
            }),
        },
        ToolSpec {
            name: "读取文件".to_string(),
            description: i18n::t("tool.spec.read.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string", "description": i18n::t("tool.spec.read.args.files.path")},
                                "start_line": {"type": "integer", "description": i18n::t("tool.spec.read.args.files.start_line")},
                                "end_line": {"type": "integer", "description": i18n::t("tool.spec.read.args.files.end_line")}
                            },
                            "required": ["path"]
                        }
                    }
                },
                "required": ["files"]
            }),
        },
        ToolSpec {
            name: "写入文件".to_string(),
            description: i18n::t("tool.spec.write.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": i18n::t("tool.spec.write.args.path")},
                    "content": {"type": "string", "description": i18n::t("tool.spec.write.args.content")}
                },
                "required": ["path", "content"]
            }),
        },
        ToolSpec {
            name: "替换文本".to_string(),
            description: i18n::t("tool.spec.replace.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": i18n::t("tool.spec.replace.args.path")},
                    "old_string": {"type": "string", "description": i18n::t("tool.spec.replace.args.old_string")},
                    "new_string": {"type": "string", "description": i18n::t("tool.spec.replace.args.new_string")},
                    "expected_replacements": {"type": "integer", "description": i18n::t("tool.spec.replace.args.expected_replacements")}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        ToolSpec {
            name: "编辑文件".to_string(),
            description: i18n::t("tool.spec.edit.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": i18n::t("tool.spec.edit.args.path")},
                    "edits": {"type": "array", "description": i18n::t("tool.spec.edit.args.edits")},
                    "ensure_newline_at_eof": {"type": "boolean", "description": i18n::t("tool.spec.edit.args.ensure_newline")}
                },
                "required": ["path", "edits"]
            }),
        },
    ]
}

pub fn builtin_aliases() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("final_response".to_string(), "最终回复".to_string());
    map.insert("a2a_observe".to_string(), "a2a观察".to_string());
    map.insert("a2a_wait".to_string(), "a2a等待".to_string());
    map.insert("execute_command".to_string(), "执行命令".to_string());
    map.insert("programmatic_tool_call".to_string(), "ptc".to_string());
    map.insert("list_files".to_string(), "列出文件".to_string());
    map.insert("search_content".to_string(), "搜索内容".to_string());
    map.insert("read_file".to_string(), "读取文件".to_string());
    map.insert("write_file".to_string(), "写入文件".to_string());
    map.insert("replace_text".to_string(), "替换文本".to_string());
    map.insert("edit_file".to_string(), "编辑文件".to_string());
    map
}

pub fn resolve_tool_name(name: &str) -> String {
    let alias_map = builtin_aliases();
    alias_map
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// 工具调度入口：优先处理 A2A 与 MCP，再回落到内置工具。
pub async fn execute_tool(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let _ = context.session_id;
    let canonical = resolve_tool_name(name);
    if let Some(bindings) = context.user_tool_bindings {
        if let Some(alias) = bindings.alias_map.get(&canonical) {
            return execute_user_tool(context, alias, args).await;
        }
    }
    if let Some(skill) = context.skills.get(&canonical) {
        let result = execute_skill(&skill, args, 60).await?;
        context.workspace.mark_tree_dirty(context.user_id);
        return Ok(result);
    }
    if is_a2a_service_tool(&canonical) {
        return execute_a2a_service(context, &canonical, args).await;
    }
    if is_mcp_tool_name(&canonical) {
        return execute_mcp_tool(context, &canonical, args).await;
    }
    if let Some(base) = find_knowledge_base(context.config, &canonical) {
        return execute_knowledge_tool(context, &base, args).await;
    }
    execute_builtin_tool(context, &canonical, args).await
}

/// 汇总系统当前可用的工具名称（包含内置别名、MCP、A2A、技能与用户工具）。
pub fn collect_available_tool_names(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut enabled_builtin = HashSet::new();
    for name in &config.tools.builtin.enabled {
        let canonical = resolve_tool_name(name);
        if canonical.is_empty() {
            continue;
        }
        enabled_builtin.insert(canonical.clone());
        names.insert(canonical);
    }
    for server in &config.mcp.servers {
        if !server.enabled {
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
    for service in &config.a2a.services {
        if !service.enabled {
            continue;
        }
        if service.name.is_empty() {
            continue;
        }
        names.insert(format!("a2a@{}", service.name));
    }
    let skill_names: HashSet<String> = skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    names.extend(skill_names.clone());
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
    if let Some(bindings) = user_tool_bindings {
        names.extend(bindings.alias_map.keys().cloned());
        names.extend(bindings.skill_specs.iter().map(|spec| spec.name.clone()));
    }
    let alias_map = builtin_aliases();
    for (alias, canonical) in alias_map {
        if enabled_builtin.contains(&canonical) && !names.contains(&alias) {
            names.insert(alias);
        }
    }
    names
}

/// 构建提示词使用的工具规格，避免向模型暴露未启用的工具。
pub fn collect_prompt_tool_specs(
    config: &Config,
    skills: &SkillRegistry,
    allowed_names: &HashSet<String>,
    user_tool_bindings: Option<&UserToolBindings>,
) -> Vec<ToolSpec> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let language = i18n::get_language().to_lowercase();
    let alias_map = builtin_aliases();
    let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in alias_map {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }
    for spec in builtin_tool_specs() {
        let aliases: &[String] = canonical_aliases
            .get(&spec.name)
            .map(|value| value.as_slice())
            .unwrap_or(&[]);
        let enabled = allowed_names.contains(&spec.name)
            || aliases.iter().any(|alias| allowed_names.contains(alias));
        if !enabled {
            continue;
        }
        let preferred_alias = if language.starts_with("en") {
            aliases.iter().find(|alias| allowed_names.contains(*alias))
        } else {
            None
        };
        let name = preferred_alias
            .cloned()
            .unwrap_or_else(|| spec.name.clone());
        if !seen.insert(name.clone()) {
            continue;
        }
        output.push(ToolSpec {
            name,
            description: spec.description.clone(),
            input_schema: spec.input_schema.clone(),
        });
    }
    for server in &config.mcp.servers {
        if !server.enabled {
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
            let full_name = format!("{}@{}", server.name, tool.name);
            if !allowed_names.contains(&full_name) || !seen.insert(full_name.clone()) {
                continue;
            }
            output.push(ToolSpec {
                name: full_name,
                description: tool.description.clone(),
                input_schema: yaml_to_json(&tool.input_schema),
            });
        }
    }
    for service in &config.a2a.services {
        if !service.enabled {
            continue;
        }
        if service.name.is_empty() {
            continue;
        }
        let full_name = format!("a2a@{}", service.name);
        if !allowed_names.contains(&full_name) || !seen.insert(full_name.clone()) {
            continue;
        }
        output.push(ToolSpec {
            name: full_name,
            description: service.description.clone().unwrap_or_default(),
            input_schema: a2a_service_schema(),
        });
    }
    let skill_names: HashSet<String> = skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.name)
        .collect();
    for base in &config.knowledge.bases {
        if !base.enabled {
            continue;
        }
        let name = base.name.trim();
        if name.is_empty() || skill_names.contains(name) {
            continue;
        }
        if !allowed_names.contains(name) || !seen.insert(name.to_string()) {
            continue;
        }
        let description = if base.description.trim().is_empty() {
            i18n::t_with_params(
                "knowledge.tool.description",
                &HashMap::from([("name".to_string(), name.to_string())]),
            )
        } else {
            base.description.clone()
        };
        output.push(ToolSpec {
            name: name.to_string(),
            description,
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": i18n::t("knowledge.tool.query.description")},
                    "limit": {"type": "integer", "minimum": 1, "description": i18n::t("knowledge.tool.limit.description")}
                },
                "required": ["query"]
            }),
        });
    }
    if let Some(bindings) = user_tool_bindings {
        for (name, spec) in &bindings.alias_specs {
            if !allowed_names.contains(name) || !seen.insert(name.clone()) {
                continue;
            }
            output.push(spec.clone());
        }
    }
    output
}

/// 将 YAML 配置值转换为 JSON，便于统一处理输入 Schema 与鉴权字段。
fn yaml_to_json(value: &YamlValue) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

/// A2A 服务工具的通用入参 Schema。
pub fn a2a_service_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": i18n::t("tool.spec.a2a_service.args.content")},
            "session_id": {"type": "string", "description": i18n::t("tool.spec.a2a_service.args.session_id")}
        },
        "required": ["content"]
    })
}

pub async fn execute_builtin_tool(
    context: &ToolContext<'_>,
    name: &str,
    args: &Value,
) -> Result<Value> {
    let canonical = resolve_tool_name(name);
    match canonical.as_str() {
        "最终回复" => Ok(json!({
            "answer": args.get("content").and_then(Value::as_str).unwrap_or("").to_string()
        })),
        "执行命令" => execute_command(context, args).await,
        "ptc" => execute_ptc(context, args).await,
        "列出文件" => list_files(context, args).await,
        "搜索内容" => search_content(context, args).await,
        "读取文件" => read_files(context, args),
        "写入文件" => write_file(context, args),
        "替换文本" => replace_text(context, args),
        "编辑文件" => edit_file(context, args),
        "a2a观察" => a2a_observe(context, args).await,
        "a2a等待" => a2a_wait(context, args).await,
        "a2ui" => Ok(
            json!({"uid": args.get("uid"), "a2ui": args.get("a2ui"), "content": args.get("content")}),
        ),
        _ => Err(anyhow!("未知内置工具: {canonical}")),
    }
}

async fn execute_user_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    match alias.kind {
        UserToolKind::Mcp => execute_user_mcp_tool(context, alias, args).await,
        UserToolKind::Skill => execute_user_skill(context, alias, args).await,
        UserToolKind::Knowledge => execute_user_knowledge(context, alias, args).await,
    }
}

async fn execute_user_skill(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let manager = context
        .user_tool_manager
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let registry = manager
        .get_user_skill_registry(context.config, bindings, &alias.owner_id)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_loaded")))?;
    let spec = registry
        .get(&alias.target)
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.user_skill_not_found")))?;
    let result = execute_skill(&spec, args, 60).await.map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.invoke.user_skill_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    context.workspace.mark_tree_dirty(context.user_id);
    Ok(result)
}

async fn execute_user_mcp_tool(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let target = alias.target.trim();
    let Some((server_name, tool_name)) = split_mcp_target(target) else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_name_invalid")));
    };
    let bindings = context
        .user_tool_bindings
        .ok_or_else(|| anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")))?;
    let server_map = bindings.mcp_servers.get(&alias.owner_id);
    let server_config = server_map.and_then(|map| map.get(server_name));
    let Some(server_config) = server_config else {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_server_unavailable")));
    };
    let result = mcp::call_tool_with_server(context.config, server_config, tool_name, args)
        .await
        .map_err(|err| {
            anyhow!(i18n::t_with_params(
                "tool.invoke.mcp_call_failed",
                &HashMap::from([("detail".to_string(), err.to_string())]),
            ))
        })?;
    if result
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(anyhow!(i18n::t("tool.invoke.mcp_result_error")));
    }
    Ok(json!({
        "server": server_name,
        "tool": tool_name,
        "result": result
    }))
}

async fn execute_user_knowledge(
    context: &ToolContext<'_>,
    alias: &UserToolAlias,
    args: &Value,
) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    }
    let store = context
        .user_tool_store
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let root = store
        .resolve_knowledge_base_root(&alias.owner_id, &alias.target, false)
        .map_err(|err| anyhow!(err.to_string()))?;
    let base = KnowledgeBaseConfig {
        name: alias.target.clone(),
        description: String::new(),
        root: root.to_string_lossy().to_string(),
        enabled: true,
        shared: None,
    };
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = knowledge::query_knowledge_documents(
        &query,
        &base,
        llm_config.as_ref(),
        extract_limit(args),
        None,
    )
    .await;
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(json!({
        "knowledge_base": base.name,
        "documents": documents
    }))
}

async fn execute_knowledge_tool(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    args: &Value,
) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    }
    let _ =
        knowledge::resolve_knowledge_root(base, false).map_err(|err| anyhow!(err.to_string()))?;
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = knowledge::query_knowledge_documents(
        &query,
        base,
        llm_config.as_ref(),
        extract_limit(args),
        None,
    )
    .await;
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(json!({
        "knowledge_base": base.name,
        "documents": documents
    }))
}

fn split_mcp_target(target: &str) -> Option<(&str, &str)> {
    let mut parts = target.splitn(2, '@');
    let server = parts.next()?.trim();
    let tool = parts.next()?.trim();
    if server.is_empty() || tool.is_empty() {
        None
    } else {
        Some((server, tool))
    }
}

fn find_knowledge_base(config: &Config, name: &str) -> Option<KnowledgeBaseConfig> {
    config
        .knowledge
        .bases
        .iter()
        .find(|base| base.enabled && base.name == name && !base.root.trim().is_empty())
        .cloned()
}

fn extract_limit(args: &Value) -> Option<usize> {
    let Some(value) = args.get("limit") else {
        return None;
    };
    if let Some(num) = value.as_u64() {
        return Some(num as usize);
    }
    if let Some(num) = value.as_i64() {
        if num > 0 {
            return Some(num as usize);
        }
    }
    if let Some(num) = value.as_f64() {
        if num > 0.0 {
            return Some(num as usize);
        }
    }
    if let Some(text) = value.as_str() {
        if let Ok(num) = text.trim().parse::<usize>() {
            if num > 0 {
                return Some(num);
            }
        }
    }
    None
}

async fn execute_command(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.session_id,
            "执行命令",
            args,
            context.user_tool_bindings,
        )
        .await;
        context.workspace.mark_tree_dirty(context.user_id);
        return Ok(result);
    }

    let command = args
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少命令内容"))?;
    let allow_commands = &context.config.security.allow_commands;
    if !allow_commands.contains(&"*".to_string())
        && !allow_commands.iter().any(|item| command.starts_with(item))
    {
        return Err(anyhow!("命令未在白名单内"));
    }
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let cwd = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.user_id)?
    } else {
        context.workspace.resolve_path(context.user_id, workdir)?
    };
    let mut cmd = Command::new("bash");
    cmd.arg("-lc").arg(command).current_dir(cwd);
    let output = cmd.output().await?;
    let exit_code = output.status.code().unwrap_or(-1);
    let error = if exit_code == 0 {
        String::new()
    } else {
        i18n::t("tool.exec.failed")
    };
    context.workspace.mark_tree_dirty(context.user_id);
    Ok(json!({
        "ok": exit_code == 0,
        "data": {
            "exit_code": exit_code,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "timestamp": Utc::now().to_rfc3339(),
        },
        "error": error,
        "sandbox": false,
    }))
}

async fn execute_ptc(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.session_id,
            "ptc",
            args,
            context.user_tool_bindings,
        )
        .await;
        context.workspace.mark_tree_dirty(context.user_id);
        return Ok(result);
    }

    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or("ptc.tmp");
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let dir = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.user_id)?
    } else {
        context.workspace.resolve_path(context.user_id, workdir)?
    };
    let file_path = dir.join(filename);
    tokio::fs::create_dir_all(&dir).await.ok();
    tokio::fs::write(&file_path, content).await?;
    context.workspace.mark_tree_dirty(context.user_id);
    Ok(json!({
        "ok": true,
        "data": { "path": file_path.to_string_lossy() },
        "error": "",
        "sandbox": false,
    }))
}

async fn list_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
    let entries = context
        .workspace
        .list_entries_async(context.user_id, path)
        .await?;
    Ok(serde_json::to_value(entries)?)
}

async fn search_content(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let query = args.get("query").and_then(Value::as_str).unwrap_or("");
    let (entries, _total) = context
        .workspace
        .search_workspace_entries_async(context.user_id, query, 0, 0, true, true)
        .await?;
    Ok(serde_json::to_value(entries)?)
}

fn read_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let files = args
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("files ????"))?;
    let mut results = Vec::new();
    for file in files {
        if let Some(path) = file.get("path").and_then(Value::as_str) {
            let content = match context
                .workspace
                .read_file(context.user_id, path, 1024 * 1024)
            {
                Ok(content) => content,
                Err(err) => {
                    if let Some(resolved) = resolve_skill_read_path(context, path) {
                        std::fs::read_to_string(&resolved)?
                    } else {
                        return Err(err);
                    }
                }
            };
            results.push(json!({"path": path, "content": content}));
        }
    }
    Ok(Value::Array(results))
}

fn resolve_skill_read_path(context: &ToolContext<'_>, raw_path: &str) -> Option<PathBuf> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let candidate = {
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            path
        } else {
            let relative = sanitize_relative_path(trimmed)?;
            let cwd = std::env::current_dir().ok()?;
            cwd.join(relative)
        }
    };
    let mut roots: Vec<PathBuf> = context
        .skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.root)
        .collect();
    if let Some(bindings) = context.user_tool_bindings {
        for source in bindings.skill_sources.values() {
            roots.push(source.root.clone());
        }
    }
    for root in roots {
        if is_within_root(&root, &candidate) {
            return Some(candidate.clone());
        }
    }
    None
}

fn sanitize_relative_path(raw_path: &str) -> Option<PathBuf> {
    let normalized = raw_path.trim().replace('\\', "/");
    let stripped = normalized.strip_prefix("./").unwrap_or(&normalized);
    if stripped.is_empty() {
        return None;
    }
    let path = PathBuf::from(stripped);
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::ParentDir => {
                return None;
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(path)
}

fn write_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    context
        .workspace
        .write_file(context.user_id, path, content, true)?;
    Ok(json!({"ok": true, "path": path}))
}

fn replace_text(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let old = args
        .get("old_string")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 old_string"))?;
    let new_str = args.get("new_string").and_then(Value::as_str).unwrap_or("");
    let expected = args.get("expected_replacements").and_then(Value::as_u64);
    let target = context.workspace.resolve_path(context.user_id, path)?;
    let content = std::fs::read_to_string(&target)?;
    let replaced = content.replace(old, new_str);
    let count = content.matches(old).count() as u64;
    if let Some(expected) = expected {
        if count != expected {
            return Err(anyhow!("替换次数不匹配，期望 {expected}，实际 {count}"));
        }
    }
    std::fs::write(&target, replaced)?;
    context.workspace.bump_version(context.user_id);
    Ok(json!({"ok": true, "replacements": count}))
}

fn edit_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let edits = args
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("缺少 edits"))?;
    let target = context.workspace.resolve_path(context.user_id, path)?;
    let content = std::fs::read_to_string(&target)?;
    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    for edit in edits {
        let action = edit
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("replace");
        let start_line = edit.get("start_line").and_then(Value::as_u64).unwrap_or(1);
        let end_line = edit
            .get("end_line")
            .and_then(Value::as_u64)
            .unwrap_or(start_line);
        let new_content = edit
            .get("new_content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let start_idx = (start_line.saturating_sub(1)) as usize;
        let end_idx = (end_line.saturating_sub(1)) as usize;
        match action {
            "replace" => {
                for idx in start_idx..=end_idx.min(lines.len().saturating_sub(1)) {
                    lines[idx] = new_content.to_string();
                }
            }
            "insert_before" => {
                if start_idx <= lines.len() {
                    lines.insert(start_idx, new_content.to_string());
                }
            }
            "insert_after" => {
                let idx = (end_idx + 1).min(lines.len());
                lines.insert(idx, new_content.to_string());
            }
            "delete" => {
                if start_idx < lines.len() {
                    let end = end_idx.min(lines.len().saturating_sub(1));
                    lines.drain(start_idx..=end);
                }
            }
            _ => warn!("未知编辑动作: {action}"),
        }
    }
    let ensure_newline = args
        .get("ensure_newline_at_eof")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let mut output = lines.join("\n");
    if ensure_newline && !output.ends_with('\n') {
        output.push('\n');
    }
    std::fs::write(&target, output)?;
    context.workspace.bump_version(context.user_id);
    Ok(json!({"ok": true}))
}

#[derive(Clone)]
struct A2aTaskSnapshot {
    task_id: String,
    context_id: Option<String>,
    status: Option<String>,
    endpoint: Option<String>,
    service_name: Option<String>,
    answer: Option<String>,
    updated_time: Option<String>,
    refresh_error: Option<String>,
}

impl A2aTaskSnapshot {
    fn to_value(&self) -> Value {
        json!({
            "task_id": self.task_id,
            "context_id": self.context_id,
            "status": self.status,
            "endpoint": self.endpoint,
            "service_name": self.service_name,
            "answer": self.answer,
            "updated_time": self.updated_time,
            "refresh_error": self.refresh_error,
        })
    }

    fn is_done(&self) -> bool {
        self.status
            .as_deref()
            .map(is_a2a_task_finished)
            .unwrap_or(false)
    }
}

struct A2aTaskInfo {
    id: String,
    context_id: Option<String>,
    status: Option<String>,
    answer: Option<String>,
}

struct A2aObserveSnapshot {
    tasks: Vec<A2aTaskSnapshot>,
    pending: Vec<A2aTaskSnapshot>,
}

fn is_a2a_service_tool(name: &str) -> bool {
    name.starts_with("a2a@") && name.len() > "a2a@".len()
}

fn is_mcp_tool_name(name: &str) -> bool {
    name.contains('@') && !is_a2a_service_tool(name)
}

fn split_mcp_tool_name(name: &str) -> Result<(String, String)> {
    let (server, tool) = name
        .split_once('@')
        .ok_or_else(|| anyhow!("MCP 工具名称格式不正确"))?;
    if server.trim().is_empty() || tool.trim().is_empty() {
        return Err(anyhow!("MCP 工具名称格式不正确"));
    }
    Ok((server.trim().to_string(), tool.trim().to_string()))
}

async fn execute_mcp_tool(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let (server_name, tool_name) = split_mcp_tool_name(name)?;
    mcp::call_tool(context.config, &server_name, &tool_name, args).await
}

/// 调用 A2A 服务执行任务，并将结果写入任务存储。
async fn execute_a2a_service(context: &ToolContext<'_>, name: &str, args: &Value) -> Result<Value> {
    let service_name = name.trim_start_matches("a2a@");
    let service = resolve_a2a_service(context.config, service_name, "")
        .ok_or_else(|| anyhow!("A2A 服务不存在: {service_name}"))?;
    if !service.enabled {
        return Err(anyhow!("A2A 服务已禁用: {service_name}"));
    }
    let content = extract_text_arg(args, &["content", "task", "message", "text"])
        .ok_or_else(|| anyhow!("A2A 任务内容不能为空"))?;
    let session_id = extract_text_arg(args, &["session_id", "context_id", "task_id"]);
    let user_id = service
        .user_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(context.user_id);
    let mut message = json!({
        "parts": [
            { "text": content }
        ]
    });
    if let Some(session_id) = session_id.as_ref() {
        message["taskId"] = Value::String(session_id.clone());
        message["contextId"] = Value::String(session_id.clone());
    }
    let mut params = json!({ "message": message });
    if !user_id.trim().is_empty() {
        params["userId"] = Value::String(user_id.to_string());
    }
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "SendMessage",
        "params": params
    });
    let headers = build_a2a_headers(context.config, service)?;
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);
    let response = send_a2a_request(
        context.http,
        &service.endpoint,
        headers,
        &payload,
        timeout_s,
    )
    .await?;
    let info = parse_a2a_task_info(&response).ok_or_else(|| anyhow!("A2A 返回缺少任务信息"))?;
    let now = Utc::now();
    context.a2a_store.insert(A2aTask {
        id: info.id.clone(),
        user_id: context.user_id.to_string(),
        status: info.status.clone().unwrap_or_default(),
        context_id: info.context_id.clone(),
        endpoint: Some(service.endpoint.clone()),
        service_name: Some(service.name.clone()),
        method: Some("SendMessage".to_string()),
        created_time: now,
        updated_time: now,
        answer: info.answer.clone().unwrap_or_default(),
    });
    Ok(json!({
        "endpoint": service.endpoint,
        "service_name": service.name,
        "task_id": info.id,
        "context_id": info.context_id,
        "status": info.status,
        "answer": info.answer,
    }))
}

/// 观察 A2A 任务状态并返回快照。
async fn a2a_observe(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let snapshot = a2a_observe_snapshot(context, args).await?;
    Ok(json!({
        "tasks": snapshot.tasks.iter().map(|item| item.to_value()).collect::<Vec<_>>(),
        "pending": snapshot.pending.iter().map(|item| item.to_value()).collect::<Vec<_>>(),
        "done": snapshot.pending.is_empty(),
        "total": snapshot.tasks.len(),
    }))
}

/// 等待 A2A 任务完成或达到超时时间。
async fn a2a_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let timeout_s = args
        .get("wait_s")
        .and_then(Value::as_f64)
        .or_else(|| args.get("timeout_s").and_then(Value::as_f64))
        .unwrap_or(30.0)
        .max(0.0);
    let poll_interval_s = args
        .get("poll_interval_s")
        .and_then(Value::as_f64)
        .unwrap_or(1.5)
        .max(0.2);
    let start = Instant::now();
    let mut last_snapshot = a2a_observe_snapshot(context, args).await?;
    loop {
        if last_snapshot.pending.is_empty() {
            break;
        }
        if timeout_s > 0.0 && start.elapsed().as_secs_f64() >= timeout_s {
            break;
        }
        let remaining = if timeout_s > 0.0 {
            (timeout_s - start.elapsed().as_secs_f64()).max(0.0)
        } else {
            poll_interval_s
        };
        let delay = poll_interval_s.min(remaining.max(0.0));
        if delay <= 0.0 {
            break;
        }
        sleep(Duration::from_secs_f64(delay)).await;
        last_snapshot = a2a_observe_snapshot(context, args).await?;
    }
    let elapsed = start.elapsed().as_secs_f64();
    Ok(json!({
        "tasks": last_snapshot.tasks.iter().map(|item| item.to_value()).collect::<Vec<_>>(),
        "pending": last_snapshot.pending.iter().map(|item| item.to_value()).collect::<Vec<_>>(),
        "done": last_snapshot.pending.is_empty(),
        "total": last_snapshot.tasks.len(),
        "elapsed_s": (elapsed * 1000.0).round() / 1000.0,
        "timeout": !last_snapshot.pending.is_empty() && timeout_s > 0.0 && elapsed >= timeout_s,
    }))
}

async fn a2a_observe_snapshot(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<A2aObserveSnapshot> {
    let explicit_task_ids = parse_string_list(
        args.get("task_ids")
            .or_else(|| args.get("task_id"))
            .or_else(|| args.get("taskId")),
    );
    let explicit_endpoint = args
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .unwrap_or_default();
    let explicit_service = args
        .get("service_name")
        .or_else(|| args.get("service"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(true);
    let timeout_s = args
        .get("timeout_s")
        .and_then(Value::as_u64)
        .unwrap_or(context.config.a2a.timeout_s);

    let mut tasks = Vec::new();
    let mut seen = HashSet::new();

    for task in context.a2a_store.list_by_user(context.user_id) {
        if !explicit_task_ids.is_empty() && !explicit_task_ids.contains(&task.id) {
            continue;
        }
        if !explicit_service.is_empty()
            && task
                .service_name
                .as_deref()
                .map(|name| name != explicit_service)
                .unwrap_or(true)
        {
            continue;
        }
        if !explicit_endpoint.is_empty()
            && task
                .endpoint
                .as_deref()
                .map(|value| normalize_a2a_endpoint(value) != explicit_endpoint)
                .unwrap_or(true)
        {
            continue;
        }
        let snapshot = build_snapshot_from_task(&task);
        seen.insert(task.id.clone());
        tasks.push(snapshot);
    }

    if let Some(entries) = args.get("tasks").and_then(Value::as_array) {
        for entry in entries {
            if let Some(snapshot) =
                build_snapshot_from_value(entry, &explicit_endpoint, &explicit_service)
            {
                if seen.insert(snapshot.task_id.clone()) {
                    tasks.push(snapshot);
                }
            }
        }
    }

    for task_id in explicit_task_ids {
        if seen.contains(&task_id) {
            continue;
        }
        tasks.push(A2aTaskSnapshot {
            task_id,
            context_id: None,
            status: None,
            endpoint: if explicit_endpoint.is_empty() {
                None
            } else {
                Some(explicit_endpoint.clone())
            },
            service_name: if explicit_service.is_empty() {
                None
            } else {
                Some(explicit_service.clone())
            },
            answer: None,
            updated_time: None,
            refresh_error: None,
        });
    }

    if refresh {
        for item in tasks.iter_mut() {
            if let Err(err) = refresh_a2a_task(context, item, timeout_s).await {
                item.refresh_error = Some(err.to_string());
            }
        }
    }

    let pending = tasks
        .iter()
        .cloned()
        .filter(|item| !item.is_done())
        .collect::<Vec<_>>();
    Ok(A2aObserveSnapshot { tasks, pending })
}

fn build_snapshot_from_task(task: &A2aTask) -> A2aTaskSnapshot {
    A2aTaskSnapshot {
        task_id: task.id.clone(),
        context_id: task.context_id.clone(),
        status: Some(task.status.clone()),
        endpoint: task.endpoint.clone(),
        service_name: task.service_name.clone(),
        answer: if task.answer.is_empty() {
            None
        } else {
            Some(task.answer.clone())
        },
        updated_time: Some(task.updated_time.to_rfc3339()),
        refresh_error: None,
    }
}

fn build_snapshot_from_value(
    value: &Value,
    default_endpoint: &str,
    default_service: &str,
) -> Option<A2aTaskSnapshot> {
    let obj = value.as_object()?;
    let task_id = obj
        .get("task_id")
        .or_else(|| obj.get("taskId"))
        .or_else(|| obj.get("id"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if task_id.is_empty() {
        return None;
    }
    let endpoint = obj
        .get("endpoint")
        .and_then(Value::as_str)
        .map(normalize_a2a_endpoint)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_endpoint.is_empty() {
                None
            } else {
                Some(default_endpoint.to_string())
            }
        });
    let service_name = obj
        .get("service_name")
        .or_else(|| obj.get("service"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            if default_service.is_empty() {
                None
            } else {
                Some(default_service.to_string())
            }
        });
    Some(A2aTaskSnapshot {
        task_id,
        context_id: obj
            .get("context_id")
            .or_else(|| obj.get("contextId"))
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        status: obj
            .get("status")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        endpoint,
        service_name,
        answer: obj
            .get("answer")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        updated_time: obj
            .get("updated_time")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        refresh_error: None,
    })
}

fn parse_string_list(value: Option<&Value>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(|text| text.trim().to_string()))
            .filter(|text| !text.is_empty())
            .collect(),
        Value::String(text) => text
            .split(',')
            .map(|part| part.trim().to_string())
            .filter(|part| !part.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_text_arg(args: &Value, keys: &[&str]) -> Option<String> {
    let obj = args.as_object()?;
    for key in keys {
        if let Some(Value::String(text)) = obj.get(*key) {
            let value = text.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

async fn refresh_a2a_task(
    context: &ToolContext<'_>,
    snapshot: &mut A2aTaskSnapshot,
    timeout_s: u64,
) -> Result<()> {
    if snapshot.task_id.trim().is_empty() {
        return Ok(());
    }
    let endpoint = match snapshot.endpoint.clone() {
        Some(endpoint) if !endpoint.is_empty() => endpoint,
        _ => {
            let service_name = snapshot.service_name.as_deref().unwrap_or("");
            if let Some(service) = resolve_a2a_service(context.config, service_name, "") {
                snapshot.endpoint = Some(service.endpoint.clone());
                snapshot.service_name = Some(service.name.clone());
                service.endpoint.clone()
            } else {
                return Ok(());
            }
        }
    };
    let service_name = snapshot.service_name.clone().unwrap_or_default();
    let service = resolve_a2a_service(context.config, &service_name, &endpoint);
    let headers = match service {
        Some(service) => build_a2a_headers(context.config, service)?,
        None => build_a2a_headers_for_endpoint(context.config, &endpoint)?,
    };
    let payload = json!({
        "jsonrpc": "2.0",
        "id": Uuid::new_v4().to_string(),
        "method": "GetTask",
        "params": { "name": format!("tasks/{}", snapshot.task_id) }
    });
    let response = send_a2a_request(context.http, &endpoint, headers, &payload, timeout_s).await?;
    if let Some(info) = parse_a2a_task_info(&response) {
        snapshot.context_id = info.context_id.clone();
        snapshot.status = info.status.clone();
        snapshot.answer = info.answer.clone();
        snapshot.updated_time = Some(Utc::now().to_rfc3339());
        snapshot.refresh_error = None;
        context.a2a_store.update(&info.id, |task| {
            task.context_id = info.context_id.clone();
            task.status = info.status.clone().unwrap_or_default();
            task.answer = info.answer.clone().unwrap_or_default();
            task.updated_time = Utc::now();
        });
    }
    Ok(())
}

fn resolve_a2a_service<'a>(
    config: &'a Config,
    service_name: &str,
    endpoint: &str,
) -> Option<&'a A2aServiceConfig> {
    let normalized_endpoint = normalize_a2a_endpoint(endpoint);
    config.a2a.services.iter().find(|service| {
        if !service_name.is_empty() && service.name == service_name {
            return true;
        }
        if !normalized_endpoint.is_empty() {
            return normalize_a2a_endpoint(&service.endpoint) == normalized_endpoint;
        }
        false
    })
}

fn normalize_a2a_endpoint(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn build_a2a_headers(config: &Config, service: &A2aServiceConfig) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    for (key, value) in &service.headers {
        let name = HeaderName::from_bytes(key.as_bytes())?;
        let value = HeaderValue::from_str(value)?;
        header_map.insert(name, value);
    }
    if let Some(auth) = &service.auth {
        let auth_json = yaml_to_json(auth);
        if let Value::Object(map) = auth_json {
            if let Some(Value::String(token)) = map.get("bearer_token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("token") {
                let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
                header_map.insert(HeaderName::from_static("authorization"), header);
            }
            if let Some(Value::String(token)) = map.get("api_key") {
                let header = HeaderValue::from_str(token)?;
                header_map.insert(HeaderName::from_static("x-api-key"), header);
            }
        }
    }
    let has_auth = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("authorization"));
    let has_api_key = header_map
        .keys()
        .any(|key| key.as_str().eq_ignore_ascii_case("x-api-key"));
    if should_attach_a2a_api_key(config, service) && !has_auth && !has_api_key {
        if let Some(api_key) = config.api_key() {
            header_map.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(&api_key)?,
            );
        }
    }
    Ok(header_map)
}

fn build_a2a_headers_for_endpoint(config: &Config, endpoint: &str) -> Result<HeaderMap> {
    let mut header_map = HeaderMap::new();
    if let Some(api_key) = config.api_key() {
        if let Ok(parsed) = url::Url::parse(endpoint) {
            let path = parsed.path().trim_end_matches('/');
            if path.ends_with("/a2a") {
                header_map.insert(
                    HeaderName::from_static("x-api-key"),
                    HeaderValue::from_str(&api_key)?,
                );
            }
        }
    }
    Ok(header_map)
}

fn should_attach_a2a_api_key(config: &Config, service: &A2aServiceConfig) -> bool {
    if config.api_key().is_none() {
        return false;
    }
    if service.name.eq_ignore_ascii_case("wunder") {
        return true;
    }
    if let Ok(parsed) = url::Url::parse(&service.endpoint) {
        let path = parsed.path().trim_end_matches('/');
        return path.ends_with("/a2a");
    }
    false
}

async fn send_a2a_request(
    client: &reqwest::Client,
    endpoint: &str,
    headers: HeaderMap,
    payload: &Value,
    timeout_s: u64,
) -> Result<Value> {
    let mut request = client.post(endpoint).headers(headers).json(payload);
    if timeout_s > 0 {
        request = request.timeout(Duration::from_secs(timeout_s));
    }
    let response = request.send().await?;
    let status = response.status();
    let text = response.text().await?;
    let body: Value =
        serde_json::from_str(&text).map_err(|_| anyhow!("A2A 响应非 JSON: {text}"))?;
    if !status.is_success() {
        return Err(anyhow!("A2A 请求失败: {status}"));
    }
    if body.get("error").is_some() {
        return Err(anyhow!("A2A 返回错误: {body}"));
    }
    Ok(body)
}

fn parse_a2a_task_info(value: &Value) -> Option<A2aTaskInfo> {
    let result = value.get("result").unwrap_or(value);
    let task = result.get("task").unwrap_or(result);
    let task_obj = task.as_object()?;
    let id = task_obj
        .get("id")
        .or_else(|| task_obj.get("task_id"))
        .or_else(|| task_obj.get("taskId"))
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if id.is_empty() {
        return None;
    }
    let context_id = task_obj
        .get("contextId")
        .or_else(|| task_obj.get("context_id"))
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    let status = match task_obj.get("status") {
        Some(Value::Object(status_obj)) => status_obj
            .get("state")
            .and_then(Value::as_str)
            .map(|value| value.to_string()),
        Some(Value::String(text)) => Some(text.to_string()),
        _ => None,
    };
    let answer = extract_a2a_answer(task);
    Some(A2aTaskInfo {
        id,
        context_id,
        status,
        answer: if answer.is_empty() {
            None
        } else {
            Some(answer)
        },
    })
}

fn extract_a2a_answer(task: &Value) -> String {
    if let Some(answer) = task.get("answer").and_then(Value::as_str) {
        return answer.to_string();
    }
    let mut parts = Vec::new();
    if let Some(artifacts) = task.get("artifacts").and_then(Value::as_array) {
        for artifact in artifacts {
            if let Some(items) = artifact.get("parts").and_then(Value::as_array) {
                for part in items {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        parts.push(text.to_string());
                    }
                }
            }
        }
    }
    parts.join("\n")
}

fn is_a2a_task_finished(status: &str) -> bool {
    matches!(
        status.to_lowercase().as_str(),
        "completed" | "failed" | "cancelled" | "rejected"
    )
}
