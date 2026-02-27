// 内置工具定义与执行入口，保持工具名称与协议一致。
use crate::a2a_store::{A2aStore, A2aTask};
use crate::command_utils;
use crate::config::{
    is_debug_log_level, normalize_knowledge_base_type, A2aServiceConfig, Config,
    KnowledgeBaseConfig, KnowledgeBaseType,
};
use crate::core::python_runtime;
use crate::cron::{handle_cron_action, CronActionRequest};
use crate::gateway::{GatewayHub, GatewayNodeInvokeRequest};
use crate::history::HistoryManager;
use crate::i18n;
use crate::knowledge;
use crate::llm::embed_texts;
use crate::lsp::{LspDiagnostic, LspManager};
use crate::mcp;
use crate::monitor::MonitorState;
use crate::orchestrator::Orchestrator;
use crate::path_utils::{
    is_within_root, normalize_existing_path, normalize_path_for_compare, normalize_target_path,
};
use crate::sandbox;
use crate::schemas::{ToolSpec, WunderRequest};
use crate::skills::{execute_skill, SkillRegistry, SkillSpec};
use crate::storage::{
    ChatSessionRecord, SessionRunRecord, StorageBackend, UserAgentAccessRecord, UserAgentRecord,
    DEFAULT_HIVE_ID,
};
use crate::user_store::UserStore;
use crate::user_tools::{
    UserToolAlias, UserToolBindings, UserToolKind, UserToolManager, UserToolStore,
};
use crate::user_world::UserWorldService;
use crate::vector_knowledge;
use crate::workspace::WorkspaceManager;
use anyhow::{anyhow, Result};
use chrono::{Local, Utc};
#[cfg(windows)]
use encoding_rs::GBK;
use futures::stream::{self, StreamExt};
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::sync::{oneshot, RwLock};
use tokio::time::{sleep, timeout};
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
const MAX_SESSION_LIST_ITEMS: i64 = 200;
const MAX_SESSION_HISTORY_ITEMS: i64 = 500;
const MAX_SESSION_MESSAGE_ITEMS: i64 = 50;
const MAX_USER_WORLD_LIST_LIMIT: i64 = 500;
const USER_WORLD_FILE_STAGING_DIR: &str = "user_world_uploads";
const SESSION_RESULT_MAX_CHARS: usize = 2000;
const LOCAL_PTC_TIMEOUT_S: u64 = 60;
const LOCAL_PTC_DIR_NAME: &str = "ptc_temp";
const TOOL_OVERRIDE_NONE: &str = "__no_tools__";
const DEFAULT_SESSION_TITLE: &str = "新会话";
const ANNOUNCE_SKIP: &str = "ANNOUNCE_SKIP";
const SWARM_WAIT_DEFAULT_POLL_S: f64 = 1.0;
const SWARM_WAIT_MIN_POLL_S: f64 = 0.2;
const SWARM_WAIT_MAX_POLL_S: f64 = 5.0;

type ToolEventCallback = dyn Fn(&str, Value) + Send + Sync;

#[derive(Clone)]
pub struct ToolEventEmitter {
    callback: Arc<ToolEventCallback>,
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
    pub is_admin: bool,
    pub storage: Arc<dyn StorageBackend>,
    pub orchestrator: Option<Arc<Orchestrator>>,
    pub monitor: Option<Arc<MonitorState>>,
    pub workspace: Arc<WorkspaceManager>,
    pub lsp_manager: Arc<LspManager>,
    pub config: &'a Config,
    pub a2a_store: &'a A2aStore,
    pub skills: &'a SkillRegistry,
    pub gateway: Option<Arc<GatewayHub>>,
    pub user_world: Option<Arc<UserWorldService>>,
    pub user_tool_manager: Option<Arc<UserToolManager>>,
    pub user_tool_bindings: Option<&'a UserToolBindings>,
    pub user_tool_store: Option<&'a UserToolStore>,
    pub request_config_overrides: Option<&'a Value>,
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
            name: "定时任务".to_string(),
            description: t("tool.spec.schedule_task.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.schedule_task.args.action"),
                        "enum": ["add", "update", "remove", "enable", "disable", "get", "list", "run"]
                    },
                    "job": {
                        "type": "object",
                        "description": t("tool.spec.schedule_task.args.job"),
                        "properties": {
                            "job_id": {"type": "string", "description": t("tool.spec.schedule_task.args.job.job_id")},
                            "name": {"type": "string", "description": t("tool.spec.schedule_task.args.job.name")},
                            "schedule": {
                                "type": "object",
                                "description": t("tool.spec.schedule_task.args.job.schedule"),
                                "properties": {
                                    "kind": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.kind"), "enum": ["at", "every", "cron"]},
                                    "at": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.at")},
                                    "every_ms": {"type": "integer", "description": t("tool.spec.schedule_task.args.job.schedule.every_ms"), "minimum": 1000},
                                    "cron": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.cron")},
                                    "tz": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.tz")}
                                },
                                "required": ["kind"]
                            },
                            "session": {"type": "string", "description": t("tool.spec.schedule_task.args.job.session"), "enum": ["main", "isolated"]},
                            "payload": {
                                "type": "object",
                                "description": t("tool.spec.schedule_task.args.job.payload"),
                                "properties": {
                                    "message": {"type": "string", "description": t("tool.spec.schedule_task.args.job.payload.message")}
                                }
                            },
                            "deliver": {"type": "object", "description": t("tool.spec.schedule_task.args.job.deliver")},
                            "enabled": {"type": "boolean", "description": t("tool.spec.schedule_task.args.job.enabled")},
                            "delete_after_run": {"type": "boolean", "description": t("tool.spec.schedule_task.args.job.delete_after_run")},
                            "dedupe_key": {"type": "string", "description": t("tool.spec.schedule_task.args.job.dedupe_key")}
                        }
                    }
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: "用户世界工具".to_string(),
            description: t("tool.spec.user_world.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.user_world.args.action"),
                        "enum": ["list_users", "send_message"]
                    },
                    "keyword": {"type": "string", "description": t("tool.spec.user_world.args.keyword")},
                    "offset": {"type": "integer", "description": t("tool.spec.user_world.args.offset"), "minimum": 0},
                    "limit": {"type": "integer", "description": t("tool.spec.user_world.args.limit"), "minimum": 0},
                    "user_id": {"type": "string", "description": t("tool.spec.user_world.args.user_id")},
                    "user_ids": {"type": "array", "items": {"type": "string"}, "description": t("tool.spec.user_world.args.user_ids")},
                    "content": {"type": "string", "description": t("tool.spec.user_world.args.content")},
                    "content_type": {"type": "string", "description": t("tool.spec.user_world.args.content_type")},
                    "client_msg_id": {"type": "string", "description": t("tool.spec.user_world.args.client_msg_id")}
                },
                "required": ["action"],
                "allOf": [
                    {
                        "if": {"properties": {"action": {"const": "send_message"}}},
                        "then": {
                            "required": ["content"],
                            "anyOf": [
                                {"required": ["user_id"]},
                                {"required": ["user_ids"]}
                            ]
                        }
                    }
                ]
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
                "required": ["filename", "content"]
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
        ToolSpec {
            name: "子智能体控制".to_string(),
            description: t("tool.spec.subagent_control.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.subagent_control.args.action"),
                        "enum": ["list", "history", "send", "spawn"]
                    },
                    "limit": {"type": "integer", "description": t("tool.spec.sessions_list.args.limit"), "minimum": 1},
                    "activeMinutes": {"type": "number", "description": t("tool.spec.sessions_list.args.active_minutes"), "minimum": 0},
                    "messageLimit": {"type": "integer", "description": t("tool.spec.sessions_list.args.message_limit"), "minimum": 0},
                    "parentId": {"type": "string", "description": t("tool.spec.sessions_list.args.parent_id")},
                    "session_id": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                    "sessionKey": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                    "includeTools": {"type": "boolean", "description": t("tool.spec.sessions_history.args.include_tools")},
                    "message": {"type": "string", "description": t("tool.spec.sessions_send.args.message")},
                    "timeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_send.args.timeout")},
                    "task": {"type": "string", "description": t("tool.spec.sessions_spawn.args.task")},
                    "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                    "agentId": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                    "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                    "runTimeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                    "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: "智能体蜂群".to_string(),
            description: t("tool.spec.agent_swarm.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.agent_swarm.args.action"),
                        "enum": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
                    },
                    "agentId": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                    "agent_id": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                    "limit": {"type": "integer", "description": t("tool.spec.sessions_list.args.limit"), "minimum": 1},
                    "activeMinutes": {"type": "number", "description": t("tool.spec.sessions_list.args.active_minutes"), "minimum": 0},
                    "session_id": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                    "sessionKey": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                    "includeTools": {"type": "boolean", "description": t("tool.spec.sessions_history.args.include_tools")},
                    "message": {"type": "string", "description": t("tool.spec.sessions_send.args.message")},
                    "timeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_send.args.timeout")},
                    "task": {"type": "string", "description": t("tool.spec.sessions_spawn.args.task")},
                    "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                    "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                    "runTimeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                    "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]},
                    "createIfMissing": {"type": "boolean", "description": t("tool.spec.agent_swarm.args.create_if_missing")},
                    "includeCurrent": {"type": "boolean", "description": t("tool.spec.agent_swarm.args.include_current")},
                    "hiveId": {"type": "string", "description": t("tool.spec.agent_swarm.args.hive_id")},
                    "hive_id": {"type": "string", "description": t("tool.spec.agent_swarm.args.hive_id")},
                    "tasks": {
                        "type": "array",
                        "description": t("tool.spec.agent_swarm.args.tasks"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "agentId": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                                "agent_id": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                                "session_id": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                                "sessionKey": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                                "message": {"type": "string", "description": t("tool.spec.sessions_send.args.message")},
                                "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                                "createIfMissing": {"type": "boolean", "description": t("tool.spec.agent_swarm.args.create_if_missing")}
                            }
                        }
                    },
                    "runIds": {"type": "array", "description": t("tool.spec.agent_swarm.args.run_ids"), "items": {"type": "string"}},
                    "run_ids": {"type": "array", "description": t("tool.spec.agent_swarm.args.run_ids"), "items": {"type": "string"}},
                    "waitSeconds": {"type": "number", "description": t("tool.spec.agent_swarm.args.wait_seconds")},
                    "pollIntervalSeconds": {"type": "number", "description": t("tool.spec.agent_swarm.args.poll_interval")}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: "节点调用".to_string(),
            description: t("tool.spec.node_invoke.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.node_invoke.args.action"),
                        "enum": ["list", "invoke"]
                    },
                    "node_id": {"type": "string", "description": t("tool.spec.node_invoke.args.node_id")},
                    "command": {"type": "string", "description": t("tool.spec.node_invoke.args.command")},
                    "args": {"type": "object", "description": t("tool.spec.node_invoke.args.args")},
                    "timeout_s": {"type": "number", "description": t("tool.spec.node_invoke.args.timeout")},
                    "metadata": {"type": "object", "description": t("tool.spec.node_invoke.args.metadata")}
                },
                "anyOf": [
                    {"required": ["action"]},
                    {"required": ["node_id", "command"]}
                ],
                "allOf": [
                    {
                        "if": {"properties": {"action": {"const": "invoke"}}},
                        "then": {"required": ["node_id", "command"]}
                    }
                ]
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
    map.insert("schedule_task".to_string(), "定时任务".to_string());
    map.insert("user_world".to_string(), "用户世界工具".to_string());
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
    map.insert("subagent_control".to_string(), "子智能体控制".to_string());
    map.insert(
        "agent_swarm".to_string(),
        "\u{667a}\u{80fd}\u{4f53}\u{8702}\u{7fa4}".to_string(),
    );
    map.insert(
        "swarm_control".to_string(),
        "\u{667a}\u{80fd}\u{4f53}\u{8702}\u{7fa4}".to_string(),
    );
    map.insert("node.invoke".to_string(), "节点调用".to_string());
    map.insert("node_invoke".to_string(), "节点调用".to_string());
    map
}

pub fn resolve_tool_name(name: &str) -> String {
    let alias_map = builtin_aliases();
    alias_map
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn preferred_english_alias(canonical: &str) -> Option<&'static str> {
    match canonical {
        "问询面板" => Some("question_panel"),
        "技能调用" => Some("skill_call"),
        "智能体蜂群" => Some("agent_swarm"),
        "节点调用" => Some("node_invoke"),
        "用户世界工具" => Some("user_world"),
        _ => None,
    }
}

fn select_english_tool_alias(
    canonical: &str,
    aliases: &[String],
    allowed_names: &HashSet<String>,
) -> Option<String> {
    if aliases.is_empty() {
        return None;
    }
    if let Some(preferred) = preferred_english_alias(canonical).filter(|value| {
        aliases.iter().any(|alias| alias == *value) && allowed_names.contains(*value)
    }) {
        return Some(preferred.to_string());
    }
    aliases
        .iter()
        .find(|alias| allowed_names.contains(*alias))
        .cloned()
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
    for aliases in canonical_aliases.values_mut() {
        aliases.sort();
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
            select_english_tool_alias(&spec.name, aliases, allowed_names)
        } else {
            None
        };
        let name = preferred_alias.unwrap_or_else(|| spec.name.clone());
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
        "子智能体控制" => subagent_control(context, args).await,
        "\u{667a}\u{80fd}\u{4f53}\u{8702}\u{7fa4}" => agent_swarm(context, args).await,
        "节点调用" => execute_node_invoke(context, args).await,
        "a2a观察" => a2a_observe(context, args).await,
        "a2a等待" => a2a_wait(context, args).await,
        "a2ui" => Ok(
            json!({"uid": args.get("uid"), "a2ui": args.get("a2ui"), "content": args.get("content")}),
        ),
        "计划面板" => execute_plan_tool(context, args).await,
        "问询面板" => execute_question_panel_tool(context, args).await,
        "定时任务" => {
            let payload: CronActionRequest =
                serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
            let user_tool_manager = context
                .user_tool_manager
                .clone()
                .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
            let user_store = Arc::new(UserStore::new(context.storage.clone()));
            let skills = Arc::new(RwLock::new(context.skills.clone()));
            handle_cron_action(
                context.config.clone(),
                context.storage.clone(),
                context.orchestrator.clone(),
                user_store,
                user_tool_manager,
                skills,
                context.user_id,
                Some(context.session_id),
                context.agent_id,
                payload,
            )
            .await
            .map(compact_cron_tool_result)
        }
        "用户世界工具" => user_world_tool(context, args).await,
        _ => Err(anyhow!("未知内置工具: {canonical}")),
    }
}

fn compact_cron_tool_result(value: Value) -> Value {
    let action = value
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let mut output = json!({ "action": action });
    if let Some(removed) = value.get("removed") {
        output["removed"] = removed.clone();
    }
    if let Some(queued) = value.get("queued") {
        output["queued"] = queued.clone();
    }
    if let Some(reason) = value.get("reason") {
        output["reason"] = reason.clone();
    }
    if let Some(deduped) = value.get("deduped") {
        output["deduped"] = deduped.clone();
    }
    if let Some(job) = value.get("job") {
        output["job"] = compact_cron_job(job);
    }
    if let Some(jobs) = value.get("jobs") {
        output["jobs"] = compact_cron_jobs(jobs);
    }
    output
}

fn compact_cron_jobs(value: &Value) -> Value {
    let Some(items) = value.as_array() else {
        return Value::Array(Vec::new());
    };
    let jobs = items.iter().map(compact_cron_job).collect::<Vec<_>>();
    Value::Array(jobs)
}

fn compact_cron_job(job: &Value) -> Value {
    let schedule = job.get("schedule").and_then(Value::as_object);
    let schedule = json!({
        "kind": schedule.and_then(|map| map.get("kind")).cloned().unwrap_or(Value::Null),
        "at": schedule.and_then(|map| map.get("at")).cloned().unwrap_or(Value::Null),
        "every_ms": schedule.and_then(|map| map.get("every_ms")).cloned().unwrap_or(Value::Null),
        "cron": schedule.and_then(|map| map.get("cron")).cloned().unwrap_or(Value::Null),
        "tz": schedule.and_then(|map| map.get("tz")).cloned().unwrap_or(Value::Null)
    });
    let next_run = job
        .get("next_run_at_text")
        .cloned()
        .or_else(|| job.get("next_run_at").cloned())
        .unwrap_or(Value::Null);
    let last_run = job
        .get("last_run_at_text")
        .cloned()
        .or_else(|| job.get("last_run_at").cloned())
        .unwrap_or(Value::Null);
    json!({
        "job_id": job.get("job_id").cloned().unwrap_or(Value::Null),
        "name": job.get("name").cloned().unwrap_or(Value::Null),
        "enabled": job.get("enabled").cloned().unwrap_or(Value::Null),
        "schedule": schedule,
        "next_run_at": next_run,
        "last_run_at": last_run,
        "last_status": job.get("last_status").cloned().unwrap_or(Value::Null)
    })
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
    let normalized = raw.replace(['-', ' '], "_");
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

#[derive(Debug, Deserialize)]
struct UserWorldToolArgs {
    action: String,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user_ids: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

async fn user_world_tool(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: UserWorldToolArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim().to_lowercase();
    match action.as_str() {
        "list_users" | "list" | "users" => user_world_list_users(context, &payload).await,
        "send_message" | "send" | "message" => user_world_send_message(context, &payload).await,
        _ => Err(anyhow!("未知用户世界工具 action: {action}")),
    }
}

async fn user_world_list_users(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let keyword = payload
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let offset = payload.offset.unwrap_or(0).max(0);
    let limit = payload.limit.unwrap_or(0);
    let limit = if limit <= 0 {
        0
    } else {
        limit.clamp(1, MAX_USER_WORLD_LIST_LIMIT)
    };
    let user_store = UserStore::new(context.storage.clone());
    let (users, total) = user_store.list_users(keyword, None, offset, limit)?;
    let items = users
        .into_iter()
        .map(|user| {
            json!({
                "user_id": user.user_id,
                "username": user.username,
                "status": user.status,
                "unit_id": user.unit_id
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "action": "list_users",
        "items": items,
        "total": total,
        "offset": offset,
        "limit": limit
    }))
}

#[derive(Debug, Clone)]
struct UserWorldFileRefMatch {
    token_start: usize,
    token_end: usize,
    normalized_path: String,
    suffix: String,
}

#[derive(Debug, Clone)]
struct UserWorldCopiedFile {
    source_path: String,
    staged_path: String,
    entry_type: &'static str,
}

fn user_world_file_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?m)(^|[\s\n])@("[^"]+"|'[^']+'|\S+)"#)
            .expect("user world file ref regex must be valid")
    })
}

fn is_user_world_file_ref_suffix(ch: char) -> bool {
    matches!(
        ch,
        ')' | ']'
            | '}'
            | '>'
            | ','
            | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '，'
            | '。'
            | '；'
            | '：'
            | '！'
            | '？'
            | '）'
            | '】'
            | '》'
            | '、'
    )
}

fn split_user_world_file_ref_suffix(value: &str) -> (&str, &str) {
    let mut split_at = value.len();
    for (index, ch) in value.char_indices().rev() {
        if is_user_world_file_ref_suffix(ch) {
            split_at = index;
        } else {
            break;
        }
    }
    if split_at == value.len() {
        (value, "")
    } else {
        (&value[..split_at], &value[split_at..])
    }
}

fn looks_like_user_world_file_ref(raw: &str, normalized: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('@') {
        return false;
    }
    if raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with("workspaces/")
        || raw.starts_with("/workspaces/")
        || raw.starts_with("workspace/")
        || raw.starts_with("/workspace/")
    {
        return true;
    }
    normalized.contains('/') || normalized.contains('.')
}

fn normalize_user_world_file_ref_path(raw: &str, source_workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("/workspace/") {
        value = stripped.trim_matches('/').to_string();
    } else if let Some(stripped) = value.strip_prefix("workspace/") {
        value = stripped.trim_matches('/').to_string();
    }
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    value = value.trim_start_matches('/').trim().to_string();
    if value.is_empty() {
        return None;
    }
    if !looks_like_user_world_file_ref(raw, &value) {
        return None;
    }
    let candidate = Path::new(&value);
    for component in candidate.components() {
        match component {
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(value)
}

fn extract_user_world_file_refs(
    content: &str,
    source_workspace_id: &str,
) -> Vec<UserWorldFileRefMatch> {
    let mut items = Vec::new();
    for captures in user_world_file_ref_regex().captures_iter(content) {
        let Some(token_match) = captures.get(2) else {
            continue;
        };
        let token = token_match.as_str();
        if token.trim().is_empty() {
            continue;
        }
        let (raw_path, suffix) =
            if token.starts_with('"') && token.ends_with('"') && token.len() >= 2 {
                (&token[1..token.len().saturating_sub(1)], "")
            } else if token.starts_with('\'') && token.ends_with('\'') && token.len() >= 2 {
                (&token[1..token.len().saturating_sub(1)], "")
            } else {
                split_user_world_file_ref_suffix(token)
            };
        let Some(normalized_path) =
            normalize_user_world_file_ref_path(raw_path, source_workspace_id)
        else {
            continue;
        };
        items.push(UserWorldFileRefMatch {
            token_start: token_match.start(),
            token_end: token_match.end(),
            normalized_path,
            suffix: suffix.to_string(),
        });
    }
    items
}

fn copy_user_world_staged_path(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in WalkDir::new(source).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                return Err(anyhow!(
                    "symbolic links are not supported in user_world file refs"
                ));
            }
            let relative = entry.path().strip_prefix(source).unwrap_or(entry.path());
            let target = destination.join(relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &target)?;
            }
        }
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn stage_user_world_file_refs(
    context: &ToolContext<'_>,
    content: &str,
) -> Result<(String, Vec<UserWorldCopiedFile>)> {
    let matches = extract_user_world_file_refs(content, context.workspace_id);
    if matches.is_empty() {
        return Ok((content.to_string(), Vec::new()));
    }
    let source_workspace_id = context.workspace_id;
    let sender_workspace_id = context.workspace.scoped_user_id(context.user_id, None);
    let source_root = context.workspace.ensure_user_root(source_workspace_id)?;
    let _ = context.workspace.ensure_user_root(&sender_workspace_id)?;
    let transfer_id = format!(
        "{}_{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        Uuid::new_v4().simple()
    );
    let mut staged_path_map = HashMap::<String, UserWorldCopiedFile>::new();
    for item in &matches {
        if staged_path_map.contains_key(&item.normalized_path) {
            continue;
        }
        let source_target = context
            .workspace
            .resolve_path(source_workspace_id, &item.normalized_path)?;
        if !source_target.exists() {
            return Err(anyhow!(
                "user_world file ref not found in workspace: {}",
                item.normalized_path
            ));
        }
        if !is_within_root(&source_root, &source_target) {
            return Err(anyhow!(
                "user_world file ref is outside workspace: {}",
                item.normalized_path
            ));
        }
        let source_meta = fs::symlink_metadata(&source_target)?;
        if source_meta.file_type().is_symlink() {
            return Err(anyhow!(
                "symbolic links are not supported in user_world file refs: {}",
                item.normalized_path
            ));
        }
        let staged_path = format!(
            "{}/{}/{}",
            USER_WORLD_FILE_STAGING_DIR, transfer_id, item.normalized_path
        );
        let destination = context
            .workspace
            .resolve_path(&sender_workspace_id, &staged_path)?;
        copy_user_world_staged_path(&source_target, &destination)?;
        staged_path_map.insert(
            item.normalized_path.clone(),
            UserWorldCopiedFile {
                source_path: item.normalized_path.clone(),
                staged_path,
                entry_type: if source_target.is_dir() {
                    "dir"
                } else {
                    "file"
                },
            },
        );
    }
    let mut rewritten = String::with_capacity(content.len() + matches.len() * 32);
    let mut cursor = 0usize;
    for item in &matches {
        let Some(staged) = staged_path_map.get(&item.normalized_path) else {
            continue;
        };
        rewritten.push_str(&content[cursor..item.token_start]);
        let quoted_path = staged.staged_path.replace('"', "%22");
        rewritten.push('"');
        rewritten.push_str(&quoted_path);
        rewritten.push('"');
        rewritten.push_str(&item.suffix);
        cursor = item.token_end;
    }
    rewritten.push_str(&content[cursor..]);
    let mut copied = staged_path_map.into_values().collect::<Vec<_>>();
    copied.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok((rewritten, copied))
}

async fn user_world_send_message(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let user_world = context
        .user_world
        .as_ref()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let sender = context.user_id.trim();
    if sender.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let content = payload.content.as_deref().unwrap_or("").trim();
    if content.is_empty() {
        return Err(anyhow!("content is required"));
    }
    let content_type = payload
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("text");
    let mut raw_targets = Vec::new();
    if let Some(user_id) = payload.user_id.as_deref() {
        raw_targets.push(user_id.to_string());
    }
    if let Some(user_ids) = payload.user_ids.as_ref() {
        raw_targets.extend(user_ids.iter().map(|value| value.to_string()));
    }
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_targets {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        return Err(anyhow!("user_id or user_ids required"));
    }
    let user_store = UserStore::new(context.storage.clone());
    let mut target_exists = HashMap::new();
    let mut has_valid_target = false;
    for target in &targets {
        if target == sender {
            continue;
        }
        let exists = user_store.get_user_by_id(target)?.is_some();
        if exists {
            has_valid_target = true;
        }
        target_exists.insert(target.to_string(), exists);
    }
    let (content, copied_files) = if has_valid_target {
        stage_user_world_file_refs(context, content)?
    } else {
        (content.to_string(), Vec::new())
    };
    let mut results = Vec::new();
    for target in targets {
        if target == sender {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "cannot send to self"
            }));
            continue;
        }
        if !target_exists.get(&target).copied().unwrap_or(false) {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "user not found"
            }));
            continue;
        }
        let now = now_ts();
        let conversation =
            match user_world.resolve_or_create_direct_conversation(sender, &target, now) {
                Ok(value) => value,
                Err(err) => {
                    results.push(json!({
                        "user_id": target,
                        "ok": false,
                        "error": err.to_string()
                    }));
                    continue;
                }
            };
        let send_result = match user_world
            .send_message(
                sender,
                &conversation.conversation_id,
                &content,
                content_type,
                payload.client_msg_id.as_deref(),
                now,
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                results.push(json!({
                    "user_id": target,
                    "ok": false,
                    "error": err.to_string()
                }));
                continue;
            }
        };
        results.push(json!({
            "user_id": target,
            "ok": true,
            "conversation_id": conversation.conversation_id,
            "message_id": send_result.message.message_id,
            "inserted": send_result.inserted
        }));
    }
    Ok(json!({
        "action": "send_message",
        "results": results,
        "staged_files": copied_files.iter().map(|item| {
            json!({
                "source_path": item.source_path,
                "staged_path": item.staged_path,
                "entry_type": item.entry_type
            })
        }).collect::<Vec<_>>()
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeInvokeAction {
    List,
    Invoke,
}

#[derive(Debug, Deserialize)]
struct NodeInvokeArgs {
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    node_id: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Value>,
    #[serde(default)]
    timeout_s: Option<f64>,
    #[serde(default)]
    metadata: Option<Value>,
}

async fn execute_node_invoke(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: NodeInvokeArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    match resolve_node_invoke_action(&payload)? {
        NodeInvokeAction::List => execute_node_list(context).await,
        NodeInvokeAction::Invoke => execute_node_invoke_action(context, payload).await,
    }
}

fn resolve_node_invoke_action(payload: &NodeInvokeArgs) -> Result<NodeInvokeAction> {
    if let Some(action) = payload.action.as_deref() {
        let action = action.trim();
        if action.is_empty() {
            return Err(anyhow!("节点调用 action 不能为空"));
        }
        let normalized = action.to_ascii_lowercase();
        return match normalized.as_str() {
            "list" | "ls" | "列表" | "列出" => Ok(NodeInvokeAction::List),
            "invoke" | "call" | "调用" => Ok(NodeInvokeAction::Invoke),
            _ => Err(anyhow!("未知节点调用 action: {action}")),
        };
    }
    let has_node_id = payload
        .node_id
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let has_command = payload
        .command
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if has_node_id && has_command {
        Ok(NodeInvokeAction::Invoke)
    } else {
        Err(anyhow!(
            "节点调用缺少 action，支持 list/invoke；兼容模式下需提供 node_id 与 command"
        ))
    }
}

async fn execute_node_list(context: &ToolContext<'_>) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let snapshot = gateway.snapshot().await;
    let mut nodes = Vec::new();
    for item in snapshot.items {
        if !item.role.eq_ignore_ascii_case("node") {
            continue;
        }
        let Some(node_id) = normalize_optional_string(item.node_id) else {
            continue;
        };
        nodes.push(json!({
            "node_id": node_id,
            "connection_id": item.connection_id,
            "scopes": item.scopes,
            "caps": item.caps,
            "commands": item.commands,
            "connected_at": item.connected_at,
            "last_seen_at": item.last_seen_at,
            "client": item.client
        }));
    }
    nodes.sort_by(|left, right| {
        let left_node = left.get("node_id").and_then(Value::as_str).unwrap_or("");
        let right_node = right.get("node_id").and_then(Value::as_str).unwrap_or("");
        left_node.cmp(right_node).then_with(|| {
            let left_connection = left
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            let right_connection = right
                .get("connection_id")
                .and_then(Value::as_str)
                .unwrap_or("");
            left_connection.cmp(right_connection)
        })
    });
    Ok(json!({
        "action": "list",
        "state_version": snapshot.state_version,
        "count": nodes.len(),
        "nodes": nodes
    }))
}

async fn execute_node_invoke_action(
    context: &ToolContext<'_>,
    payload: NodeInvokeArgs,
) -> Result<Value> {
    let gateway = context
        .gateway
        .clone()
        .ok_or_else(|| anyhow!("gateway not available"))?;
    let node_id = normalize_optional_string(payload.node_id)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 node_id"))?;
    let command = normalize_optional_string(payload.command)
        .ok_or_else(|| anyhow!("节点调用 invoke 需要 command"))?;
    let timeout_s = payload.timeout_s.unwrap_or(30.0);
    let result = gateway
        .invoke_node(GatewayNodeInvokeRequest {
            node_id: node_id.clone(),
            command: command.clone(),
            args: payload.args,
            timeout_s,
            metadata: payload.metadata,
        })
        .await?;
    if result.ok {
        Ok(json!({
            "action": "invoke",
            "node_id": node_id,
            "command": command,
            "result": result.payload
        }))
    } else {
        let message = result
            .error
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("node invoke failed");
        Err(anyhow!(message.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct SessionListArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "activeMinutes", alias = "active_minutes")]
    active_minutes: Option<f64>,
    #[serde(default, rename = "messageLimit", alias = "message_limit")]
    message_limit: Option<i64>,
    #[serde(
        default,
        alias = "parent_id",
        alias = "parentId",
        alias = "parentSessionId"
    )]
    parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionHistoryArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "includeTools", alias = "include_tools")]
    include_tools: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct SessionSendArgs {
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    message: String,
    #[serde(default, rename = "timeoutSeconds", alias = "timeout_seconds")]
    timeout_seconds: Option<f64>,
    #[serde(
        default,
        rename = "announceParentSessionId",
        alias = "announce_parent_session_id"
    )]
    announce_parent_session_id: Option<String>,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionSpawnArgs {
    task: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, rename = "runTimeoutSeconds", alias = "run_timeout_seconds")]
    run_timeout_seconds: Option<f64>,
    #[serde(default)]
    cleanup: Option<String>,
}

#[derive(Debug)]
struct SessionRunOutcome {
    status: String,
    answer: Option<String>,
    error: Option<String>,
    elapsed_s: f64,
}

#[derive(Clone, Copy)]
enum SessionCleanup {
    Keep,
    Delete,
}

#[derive(Clone)]
struct AnnounceConfig {
    parent_session_id: String,
    label: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubagentControlArgs {
    action: String,
}

async fn subagent_control(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SubagentControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim();
    if action.is_empty() {
        return Err(anyhow!("子智能体控制 action 不能为空"));
    }
    let action_lower = action.to_lowercase();
    match action_lower.as_str() {
        "list" | "sessions_list" | "session_list" | "会话列表" | "列表" => {
            sessions_list(context, args).await
        }
        "history" | "sessions_history" | "session_history" | "会话历史" | "历史" => {
            sessions_history(context, args).await
        }
        "send" | "sessions_send" | "session_send" | "会话发送" | "发送" => {
            sessions_send(context, args).await
        }
        "spawn" | "sessions_spawn" | "session_spawn" | "会话派生" | "派生" => {
            sessions_spawn(context, args).await
        }
        _ => Err(anyhow!("未知子智能体控制 action: {action}")),
    }
}

#[derive(Debug, Deserialize)]
struct AgentSwarmControlArgs {
    action: String,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmListArgs {
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "activeMinutes", alias = "active_minutes")]
    active_minutes: Option<f64>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmStatusArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmSendArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    message: String,
    #[serde(default, rename = "timeoutSeconds", alias = "timeout_seconds")]
    timeout_seconds: Option<f64>,
    #[serde(default, rename = "createIfMissing", alias = "create_if_missing")]
    create_if_missing: Option<bool>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmBatchTaskArgs {
    #[serde(default, alias = "agentId", alias = "agent_id")]
    agent_id: Option<String>,
    #[serde(
        default,
        alias = "session_id",
        alias = "sessionId",
        alias = "sessionKey",
        alias = "session_key"
    )]
    session_key: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "createIfMissing", alias = "create_if_missing")]
    create_if_missing: Option<bool>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmBatchSendArgs {
    #[serde(default)]
    tasks: Vec<AgentSwarmBatchTaskArgs>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
    #[serde(default, rename = "createIfMissing", alias = "create_if_missing")]
    create_if_missing: Option<bool>,
    #[serde(default, rename = "includeCurrent", alias = "include_current")]
    include_current: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AgentSwarmWaitArgs {
    #[serde(default, rename = "runIds", alias = "run_ids")]
    run_ids: Option<Vec<String>>,
    #[serde(default, alias = "runId", alias = "run_id")]
    run_id: Option<String>,
    #[serde(default, rename = "waitSeconds", alias = "wait_seconds")]
    wait_seconds: Option<f64>,
    #[serde(
        default,
        rename = "pollIntervalSeconds",
        alias = "poll_interval_seconds"
    )]
    poll_interval_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
struct SwarmRunSnapshot {
    status: String,
    terminal: bool,
    failed: bool,
    payload: Value,
}

#[derive(Debug, Default, Clone)]
struct AgentSwarmRuntime {
    lock_sessions: HashSet<String>,
    running_sessions: HashSet<String>,
}

#[derive(Debug, Clone)]
struct SwarmBatchDispatchTask {
    index: usize,
    message: String,
    label: Option<String>,
    agent_id: String,
    session_id: String,
    created_session: bool,
    tool_names: Vec<String>,
    agent_prompt: Option<String>,
}

fn resolve_swarm_batch_tool_names(
    allowed_tools: &HashSet<String>,
    session: &ChatSessionRecord,
    agent: &UserAgentRecord,
) -> Vec<String> {
    let overrides = resolve_session_tool_overrides(session, Some(agent));
    let filtered = apply_tool_overrides(allowed_tools.clone(), &overrides);
    finalize_tool_names(filtered)
}

async fn dispatch_swarm_batch_task(
    context: &ToolContext<'_>,
    task: SwarmBatchDispatchTask,
) -> Result<Value> {
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: task.message,
        tool_names: task.tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(task.session_id.clone()),
        agent_id: Some(task.agent_id.clone()),
        model_name: None,
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt: task.agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        approval_tx: None,
    };

    let parent_session_id = context.session_id.trim();
    let announce = if parent_session_id.is_empty() || parent_session_id == task.session_id.as_str()
    {
        None
    } else {
        Some(AnnounceConfig {
            parent_session_id: parent_session_id.to_string(),
            label: task.label,
        })
    };

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let _receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(context.session_id.to_string()),
        Some(task.agent_id.clone()),
        None,
        announce,
        SessionCleanup::Keep,
        None,
    )
    .await?;

    Ok(json!({
        "status": "accepted",
        "run_id": run_id,
        "session_id": task.session_id,
        "agent_id": task.agent_id,
        "created_session": task.created_session,
    }))
}

async fn agent_swarm(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmControlArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim();
    if action.is_empty() {
        return Err(anyhow!("agent_swarm action cannot be empty"));
    }
    let action_lower = action.to_lowercase();
    match action_lower.as_str() {
        "list" | "agents_list" | "agent_list" | "swarm_list" => {
            agent_swarm_list(context, args).await
        }
        "status" | "agent_status" | "agents_status" | "swarm_status" => {
            agent_swarm_status(context, args).await
        }
        "send" | "agent_send" | "agents_send" | "swarm_send" => {
            agent_swarm_send(context, args).await
        }
        "batch_send" | "swarm_batch_send" | "agents_batch_send" | "batch" | "fanout"
        | "dispatch" => agent_swarm_batch_send(context, args).await,
        "wait" | "join" | "collect" | "swarm_wait" => agent_swarm_wait(context, args).await,
        "history" | "agent_history" | "agents_history" | "swarm_history" => {
            agent_swarm_history(context, args).await
        }
        "spawn" | "agent_spawn" | "agents_spawn" | "swarm_spawn" => {
            agent_swarm_spawn(context, args).await
        }
        _ => Err(anyhow!("unknown agent_swarm action: {action}")),
    }
}

async fn agent_swarm_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, None)?;
    let cutoff = payload
        .active_minutes
        .filter(|value| *value > 0.0)
        .map(|value| now_ts() - value * 60.0);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let mut items = Vec::new();
    for agent in collect_swarm_agents(context, user_id, include_current, &swarm_hive_id)? {
        let runtime = runtime_map.get(&agent.agent_id);
        let (sessions, session_total) =
            context
                .storage
                .list_chat_sessions(user_id, Some(&agent.agent_id), None, 0, 1)?;
        let latest = sessions.first();
        if let Some(cutoff) = cutoff {
            let latest_updated = latest.map(|record| record.updated_at).unwrap_or(0.0);
            let active_count = merge_swarm_active_sessions(runtime).len();
            if latest_updated < cutoff && active_count == 0 {
                continue;
            }
        }
        let active_session_ids = merge_swarm_active_sessions(runtime);
        let last_status =
            latest.and_then(|record| monitor_session_status(context, &record.session_id));
        items.push(json!({
            "agent_id": agent.agent_id,
            "hive_id": DEFAULT_HIVE_ID,
            "name": agent.name,
            "description": agent.description,
            "status": agent.status,
            "is_shared": agent.is_shared,
            "access_level": agent.access_level,
            "updated_at": format_ts(agent.updated_at),
            "session_total": session_total,
            "active_session_total": active_session_ids.len(),
            "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
            "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
            "active_session_ids": active_session_ids,
            "last_session_id": latest.map(|record| record.session_id.clone()),
            "last_message_at": latest.map(|record| format_ts(record.last_message_at)),
            "last_session_status": last_status,
        }));
        if items.len() as i64 >= limit {
            break;
        }
    }
    Ok(json!({ "total": items.len(), "items": items }))
}

async fn agent_swarm_status(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmStatusArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let Some(agent_id) = normalize_optional_string(payload.agent_id) else {
        return agent_swarm_list(context, args).await;
    };
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, None)?;
    let include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    if !include_current {
        ensure_swarm_target_not_current(&agent_id, current_agent_id.as_deref())?;
    }
    let Some(agent) = load_agent_record(context.storage.as_ref(), user_id, Some(&agent_id), false)?
    else {
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    };
    ensure_swarm_agent_in_hive(&agent, &swarm_hive_id)?;
    let limit = clamp_limit(payload.limit, 20, MAX_SESSION_LIST_ITEMS);
    let runtime_map = collect_swarm_runtime(context, user_id)?;
    let runtime = runtime_map.get(&agent_id);
    let active_session_ids = merge_swarm_active_sessions(runtime);
    let active_set: HashSet<String> = active_session_ids.iter().cloned().collect();
    let (sessions, session_total) =
        context
            .storage
            .list_chat_sessions(user_id, Some(&agent_id), None, 0, limit)?;
    let mut recent_sessions = Vec::with_capacity(sessions.len());
    for record in sessions {
        let status = monitor_session_status(context, &record.session_id);
        recent_sessions.push(json!({
            "session_id": record.session_id,
            "title": record.title,
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "parent_session_id": record.parent_session_id,
            "status": status,
            "active": active_set.contains(&record.session_id),
        }));
    }
    Ok(json!({
        "agent": {
            "agent_id": agent.agent_id,
            "hive_id": DEFAULT_HIVE_ID,
            "name": agent.name,
            "description": agent.description,
            "status": agent.status,
            "is_shared": agent.is_shared,
            "access_level": agent.access_level,
            "updated_at": format_ts(agent.updated_at),
        },
        "session_total": session_total,
        "active_session_total": active_session_ids.len(),
        "running_session_total": runtime.map(|entry| entry.running_sessions.len()).unwrap_or(0),
        "lock_session_total": runtime.map(|entry| entry.lock_sessions.len()).unwrap_or(0),
        "active_session_ids": active_session_ids,
        "recent_sessions": recent_sessions,
    }))
}

async fn agent_swarm_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmSendArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let current_agent_id = current_agent_id(context);
    let include_current = payload.include_current.unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, None)?;
    let requested_agent_id = normalize_optional_string(payload.agent_id);
    let (target_agent_id, target_session_id, created_session) =
        if let Some(session_key) = payload.session_key {
            let session_id = resolve_session_key(Some(session_key))?;
            let record = context
                .storage
                .get_chat_session(user_id, &session_id)?
                .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
            let resolved_agent_id = normalize_optional_string(record.agent_id.clone())
                .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
            if !include_current {
                ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
            }
            if let Some(requested) = requested_agent_id.as_ref() {
                if requested != &resolved_agent_id {
                    return Err(anyhow!(
                        "agent_swarm send agent_id does not match target session"
                    ));
                }
            }
            let target_agent = load_agent_record(
                context.storage.as_ref(),
                user_id,
                Some(&resolved_agent_id),
                false,
            )?
            .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
            ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
            (resolved_agent_id, session_id, false)
        } else {
            let agent_id = requested_agent_id
                .ok_or_else(|| anyhow!("agent_swarm send requires agent_id or session_id"))?;
            ensure_swarm_target_not_current(&agent_id, current_agent_id.as_deref())?;
            let target_agent =
                load_agent_record(context.storage.as_ref(), user_id, Some(&agent_id), false)?
                    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
            ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
            if let Some(record) = context
                .storage
                .list_chat_sessions(user_id, Some(&agent_id), None, 0, 1)?
                .0
                .into_iter()
                .next()
            {
                (agent_id, record.session_id, false)
            } else if payload.create_if_missing.unwrap_or(true) {
                let now = now_ts();
                let session_id = format!("sess_{}", Uuid::new_v4().simple());
                let label = normalize_optional_string(payload.label.clone());
                let title = label
                    .clone()
                    .unwrap_or_else(|| format!("swarm-{}", target_agent.name));
                let parent_session_id = if context.session_id.trim().is_empty() {
                    None
                } else {
                    Some(context.session_id.to_string())
                };
                let record = ChatSessionRecord {
                    session_id: session_id.clone(),
                    user_id: user_id.to_string(),
                    title,
                    created_at: now,
                    updated_at: now,
                    last_message_at: now,
                    agent_id: Some(agent_id.clone()),
                    tool_overrides: target_agent.tool_names.clone(),
                    parent_session_id,
                    parent_message_id: None,
                    spawn_label: label,
                    spawned_by: Some("swarm".to_string()),
                };
                context.storage.upsert_chat_session(&record)?;
                (agent_id, session_id, true)
            } else {
                return Err(anyhow!(
                    "target agent session not found and createIfMissing is false"
                ));
            }
        };

    let mut send_args = json!({
        "session_id": target_session_id,
        "message": message,
    });
    if let Some(timeout_seconds) = payload.timeout_seconds {
        send_args["timeoutSeconds"] = json!(timeout_seconds);
    }
    let parent_session_id = context.session_id.trim();
    if !parent_session_id.is_empty() && parent_session_id != target_session_id {
        send_args["announceParentSessionId"] = json!(parent_session_id);
        if let Some(label) = normalize_optional_string(payload.label.clone()) {
            send_args["label"] = json!(label);
        }
    }
    let mut result = sessions_send(context, &send_args).await?;
    if let Value::Object(ref mut map) = result {
        map.insert("agent_id".to_string(), json!(target_agent_id));
        map.insert("session_id".to_string(), json!(target_session_id));
        map.insert("created_session".to_string(), json!(created_session));
    }
    Ok(result)
}

async fn agent_swarm_batch_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmBatchSendArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    if payload.tasks.is_empty() {
        return Err(anyhow!("agent_swarm batch_send requires non-empty tasks"));
    }

    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }

    let max_tasks = context
        .config
        .tools
        .swarm
        .max_parallel_tasks_per_team
        .max(1);
    if payload.tasks.len() > max_tasks {
        return Err(anyhow!(
            "agent_swarm batch_send task count {} exceeds max_parallel_tasks_per_team {}",
            payload.tasks.len(),
            max_tasks
        ));
    }

    let shared_message = normalize_optional_string(payload.message.clone());
    let shared_label = normalize_optional_string(payload.label.clone());
    let default_create_if_missing = payload.create_if_missing.unwrap_or(true);
    let default_include_current = payload.include_current.unwrap_or(false);
    let current_agent_id = current_agent_id(context);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, None)?;
    let allowed_tools = collect_user_allowed_tools(context, user_id)?;

    let agent_access = context.storage.get_user_agent_access(user_id)?;
    let mut agent_map = HashMap::new();
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, agent_access.as_ref(), &agent) {
            continue;
        }
        ensure_swarm_agent_in_hive(&agent, &swarm_hive_id)?;
        agent_map.entry(agent.agent_id.clone()).or_insert(agent);
    }

    let (sessions, _) = context
        .storage
        .list_chat_sessions(user_id, None, None, 0, 4096)?;
    let mut sessions_by_id = HashMap::with_capacity(sessions.len());
    let mut latest_session_by_agent = HashMap::new();
    for session in sessions {
        sessions_by_id.insert(session.session_id.clone(), session.clone());
        if let Some(agent_id) = normalize_optional_string(session.agent_id.clone()) {
            let should_replace = latest_session_by_agent
                .get(&agent_id)
                .map(|existing: &ChatSessionRecord| session.updated_at > existing.updated_at)
                .unwrap_or(true);
            if should_replace {
                latest_session_by_agent.insert(agent_id, session);
            }
        }
    }

    let mut dispatch_plan = Vec::with_capacity(payload.tasks.len());
    for (index, task) in payload.tasks.into_iter().enumerate() {
        let message = normalize_optional_string(task.message)
            .or_else(|| shared_message.clone())
            .ok_or_else(|| anyhow!("agent_swarm batch_send task[{index}] requires message"))?;
        let label = normalize_optional_string(task.label).or_else(|| shared_label.clone());
        let include_current = task.include_current.unwrap_or(default_include_current);
        let create_if_missing = task.create_if_missing.unwrap_or(default_create_if_missing);
        let requested_agent_id = normalize_optional_string(task.agent_id);
        let requested_session_id = task
            .session_key
            .map(|value| resolve_session_key(Some(value)))
            .transpose()?;

        let (agent_record, session_record, created_session) = if let Some(session_id) =
            requested_session_id
        {
            let session_record = if let Some(record) = sessions_by_id.get(&session_id).cloned() {
                record
            } else {
                context
                    .storage
                    .get_chat_session(user_id, &session_id)?
                    .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?
            };

            let resolved_agent_id = normalize_optional_string(session_record.agent_id.clone())
                .ok_or_else(|| anyhow!("agent_swarm send target session is missing agent_id"))?;
            if !include_current {
                ensure_swarm_target_not_current(&resolved_agent_id, current_agent_id.as_deref())?;
            }
            if let Some(requested) = requested_agent_id.as_ref() {
                if requested != &resolved_agent_id {
                    return Err(anyhow!(
                        "agent_swarm send agent_id does not match target session"
                    ));
                }
            }

            let agent_record = if let Some(agent) = agent_map.get(&resolved_agent_id).cloned() {
                agent
            } else {
                load_agent_record(
                    context.storage.as_ref(),
                    user_id,
                    Some(&resolved_agent_id),
                    false,
                )?
                .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
            };
            ensure_swarm_agent_in_hive(&agent_record, &swarm_hive_id)?;

            sessions_by_id.insert(session_record.session_id.clone(), session_record.clone());
            let should_replace = latest_session_by_agent
                .get(&resolved_agent_id)
                .map(|existing| session_record.updated_at > existing.updated_at)
                .unwrap_or(true);
            if should_replace {
                latest_session_by_agent.insert(resolved_agent_id, session_record.clone());
            }

            (agent_record, session_record, false)
        } else {
            let agent_id = requested_agent_id
                .ok_or_else(|| anyhow!("agent_swarm send requires agent_id or session_id"))?;
            if !include_current {
                ensure_swarm_target_not_current(&agent_id, current_agent_id.as_deref())?;
            }
            let agent_record = if let Some(agent) = agent_map.get(&agent_id).cloned() {
                agent
            } else {
                load_agent_record(context.storage.as_ref(), user_id, Some(&agent_id), false)?
                    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?
            };
            ensure_swarm_agent_in_hive(&agent_record, &swarm_hive_id)?;

            if let Some(existing) = latest_session_by_agent.get(&agent_id).cloned() {
                (agent_record, existing, false)
            } else if create_if_missing {
                let now = now_ts();
                let session_id = format!("sess_{}", Uuid::new_v4().simple());
                let title = label
                    .clone()
                    .unwrap_or_else(|| format!("swarm-{}", agent_record.name));
                let parent_session_id = if context.session_id.trim().is_empty() {
                    None
                } else {
                    Some(context.session_id.to_string())
                };
                let record = ChatSessionRecord {
                    session_id: session_id.clone(),
                    user_id: user_id.to_string(),
                    title,
                    created_at: now,
                    updated_at: now,
                    last_message_at: now,
                    agent_id: Some(agent_id.clone()),
                    tool_overrides: agent_record.tool_names.clone(),
                    parent_session_id,
                    parent_message_id: None,
                    spawn_label: label.clone(),
                    spawned_by: Some("swarm".to_string()),
                };
                context.storage.upsert_chat_session(&record)?;
                sessions_by_id.insert(session_id, record.clone());
                latest_session_by_agent.insert(agent_id, record.clone());
                (agent_record, record, true)
            } else {
                return Err(anyhow!(
                    "target agent session not found and createIfMissing is false"
                ));
            }
        };

        let tool_names =
            resolve_swarm_batch_tool_names(&allowed_tools, &session_record, &agent_record);
        let agent_prompt = {
            let prompt = agent_record.system_prompt.trim();
            if prompt.is_empty() {
                None
            } else {
                Some(prompt.to_string())
            }
        };

        dispatch_plan.push(SwarmBatchDispatchTask {
            index,
            message,
            label,
            agent_id: agent_record.agent_id,
            session_id: session_record.session_id,
            created_session,
            tool_names,
            agent_prompt,
        });
    }

    let dispatch_parallelism = dispatch_plan.len().min(max_tasks).max(1);
    let mut dispatches = stream::iter(dispatch_plan.into_iter().map(|task| async move {
        let index = task.index;
        let result = dispatch_swarm_batch_task(context, task).await;
        (index, result)
    }))
    .buffer_unordered(dispatch_parallelism);

    let mut indexed_items = Vec::new();
    let mut run_ids = Vec::new();
    while let Some((index, result)) = dispatches.next().await {
        match result {
            Ok(result) => {
                let run_id = result
                    .get("run_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_string();
                if !run_id.is_empty() {
                    run_ids.push(run_id.clone());
                }
                let mut item = json!({
                    "index": index,
                    "status": result.get("status").cloned().unwrap_or_else(|| json!("accepted")),
                    "run_id": if run_id.is_empty() { Value::Null } else { json!(run_id) },
                    "agent_id": result.get("agent_id").cloned().unwrap_or(Value::Null),
                    "session_id": result.get("session_id").cloned().unwrap_or(Value::Null),
                    "created_session": result
                        .get("created_session")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
                if let Some(error) = result.get("error") {
                    if let Value::Object(ref mut map) = item {
                        map.insert("error".to_string(), error.clone());
                    }
                }
                indexed_items.push((index, item));
            }
            Err(err) => {
                indexed_items.push((
                    index,
                    json!({
                        "index": index,
                        "status": "error",
                        "error": err.to_string(),
                    }),
                ));
            }
        }
    }

    indexed_items.sort_by_key(|(index, _)| *index);
    let items = indexed_items
        .into_iter()
        .map(|(_, item)| item)
        .collect::<Vec<_>>();
    let run_ids = dedupe_non_empty_strings(run_ids);
    let accepted_total = items
        .iter()
        .filter(|item| {
            item.get("run_id")
                .and_then(Value::as_str)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    let failed_total = items.len().saturating_sub(accepted_total);

    let mut response = json!({
        "status": if accepted_total > 0 {
            if failed_total > 0 { "partial" } else { "accepted" }
        } else {
            "error"
        },
        "task_total": items.len(),
        "accepted_total": accepted_total,
        "failed_total": failed_total,
        "run_ids": run_ids,
        "items": items,
    });

    let wait_seconds = payload.wait_seconds.unwrap_or(0.0).max(0.0);
    if wait_seconds > 0.0 {
        let poll_interval_seconds = payload
            .poll_interval_seconds
            .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
        let wait_result =
            wait_for_swarm_runs(context, &run_ids, wait_seconds, poll_interval_seconds, true)
                .await?;
        if let Value::Object(ref mut map) = response {
            map.insert("wait".to_string(), wait_result.clone());
            if let Some(status) = wait_result.get("status").and_then(Value::as_str) {
                map.insert("status".to_string(), json!(status));
            }
        }
    }

    Ok(response)
}

async fn agent_swarm_wait(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: AgentSwarmWaitArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let mut run_ids = payload.run_ids.unwrap_or_default();
    if let Some(run_id) = payload.run_id {
        run_ids.push(run_id);
    }
    let run_ids = dedupe_non_empty_strings(run_ids);
    if run_ids.is_empty() {
        return Err(anyhow!("agent_swarm wait requires runIds"));
    }
    let wait_seconds = payload
        .wait_seconds
        .unwrap_or(context.config.tools.swarm.default_timeout_s as f64)
        .max(0.0);
    let poll_interval_seconds = payload
        .poll_interval_seconds
        .unwrap_or(SWARM_WAIT_DEFAULT_POLL_S);
    wait_for_swarm_runs(context, &run_ids, wait_seconds, poll_interval_seconds, true).await
}

async fn wait_for_swarm_runs(
    context: &ToolContext<'_>,
    run_ids: &[String],
    wait_seconds: f64,
    poll_interval_seconds: f64,
    emit_progress: bool,
) -> Result<Value> {
    let run_ids = dedupe_non_empty_strings(run_ids.to_vec());
    if run_ids.is_empty() {
        return Ok(json!({
            "status": "error",
            "total": 0,
            "run_ids": [],
            "items": [],
        }));
    }

    let poll_interval = normalize_swarm_poll_interval(poll_interval_seconds);
    let started_at = Instant::now();

    loop {
        let snapshots = collect_swarm_run_snapshots(context, &run_ids)?;
        let total = snapshots.len();
        let done_total = snapshots.iter().filter(|item| item.terminal).count();
        let success_total = snapshots
            .iter()
            .filter(|item| item.status == "success")
            .count();
        let failed_total = snapshots.iter().filter(|item| item.failed).count();
        let queued_total = snapshots
            .iter()
            .filter(|item| item.status == "queued")
            .count();
        let running_total = snapshots
            .iter()
            .filter(|item| item.status == "running")
            .count();
        let elapsed_s = started_at.elapsed().as_secs_f64();
        let all_finished = done_total >= total;
        let timed_out = wait_seconds > 0.0 && elapsed_s >= wait_seconds && !all_finished;

        if all_finished || timed_out || wait_seconds <= 0.0 {
            let status = if all_finished {
                if failed_total == 0 {
                    "ok"
                } else {
                    "partial"
                }
            } else if timed_out {
                "timeout"
            } else {
                "running"
            };
            let items = snapshots
                .into_iter()
                .map(|item| item.payload)
                .collect::<Vec<_>>();
            return Ok(json!({
                "status": status,
                "wait_seconds": wait_seconds,
                "elapsed_s": elapsed_s,
                "all_finished": all_finished,
                "total": total,
                "done_total": done_total,
                "success_total": success_total,
                "failed_total": failed_total,
                "queued_total": queued_total,
                "running_total": running_total,
                "run_ids": run_ids,
                "items": items,
            }));
        }

        if emit_progress {
            if let Some(emitter) = context.event_emitter.as_ref() {
                emitter.emit(
                    "progress",
                    json!({
                        "stage": "swarm_wait",
                        "summary": i18n::t("monitor.summary.swarm_wait"),
                        "total": total,
                        "done_total": done_total,
                        "success_total": success_total,
                        "failed_total": failed_total,
                        "elapsed_s": elapsed_s,
                    }),
                );
            }
        }

        sleep(Duration::from_secs_f64(poll_interval)).await;
    }
}

fn collect_swarm_run_snapshots(
    context: &ToolContext<'_>,
    run_ids: &[String],
) -> Result<Vec<SwarmRunSnapshot>> {
    let mut output = Vec::with_capacity(run_ids.len());
    for run_id in run_ids {
        let record = context.storage.get_session_run(run_id)?;
        if let Some(record) = record {
            let status = record.status.trim().to_ascii_lowercase();
            let terminal = is_swarm_run_terminal(&status);
            let failed = is_swarm_run_failed(&status);
            output.push(SwarmRunSnapshot {
                status,
                terminal,
                failed,
                payload: json!({
                    "run_id": record.run_id,
                    "status": record.status,
                    "terminal": terminal,
                    "failed": failed,
                    "session_id": record.session_id,
                    "parent_session_id": record.parent_session_id,
                    "agent_id": record.agent_id,
                    "model_name": record.model_name,
                    "queued_time": record.queued_time,
                    "started_time": record.started_time,
                    "finished_time": record.finished_time,
                    "elapsed_s": record.elapsed_s,
                    "result": record.result,
                    "error": record.error,
                    "updated_time": record.updated_time,
                }),
            });
        } else {
            output.push(SwarmRunSnapshot {
                status: "not_found".to_string(),
                terminal: true,
                failed: true,
                payload: json!({
                    "run_id": run_id,
                    "status": "not_found",
                    "terminal": true,
                    "failed": true,
                    "error": "run not found",
                }),
            });
        }
    }
    Ok(output)
}

fn is_swarm_run_terminal(status: &str) -> bool {
    matches!(
        status,
        "success" | "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

fn is_swarm_run_failed(status: &str) -> bool {
    matches!(
        status,
        "error" | "timeout" | "cancelled" | "failed" | "not_found"
    )
}

fn normalize_swarm_poll_interval(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return SWARM_WAIT_DEFAULT_POLL_S;
    }
    value.clamp(SWARM_WAIT_MIN_POLL_S, SWARM_WAIT_MAX_POLL_S)
}

fn dedupe_non_empty_strings(items: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in items {
        let cleaned = item.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            output.push(cleaned.to_string());
        }
    }
    output
}

async fn agent_swarm_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key.clone())?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let requested_agent_id = args
        .get("agentId")
        .or_else(|| args.get("agent_id"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let target_agent_id = normalize_optional_string(record.agent_id.clone())
        .ok_or_else(|| anyhow!("agent_swarm history target session is missing agent_id"))?;
    let target_agent = load_agent_record(
        context.storage.as_ref(),
        user_id,
        Some(&target_agent_id),
        false,
    )?
    .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
    if let Some(requested_agent_id) = requested_agent_id {
        if requested_agent_id != target_agent_id {
            return Err(anyhow!(
                "agent_swarm history agent_id does not match target session"
            ));
        }
    }
    if !include_current {
        ensure_swarm_target_not_current(&target_agent_id, current_agent_id(context).as_deref())?;
    }
    sessions_history(context, args).await
}

async fn agent_swarm_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSpawnArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let agent_id = normalize_optional_string(payload.agent_id)
        .ok_or_else(|| anyhow!("agent_swarm spawn requires agent_id"))?;
    let include_current = args
        .get("includeCurrent")
        .or_else(|| args.get("include_current"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let user_id = context.user_id.trim();
    let swarm_hive_id = resolve_swarm_hive_id(context, user_id, swarm_hive_arg(args))?;
    let target_agent =
        load_agent_record(context.storage.as_ref(), user_id, Some(&agent_id), false)?
            .ok_or_else(|| anyhow!(i18n::t("error.agent_not_found")))?;
    ensure_swarm_agent_in_hive(&target_agent, &swarm_hive_id)?;
    if !include_current {
        ensure_swarm_target_not_current(&agent_id, current_agent_id(context).as_deref())?;
    }
    sessions_spawn(context, args).await
}

fn collect_swarm_agents(
    context: &ToolContext<'_>,
    user_id: &str,
    include_current: bool,
    _hive_id: &str,
) -> Result<Vec<UserAgentRecord>> {
    let access = context.storage.get_user_agent_access(user_id)?;
    let current_agent_id = current_agent_id(context);
    let mut agents = context.storage.list_user_agents(user_id)?;
    agents.extend(context.storage.list_shared_user_agents(user_id)?);
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for agent in agents {
        if agent.agent_id.trim().is_empty() {
            continue;
        }
        if !seen.insert(agent.agent_id.clone()) {
            continue;
        }
        if !is_agent_allowed_by_access(user_id, access.as_ref(), &agent) {
            continue;
        }
        if !include_current
            && current_agent_id
                .as_deref()
                .is_some_and(|value| value == agent.agent_id.as_str())
        {
            continue;
        }
        output.push(agent);
    }
    output.sort_by(|a, b| {
        b.updated_at
            .partial_cmp(&a.updated_at)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    Ok(output)
}

fn collect_swarm_runtime(
    context: &ToolContext<'_>,
    user_id: &str,
) -> Result<HashMap<String, AgentSwarmRuntime>> {
    let mut output = HashMap::new();
    for lock in context.storage.list_session_locks_by_user(user_id)? {
        let agent_id = lock.agent_id.trim();
        let session_id = lock.session_id.trim();
        if agent_id.is_empty() || session_id.is_empty() {
            continue;
        }
        output
            .entry(agent_id.to_string())
            .or_insert_with(AgentSwarmRuntime::default)
            .lock_sessions
            .insert(session_id.to_string());
    }
    if let Some(monitor) = context.monitor.as_ref() {
        for session in monitor.list_sessions(true) {
            let session_user_id = session.get("user_id").and_then(Value::as_str).unwrap_or("");
            if session_user_id.trim() != user_id {
                continue;
            }
            let agent_id = session
                .get("agent_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            let session_id = session
                .get("session_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if agent_id.is_empty() || session_id.is_empty() {
                continue;
            }
            output
                .entry(agent_id.to_string())
                .or_insert_with(AgentSwarmRuntime::default)
                .running_sessions
                .insert(session_id.to_string());
        }
    }
    Ok(output)
}

fn merge_swarm_active_sessions(runtime: Option<&AgentSwarmRuntime>) -> Vec<String> {
    let Some(runtime) = runtime else {
        return Vec::new();
    };
    let mut sessions = runtime.lock_sessions.clone();
    sessions.extend(runtime.running_sessions.clone());
    let mut output = sessions.into_iter().collect::<Vec<_>>();
    output.sort();
    output
}

fn monitor_session_status(context: &ToolContext<'_>, session_id: &str) -> Option<String> {
    context
        .monitor
        .as_ref()
        .and_then(|monitor| monitor.get_record(session_id))
        .and_then(|entry| {
            entry
                .get("status")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
}

fn ensure_swarm_target_not_current(
    target_agent_id: &str,
    current_agent_id: Option<&str>,
) -> Result<()> {
    if current_agent_id.is_some_and(|value| value == target_agent_id) {
        return Err(anyhow!(
            "agent_swarm only manages agents other than the current agent"
        ));
    }
    Ok(())
}

fn swarm_hive_arg(args: &Value) -> Option<&str> {
    args.get("hiveId")
        .or_else(|| args.get("hive_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn resolve_swarm_hive_id(
    _context: &ToolContext<'_>,
    _user_id: &str,
    _requested_hive_id: Option<&str>,
) -> Result<String> {
    Ok(DEFAULT_HIVE_ID.to_string())
}

fn ensure_swarm_agent_in_hive(_agent: &UserAgentRecord, _hive_id: &str) -> Result<()> {
    Ok(())
}

async fn sessions_list(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionListArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 50, MAX_SESSION_LIST_ITEMS);
    let message_limit = clamp_limit(payload.message_limit, 0, MAX_SESSION_MESSAGE_ITEMS);
    let parent_id = normalize_optional_string(payload.parent_id);
    let (mut sessions, _) =
        context
            .storage
            .list_chat_sessions(user_id, None, parent_id.as_deref(), 0, limit)?;

    if let Some(active_minutes) = payload.active_minutes.filter(|value| *value > 0.0) {
        let cutoff = now_ts() - active_minutes * 60.0;
        sessions.retain(|record| record.updated_at >= cutoff);
    }
    let total = sessions.len() as i64;
    let mut items = Vec::with_capacity(sessions.len());
    for record in sessions {
        let status = context
            .monitor
            .as_ref()
            .and_then(|monitor| monitor.get_record(&record.session_id))
            .and_then(|entry| {
                entry
                    .get("status")
                    .and_then(Value::as_str)
                    .map(|value| value.to_string())
            });
        let messages = if message_limit > 0 {
            Some(
                load_session_messages(
                    context.workspace.clone(),
                    user_id.to_string(),
                    record.session_id.clone(),
                    message_limit,
                    false,
                )
                .await,
            )
        } else {
            None
        };
        let mut item = json!({
            "session_id": record.session_id,
            "title": record.title,
            "agent_id": record.agent_id,
            "updated_at": format_ts(record.updated_at),
            "last_message_at": format_ts(record.last_message_at),
            "parent_session_id": record.parent_session_id,
            "parent_message_id": record.parent_message_id,
            "spawn_label": record.spawn_label,
            "spawned_by": record.spawned_by,
            "status": status,
        });
        if let Some(messages) = messages {
            if let Value::Object(ref mut map) = item {
                map.insert("messages".to_string(), json!(messages));
            }
        }
        items.push(item);
    }
    Ok(json!({ "total": total, "items": items }))
}

async fn sessions_history(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionHistoryArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key)?;
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let limit = clamp_limit(payload.limit, 200, MAX_SESSION_HISTORY_ITEMS);
    let include_tools = payload.include_tools.unwrap_or(false);
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let messages = load_session_messages(
        context.workspace.clone(),
        user_id.to_string(),
        record.session_id,
        limit,
        include_tools,
    )
    .await;
    Ok(json!({ "session_id": session_id, "messages": messages }))
}

async fn sessions_send(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSendArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let session_id = resolve_session_key(payload.session_key)?;
    let message = payload.message.trim().to_string();
    if message.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let record = context
        .storage
        .get_chat_session(user_id, &session_id)?
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let agent_record = load_agent_record(
        context.storage.as_ref(),
        user_id,
        record.agent_id.as_deref(),
        true,
    )?;
    let tool_names = build_effective_tool_names(context, user_id, &record, agent_record.as_ref())?;
    let agent_prompt = agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());
    let now = now_ts();
    let _ = context
        .storage
        .touch_chat_session(user_id, &session_id, now, now);
    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: message,
        tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(session_id.clone()),
        agent_id: record.agent_id.clone(),
        model_name: None,
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        approval_tx: None,
    };
    let announce_parent_session_id = normalize_optional_string(payload.announce_parent_session_id);
    let announce_label = normalize_optional_string(payload.label);
    let announce = announce_parent_session_id
        .filter(|parent_session_id| parent_session_id != &session_id)
        .map(|parent_session_id| AnnounceConfig {
            parent_session_id,
            label: announce_label,
        });

    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(context.session_id.to_string()),
        record.agent_id.clone(),
        None,
        announce,
        SessionCleanup::Keep,
        None,
    )
    .await?;

    let timeout_seconds = payload.timeout_seconds.unwrap_or(0.0).max(0.0);
    if timeout_seconds <= 0.0 {
        return Ok(json!({
            "status": "accepted",
            "run_id": run_id,
            "session_id": session_id
        }));
    }
    let outcome = timeout(Duration::from_secs_f64(timeout_seconds), receiver).await;
    match outcome {
        Ok(Ok(outcome)) => {
            if outcome.status == "success" {
                Ok(json!({
                    "status": "ok",
                    "run_id": run_id,
                    "reply": outcome.answer.unwrap_or_default(),
                    "elapsed_s": outcome.elapsed_s
                }))
            } else {
                Ok(json!({
                    "status": outcome.status,
                    "run_id": run_id,
                    "error": outcome.error.unwrap_or_else(|| "unknown".to_string()),
                    "elapsed_s": outcome.elapsed_s
                }))
            }
        }
        Ok(Err(err)) => Ok(json!({
            "status": "error",
            "run_id": run_id,
            "error": err.to_string()
        })),
        Err(_) => Ok(json!({
            "status": "timeout",
            "run_id": run_id,
            "error": "timeout"
        })),
    }
}

async fn sessions_spawn(context: &ToolContext<'_>, args: &Value) -> Result<Value> {
    let payload: SessionSpawnArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let task = payload.task.trim().to_string();
    if task.is_empty() {
        return Err(anyhow!(i18n::t("error.content_required")));
    }
    let user_id = context.user_id.trim();
    if user_id.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let parent_session_id = context.session_id.trim().to_string();
    if parent_session_id.is_empty() {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    }
    let label = normalize_optional_string(payload.label);
    let agent_id = normalize_optional_string(payload.agent_id);
    let model_name = normalize_optional_string(payload.model);

    let parent_record = context
        .storage
        .get_chat_session(user_id, &parent_session_id)
        .unwrap_or(None);
    let parent_agent_id = parent_record
        .as_ref()
        .and_then(|record| record.agent_id.clone())
        .or_else(|| context.agent_id.map(|value| value.to_string()));
    let parent_agent_record = load_agent_record(
        context.storage.as_ref(),
        user_id,
        parent_agent_id.as_deref(),
        true,
    )?;
    let parent_tool_names = if let Some(record) = parent_record.as_ref() {
        build_effective_tool_names(context, user_id, record, parent_agent_record.as_ref())?
    } else {
        finalize_tool_names(collect_user_allowed_tools(context, user_id)?)
    };

    let (child_agent_id, child_agent_record) = resolve_child_agent(
        context.storage.as_ref(),
        user_id,
        agent_id.as_deref(),
        parent_agent_id.as_deref(),
    )?;
    let agent_prompt = child_agent_record
        .as_ref()
        .map(|record| record.system_prompt.trim().to_string())
        .filter(|value| !value.is_empty());

    let now = now_ts();
    let child_session_id = format!("sess_{}", Uuid::new_v4().simple());
    let child_record = ChatSessionRecord {
        session_id: child_session_id.clone(),
        user_id: user_id.to_string(),
        title: label
            .clone()
            .unwrap_or_else(|| DEFAULT_SESSION_TITLE.to_string()),
        created_at: now,
        updated_at: now,
        last_message_at: now,
        agent_id: child_agent_id.clone(),
        tool_overrides: parent_tool_names.clone(),
        parent_session_id: Some(parent_session_id.clone()),
        parent_message_id: None,
        spawn_label: label.clone(),
        spawned_by: Some("model".to_string()),
    };
    context.storage.upsert_chat_session(&child_record)?;

    let request = WunderRequest {
        user_id: user_id.to_string(),
        question: task,
        tool_names: parent_tool_names,
        skip_tool_calls: false,
        stream: false,
        debug_payload: false,
        session_id: Some(child_session_id.clone()),
        agent_id: child_agent_id.clone(),
        model_name: model_name.clone(),
        language: Some(i18n::get_language()),
        config_overrides: context.request_config_overrides.cloned(),
        agent_prompt,
        attachments: None,
        allow_queue: true,
        is_admin: context.is_admin,
        approval_tx: None,
    };
    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let cleanup = parse_cleanup_mode(payload.cleanup.as_deref());
    let announce = AnnounceConfig {
        parent_session_id: parent_session_id.clone(),
        label,
    };
    let mut receiver = spawn_session_run(
        context,
        request,
        run_id.clone(),
        Some(parent_session_id),
        child_agent_id,
        model_name,
        Some(announce),
        cleanup,
        payload.run_timeout_seconds,
    )
    .await?;
    let wait_seconds = payload.run_timeout_seconds.unwrap_or(0.0).max(0.0);
    if wait_seconds <= 0.0 {
        return Ok(json!({
            "status": "accepted",
            "run_id": run_id,
            "child_session_id": child_session_id
        }));
    }
    let summary = i18n::t("monitor.summary.subagent_wait");
    let wait_payload = json!({
        "stage": "subagent_wait",
        "summary": summary,
        "run_id": run_id,
        "child_session_id": child_session_id
    });
    if let Some(emitter) = context.event_emitter.as_ref() {
        emitter.emit("progress", wait_payload);
    }
    let start_wait = Instant::now();
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    let deadline = tokio::time::sleep(Duration::from_secs_f64(wait_seconds));
    tokio::pin!(deadline);
    let outcome = loop {
        tokio::select! {
            result = &mut receiver => {
                break Ok(result);
            }
            _ = heartbeat.tick() => {
                if let Some(emitter) = context.event_emitter.as_ref() {
                    emitter.emit("progress", json!({
                        "stage": "subagent_wait",
                        "summary": i18n::t("monitor.summary.subagent_wait"),
                        "run_id": run_id,
                        "child_session_id": child_session_id,
                        "elapsed_s": start_wait.elapsed().as_secs_f64()
                    }));
                }
            }
            _ = &mut deadline => {
                break Err("timeout");
            }
        }
    };
    match outcome {
        Ok(Ok(outcome)) => {
            if outcome.status == "success" {
                Ok(json!({
                    "status": "ok",
                    "run_id": run_id,
                    "child_session_id": child_session_id,
                    "reply": outcome.answer.unwrap_or_default(),
                    "elapsed_s": outcome.elapsed_s
                }))
            } else {
                Ok(json!({
                    "status": outcome.status,
                    "run_id": run_id,
                    "child_session_id": child_session_id,
                    "error": outcome.error.unwrap_or_else(|| "unknown".to_string()),
                    "elapsed_s": outcome.elapsed_s
                }))
            }
        }
        Ok(Err(err)) => Ok(json!({
            "status": "error",
            "run_id": run_id,
            "child_session_id": child_session_id,
            "error": err.to_string()
        })),
        Err(_) => Ok(json!({
            "status": "timeout",
            "run_id": run_id,
            "child_session_id": child_session_id,
            "error": "timeout"
        })),
    }
}

fn resolve_child_agent(
    storage: &dyn StorageBackend,
    user_id: &str,
    requested_agent_id: Option<&str>,
    parent_agent_id: Option<&str>,
) -> Result<(Option<String>, Option<UserAgentRecord>)> {
    let requested = requested_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(requested) = requested {
        if let Some(record) = load_agent_record(storage, user_id, Some(requested), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    let parent = parent_agent_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(parent) = parent {
        if let Some(record) = load_agent_record(storage, user_id, Some(parent), true)? {
            return Ok((Some(record.agent_id.clone()), Some(record)));
        }
    }
    Ok((None, None))
}

fn session_run_runtime() -> &'static tokio::runtime::Runtime {
    static SESSION_RUN_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    SESSION_RUN_RUNTIME.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get().clamp(8, 128))
            .unwrap_or(16);
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(threads)
            .max_blocking_threads(1024)
            .thread_name("wunder-session-run")
            .enable_all()
            .build()
            .expect("build session run runtime")
    })
}

#[allow(clippy::too_many_arguments)]
async fn spawn_session_run(
    context: &ToolContext<'_>,
    request: WunderRequest,
    run_id: String,
    parent_session_id: Option<String>,
    agent_id: Option<String>,
    model_name: Option<String>,
    announce: Option<AnnounceConfig>,
    cleanup: SessionCleanup,
    run_timeout_s: Option<f64>,
) -> Result<oneshot::Receiver<SessionRunOutcome>> {
    let orchestrator = context
        .orchestrator
        .clone()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let session_id = request
        .session_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!(i18n::t("error.session_not_found")))?;
    let user_id = request.user_id.clone();
    let now = now_ts();
    let record = SessionRunRecord {
        run_id: run_id.clone(),
        session_id: session_id.clone(),
        parent_session_id: parent_session_id.clone(),
        user_id: user_id.clone(),
        agent_id: agent_id.clone(),
        model_name: model_name.clone(),
        status: "queued".to_string(),
        queued_time: now,
        started_time: 0.0,
        finished_time: 0.0,
        elapsed_s: 0.0,
        result: None,
        error: None,
        updated_time: now,
    };
    {
        let queued_storage = context.storage.clone();
        let queued_record = record.clone();
        tokio::task::spawn_blocking(move || queued_storage.upsert_session_run(&queued_record))
            .await
            .map_err(|err| anyhow!(err.to_string()))??;
    }

    let storage = context.storage.clone();
    let workspace = context.workspace.clone();
    let monitor = context.monitor.clone();
    let (tx, rx) = oneshot::channel::<SessionRunOutcome>();
    tokio::spawn(async move {
        let started = now_ts();
        let running = SessionRunRecord {
            status: "running".to_string(),
            started_time: started,
            updated_time: started,
            ..record.clone()
        };
        {
            let storage_for_start = storage.clone();
            let user_for_start = user_id.clone();
            let session_for_start = session_id.clone();
            let running_for_start = running.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = storage_for_start.touch_chat_session(
                    &user_for_start,
                    &session_for_start,
                    started,
                    started,
                );
                let _ = storage_for_start.upsert_session_run(&running_for_start);
            })
            .await;
        }

        // Use a dedicated runtime so high fan-out runs do not contend with the main runtime worker pool.
        let mut run_handle = tokio::task::spawn_blocking(move || {
            session_run_runtime().block_on(orchestrator.run(request))
        });
        let mut timeout_triggered = false;
        let run_result = if let Some(timeout_s) = run_timeout_s.filter(|value| *value > 0.0) {
            let timeout_duration = Duration::from_secs_f64(timeout_s);
            tokio::select! {
                res = &mut run_handle => match res {
                    Ok(value) => value,
                    Err(err) => Err(anyhow!(err.to_string())),
                },
                _ = sleep(timeout_duration) => {
                    timeout_triggered = true;
                    run_handle.abort();
                    if let Some(monitor) = monitor.as_ref() {
                        let _ = monitor.cancel(&session_id);
                    }
                    Err(anyhow!("timeout"))
                }
            }
        } else {
            match run_handle.await {
                Ok(value) => value,
                Err(err) => Err(anyhow!(err.to_string())),
            }
        };
        let finished = now_ts();
        let elapsed = (finished - started).max(0.0);
        let (status, answer, error) = match run_result {
            Ok(response) => {
                let answer = truncate_text(&response.answer, SESSION_RESULT_MAX_CHARS);
                ("success".to_string(), Some(answer), None)
            }
            Err(err) => {
                if timeout_triggered {
                    ("timeout".to_string(), None, Some("timeout".to_string()))
                } else {
                    ("error".to_string(), None, Some(err.to_string()))
                }
            }
        };
        let finished_record = SessionRunRecord {
            status: status.clone(),
            finished_time: finished,
            elapsed_s: elapsed,
            result: answer.clone(),
            error: error.clone(),
            updated_time: finished,
            ..running
        };
        {
            let storage_for_finish = storage.clone();
            let finished_for_write = finished_record.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = storage_for_finish.upsert_session_run(&finished_for_write);
            })
            .await;
        }

        if let Some(announce) = announce {
            if !should_skip_announce(answer.as_deref()) {
                append_child_announce(
                    &workspace,
                    &storage,
                    &user_id,
                    &announce.parent_session_id,
                    &session_id,
                    &run_id,
                    &status,
                    answer.as_deref(),
                    error.as_deref(),
                    elapsed,
                    model_name.as_deref(),
                    announce.label.as_deref(),
                );
            }
        }
        if matches!(cleanup, SessionCleanup::Delete) {
            cleanup_session(
                &storage,
                &workspace,
                monitor.as_ref(),
                &user_id,
                &session_id,
            );
        }

        let _ = tx.send(SessionRunOutcome {
            status,
            answer,
            error,
            elapsed_s: elapsed,
        });
    });
    Ok(rx)
}

fn parse_cleanup_mode(value: Option<&str>) -> SessionCleanup {
    match value.unwrap_or("").trim().to_lowercase().as_str() {
        "delete" | "remove" => SessionCleanup::Delete,
        _ => SessionCleanup::Keep,
    }
}

fn cleanup_session(
    storage: &Arc<dyn StorageBackend>,
    workspace: &WorkspaceManager,
    monitor: Option<&Arc<MonitorState>>,
    user_id: &str,
    session_id: &str,
) {
    workspace.purge_session_data(user_id, session_id);
    let _ = storage.delete_chat_session(user_id, session_id);
    if let Some(monitor) = monitor {
        let _ = monitor.purge_session(session_id);
    }
}

#[allow(clippy::too_many_arguments)]
fn append_child_announce(
    workspace: &WorkspaceManager,
    storage: &Arc<dyn StorageBackend>,
    user_id: &str,
    parent_session_id: &str,
    child_session_id: &str,
    run_id: &str,
    status: &str,
    answer: Option<&str>,
    error: Option<&str>,
    elapsed_s: f64,
    model_name: Option<&str>,
    label: Option<&str>,
) {
    let result_text = if status == "success" {
        answer.unwrap_or("ok").trim()
    } else {
        error.unwrap_or("error").trim()
    };
    let mut notes = vec![
        format!("run_id={run_id}"),
        format!("session_id={child_session_id}"),
        format!("elapsed_s={:.2}", elapsed_s),
    ];
    if let Some(model) = model_name {
        if !model.trim().is_empty() {
            notes.push(format!("model={}", model.trim()));
        }
    }
    if let Some(label) = label {
        if !label.trim().is_empty() {
            notes.push(format!("label={}", label.trim()));
        }
    }
    let content = format!(
        "Status: {status}\nResult: {result}\nNotes: {notes}",
        status = status,
        result = result_text,
        notes = notes.join(", ")
    );
    let timestamp = Local::now().to_rfc3339();
    let meta = json!({
        "type": "subagent_announce",
        "run_id": run_id,
        "child_session_id": child_session_id,
        "status": status,
        "elapsed_s": elapsed_s
    });
    let payload = json!({
        "role": "assistant",
        "content": content,
        "session_id": parent_session_id,
        "timestamp": timestamp,
        "meta": meta,
    });
    let _ = workspace.append_chat(user_id, &payload);
    let now = now_ts();
    let _ = storage.touch_chat_session(user_id, parent_session_id, now, now);
}

fn should_skip_announce(answer: Option<&str>) -> bool {
    answer
        .map(|value| value.trim() == ANNOUNCE_SKIP)
        .unwrap_or(false)
}

async fn load_session_messages(
    workspace: Arc<WorkspaceManager>,
    user_id: String,
    session_id: String,
    limit: i64,
    include_tools: bool,
) -> Vec<Value> {
    tokio::task::spawn_blocking(move || {
        if include_tools {
            let history = workspace
                .load_history(&user_id, &session_id, limit)
                .unwrap_or_default();
            history
                .into_iter()
                .filter(|item| item.get("role").and_then(Value::as_str) != Some("system"))
                .collect()
        } else {
            let manager = HistoryManager;
            manager.load_history_messages(&workspace, &user_id, &session_id, limit)
        }
    })
    .await
    .unwrap_or_default()
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn current_agent_id(context: &ToolContext<'_>) -> Option<String> {
    context
        .agent_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_session_key(value: Option<String>) -> Result<String> {
    let Some(key) = normalize_optional_string(value) else {
        return Err(anyhow!(i18n::t("error.session_not_found")));
    };
    Ok(key)
}

fn clamp_limit(value: Option<i64>, default: i64, max: i64) -> i64 {
    value.unwrap_or(default).max(0).min(max)
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn format_ts(ts: f64) -> String {
    let millis = (ts * 1000.0) as i64;
    chrono::DateTime::<Utc>::from_timestamp_millis(millis)
        .map(|dt| dt.with_timezone(&Local).to_rfc3339())
        .unwrap_or_default()
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut output = trimmed.chars().take(max_chars).collect::<String>();
    output.push_str("...");
    output
}

fn collect_user_allowed_tools(context: &ToolContext<'_>, user_id: &str) -> Result<HashSet<String>> {
    let mut allowed =
        collect_available_tool_names(context.config, context.skills, context.user_tool_bindings);
    let access = context.storage.get_user_tool_access(user_id)?;
    if let Some(access) = access {
        if let Some(allowed_tools) = access.allowed_tools.as_ref() {
            let allowed_set: HashSet<String> = allowed_tools
                .iter()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
            allowed = allowed
                .intersection(&allowed_set)
                .cloned()
                .collect::<HashSet<_>>();
        }
    }
    Ok(allowed)
}

fn normalize_tool_overrides(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    let mut has_none = false;
    for raw in values {
        let name = raw.trim().to_string();
        if name.is_empty() || seen.contains(&name) {
            continue;
        }
        if name == TOOL_OVERRIDE_NONE {
            has_none = true;
        }
        seen.insert(name.clone());
        output.push(name);
    }
    if has_none {
        vec![TOOL_OVERRIDE_NONE.to_string()]
    } else {
        output
    }
}

fn resolve_session_tool_overrides(
    record: &ChatSessionRecord,
    agent: Option<&UserAgentRecord>,
) -> Vec<String> {
    if !record.tool_overrides.is_empty() {
        normalize_tool_overrides(record.tool_overrides.clone())
    } else {
        agent
            .map(|record| record.tool_names.clone())
            .unwrap_or_default()
    }
}

fn apply_tool_overrides(allowed: HashSet<String>, overrides: &[String]) -> HashSet<String> {
    if overrides.is_empty() {
        return allowed;
    }
    if overrides.iter().any(|name| name == TOOL_OVERRIDE_NONE) {
        return HashSet::new();
    }
    let override_set: HashSet<String> = overrides
        .iter()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();
    allowed
        .intersection(&override_set)
        .cloned()
        .collect::<HashSet<_>>()
}

fn finalize_tool_names(mut allowed: HashSet<String>) -> Vec<String> {
    if allowed.is_empty() {
        return vec![TOOL_OVERRIDE_NONE.to_string()];
    }
    let mut list = allowed.drain().collect::<Vec<_>>();
    list.sort();
    list
}

fn build_effective_tool_names(
    context: &ToolContext<'_>,
    user_id: &str,
    record: &ChatSessionRecord,
    agent: Option<&UserAgentRecord>,
) -> Result<Vec<String>> {
    let allowed = collect_user_allowed_tools(context, user_id)?;
    let overrides = resolve_session_tool_overrides(record, agent);
    let allowed = apply_tool_overrides(allowed, &overrides);
    Ok(finalize_tool_names(allowed))
}

fn is_agent_allowed_by_access(
    user_id: &str,
    access: Option<&UserAgentAccessRecord>,
    agent: &UserAgentRecord,
) -> bool {
    if agent.user_id != user_id && !agent.is_shared {
        return false;
    }
    if let Some(access) = access {
        if !access.blocked_agent_ids.is_empty()
            && access
                .blocked_agent_ids
                .iter()
                .any(|id| id == &agent.agent_id)
        {
            return false;
        }
        if let Some(allowed) = access.allowed_agent_ids.as_ref() {
            return allowed.iter().any(|id| id == &agent.agent_id);
        }
    }
    true
}

fn load_agent_record(
    storage: &dyn StorageBackend,
    user_id: &str,
    agent_id: Option<&str>,
    allow_missing: bool,
) -> Result<Option<UserAgentRecord>> {
    let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let record = storage.get_user_agent_by_id(agent_id)?;
    let Some(record) = record else {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    };
    let access = storage.get_user_agent_access(user_id)?;
    if !is_agent_allowed_by_access(user_id, access.as_ref(), &record) {
        if allow_missing {
            return Ok(None);
        }
        return Err(anyhow!(i18n::t("error.agent_not_found")));
    }
    Ok(Some(record))
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
        .as_ref()
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
    let multi_query = queries.len() > 1;
    let mut seen_chunks = HashSet::new();
    for (_, keyword, hits) in aggregated {
        let documents = hits
            .into_iter()
            .filter_map(|hit| {
                if multi_query {
                    let key = format!("{}::{}", hit.doc_id, hit.chunk_index);
                    if !seen_chunks.insert(key) {
                        return None;
                    }
                }
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
                if multi_query {
                    doc["keyword"] = json!(keyword);
                }
                Some(doc)
            })
            .collect::<Vec<_>>();
        if multi_query {
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
    let value = args.get("limit")?;
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
        pending_text.push_str(decode_command_output(&pending_bytes).as_str());
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

fn decode_command_output(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }

    #[cfg(windows)]
    {
        if looks_like_utf16_output(bytes) {
            if let Some(text) = decode_utf16_output(bytes) {
                return text;
            }
        }
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let utf8_lossy = String::from_utf8_lossy(bytes).to_string();

    #[cfg(windows)]
    {
        let (decoded, _, _) = GBK.decode(bytes);
        let gbk_text = decoded.into_owned();
        if should_prefer_decoded_text(&gbk_text, &utf8_lossy) {
            return gbk_text;
        }

        if let Some(text) = decode_utf16_output(bytes) {
            if should_prefer_decoded_text(&text, &utf8_lossy) {
                return text;
            }
        }
    }

    utf8_lossy
}

#[cfg(windows)]
fn looks_like_utf16_output(bytes: &[u8]) -> bool {
    if bytes.len() < 4 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return true;
    }

    let odd_bytes = bytes.len() / 2;
    if odd_bytes == 0 {
        return false;
    }

    let zero_odd = bytes
        .iter()
        .skip(1)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    zero_odd * 100 >= odd_bytes * 20
}

#[cfg(windows)]
fn should_prefer_decoded_text(candidate: &str, fallback: &str) -> bool {
    if candidate.trim().is_empty() {
        return false;
    }

    let candidate_replacement = candidate.chars().filter(|ch| *ch == '\u{FFFD}').count();
    let fallback_replacement = fallback.chars().filter(|ch| *ch == '\u{FFFD}').count();

    if candidate_replacement < fallback_replacement {
        return true;
    }
    if candidate_replacement > fallback_replacement {
        return false;
    }

    fallback_replacement > 0 && contains_cjk(candidate) && !contains_cjk(fallback)
}

#[cfg(windows)]
fn contains_cjk(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch,
            '\u{3400}'..='\u{4DBF}'
                | '\u{4E00}'..='\u{9FFF}'
                | '\u{F900}'..='\u{FAFF}'
                | '\u{20000}'..='\u{2A6DF}'
                | '\u{2A700}'..='\u{2B73F}'
                | '\u{2B740}'..='\u{2B81F}'
                | '\u{2B820}'..='\u{2CEAF}'
        )
    })
}

#[cfg(windows)]
fn decode_utf16_output(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 || !bytes.len().is_multiple_of(2) {
        return None;
    }

    let (is_big_endian, start) = if bytes.starts_with(&[0xFE, 0xFF]) {
        (true, 2)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        (false, 2)
    } else {
        (false, 0)
    };

    let payload = &bytes[start..];
    if payload.is_empty() || !payload.len().is_multiple_of(2) {
        return None;
    }

    let units = payload
        .chunks_exact(2)
        .map(|chunk| {
            if is_big_endian {
                u16::from_be_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_le_bytes([chunk[0], chunk[1]])
            }
        })
        .collect::<Vec<_>>();
    let text = String::from_utf16(&units).ok()?;
    if text.is_empty() {
        None
    } else {
        Some(text.trim_matches('\u{FEFF}').to_string())
    }
}

async fn run_spawned_child_streaming(
    context: &ToolContext<'_>,
    mut child: tokio::process::Child,
    tool_name: &str,
    command_text: &str,
    timeout: Option<Duration>,
) -> Result<CommandRunResult> {
    let chunk_size = resolve_stream_chunk_size(context.config);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let stdout_task = stdout.map(|stdout| {
        let emitter = context.event_emitter.clone();
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
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
        let tool_name = tool_name.to_string();
        let command_text = command_text.to_string();
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
    let stdout = decode_command_output(&stdout_bytes);
    let stderr = decode_command_output(&stderr_bytes);
    let returncode = status.and_then(|value| value.code()).unwrap_or(-1);

    Ok(CommandRunResult {
        returncode,
        stdout,
        stderr,
        timed_out,
    })
}

async fn run_command_streaming(
    context: &ToolContext<'_>,
    command: &str,
    cwd: &Path,
    timeout: Option<Duration>,
    tool_name: &str,
) -> Result<CommandRunResult> {
    let command_text = command.to_string();
    let (mut cmd, used_direct) =
        if let Some(cmd) = command_utils::build_direct_command(command, cwd) {
            (cmd, true)
        } else {
            (command_utils::build_shell_command(command, cwd), false)
        };
    cmd.kill_on_drop(true);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) if used_direct && command_utils::is_not_found_error(&err) => {
            let mut cmd = command_utils::build_shell_command(command, cwd);
            cmd.kill_on_drop(true);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            cmd.spawn()?
        }
        Err(err) => return Err(anyhow!(err)),
    };
    run_spawned_child_streaming(context, child, tool_name, &command_text, timeout).await
}

async fn run_ptc_python_script_streaming(
    context: &ToolContext<'_>,
    script_path: &Path,
    workdir: &Path,
    timeout: Option<Duration>,
) -> Result<CommandRunResult> {
    #[cfg(windows)]
    let candidates: &[(&str, &[&str])] = &[("py", &["-3"]), ("python", &[]), ("python3", &[])];
    #[cfg(not(windows))]
    let candidates: &[(&str, &[&str])] = &[("python3", &[]), ("python", &[])];

    let tool_name = resolve_tool_name("ptc");
    let script_text = script_path.to_string_lossy().to_string();
    let mut last_error: Option<anyhow::Error> = None;
    let mut tried = Vec::new();

    if let Some(runtime) = python_runtime::resolve_python_runtime() {
        let program = runtime.bin.to_string_lossy().to_string();
        tried.push(program.clone());
        let mut cmd = tokio::process::Command::new(&program);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        cmd.env("PYTHONIOENCODING", "utf-8");
        python_runtime::apply_python_env(&mut cmd, &runtime);
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let command_text = format!("{program} {script_text}");
        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => {}
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
            }
        }
    }
    for (program, prefix_args) in candidates {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(*prefix_args);
        cmd.arg(script_path);
        cmd.current_dir(workdir);
        cmd.env("PYTHONIOENCODING", "utf-8");
        cmd.kill_on_drop(true);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut parts = Vec::new();
        parts.push((*program).to_string());
        parts.extend(prefix_args.iter().map(|value| (*value).to_string()));
        parts.push(script_text.clone());
        let command_text = parts.join(" ");
        tried.push((*program).to_string());

        match cmd.spawn() {
            Ok(child) => {
                return run_spawned_child_streaming(
                    context,
                    child,
                    &tool_name,
                    &command_text,
                    timeout,
                )
                .await;
            }
            Err(err) if command_utils::is_not_found_error(&err) => continue,
            Err(err) => {
                let detail = format!("{program}: {err}");
                last_error = Some(anyhow!(detail));
                break;
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| anyhow!("python interpreter not found (tried: {})", tried.join(", "))))
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
    let execute_tool_name = resolve_tool_name("execute_command");
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
        let run =
            run_command_streaming(context, command, &cwd, timeout, &execute_tool_name).await?;
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

fn normalize_ptc_script_name(raw_filename: &str) -> std::result::Result<PathBuf, &'static str> {
    let filename = raw_filename.trim();
    if filename.is_empty() {
        return Err("tool.ptc.filename_required");
    }

    let mut script_name = PathBuf::from(filename);
    if script_name.file_name().and_then(|name| name.to_str()) != Some(filename) {
        return Err("tool.ptc.filename_invalid");
    }
    if script_name.extension().is_none() {
        script_name.set_extension("py");
    }
    if !script_name
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("py"))
        .unwrap_or(false)
    {
        return Err("tool.ptc.ext_invalid");
    }

    Ok(script_name)
}

fn build_ptc_exec_error(detail: impl Into<String>) -> Value {
    json!({
        "ok": false,
        "data": {},
        "error": i18n::t_with_params(
            "tool.ptc.exec_error",
            &HashMap::from([("detail".to_string(), detail.into())]),
        ),
        "sandbox": false,
    })
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
        .unwrap_or_default();
    let script_name = match normalize_ptc_script_name(filename) {
        Ok(name) => name,
        Err(key) => {
            return Ok(json!({
                "ok": false,
                "data": {},
                "error": i18n::t(key),
                "sandbox": false,
            }));
        }
    };

    let workdir = args.get("workdir").and_then(Value::as_str).unwrap_or("");
    let content = args.get("content").and_then(Value::as_str).unwrap_or("");
    if content.trim().is_empty() {
        return Ok(json!({
            "ok": false,
            "data": {},
            "error": i18n::t("tool.ptc.content_required"),
            "sandbox": false,
        }));
    }

    let content = context
        .workspace
        .replace_public_root_in_text(context.workspace_id, content);
    let workdir_path = if workdir.is_empty() {
        if context
            .config
            .server
            .mode
            .trim()
            .eq_ignore_ascii_case("cli")
        {
            let configured_root = context.config.workspace.root.trim();
            if configured_root.is_empty() {
                context.workspace.ensure_user_root(context.workspace_id)?
            } else {
                PathBuf::from(configured_root)
            }
        } else {
            context.workspace.ensure_user_root(context.workspace_id)?
        }
    } else {
        context
            .workspace
            .resolve_path(context.workspace_id, workdir)?
    };

    if let Err(err) = tokio::fs::create_dir_all(&workdir_path).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let ptc_root = context
        .workspace
        .resolve_path(context.workspace_id, LOCAL_PTC_DIR_NAME)?;
    if let Err(err) = tokio::fs::create_dir_all(&ptc_root).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let script_path = ptc_root.join(script_name);
    if let Err(err) = tokio::fs::write(&script_path, content).await {
        return Ok(build_ptc_exec_error(err.to_string()));
    }

    let output = match run_ptc_python_script_streaming(
        context,
        &script_path,
        &workdir_path,
        Some(Duration::from_secs(LOCAL_PTC_TIMEOUT_S)),
    )
    .await
    {
        Ok(output) => output,
        Err(err) => return Ok(build_ptc_exec_error(err.to_string())),
    };

    let data = json!({
        "path": context
            .workspace
            .display_path(context.workspace_id, &script_path),
        "workdir": context
            .workspace
            .display_path(context.workspace_id, &workdir_path),
        "returncode": output.returncode,
        "stdout": output.stdout,
        "stderr": output.stderr,
    });

    context.workspace.mark_tree_dirty(context.workspace_id);

    if output.timed_out {
        let detail = format!("timeout after {}s", LOCAL_PTC_TIMEOUT_S);
        return Ok(json!({
            "ok": false,
            "data": data,
            "error": i18n::t_with_params(
                "tool.ptc.exec_error",
                &HashMap::from([("detail".to_string(), detail)]),
            ),
            "sandbox": false,
        }));
    }

    if output.returncode != 0 {
        return Ok(json!({
            "ok": false,
            "data": data,
            "error": i18n::t("tool.ptc.exec_failed"),
            "sandbox": false,
        }));
    }

    Ok(json!({
        "ok": true,
        "data": data,
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

#[allow(clippy::too_many_arguments)]
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
    let mut specs = Vec::new();

    if let Some(files) = args.get("files").and_then(Value::as_array) {
        for file in files.iter().take(MAX_READ_FILES) {
            let Some(obj) = file.as_object() else {
                continue;
            };
            if let Some(spec) = parse_read_file_spec_object(obj) {
                specs.push(spec);
            }
        }
    }

    if specs.is_empty() {
        if let Some(obj) = args.as_object() {
            if let Some(spec) = parse_read_file_spec_object(obj) {
                specs.push(spec);
            }
        } else if let Some(path) = args
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            specs.push(ReadFileSpec {
                path: path.to_string(),
                ranges: vec![(1, MAX_READ_LINES)],
            });
        }
    }

    if specs.is_empty() {
        return Err(i18n::t("tool.read.no_path"));
    }
    Ok(specs)
}

fn parse_read_file_spec_object(obj: &serde_json::Map<String, Value>) -> Option<ReadFileSpec> {
    let path = normalize_read_path_hint(
        obj.get("path")
            .or_else(|| obj.get("file_path"))
            .or_else(|| obj.get("file"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string(),
    );
    if path.is_empty() {
        return None;
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
            let Some(start) = pair.first().and_then(parse_line_number) else {
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

    Some(ReadFileSpec { path, ranges })
}

fn normalize_read_path_hint(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    for prefix in ["/workspaces/", "\\workspaces\\", "/workspace/"] {
        if let Some(value) = trimmed.strip_prefix(prefix) {
            let candidate = value.trim_matches(|ch| matches!(ch, '/' | '\\')).trim();
            if !candidate.is_empty() {
                return candidate.to_string();
            }
        }
    }

    trimmed.to_string()
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
            for (idx, line) in lines.iter().enumerate().take(last).skip(start - 1) {
                slice_lines.push(format!("{}: {}", idx + 1, line));
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
    let bytes = content.len();
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
        .filter(|&item| !item.is_done())
        .cloned()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_user_world_file_refs_handles_quotes_suffix_and_email_mentions() {
        let content =
            "查看 @./docs/report.md, 以及 @\"assets/my file.txt\"，并抄送 @alice@example.com";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].normalized_path, "docs/report.md");
        assert_eq!(refs[0].suffix, ",");
        assert_eq!(refs[1].normalized_path, "assets/my file.txt");
        assert_eq!(refs[1].suffix, "");
    }

    #[test]
    fn extract_user_world_file_refs_accepts_workspace_prefixed_token() {
        let content = "@/workspaces/owner__c__2/projects/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].normalized_path, "projects/demo.md");
    }

    #[test]
    fn extract_user_world_file_refs_ignores_mismatched_workspace_owner() {
        let content = "@/workspaces/another_owner/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert!(refs.is_empty());
    }

    #[test]
    fn parse_read_file_specs_accepts_shorthand_path_payload() {
        let specs = parse_read_file_specs(&json!({
            "path": "Cargo.toml",
            "start_line": 2,
            "end_line": 5,
        }))
        .expect("shorthand path should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "Cargo.toml");
        assert_eq!(specs[0].ranges, vec![(2, 5)]);
    }

    #[test]
    fn parse_read_file_specs_accepts_file_path_alias() {
        let specs = parse_read_file_specs(&json!({
            "file_path": "README.md",
        }))
        .expect("file_path alias should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "README.md");
        assert_eq!(specs[0].ranges, vec![(1, MAX_READ_LINES)]);
    }

    #[test]
    fn parse_read_file_specs_normalizes_workspace_prefixed_path() {
        let specs = parse_read_file_specs(&json!({
            "path": "/workspaces/Cargo.toml",
        }))
        .expect("workspace-prefixed path should parse");

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].path, "Cargo.toml");
    }

    #[test]
    fn normalize_ptc_script_name_accepts_simple_filename() {
        let script = normalize_ptc_script_name("demo").expect("filename should be normalized");
        assert_eq!(script, PathBuf::from("demo.py"));
    }

    #[test]
    fn normalize_ptc_script_name_rejects_path_segments() {
        let error = normalize_ptc_script_name("nested/demo.py").expect_err("path must be rejected");
        assert_eq!(error, "tool.ptc.filename_invalid");
    }

    #[test]
    fn normalize_ptc_script_name_rejects_non_python_extension() {
        let error = normalize_ptc_script_name("demo.txt").expect_err("non-python ext should fail");
        assert_eq!(error, "tool.ptc.ext_invalid");
    }

    #[cfg(windows)]
    #[test]
    fn decode_command_output_prefers_gbk_when_utf8_lossy_contains_replacements() {
        let expected = "\u{65e0}\u{6cd5}\u{5c06} pip \u{8bc6}\u{522b}\u{4e3a} cmdlet";
        let (encoded, _, _) = GBK.encode(expected);
        let decoded = decode_command_output(encoded.as_ref());
        assert!(decoded.contains("\u{65e0}\u{6cd5}\u{5c06}"));
        assert!(decoded.contains("cmdlet"));
    }

    #[cfg(windows)]
    #[test]
    fn decode_command_output_handles_utf16_le_streams() {
        let expected = "PowerShell output";
        let utf16_bytes = expected
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect::<Vec<_>>();
        let decoded = decode_command_output(&utf16_bytes);
        assert_eq!(decoded, expected);
    }
}
