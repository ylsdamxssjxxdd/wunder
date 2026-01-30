// 内置工具定义与执行入口，保持工具名称与协议一致。
use crate::a2a_store::{A2aStore, A2aTask};
use crate::command_utils;
use crate::config::{
    is_debug_log_level, normalize_knowledge_base_type, A2aServiceConfig, Config,
    KnowledgeBaseConfig, KnowledgeBaseType,
};
use crate::i18n;
use crate::knowledge;
use crate::llm::embed_texts;
use crate::lsp::{LspDiagnostic, LspManager};
use crate::mcp;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::sandbox;
use crate::schemas::ToolSpec;
use crate::skills::{execute_skill, SkillRegistry, SkillSpec};
use crate::user_tools::{
    UserToolAlias, UserToolBindings, UserToolKind, UserToolManager, UserToolStore,
};
use crate::vector_knowledge;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::time::sleep;
use tracing::warn;
use url::Url;
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_READ_BYTES: usize = 1024 * 1024;
const MAX_READ_LINES: usize = 1000;
const MAX_READ_FILES: usize = 5;
const MAX_RANGE_SPAN: usize = 1000;
const DEFAULT_LIST_DEPTH: usize = 2;
const MAX_LIST_ITEMS: usize = 200;
const MAX_SEARCH_MATCHES: usize = 200;
const MAX_LSP_DIAGNOSTICS: usize = 20;

#[derive(Clone)]
pub struct ToolEventEmitter {
    callback: Arc<dyn Fn(&str, Value) + Send + Sync>,
    stream: bool,
}

impl ToolEventEmitter {
    pub fn new<F>(callback: F, stream: bool) -> Self
    where
        F: Fn(&str, Value) + Send + Sync + 'static,
    {
        Self {
            callback: Arc::new(callback),
            stream,
        }
    }

    pub fn emit(&self, event_type: &str, data: Value) {
        (self.callback)(event_type, data);
    }

    pub fn stream_enabled(&self) -> bool {
        self.stream
    }
}

pub struct ToolContext<'a> {
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub workspace_id: &'a str,
    pub agent_id: Option<&'a str>,
    pub workspace: Arc<WorkspaceManager>,
    pub lsp_manager: Arc<LspManager>,
    pub config: &'a Config,
    pub a2a_store: &'a A2aStore,
    pub skills: &'a SkillRegistry,
    pub user_tool_manager: Option<&'a UserToolManager>,
    pub user_tool_bindings: Option<&'a UserToolBindings>,
    pub user_tool_store: Option<&'a UserToolStore>,
    pub allow_roots: Option<Arc<Vec<PathBuf>>>,
    pub read_roots: Option<Arc<Vec<PathBuf>>>,
    pub event_emitter: Option<ToolEventEmitter>,
    pub http: &'a reqwest::Client,
}

#[derive(Clone)]
pub struct ToolRoots {
    pub allow_roots: Arc<Vec<PathBuf>>,
    pub read_roots: Arc<Vec<PathBuf>>,
}

pub fn build_tool_roots(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> ToolRoots {
    let allow_roots = build_allow_roots(config);
    let mut read_roots = allow_roots.clone();
    read_roots.extend(build_skill_roots(skills, user_tool_bindings));
    let read_roots = dedupe_roots(read_roots);
    ToolRoots {
        allow_roots: Arc::new(allow_roots),
        read_roots: Arc::new(read_roots),
    }
}

fn builtin_tool_specs_with_language(language: &str) -> Vec<ToolSpec> {
    let t = |key: &str| i18n::t_in_language(key, language);
    vec![
        ToolSpec {
            name: "最终回复".to_string(),
            description: t("tool.spec.final.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": t("tool.spec.final.args.content")}
                },
                "required": ["content"]
            }),
        },
        ToolSpec {
            name: "a2ui".to_string(),
            description: t("tool.spec.a2ui.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "uid": {"type": "string", "description": t("tool.spec.a2ui.args.uid")},
                    "a2ui": {"type": "array", "description": t("tool.spec.a2ui.args.messages"), "items": {"type": "object"}},
                    "content": {"type": "string", "description": t("tool.spec.a2ui.args.content")}
                },
                "required": ["uid", "a2ui"]
            }),
        },
        ToolSpec {
            name: "计划面板".to_string(),
            description: t("tool.spec.plan.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "explanation": {"type": "string", "description": t("tool.spec.plan.args.explanation")},
                    "plan": {
                        "type": "array",
                        "description": t("tool.spec.plan.args.plan"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {"type": "string", "description": t("tool.spec.plan.args.plan.step")},
                                "status": {
                                    "type": "string",
                                    "description": t("tool.spec.plan.args.plan.status"),
                                    "enum": ["pending", "in_progress", "completed"]
                                }
                            },
                            "required": ["step", "status"]
                        }
                    }
                },
                "required": ["plan"]
            }),
        },
        ToolSpec {
            name: "问询面板".to_string(),
            description: t("tool.spec.question_panel.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "question": {"type": "string", "description": t("tool.spec.question_panel.args.question")},
                    "routes": {
                        "type": "array",
                        "description": t("tool.spec.question_panel.args.routes"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string", "description": t("tool.spec.question_panel.args.routes.label")},
                                "description": {"type": "string", "description": t("tool.spec.question_panel.args.routes.description")},
                                "recommended": {"type": "boolean", "description": t("tool.spec.question_panel.args.routes.recommended")}
                            },
                            "required": ["label"]
                        }
                    },
                    "multiple": {"type": "boolean", "description": t("tool.spec.question_panel.args.multiple")}
                },
                "required": ["routes"]
            }),
        },
        ToolSpec {
            name: "a2a观察".to_string(),
            description: t("tool.spec.a2a_observe.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_ids": {"type": "array", "items": {"type": "string"}, "description": t("tool.spec.a2a_observe.args.task_ids")},
                    "tasks": {"type": "array", "items": {"type": "object"}, "description": t("tool.spec.a2a_observe.args.tasks")},
                    "endpoint": {"type": "string", "description": t("tool.spec.a2a_observe.args.endpoint")},
                    "service_name": {"type": "string", "description": t("tool.spec.a2a_observe.args.service_name")},
                    "refresh": {"type": "boolean", "description": t("tool.spec.a2a_observe.args.refresh")},
                    "timeout_s": {"type": "number", "description": t("tool.spec.a2a_observe.args.timeout")}
                }
            }),
        },
        ToolSpec {
            name: "a2a等待".to_string(),
            description: t("tool.spec.a2a_wait.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wait_s": {"type": "number", "description": t("tool.spec.a2a_wait.args.wait_s")},
                    "poll_interval_s": {"type": "number", "description": t("tool.spec.a2a_wait.args.poll_interval")},
                    "task_ids": {"type": "array", "items": {"type": "string"}},
                    "tasks": {"type": "array", "items": {"type": "object"}},
                    "endpoint": {"type": "string", "description": t("tool.spec.a2a_wait.args.endpoint")},
                    "service_name": {"type": "string", "description": t("tool.spec.a2a_wait.args.service_name")},
                    "refresh": {"type": "boolean", "description": t("tool.spec.a2a_wait.args.refresh")},
                    "timeout_s": {"type": "number", "description": t("tool.spec.a2a_wait.args.timeout")}
                }
            }),
        },
        ToolSpec {
            name: "执行命令".to_string(),
            description: t("tool.spec.exec.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": {"type": "string", "description": t("tool.spec.exec.args.content")},
                    "workdir": {"type": "string", "description": t("tool.spec.exec.args.workdir")},
                    "timeout_s": {"type": "integer", "description": t("tool.spec.exec.args.timeout")}
                },
                "required": ["content"]
            }),
        },
        ToolSpec {
            name: "ptc".to_string(),
            description: t("tool.spec.ptc.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "filename": {"type": "string", "description": t("tool.spec.ptc.args.filename")},
                    "workdir": {"type": "string", "description": t("tool.spec.ptc.args.workdir")},
                    "content": {"type": "string", "description": t("tool.spec.ptc.args.content")}
                },
                "required": ["filename", "workdir", "content"]
            }),
        },
        ToolSpec {
            name: "列出文件".to_string(),
            description: t("tool.spec.list.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.list.args.path")},
                    "max_depth": {"type": "integer", "minimum": 0}
                }
            }),
        },
        ToolSpec {
            name: "搜索内容".to_string(),
            description: t("tool.spec.search.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": t("tool.spec.search.args.query")},
                    "path": {"type": "string", "description": t("tool.spec.search.args.path")},
                    "file_pattern": {"type": "string", "description": t("tool.spec.search.args.file_pattern")},
                    "max_depth": {"type": "integer", "minimum": 0, "description": t("tool.spec.search.args.max_depth")},
                    "max_files": {"type": "integer", "minimum": 0, "description": t("tool.spec.search.args.max_files")}
                },
                "required": ["query"]
            }),
        },
        ToolSpec {
            name: "读取文件".to_string(),
            description: t("tool.spec.read.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "path": {"type": "string", "description": t("tool.spec.read.args.files.path")},
                                "start_line": {"type": "integer", "description": t("tool.spec.read.args.files.start_line")},
                                "end_line": {"type": "integer", "description": t("tool.spec.read.args.files.end_line")},
                                "line_ranges": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}, "minItems": 2}}
                            },
                            "required": ["path"]
                        }
                    }
                },
                "required": ["files"]
            }),
        },
        ToolSpec {
            name: "技能调用".to_string(),
            description: t("tool.spec.skill_call.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": t("tool.spec.skill_call.args.name")}
                },
                "required": ["name"]
            }),
        },
        ToolSpec {
            name: "写入文件".to_string(),
            description: t("tool.spec.write.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.write.args.path")},
                    "content": {"type": "string", "description": t("tool.spec.write.args.content")}
                },
                "required": ["path", "content"]
            }),
        },
        ToolSpec {
            name: "替换文本".to_string(),
            description: t("tool.spec.replace.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.replace.args.path")},
                    "old_string": {"type": "string", "description": t("tool.spec.replace.args.old_string")},
                    "new_string": {"type": "string", "description": t("tool.spec.replace.args.new_string")},
                    "expected_replacements": {"type": "integer", "description": t("tool.spec.replace.args.expected_replacements")}
                },
                "required": ["path", "old_string", "new_string"]
            }),
        },
        ToolSpec {
            name: "编辑文件".to_string(),
            description: t("tool.spec.edit.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.edit.args.path")},
                    "edits": {"type": "array", "description": t("tool.spec.edit.args.edits")},
                    "ensure_newline_at_eof": {"type": "boolean", "description": t("tool.spec.edit.args.ensure_newline")}
                },
                "required": ["path", "edits"]
            }),
        },
        ToolSpec {
            name: "LSP查询".to_string(),
            description: t("tool.spec.lsp.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "description": t("tool.spec.lsp.args.operation"),
                        "enum": [
                            "definition",
                            "references",
                            "hover",
                            "documentSymbol",
                            "workspaceSymbol",
                            "implementation",
                            "callHierarchy"
                        ]
                    },
                    "path": {"type": "string", "description": t("tool.spec.lsp.args.path")},
                    "line": {"type": "integer", "description": t("tool.spec.lsp.args.line"), "minimum": 1},
                    "character": {"type": "integer", "description": t("tool.spec.lsp.args.character"), "minimum": 1},
                    "query": {"type": "string", "description": t("tool.spec.lsp.args.query")},
                    "call_hierarchy_direction": {
                        "type": "string",
                        "description": t("tool.spec.lsp.args.call_hierarchy_direction"),
                        "enum": ["incoming", "outgoing"]
                    }
                },
                "required": ["operation", "path"]
            }),
        },
    ]
}

pub fn builtin_tool_specs() -> Vec<ToolSpec> {
    let language = i18n::get_language();
    builtin_tool_specs_with_language(&language)
}

pub fn builtin_aliases() -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("final_response".to_string(), "最终回复".to_string());
    map.insert("update_plan".to_string(), "计划面板".to_string());
    map.insert("question_panel".to_string(), "问询面板".to_string());
    map.insert("ask_panel".to_string(), "问询面板".to_string());
    map.insert("a2a_observe".to_string(), "a2a观察".to_string());
    map.insert("a2a_wait".to_string(), "a2a等待".to_string());
    map.insert("execute_command".to_string(), "执行命令".to_string());
    map.insert("programmatic_tool_call".to_string(), "ptc".to_string());
    map.insert("list_files".to_string(), "列出文件".to_string());
    map.insert("search_content".to_string(), "搜索内容".to_string());
    map.insert("read_file".to_string(), "读取文件".to_string());
    map.insert("skill_call".to_string(), "技能调用".to_string());
    map.insert("skill_get".to_string(), "技能调用".to_string());
    map.insert("write_file".to_string(), "写入文件".to_string());
    map.insert("replace_text".to_string(), "替换文本".to_string());
    map.insert("edit_file".to_string(), "编辑文件".to_string());
    map.insert("lsp".to_string(), "LSP查询".to_string());
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
        context.workspace.mark_tree_dirty(context.workspace_id);
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
    let language = i18n::get_language();
    collect_prompt_tool_specs_with_language(
        config,
        skills,
        allowed_names,
        user_tool_bindings,
        &language,
    )
}

pub fn collect_prompt_tool_specs_with_language(
    config: &Config,
    skills: &SkillRegistry,
    allowed_names: &HashSet<String>,
    user_tool_bindings: Option<&UserToolBindings>,
    language: &str,
) -> Vec<ToolSpec> {
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    let language = language.trim();
    let language_lower = language.to_lowercase();
    let alias_map = builtin_aliases();
    let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
    for (alias, canonical) in alias_map {
        canonical_aliases.entry(canonical).or_default().push(alias);
    }
    for spec in builtin_tool_specs_with_language(language) {
        let aliases: &[String] = canonical_aliases
            .get(&spec.name)
            .map(|value| value.as_slice())
            .unwrap_or(&[]);
        let enabled = allowed_names.contains(&spec.name)
            || aliases.iter().any(|alias| allowed_names.contains(alias));
        if !enabled {
            continue;
        }
        let preferred_alias = if language_lower.starts_with("en") {
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
            input_schema: a2a_service_schema_with_language(language),
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
            i18n::t_with_params_in_language(
                "knowledge.tool.description",
                &HashMap::from([("name".to_string(), name.to_string())]),
                language,
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
                    "query": {"type": "string", "description": i18n::t_in_language("knowledge.tool.query.description", language)},
                    "keywords": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": i18n::t_in_language("knowledge.tool.keywords.description", language)},
                    "limit": {"type": "integer", "minimum": 1, "description": i18n::t_in_language("knowledge.tool.limit.description", language)}
                },
                "anyOf": [
                    {"required": ["query"]},
                    {"required": ["keywords"]}
                ]
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
    let language = i18n::get_language();
    a2a_service_schema_with_language(&language)
}

pub fn a2a_service_schema_with_language(language: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "content": {"type": "string", "description": i18n::t_in_language("tool.spec.a2a_service.args.content", language)},
            "session_id": {"type": "string", "description": i18n::t_in_language("tool.spec.a2a_service.args.session_id", language)}
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
        "读取文件" => read_files(context, args).await,
        "技能调用" => execute_skill_call(context, args).await,
        "写入文件" => write_file(context, args).await,
        "替换文本" => replace_text(context, args).await,
        "编辑文件" => edit_file(context, args).await,
        "LSP查询" => lsp_query(context, args).await,
        "a2a观察" => a2a_observe(context, args).await,
        "a2a等待" => a2a_wait(context, args).await,
        "a2ui" => Ok(
            json!({"uid": args.get("uid"), "a2ui": args.get("a2ui"), "content": args.get("content")}),
        ),
        "计划面板" => execute_plan_tool(context, args).await,
        "问询面板" => execute_question_panel_tool(context, args).await,
        _ => Err(anyhow!("未知内置工具: {canonical}")),
    }
}

#[derive(Debug, Deserialize)]
struct PlanUpdateArgs {
    #[serde(default)]
    explanation: Option<String>,
    plan: Vec<PlanItemArgs>,
}

#[derive(Debug, Deserialize)]
struct PlanItemArgs {
    step: String,
    #[serde(default)]
    status: Option<String>,
}

fn normalize_plan_status(value: Option<&str>) -> String {
    let raw = value.unwrap_or("").trim().to_lowercase();
    if raw.is_empty() {
        return "pending".to_string();
    }
    let normalized = raw.replace('-', "_").replace(' ', "_");
    match normalized.as_str() {
        "pending" => "pending".to_string(),
        "in_progress" | "inprogress" => "in_progress".to_string(),
        "completed" | "complete" | "done" => "completed".to_string(),
        _ => "pending".to_string(),
    }
}

async fn execute_plan_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: PlanUpdateArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    if payload.plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let mut seen_in_progress = false;
    let mut normalized_plan = Vec::new();
    for item in payload.plan {
        let step = item.step.trim().to_string();
        if step.is_empty() {
            continue;
        }
        let mut status = normalize_plan_status(item.status.as_deref());
        if status == "in_progress" {
            if seen_in_progress {
                status = "pending".to_string();
            } else {
                seen_in_progress = true;
            }
        }
        normalized_plan.push(json!({
            "step": step,
            "status": status
        }));
    }
    if normalized_plan.is_empty() {
        return Err(anyhow!(i18n::t("tool.plan.plan_required")));
    }
    let explanation = payload.explanation.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "plan_update",
            json!({
                "explanation": explanation,
                "plan": normalized_plan
            }),
        );
    }
    Ok(json!({ "status": "ok" }))
}

#[derive(Debug)]
struct QuestionPanelRoute {
    label: String,
    description: Option<String>,
    recommended: bool,
}

#[derive(Debug)]
struct QuestionPanelPayload {
    question: String,
    routes: Vec<QuestionPanelRoute>,
    multiple: bool,
}

fn normalize_question_panel_payload(args: &Value) -> Result<QuestionPanelPayload> {
    let Some(obj) = args.as_object() else {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    };
    let question = obj
        .get("question")
        .or_else(|| obj.get("prompt"))
        .or_else(|| obj.get("title"))
        .or_else(|| obj.get("header"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let question = if question.is_empty() {
        i18n::t("tool.question_panel.default_question")
    } else {
        question
    };
    let multiple = obj
        .get("multiple")
        .or_else(|| obj.get("allow_multiple"))
        .or_else(|| obj.get("multi"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let routes = obj
        .get("routes")
        .or_else(|| obj.get("options"))
        .or_else(|| obj.get("choices"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut normalized = Vec::new();
    for item in routes {
        let (label, description, recommended) = match item {
            Value::String(value) => (value, None, false),
            Value::Object(map) => {
                let label = map
                    .get("label")
                    .or_else(|| map.get("title"))
                    .or_else(|| map.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let description = map
                    .get("description")
                    .or_else(|| map.get("detail"))
                    .or_else(|| map.get("desc"))
                    .or_else(|| map.get("summary"))
                    .and_then(Value::as_str)
                    .map(|value| value.to_string());
                let recommended = map
                    .get("recommended")
                    .or_else(|| map.get("preferred"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                (label, description, recommended)
            }
            _ => (String::new(), None, false),
        };
        let label = label.trim().to_string();
        if label.is_empty() {
            continue;
        }
        let description = description.and_then(|value| {
            let trimmed = value.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let recommended = recommended || label.contains("推荐");
        normalized.push(QuestionPanelRoute {
            label,
            description,
            recommended,
        });
    }
    if normalized.is_empty() {
        return Err(anyhow!(i18n::t("tool.question_panel.routes_required")));
    }
    Ok(QuestionPanelPayload {
        question,
        routes: normalized,
        multiple,
    })
}

async fn execute_question_panel_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload = normalize_question_panel_payload(args)?;
    let question = payload.question.clone();
    let routes = payload
        .routes
        .iter()
        .map(|route| {
            json!({
                "label": route.label,
                "description": route.description,
                "recommended": route.recommended
            })
        })
        .collect::<Vec<_>>();
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit(
            "question_panel",
            json!({
                "question": question.clone(),
                "routes": routes.clone(),
                "multiple": payload.multiple,
                "keep_open": true
            }),
        );
    }
    Ok(json!({
        "question": question,
        "routes": routes,
        "multiple": payload.multiple
    }))
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
    context.workspace.mark_tree_dirty(context.workspace_id);
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
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    let store = context
        .user_tool_store
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let payload = store.load_user_tools(&alias.owner_id);
    let base_info = payload
        .knowledge_bases
        .iter()
        .find(|base| base.name == alias.target)
        .cloned()
        .ok_or_else(|| anyhow!(i18n::t("error.knowledge_base_not_found")))?;
    let base_type = normalize_knowledge_base_type(base_info.base_type.as_deref());
    let root = store
        .resolve_knowledge_base_root_with_type(&alias.owner_id, &base_info.name, base_type, false)
        .map_err(|err| anyhow!(err.to_string()))?;
    let base = KnowledgeBaseConfig {
        name: base_info.name.clone(),
        description: base_info.description.clone(),
        root: root.to_string_lossy().to_string(),
        enabled: base_info.enabled,
        shared: Some(base_info.shared),
        base_type: base_info.base_type.clone(),
        embedding_model: base_info.embedding_model.clone(),
        chunk_size: base_info.chunk_size,
        chunk_overlap: base_info.chunk_overlap,
        top_k: base_info.top_k,
        score_threshold: base_info.score_threshold,
    };
    if base_type == KnowledgeBaseType::Vector {
        return execute_vector_knowledge(context, &base, Some(&alias.owner_id), args).await;
    }
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = if let Some(emitter) = context.event_emitter.as_ref() {
        let include_payload = is_debug_log_level(&context.config.observability.log_level);
        let log_request = |mut payload: Value| {
            if !include_payload {
                if let Value::Object(ref mut map) = payload {
                    map.remove("payload");
                }
            }
            emitter.emit("knowledge_request", payload);
        };
        knowledge::query_knowledge_documents(
            &query,
            &base,
            llm_config.as_ref(),
            extract_limit(args),
            Some(&log_request),
        )
        .await
    } else {
        knowledge::query_knowledge_documents(
            &query,
            &base,
            llm_config.as_ref(),
            extract_limit(args),
            None,
        )
        .await
    };
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
    let Some(query) = resolve_query_text(args) else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    if base.is_vector() {
        return execute_vector_knowledge(context, base, None, args).await;
    }
    let _ =
        knowledge::resolve_knowledge_root(base, false).map_err(|err| anyhow!(err.to_string()))?;
    let llm_config = knowledge::resolve_llm_config(context.config, None);
    let docs = if let Some(emitter) = context.event_emitter.as_ref() {
        let include_payload = is_debug_log_level(&context.config.observability.log_level);
        let log_request = |mut payload: Value| {
            if !include_payload {
                if let Value::Object(ref mut map) = payload {
                    map.remove("payload");
                }
            }
            emitter.emit("knowledge_request", payload);
        };
        knowledge::query_knowledge_documents(
            &query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            Some(&log_request),
        )
        .await
    } else {
        knowledge::query_knowledge_documents(
            &query,
            base,
            llm_config.as_ref(),
            extract_limit(args),
            None,
        )
        .await
    };
    let documents = docs
        .into_iter()
        .map(|doc| doc.to_value())
        .collect::<Vec<_>>();
    Ok(json!({
        "knowledge_base": base.name,
        "documents": documents
    }))
}

async fn execute_vector_knowledge(
    context: &ToolContext<'_>,
    base: &KnowledgeBaseConfig,
    owner_id: Option<&str>,
    args: &Value,
) -> Result<Value> {
    let keywords = extract_keywords(args);
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let queries = if !keywords.is_empty() {
        keywords
    } else if !query.is_empty() {
        vec![query.clone()]
    } else {
        return Err(anyhow!(i18n::t("error.knowledge_query_required")));
    };
    vector_knowledge::ensure_vector_base_config(base)?;
    let embedding_name = base.embedding_model.as_deref().unwrap_or("").trim();
    let embed_config = vector_knowledge::resolve_embedding_model(context.config, embedding_name)?;
    let timeout_s = embed_config.timeout_s.unwrap_or(120);
    let vectors = embed_texts(&embed_config, &queries, timeout_s).await?;
    if vectors.len() != queries.len() {
        return Err(anyhow!("embedding response size mismatch"));
    }
    let client = vector_knowledge::resolve_weaviate_client(context.config)?;
    let owner_key = vector_knowledge::resolve_owner_key(owner_id);
    let top_k = extract_limit(args).unwrap_or_else(|| vector_knowledge::resolve_top_k(base));
    let base_name = base.name.clone();
    let embedding_name = embedding_name.to_string();
    let query_results =
        futures::future::join_all(vectors.into_iter().enumerate().map(|(index, vector)| {
            let client = client.clone();
            let owner_key = owner_key.clone();
            let base_name = base_name.clone();
            let embedding_name = embedding_name.clone();
            let keyword = queries.get(index).cloned().unwrap_or_default();
            async move {
                let mut hits = client
                    .query_chunks(&owner_key, &base_name, &embedding_name, &vector, top_k)
                    .await?;
                if let Some(threshold) = base.score_threshold {
                    hits.retain(|hit| hit.score.unwrap_or(0.0) >= f64::from(threshold));
                }
                if hits.len() > top_k {
                    hits.truncate(top_k);
                }
                Ok::<_, anyhow::Error>((index, keyword, hits))
            }
        }))
        .await;
    let mut aggregated = Vec::new();
    for result in query_results {
        aggregated.push(result?);
    }
    aggregated.sort_by_key(|(index, _, _)| *index);
    if let Some(emitter) = context.event_emitter.as_ref() {
        let mut payload = json!({
            "knowledge_base": base.name,
            "vector": true,
            "embedding_model": embedding_name.clone(),
            "owner_id": owner_key,
            "limit": top_k,
            "score_threshold": base.score_threshold
        });
        if queries.len() == 1 {
            payload["query"] = json!(queries[0].clone());
        } else {
            payload["keywords"] = json!(queries.clone());
        }
        emitter.emit("knowledge_request", payload);
    }
    let mut grouped_results = Vec::new();
    let mut flat_documents = Vec::new();
    for (_, keyword, hits) in aggregated {
        let documents = hits
            .into_iter()
            .map(|hit| {
                let mut doc = json!({
                    "doc_id": hit.doc_id,
                    "document": hit.doc_name,
                    "name": hit.doc_name,
                    "chunk_index": hit.chunk_index,
                    "start": hit.start,
                    "end": hit.end,
                    "content": hit.content,
                    "embedding_model": hit.embedding_model,
                    "score": hit.score
                });
                if queries.len() > 1 {
                    doc["keyword"] = json!(keyword);
                }
                doc
            })
            .collect::<Vec<_>>();
        if queries.len() > 1 {
            flat_documents.extend(documents.clone());
        }
        grouped_results.push(json!({
            "keyword": keyword,
            "documents": documents
        }));
    }
    let mut response = json!({
        "knowledge_base": base.name,
        "vector": true,
        "embedding_model": embedding_name.clone(),
        "queries": grouped_results
    });
    if queries.len() == 1 {
        if let Some(entry) = response
            .get("queries")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|value| value.get("documents"))
        {
            response["documents"] = entry.clone();
        }
    } else {
        response["documents"] = json!(flat_documents);
    }
    Ok(response)
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

fn extract_keywords(args: &Value) -> Vec<String> {
    let Some(Value::Array(items)) = args.get("keywords") else {
        return Vec::new();
    };
    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for item in items {
        let Some(text) = item.as_str() else {
            continue;
        };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            output.push(trimmed.to_string());
        }
    }
    output
}

fn resolve_query_text(args: &Value) -> Option<String> {
    if let Some(text) = args.get("query").and_then(Value::as_str) {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let keywords = extract_keywords(args);
    if keywords.is_empty() {
        None
    } else {
        Some(keywords.join(" "))
    }
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

fn parse_timeout_secs(value: Option<&Value>) -> Option<f64> {
    match value {
        Some(Value::Number(num)) => num.as_f64(),
        Some(Value::String(text)) => text.trim().parse::<f64>().ok(),
        Some(Value::Bool(flag)) => Some(if *flag { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn resolve_stream_chunk_size(config: &Config) -> usize {
    let size = config.server.stream_chunk_size;
    if size == 0 {
        1024
    } else {
        size
    }
}

fn safe_chunk_boundary(text: &str, max_bytes: usize) -> usize {
    if text.len() <= max_bytes {
        return text.len();
    }
    let mut index = max_bytes.min(text.len());
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    if index == 0 {
        index = max_bytes.min(text.len());
        while index < text.len() && !text.is_char_boundary(index) {
            index += 1;
        }
        if index == 0 {
            index = text.len();
        }
    }
    index
}

fn emit_tool_output_chunks(
    emitter: &ToolEventEmitter,
    tool_name: &str,
    command: &str,
    stream_name: &str,
    pending: &mut String,
    chunk_size: usize,
    force: bool,
) {
    if pending.is_empty() {
        return;
    }
    let limit = chunk_size.max(1);
    loop {
        if pending.is_empty() {
            break;
        }
        if !force && pending.len() < limit {
            break;
        }
        let take_len = if pending.len() <= limit {
            pending.len()
        } else {
            safe_chunk_boundary(pending, limit)
        };
        if take_len == 0 {
            break;
        }
        let chunk = pending[..take_len].to_string();
        pending.replace_range(..take_len, "");
        if chunk.is_empty() {
            break;
        }
        emitter.emit(
            "tool_output_delta",
            json!({
                "tool": tool_name,
                "command": command,
                "stream": stream_name,
                "delta": chunk,
            }),
        );
    }
}

async fn read_stream_output<R>(
    mut reader: R,
    emitter: Option<ToolEventEmitter>,
    tool_name: String,
    command: String,
    stream_name: &'static str,
    chunk_size: usize,
) -> Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let Some(stream_emitter) = emitter.as_ref().filter(|item| item.stream_enabled()) else {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).await?;
        return Ok(output);
    };

    let mut output = Vec::new();
    let read_size = chunk_size.max(256);
    let mut buffer = vec![0u8; read_size];
    let mut pending_bytes = Vec::new();
    let mut pending_text = String::new();
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        output.extend_from_slice(chunk);
        pending_bytes.extend_from_slice(chunk);
        loop {
            match std::str::from_utf8(&pending_bytes) {
                Ok(valid) => {
                    if !valid.is_empty() {
                        pending_text.push_str(valid);
                    }
                    pending_bytes.clear();
                    break;
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to == 0 {
                        break;
                    }
                    let valid = &pending_bytes[..valid_up_to];
                    let text = std::str::from_utf8(valid).unwrap_or_default();
                    if !text.is_empty() {
                        pending_text.push_str(text);
                    }
                    pending_bytes.drain(..valid_up_to);
                }
            }
        }
        emit_tool_output_chunks(
            stream_emitter,
            &tool_name,
            &command,
            stream_name,
            &mut pending_text,
            chunk_size,
            false,
        );
    }

    if !pending_bytes.is_empty() {
        pending_text.push_str(&String::from_utf8_lossy(&pending_bytes));
        pending_bytes.clear();
    }
    emit_tool_output_chunks(
        stream_emitter,
        &tool_name,
        &command,
        stream_name,
        &mut pending_text,
        chunk_size,
        true,
    );

    Ok(output)
}

struct CommandRunResult {
    returncode: i32,
    stdout: String,
    stderr: String,
    timed_out: bool,
}

async fn join_output_task(
    handle: Option<tokio::task::JoinHandle<Result<Vec<u8>>>>,
) -> Result<Vec<u8>> {
    match handle {
        Some(handle) => match handle.await {
            Ok(result) => result,
            Err(err) => Err(anyhow!(err.to_string())),
        },
        None => Ok(Vec::new()),
    }
}

async fn run_command_streaming(
    context: &ToolContext<'_>,
    command: &str,
    cwd: &Path,
    timeout: Option<Duration>,
) -> Result<CommandRunResult> {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let tool_name = "执行命令".to_string();
    let command_text = command.to_string();
    let (mut cmd, used_direct) =
        if let Some(cmd) = command_utils::build_direct_command(command, cwd) {
            (cmd, true)
        } else {
            (command_utils::build_shell_command(command, cwd), false)
        };
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if used_direct && command_utils::is_not_found_error(&err) => {
            let mut cmd = command_utils::build_shell_command(command, cwd);
            cmd.kill_on_drop(true);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            cmd.spawn()?
        }
        Err(err) => return Err(anyhow!(err)),
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = stdout.map(|stdout| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.clone();
        let command_text = command_text.clone();
        tokio::spawn(async move {
            read_stream_output(
                stdout,
                emitter,
                tool_name,
                command_text,
                "stdout",
                chunk_size,
            )
            .await
        })
    });
    let stderr_task = stderr.map(|stderr| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.clone();
        let command_text = command_text.clone();
        tokio::spawn(async move {
            read_stream_output(
                stderr,
                emitter,
                tool_name,
                command_text,
                "stderr",
                chunk_size,
            )
            .await
        })
    });

    let mut timed_out = false;
    let status = if let Some(timeout) = timeout {
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(result) => Some(result?),
            Err(_) => {
                timed_out = true;
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
        }
    } else {
        Some(child.wait().await?)
    };

    let stdout_bytes = join_output_task(stdout_task).await?;
    let stderr_bytes = join_output_task(stderr_task).await?;
    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();
    let returncode = status.and_then(|value| value.code()).unwrap_or(-1);

    Ok(CommandRunResult {
        returncode,
        stdout,
        stderr,
        timed_out,
    })
}

async fn execute_command(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            "执行命令",
            args,
            context.user_tool_bindings,
        )
        .await;
        context.workspace.mark_tree_dirty(context.workspace_id);
        return Ok(result);
    }

    let content = args
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if content.is_empty() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.exec.command_required"),
            "sandbox": false,
        }));
    }
    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, &content);

    let allow_commands = &context.config.security.allow_commands;
    let allow_all = allow_commands.iter().any(|item| item == "*");
    let timeout_s = parse_timeout_secs(args.get("timeout_s"))
        .unwrap_or(0.0)
        .max(0.0);
    let timeout = if timeout_s > 0.0 {
        Some(Duration::from_secs_f64(timeout_s))
    } else {
        None
    };
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let cwd = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.workspace_id)?
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };
    if !cwd.exists() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.exec.workdir_not_found"),
            "sandbox": false,
        }));
    }
    if !cwd.is_dir() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.exec.workdir_not_dir"),
            "sandbox": false,
        }));
    }

    let mut results = Vec::new();
    for raw_line in content.lines() {
        let command = raw_line.trim();
        if command.is_empty() {
            continue;
        }
        if !allow_all && !allow_commands.iter().any(|item| command.starts_with(item)) {
            return Ok(json!({
                "ok": false,
                "data": {},
                "error": i18n::t("tool.exec.not_allowed"),
                "sandbox": false,
            }));
        }
        let run = run_command_streaming(context, command, &cwd, timeout).await?;
        results.push(json!({
            "command": command,
            "returncode": run.returncode,
            "stdout": run.stdout,
            "stderr": run.stderr,
        }));
        if run.timed_out {
            let detail = if timeout_s > 0.0 {
                format!("timeout after {timeout_s}s")
            } else {
                "timeout".to_string()
            };
            if let Some(last) = results.last_mut().and_then(Value::as_object_mut) {
                let previous = last
                    .get("stderr")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let merged = if previous.trim().is_empty() {
                    detail.clone()
                } else {
                    format!("{previous}\n{detail}")
                };
                last.insert("stderr".to_string(), Value::String(merged));
            }
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(json!({
                "ok": false,
                "data": { "results": results },
                "error": i18n::t_with_params(
                    "tool.exec.command_failed",
                    &HashMap::from([("detail".to_string(), detail)]),
                ),
                "sandbox": false,
            }));
        }
        if run.returncode != 0 {
            context.workspace.mark_tree_dirty(context.workspace_id);
            return Ok(json!({
                "ok": false,
                "data": { "results": results },
                "error": i18n::t("tool.exec.failed"),
                "sandbox": false,
            }));
        }
    }
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(json!({
        "ok": true,
        "data": { "results": results },
        "error": "",
        "sandbox": false,
    }))
}

async fn execute_ptc(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if sandbox::sandbox_enabled(context.config) {
        let result = sandbox::execute_tool(
            context.config,
            context.workspace.as_ref(),
            context.user_id,
            context.workspace_id,
            context.session_id,
            "ptc",
            args,
            context.user_tool_bindings,
        )
        .await;
        context.workspace.mark_tree_dirty(context.workspace_id);
        return Ok(result);
    }

    let filename = args
        .get("filename")
        .and_then(Value::as_str)
        .unwrap_or("ptc.tmp");
    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, content);
    let dir = if workdir.is_empty() {
        context.workspace.ensure_user_root(context.workspace_id)?
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };
    let file_path = dir.join(filename);
    let display_path = context
        .workspace
        .display_path(context.workspace_id, &file_path);
    tokio::fs::create_dir_all(&dir).await.ok();
    tokio::fs::write(&file_path, content).await?;
    context.workspace.mark_tree_dirty(context.workspace_id);
    Ok(json!({
        "ok": true,
        "data": { "path": display_path },
        "error": "",
        "sandbox": false,
    }))
}

async fn list_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let max_depth = args
        .get("max_depth")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_LIST_DEPTH as u64) as usize;
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    tokio::task::spawn_blocking(move || {
        list_files_inner(workspace.as_ref(), &user_id, &path, &extra_roots, max_depth)
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?
}

fn list_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    path: &str,
    extra_roots: &[PathBuf],
    max_depth: usize,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, path, extra_roots)?;
    if !root.exists() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.list.path_not_found")
        }));
    }
    let mut items = Vec::new();
    for entry in WalkDir::new(&root)
        .min_depth(1)
        .max_depth(max_depth.saturating_add(1))
        .into_iter()
        .filter_map(|item| item.ok())
    {
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let mut display = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            display.push('/');
        }
        items.push(display);
        if items.len() >= MAX_LIST_ITEMS {
            break;
        }
    }
    Ok(json!({ "items": items }))
}

async fn search_content(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.search.empty")
        }));
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or(".")
        .to_string();
    let file_pattern = args
        .get("file_pattern")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let max_depth = args.get("max_depth").and_then(Value::as_u64).unwrap_or(0) as usize;
    let max_files = args.get("max_files").and_then(Value::as_u64).unwrap_or(0) as usize;
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    tokio::task::spawn_blocking(move || {
        search_content_inner(
            workspace.as_ref(),
            &user_id,
            &query,
            &path,
            &file_pattern,
            &extra_roots,
            max_depth,
            max_files,
        )
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?
}

fn search_content_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    query: &str,
    path: &str,
    file_pattern: &str,
    extra_roots: &[PathBuf],
    max_depth: usize,
    max_files: usize,
) -> Result<Value> {
    let root = resolve_tool_path(workspace, user_id, path, extra_roots)?;
    if !root.exists() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.search.path_not_found")
        }));
    }

    let matcher = build_glob_matcher(file_pattern);
    let lower_query = query.to_lowercase();
    let mut matches = Vec::new();
    let mut scanned_files = 0usize;
    let mut walker = WalkDir::new(&root);
    if max_depth > 0 {
        walker = walker.max_depth(max_depth);
    }
    'scan: for entry in walker.into_iter().filter_map(|item| item.ok()) {
        if entry.file_type().is_dir() {
            continue;
        }
        if max_files > 0 && scanned_files >= max_files {
            break;
        }
        scanned_files = scanned_files.saturating_add(1);
        let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
        let rel_display = rel.to_string_lossy().replace('\\', "/");
        if let Some(regex) = matcher.as_ref() {
            if !regex.is_match(&rel_display) {
                continue;
            }
        }
        if entry.metadata().map(|meta| meta.len()).unwrap_or(0) > MAX_READ_BYTES as u64 {
            continue;
        }
        let content = read_text_with_limit(entry.path(), MAX_READ_BYTES)?;
        for (idx, line) in content.lines().enumerate() {
            if line.to_lowercase().contains(&lower_query) {
                matches.push(format!("{}:{}:{}", rel_display, idx + 1, line.trim()));
                if matches.len() >= MAX_SEARCH_MATCHES {
                    break 'scan;
                }
            }
        }
    }
    Ok(json!({ "matches": matches }))
}

#[derive(Clone)]
struct ReadFileSpec {
    path: String,
    ranges: Vec<(usize, usize)>,
}

fn parse_read_file_specs(args: &Value) -> std::result::Result<Vec<ReadFileSpec>, String> {
    let Some(files) = args.get("files").and_then(Value::as_array) else {
        return Err(i18n::t("tool.read.no_path"));
    };
    let mut specs = Vec::new();
    for file in files.iter().take(MAX_READ_FILES) {
        let Some(obj) = file.as_object() else {
            continue;
        };
        let path = obj
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if path.is_empty() {
            continue;
        }
        let mut ranges = Vec::new();
        if let Some(Value::Array(items)) = obj.get("line_ranges") {
            for item in items {
                let Some(pair) = item.as_array() else {
                    continue;
                };
                if pair.len() < 2 {
                    continue;
                }
                let Some(start) = pair.get(0).and_then(parse_line_number) else {
                    continue;
                };
                let Some(end) = pair.get(1).and_then(parse_line_number) else {
                    continue;
                };
                ranges.push(normalize_range(start, end));
            }
        }
        if let Some(start) = obj.get("start_line").and_then(parse_line_number) {
            let end = obj
                .get("end_line")
                .and_then(parse_line_number)
                .unwrap_or(start);
            ranges.push(normalize_range(start, end));
        }
        if ranges.is_empty() {
            ranges.push((1, MAX_READ_LINES));
        }
        specs.push(ReadFileSpec { path, ranges });
    }
    if specs.is_empty() {
        return Err(i18n::t("tool.read.no_path"));
    }
    Ok(specs)
}

fn parse_line_number(value: &Value) -> Option<usize> {
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

fn normalize_range(start: usize, end: usize) -> (usize, usize) {
    let start = start.max(1);
    let end = end.max(start);
    if end - start + 1 > MAX_RANGE_SPAN {
        return (start, start + MAX_RANGE_SPAN - 1);
    }
    (start, end)
}

fn summarize_read_ranges(ranges: &[(usize, usize)], total_lines: usize) -> (usize, bool) {
    if total_lines == 0 {
        return (0, true);
    }
    if ranges.is_empty() {
        return (0, false);
    }
    let mut intervals = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        let start = (*start).max(1);
        if start > total_lines {
            continue;
        }
        let end = (*end).min(total_lines).max(start);
        intervals.push((start, end));
    }
    if intervals.is_empty() {
        return (0, false);
    }
    intervals.sort_by_key(|(start, _)| *start);
    let mut read_lines = 0usize;
    let mut current = intervals[0];
    for (start, end) in intervals.into_iter().skip(1) {
        if start <= current.1 + 1 {
            current.1 = current.1.max(end);
        } else {
            read_lines += current.1 - current.0 + 1;
            current = (start, end);
        }
    }
    read_lines += current.1 - current.0 + 1;
    let complete = read_lines == total_lines;
    (read_lines, complete)
}

fn read_text_with_limit(path: &Path, max_bytes: usize) -> Result<String> {
    let file = File::open(path)?;
    let mut buffer = Vec::new();
    file.take(max_bytes as u64).read_to_end(&mut buffer)?;
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn build_glob_matcher(pattern: &str) -> Option<Regex> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut regex = String::from("^");
    for ch in trimmed.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            '.' | '(' | ')' | '[' | ']' | '{' | '}' | '+' | '|' | '^' | '$' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    Regex::new(&regex).ok()
}

fn dedupe_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for root in roots {
        let normalized = normalize_existing_path(&root);
        let key = normalize_path_for_compare(&normalized);
        if seen.insert(key) {
            output.push(normalized);
        }
    }
    output
}

fn build_allow_roots(config: &Config) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let cwd = std::env::current_dir().ok();
    for raw in &config.security.allow_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let path = PathBuf::from(trimmed);
        let resolved = if path.is_absolute() {
            path
        } else if let Some(base) = &cwd {
            base.join(path)
        } else {
            path
        };
        roots.push(resolved);
    }
    dedupe_roots(roots)
}

fn collect_allow_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    if let Some(roots) = context.allow_roots.as_ref() {
        return roots.as_ref().clone();
    }
    build_allow_roots(context.config)
}

fn collect_read_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    if let Some(roots) = context.read_roots.as_ref() {
        return roots.as_ref().clone();
    }
    let mut roots = collect_allow_roots(context);
    roots.extend(collect_skill_roots(context));
    dedupe_roots(roots)
}

fn collect_skill_roots(context: &ToolContext<'_>) -> Vec<PathBuf> {
    build_skill_roots(context.skills, context.user_tool_bindings)
}

fn build_skill_roots(
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = skills
        .list_specs()
        .into_iter()
        .map(|spec| spec.root)
        .collect();
    if let Some(bindings) = user_tool_bindings {
        for source in bindings.skill_sources.values() {
            roots.push(source.root.clone());
        }
    }
    roots
}

fn resolve_path_in_roots(raw_path: &str, roots: &[PathBuf]) -> Option<PathBuf> {
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
    for root in roots {
        if is_within_root(root, &candidate) {
            return Some(candidate.clone());
        }
    }
    None
}

fn resolve_tool_path(
    workspace: &WorkspaceManager,
    user_id: &str,
    raw_path: &str,
    extra_roots: &[PathBuf],
) -> Result<PathBuf> {
    match workspace.resolve_path(user_id, raw_path) {
        Ok(path) => Ok(path),
        Err(err) => {
            if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                Ok(resolved)
            } else {
                Err(err)
            }
        }
    }
}

async fn read_files(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let specs = match parse_read_file_specs(args) {
        Ok(specs) => specs,
        Err(message) => {
            return Ok(json!({
                "ok": false,
                "data": {},
                "error": message
            }))
        }
    };

    let specs_for_lsp = specs.clone();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let extra_roots = collect_read_roots(context);
    let result = tokio::task::spawn_blocking(move || {
        read_files_inner(workspace.as_ref(), &user_id, &extra_roots, specs)
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))?;
    if result.is_ok() && context.config.lsp.enabled {
        for spec in specs_for_lsp {
            if let Ok(target) = context
                .workspace
                .resolve_path(context.workspace_id, &spec.path)
            {
                let _ = touch_lsp_file(context, &target, false).await;
            }
        }
    }
    result
}

fn read_files_inner(
    workspace: &WorkspaceManager,
    user_id: &str,
    extra_roots: &[PathBuf],
    specs: Vec<ReadFileSpec>,
) -> Result<Value> {
    let mut outputs = Vec::new();
    let mut summaries = Vec::new();
    for spec in specs {
        let raw_path = spec.path.as_str();
        let mut summary = json!({
            "path": raw_path,
            "read_lines": 0,
            "total_lines": 0,
            "complete": false
        });
        let target = match workspace.resolve_path(user_id, raw_path) {
            Ok(path) => Some(path),
            Err(err) => {
                if let Some(resolved) = resolve_path_in_roots(raw_path, extra_roots) {
                    Some(resolved)
                } else {
                    outputs.push(format!(">>> {}\n{}", raw_path, err));
                    None
                }
            }
        };
        let Some(target) = target else {
            summaries.push(summary);
            continue;
        };
        if !target.exists() {
            outputs.push(format!(
                ">>> {}\n{}",
                raw_path,
                i18n::t("tool.read.not_found")
            ));
            summaries.push(summary);
            continue;
        }
        let size = target.metadata().map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_READ_BYTES as u64 {
            outputs.push(format!(
                ">>> {}\n{}",
                raw_path,
                i18n::t("tool.read.too_large")
            ));
            summaries.push(summary);
            continue;
        }
        let content = read_text_with_limit(&target, MAX_READ_BYTES)?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let (read_lines, complete) = summarize_read_ranges(&spec.ranges, total_lines);
        if let Value::Object(ref mut map) = summary {
            map.insert("read_lines".to_string(), Value::from(read_lines as u64));
            map.insert("total_lines".to_string(), Value::from(total_lines as u64));
            map.insert("complete".to_string(), Value::Bool(complete));
        }
        let mut file_output = Vec::new();
        for (start, end) in spec.ranges {
            if lines.is_empty() {
                file_output.push(i18n::t("tool.read.empty_file"));
                continue;
            }
            if start > lines.len() {
                let params = HashMap::from([
                    ("start".to_string(), start.to_string()),
                    ("end".to_string(), end.to_string()),
                    ("total".to_string(), lines.len().to_string()),
                ]);
                file_output.push(i18n::t_with_params("tool.read.range_out_of_file", &params));
                continue;
            }
            let last = end.min(lines.len());
            let mut slice_lines = Vec::new();
            for idx in (start - 1)..last {
                slice_lines.push(format!("{}: {}", idx + 1, lines[idx]));
            }
            file_output.push(slice_lines.join("\n"));
        }
        let joined = file_output.join("\n---\n");
        outputs.push(format!(">>> {}\n{}", raw_path, joined));
        summaries.push(summary);
    }
    let result = if outputs.is_empty() {
        i18n::t("tool.read.empty_result")
    } else {
        outputs.join("\n\n")
    };
    Ok(json!({
        "content": result,
        "meta": { "files": summaries }
    }))
}

#[derive(Debug, Deserialize)]
struct SkillCallArgs {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    skill_name: Option<String>,
}

async fn execute_skill_call(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SkillCallArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let raw_name = payload
        .name
        .or(payload.skill_name)
        .unwrap_or_default()
        .trim()
        .to_string();
    if raw_name.is_empty() {
        return Err(anyhow!(i18n::t("tool.skill_call.name_required")));
    }

    let mut selected: Option<SkillSpec> = context.skills.get(&raw_name);
    if selected.is_none() {
        if let Some(bindings) = context.user_tool_bindings {
            if let Some(spec) = bindings
                .skill_specs
                .iter()
                .find(|spec| spec.name == raw_name)
            {
                selected = Some(spec.clone());
            } else {
                let suffix = format!("@{raw_name}");
                let matches: Vec<SkillSpec> = bindings
                    .skill_specs
                    .iter()
                    .filter(|spec| spec.name.ends_with(&suffix))
                    .cloned()
                    .collect();
                if matches.len() == 1 {
                    selected = Some(matches[0].clone());
                } else if matches.len() > 1 {
                    let candidates = matches
                        .iter()
                        .map(|spec| spec.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(anyhow!(i18n::t_with_params(
                        "tool.skill_call.ambiguous",
                        &HashMap::from([
                            ("name".to_string(), raw_name.clone()),
                            ("candidates".to_string(), candidates),
                        ]),
                    )));
                }
            }
        }
    }

    let Some(spec) = selected else {
        return Err(anyhow!(i18n::t_with_params(
            "tool.skill_call.not_found",
            &HashMap::from([("name".to_string(), raw_name)]),
        )));
    };

    let content = std::fs::read_to_string(&spec.path).map_err(|err| {
        anyhow!(i18n::t_with_params(
            "tool.skill_call.read_failed",
            &HashMap::from([("detail".to_string(), err.to_string())]),
        ))
    })?;
    let tree = build_skill_tree(&spec.root);
    let path = absolute_path_string_from_text(&spec.path);
    let root = absolute_path_string(&spec.root);
    Ok(json!({
        "name": spec.name,
        "description": spec.description,
        "path": path,
        "root": root,
        "skill_md": content,
        "tree": tree
    }))
}

fn build_skill_tree(root: &Path) -> Vec<String> {
    let mut items = Vec::new();
    for entry in WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(|item| item.ok())
    {
        let rel = entry.path().strip_prefix(root).unwrap_or(entry.path());
        let mut display = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            display.push('/');
        }
        items.push(display);
    }
    items
}

fn absolute_path_string(path: &Path) -> String {
    let normalized = normalize_existing_path(path);
    let mut text = normalized.to_string_lossy().to_string();
    if cfg!(windows) {
        if let Some(stripped) = text.strip_prefix(r"\\?\") {
            text = stripped.to_string();
        }
    }
    text.replace('\\', "/")
}

fn absolute_path_string_from_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    absolute_path_string(&PathBuf::from(raw))
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

fn normalize_lsp_extension(value: &str) -> String {
    value.trim().trim_start_matches('.').to_lowercase()
}

fn lsp_file_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .trim()
        .to_string();
    normalize_lsp_extension(&ext)
}

fn lsp_matches_file(config: &Config, path: &Path) -> bool {
    let extension = lsp_file_extension(path);
    config
        .lsp
        .servers
        .iter()
        .filter(|server| server.enabled)
        .any(|server| {
            if server.extensions.is_empty() {
                return true;
            }
            server
                .extensions
                .iter()
                .any(|ext| normalize_lsp_extension(ext) == extension)
        })
}

fn resolve_lsp_timeout_s(config: &Config) -> u64 {
    if config.lsp.timeout_s == 0 {
        30
    } else {
        config.lsp.timeout_s
    }
}

fn parse_lsp_position(args: &Value) -> Result<(u32, u32)> {
    let line = args
        .get("line")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 line"))?;
    let character = args
        .get("character")
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow!("缺少 character"))?;
    if line == 0 || character == 0 {
        return Err(anyhow!("line/character 必须 >= 1"));
    }
    Ok(((line - 1) as u32, (character - 1) as u32))
}

fn lsp_path_to_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| anyhow!("LSP 文件路径无效"))
}

fn format_lsp_diagnostics(diagnostics: &[LspDiagnostic]) -> Option<Value> {
    if diagnostics.is_empty() {
        return None;
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for diag in diagnostics {
        if diag.is_error() {
            errors.push(diag.pretty());
        } else {
            warnings.push(diag.pretty());
        }
    }
    let total = errors.len() + warnings.len();
    let mut items: Vec<String> = errors.iter().chain(warnings.iter()).cloned().collect();
    let truncated = items.len() > MAX_LSP_DIAGNOSTICS;
    if truncated {
        items.truncate(MAX_LSP_DIAGNOSTICS);
    }
    Some(json!({
        "total": total,
        "errors": errors.len(),
        "warnings": warnings.len(),
        "truncated": truncated,
        "items": items,
    }))
}

fn lsp_diagnostics_summary(context: &ToolContext<'_>, path: &Path) -> Option<Value> {
    let diagnostics_map = context
        .lsp_manager
        .diagnostics_for_user(context.workspace_id);
    if diagnostics_map.is_empty() {
        return None;
    }
    let target = normalize_target_path(path);
    let target_compare = normalize_path_for_compare(&target);
    for (candidate, diagnostics) in diagnostics_map {
        if normalize_path_for_compare(&candidate) == target_compare {
            return format_lsp_diagnostics(&diagnostics);
        }
    }
    None
}

async fn touch_lsp_file(
    context: &ToolContext<'_>,
    path: &Path,
    wait_for_diagnostics: bool,
) -> Value {
    if !context.config.lsp.enabled {
        return Value::Null;
    }
    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    if !is_within_root(&workspace_root, path) {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "文件不在工作区范围内"
        });
    }
    let matched = lsp_matches_file(context.config, path);
    if !matched {
        return json!({
            "enabled": true,
            "matched": false,
            "touched": false,
            "diagnostics": Option::<Value>::None,
            "error": "未匹配到可用的 LSP 服务"
        });
    }
    let mut diagnostics = None;
    let mut error = None;
    let touched = match context
        .lsp_manager
        .touch_file(
            context.config,
            context.workspace_id,
            path,
            wait_for_diagnostics,
        )
        .await
    {
        Ok(()) => true,
        Err(err) => {
            warn!("LSP touch failed: {err}");
            error = Some(err.to_string());
            false
        }
    };
    if touched && wait_for_diagnostics {
        diagnostics = lsp_diagnostics_summary(context, path);
    }
    json!({
        "enabled": true,
        "matched": matched,
        "touched": touched,
        "diagnostics": diagnostics,
        "error": error
    })
}

async fn write_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?;
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    let path = path.to_string();
    let content = content.to_string();
    let bytes = content.as_bytes().len();
    let workspace = context.workspace.clone();
    let user_id = context.workspace_id.to_string();
    let path_for_write = path.clone();
    let allow_roots = collect_allow_roots(context);
    let target = tokio::task::spawn_blocking(move || {
        let target =
            resolve_tool_path(workspace.as_ref(), &user_id, &path_for_write, &allow_roots)?;
        let workspace_root = workspace.workspace_root(&user_id);
        if is_within_root(&workspace_root, &target) {
            workspace.write_file(&user_id, &path_for_write, &content, true)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&target, &content)?;
        }
        Ok::<PathBuf, anyhow::Error>(target)
    })
    .await
    .map_err(|err| anyhow!(err.to_string()))??;
    let lsp_info = touch_lsp_file(context, &target, true).await;
    Ok(json!({
        "ok": true,
        "path": path,
        "bytes": bytes,
        "lsp": lsp_info
    }))
}

async fn replace_text(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
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
    let path = path.to_string();
    let old = old.to_string();
    let new_str = new_str.to_string();
    let allow_roots = collect_allow_roots(context);
    let target = resolve_tool_path(
        context.workspace.as_ref(),
        context.workspace_id,
        &path,
        &allow_roots,
    )?;
    let target_for_read = target.clone();
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&target_for_read))
        .await
        .map_err(|err| anyhow!(err.to_string()))??;
    let replaced = content.replace(&old, &new_str);
    let count = content.matches(&old).count() as u64;
    if let Some(expected) = expected {
        if count != expected {
            return Err(anyhow!("替换次数不匹配，期望 {expected}，实际 {count}"));
        }
    }
    let target_for_write = target.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&target_for_write, replaced))
        .await
        .map_err(|err| anyhow!(err.to_string()))??;
    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    if is_within_root(&workspace_root, &target) {
        context.workspace.bump_version(context.workspace_id);
    }
    let lsp_info = touch_lsp_file(context, &target, true).await;
    Ok(json!({
        "ok": true,
        "path": path,
        "replaced": count,
        "lsp": lsp_info
    }))
}

async fn edit_file(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?
        .to_string();
    let edits = args
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("缺少 edits"))?
        .to_vec();
    let allow_roots = collect_allow_roots(context);
    let target = resolve_tool_path(
        context.workspace.as_ref(),
        context.workspace_id,
        &path,
        &allow_roots,
    )?;
    let target_for_read = target.clone();
    let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&target_for_read))
        .await
        .map_err(|err| anyhow!(err.to_string()))??;
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
    let target_for_write = target.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&target_for_write, output))
        .await
        .map_err(|err| anyhow!(err.to_string()))??;
    let workspace_root = context.workspace.workspace_root(context.workspace_id);
    if is_within_root(&workspace_root, &target) {
        context.workspace.bump_version(context.workspace_id);
    }
    let lsp_info = touch_lsp_file(context, &target, true).await;
    Ok(json!({
        "ok": true,
        "path": path,
        "lines": lines.len(),
        "lsp": lsp_info
    }))
}

async fn lsp_query(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    if !context.config.lsp.enabled {
        return Err(anyhow!("LSP 未启用"));
    }
    let operation = args
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 operation"))?
        .trim()
        .to_string();
    if operation.is_empty() {
        return Err(anyhow!("operation 不能为空"));
    }
    let path = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("缺少 path"))?
        .trim()
        .to_string();
    if path.is_empty() {
        return Err(anyhow!("path 不能为空"));
    }
    let target = context
        .workspace
        .resolve_path(context.workspace_id, &path)?;
    if !target.exists() {
        return Err(anyhow!("LSP 文件不存在: {path}"));
    }
    context
        .lsp_manager
        .touch_file(context.config, context.workspace_id, &target, false)
        .await?;
    let uri = lsp_path_to_uri(&target)?;
    let timeout_s = resolve_lsp_timeout_s(context.config);
    let operation_key = operation.to_lowercase();
    let needs_position = matches!(
        operation_key.as_str(),
        "definition" | "references" | "hover" | "implementation" | "callhierarchy"
    );
    let position_value = if needs_position {
        let (line, character) = parse_lsp_position(args)?;
        Some(json!({ "line": line, "character": character }))
    } else {
        None
    };
    let query = if operation_key == "workspacesymbol" {
        Some(
            args.get("query")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
        )
    } else {
        None
    };
    if operation_key == "workspacesymbol" && query.as_deref().unwrap_or("").is_empty() {
        return Err(anyhow!("workspaceSymbol 缺少 query"));
    }
    let call_direction = args
        .get("call_hierarchy_direction")
        .and_then(Value::as_str)
        .unwrap_or("incoming")
        .trim()
        .to_lowercase();
    let text_document = json!({ "uri": uri });
    let position_value = position_value.clone();
    let query_value = query.clone();
    let operation_key = operation_key.clone();
    let call_direction = call_direction.clone();
    let results = context
        .lsp_manager
        .run_on_clients(
            context.config,
            context.workspace_id,
            &target,
            move |client| {
                let text_document = text_document.clone();
                let position = position_value.clone();
                let query = query_value.clone();
                let operation = operation_key.clone();
                let direction = call_direction.clone();
                async move {
                    let server_id = client.server_id().to_string();
                    let server_name = client.server_name().to_string();
                    let result = match operation.as_str() {
                        "definition" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/definition",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "references" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/references",
                                    json!({
                                        "textDocument": text_document,
                                        "position": position,
                                        "context": { "includeDeclaration": true }
                                    }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "hover" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/hover",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "documentsymbol" => {
                            client
                                .request(
                                    "textDocument/documentSymbol",
                                    json!({ "textDocument": text_document }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "workspacesymbol" => {
                            let query = query.unwrap_or_default();
                            client
                                .request("workspace/symbol", json!({ "query": query }), timeout_s)
                                .await?
                        }
                        "implementation" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            client
                                .request(
                                    "textDocument/implementation",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?
                        }
                        "callhierarchy" => {
                            let position =
                                position.ok_or_else(|| anyhow!("缺少 line/character"))?;
                            let items = client
                                .request(
                                    "textDocument/prepareCallHierarchy",
                                    json!({ "textDocument": text_document, "position": position }),
                                    timeout_s,
                                )
                                .await?;
                            let calls = if let Some(item) =
                                items.as_array().and_then(|items| items.first()).cloned()
                            {
                                let method = if direction == "outgoing" {
                                    "callHierarchy/outgoingCalls"
                                } else {
                                    "callHierarchy/incomingCalls"
                                };
                                client
                                    .request(method, json!({ "item": item }), timeout_s)
                                    .await?
                            } else {
                                Value::Null
                            };
                            json!({
                                "items": items,
                                "direction": direction,
                                "calls": calls
                            })
                        }
                        _ => {
                            return Err(anyhow!("未知 LSP operation: {operation}"));
                        }
                    };
                    Ok(json!({
                        "server_id": server_id,
                        "server_name": server_name,
                        "result": result
                    }))
                }
            },
        )
        .await?;
    Ok(json!({
        "ok": true,
        "operation": operation,
        "path": path,
        "results": results
    }))
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
        updated_time: Some(task.updated_time.with_timezone(&Local).to_rfc3339()),
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
        snapshot.updated_time = Some(Local::now().to_rfc3339());
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
