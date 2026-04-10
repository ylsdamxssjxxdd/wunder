use super::{
    browser_tool, channel_tool, desktop_control, read_image_tool, self_status_tool,
    sessions_yield_tool, sleep_tool, thread_control_tool, web_fetch_tool,
};
use crate::config::Config;
use crate::core::json_schema::normalize_tool_input_schema;
use crate::i18n;
use crate::schemas::ToolSpec;
use crate::skills::SkillRegistry;
use crate::user_tools::UserToolBindings;
use anyhow::Result;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub(crate) fn builtin_tool_specs_with_language(language: &str) -> Vec<ToolSpec> {
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
                "required": ["content"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "a2ui".to_string(),
            description: t("tool.spec.a2ui.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "uid": {"type": "string", "description": t("tool.spec.a2ui.args.uid")},
                    "a2ui": {
                        "type": "array",
                        "minItems": 1,
                        "description": t("tool.spec.a2ui.args.messages"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "beginRendering": {"type": "object"},
                                "surfaceUpdate": {"type": "object"},
                                "dataModelUpdate": {"type": "object"},
                                "deleteSurface": {"type": "object"}
                            },
                            "additionalProperties": false
                        }
                    },
                    "content": {"type": "string", "description": t("tool.spec.a2ui.args.content")}
                },
                "required": ["a2ui"],
                "additionalProperties": false
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
                        "minItems": 1,
                        "maxItems": 12,
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
                            "required": ["step", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["plan"],
                "additionalProperties": false
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
                        "minItems": 1,
                        "maxItems": 4,
                        "description": t("tool.spec.question_panel.args.routes"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string", "description": t("tool.spec.question_panel.args.routes.label")},
                                "description": {"type": "string", "description": t("tool.spec.question_panel.args.routes.description")},
                                "recommended": {"type": "boolean", "description": t("tool.spec.question_panel.args.routes.recommended")}
                            },
                            "required": ["label"],
                            "additionalProperties": false
                        }
                    },
                    "multiple": {"type": "boolean", "description": t("tool.spec.question_panel.args.multiple")}
                },
                "required": ["routes"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: sessions_yield_tool::TOOL_SESSIONS_YIELD.to_string(),
            description: t("tool.spec.sessions_yield.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": t("tool.spec.sessions_yield.args.message")
                    }
                },
                "additionalProperties": false
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
                        "enum": ["add", "update", "remove", "enable", "disable", "get", "list", "run", "status"]
                    },
                    "job_id": {"type": "string", "description": t("tool.spec.schedule_task.args.job.job_id")},
                    "name": {"type": "string", "description": t("tool.spec.schedule_task.args.job.name")},
                    "schedule": {
                        "type": "object",
                        "properties": {
                            "kind": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.kind"), "enum": ["at", "every", "cron"]},
                            "at": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.at")},
                            "every_ms": {"type": "integer", "description": t("tool.spec.schedule_task.args.job.schedule.every_ms"), "minimum": 1000},
                            "cron": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.cron")},
                            "tz": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule.tz")}
                        },
                        "required": ["kind"],
                        "additionalProperties": false
                    },
                    "schedule_text": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule_text")},
                    "session": {"type": "string", "description": t("tool.spec.schedule_task.args.job.session"), "enum": ["main", "isolated"]},
                    "message": {"type": "string", "description": t("tool.spec.schedule_task.args.job.payload.message")},
                    "enabled": {"type": "boolean", "description": t("tool.spec.schedule_task.args.job.enabled")},
                    "delete_after_run": {"type": "boolean", "description": t("tool.spec.schedule_task.args.job.delete_after_run")},
                    "dedupe_key": {"type": "string", "description": t("tool.spec.schedule_task.args.job.dedupe_key")}
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: sleep_tool::TOOL_SLEEP_WAIT.to_string(),
            description: t("tool.spec.sleep.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "seconds": {"type": "number", "description": t("tool.spec.sleep.args.seconds"), "minimum": 0.001},
                    "reason": {"type": "string", "description": t("tool.spec.sleep.args.reason")}
                },
                "required": ["seconds"],
                "additionalProperties": false
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
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: channel_tool::TOOL_CHANNEL.to_string(),
            description: t("tool.spec.channel_tool.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.channel_tool.args.action"),
                        "enum": ["list_contacts", "send_message"]
                    },
                    "channel": {"type": "string", "description": t("tool.spec.channel_tool.args.channel")},
                    "account_id": {"type": "string", "description": t("tool.spec.channel_tool.args.account_id")},
                    "keyword": {"type": "string", "description": t("tool.spec.channel_tool.args.keyword")},
                    "offset": {"type": "integer", "minimum": 0, "description": t("tool.spec.channel_tool.args.offset")},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": t("tool.spec.channel_tool.args.limit")},
                    "contact": {
                        "type": "object",
                        "description": t("tool.spec.channel_tool.args.contact"),
                        "properties": {
                            "channel": {"type": "string"},
                            "account_id": {"type": "string"},
                            "to": {"type": "string"},
                            "peer_kind": {"type": "string", "enum": ["user", "group"]},
                            "thread_id": {"type": "string"}
                        },
                        "additionalProperties": false
                    },
                    "to": {"type": "string", "description": t("tool.spec.channel_tool.args.to")},
                    "peer_kind": {"type": "string", "enum": ["user", "group"], "description": t("tool.spec.channel_tool.args.peer_kind")},
                    "thread_id": {"type": "string", "description": t("tool.spec.channel_tool.args.thread_id")},
                    "content": {"type": "string", "description": t("tool.spec.channel_tool.args.content")},
                    "attachments": {
                        "type": "array",
                        "description": t("tool.spec.channel_tool.args.attachments"),
                        "items": {
                            "type": "object",
                            "properties": {
                                "kind": {"type": "string", "description": t("tool.spec.channel_tool.args.attachments.kind")},
                                "url": {"type": "string", "description": t("tool.spec.channel_tool.args.attachments.url")},
                                "mime": {"type": "string", "description": t("tool.spec.channel_tool.args.attachments.mime")},
                                "size": {"type": "integer", "description": t("tool.spec.channel_tool.args.attachments.size")},
                                "name": {"type": "string", "description": t("tool.spec.channel_tool.args.attachments.name")}
                            },
                            "additionalProperties": false
                        }
                    },
                    "wait": {"type": "boolean", "description": t("tool.spec.channel_tool.args.wait")},
                    "wait_timeout_s": {"type": "number", "minimum": 1, "maximum": 30, "description": t("tool.spec.channel_tool.args.wait_timeout_s")}
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "记忆管理".to_string(),
            description: t("tool.spec.memory_manager.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.memory_manager.args.action"),
                        "enum": ["list", "add", "update", "delete", "clear", "recall"]
                    },
                    "memory_id": {"type": "string", "description": t("tool.spec.memory_manager.args.memory_id")},
                    "title": {"type": "string", "description": t("tool.spec.memory_manager.args.title")},
                    "summary": {"type": "string", "description": t("tool.spec.memory_manager.args.summary")},
                    "content": {"type": "string", "description": t("tool.spec.memory_manager.args.content")},
                    "category": {"type": "string", "description": t("tool.spec.memory_manager.args.category")},
                    "tags": {"type": "array", "items": {"type": "string"}, "description": t("tool.spec.memory_manager.args.tags")},
                    "query": {"type": "string", "description": t("tool.spec.memory_manager.args.query")},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 200, "description": t("tool.spec.memory_manager.args.limit")},
                    "order": {
                        "type": "string",
                        "description": t("tool.spec.memory_manager.args.order"),
                        "enum": ["desc", "asc"]
                    }
                },
                "required": ["action"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "a2a观察".to_string(),
            description: t("tool.spec.a2a_observe.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_ids": {"type": "array", "items": {"type": "string"}, "description": t("tool.spec.a2a_observe.args.task_ids")},
                    "endpoint": {"type": "string", "description": t("tool.spec.a2a_observe.args.endpoint")},
                    "service_name": {"type": "string", "description": t("tool.spec.a2a_observe.args.service_name")},
                    "refresh": {"type": "boolean", "description": t("tool.spec.a2a_observe.args.refresh")},
                    "timeout_s": {"type": "number", "description": t("tool.spec.a2a_observe.args.timeout")}
                },
                "additionalProperties": false
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
                    "endpoint": {"type": "string", "description": t("tool.spec.a2a_wait.args.endpoint")},
                    "service_name": {"type": "string", "description": t("tool.spec.a2a_wait.args.service_name")},
                    "refresh": {"type": "boolean", "description": t("tool.spec.a2a_wait.args.refresh")},
                    "timeout_s": {"type": "number", "description": t("tool.spec.a2a_wait.args.timeout")}
                },
                "additionalProperties": false
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
                    "timeout_s": {"type": "number", "description": t("tool.spec.exec.args.timeout")},
                    "dry_run": {"type": "boolean", "description": "Validate command only without execution."}
                },
                "required": ["content"],
                "additionalProperties": false
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
                "required": ["filename", "content"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "列出文件".to_string(),
            description: t("tool.spec.list.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.list.args.path")},
                    "max_depth": {"type": "integer", "minimum": 0},
                    "cursor": {"type": "string", "description": t("tool.spec.list.args.cursor")},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": t("tool.spec.list.args.limit")}
                },
                "additionalProperties": false
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
                    "glob": {"type": "string", "description": t("tool.spec.search.args.glob")},
                    "query_mode": {"type": "string", "enum": ["literal", "regex"], "description": t("tool.spec.search.args.query_mode")},
                    "case_sensitive": {"type": "boolean", "description": t("tool.spec.search.args.case_sensitive")},
                    "max_matches": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Maximum number of matches to return (default 200)."},
                    "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 120000, "description": "Search timeout in milliseconds (default 30000)."},
                    "context": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context")},
                    "context_before": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context_before")},
                    "context_after": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context_after")}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "读取文件".to_string(),
            description: t("tool.spec.read.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "description": "Resolve targets and metadata without reading full file content."},
                    "path": {"type": "string", "description": t("tool.spec.read.args.files.path")},
                    "start_line": {"type": "integer", "description": t("tool.spec.read.args.files.start_line")},
                    "end_line": {"type": "integer", "description": t("tool.spec.read.args.files.end_line")},
                    "line_ranges": {"type": "array", "items": {"type": "array", "items": {"type": "integer"}, "minItems": 2}},
                    "mode": {"type": "string", "enum": ["slice", "indentation"], "description": "Read mode: slice ranges or indentation-aware block."},
                    "indentation": {
                        "type": "object",
                        "properties": {
                            "anchor_line": {"type": "integer", "minimum": 1},
                            "max_levels": {"type": "integer", "minimum": 0},
                            "include_siblings": {"type": "boolean"},
                            "include_header": {"type": "boolean"},
                            "max_lines": {"type": "integer", "minimum": 1}
                        },
                        "additionalProperties": false
                    }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: read_image_tool::TOOL_READ_IMAGE.to_string(),
            description: t("tool.spec.read_image.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.read_image.args.path")},
                    "prompt": {"type": "string", "description": t("tool.spec.read_image.args.prompt")}
                },
                "required": ["path"],
                "additionalProperties": false
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
                "required": ["name"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "写入文件".to_string(),
            description: t("tool.spec.write.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": t("tool.spec.write.args.path")},
                    "content": {"type": "string", "description": t("tool.spec.write.args.content")},
                    "dry_run": {"type": "boolean", "description": "Preview write target and size changes without writing to disk."}
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: "应用补丁".to_string(),
            description: t("tool.spec.apply_patch.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": t("tool.spec.apply_patch.args.input")},
                    "dry_run": {"type": "boolean", "description": "Parse and resolve patch targets without writing files."}
                },
                "required": ["input"]
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
                            "document_symbol",
                            "workspace_symbol",
                            "implementation",
                            "call_hierarchy"
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
                "required": ["operation", "path"],
                "additionalProperties": false
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
                        "description": format!("{} {}", t("tool.spec.subagent_control.args.action"), "Important: spawn already sends the first turn. Do not call send immediately after spawn unless you are deliberately continuing the same child conversation."),
                        "enum": ["list", "history", "send", "spawn", "batch_spawn", "status", "wait", "interrupt", "close", "resume"]
                    },
                    "limit": {"type": "integer", "description": t("tool.spec.sessions_list.args.limit"), "minimum": 1},
                    "active_minutes": {"type": "number", "description": t("tool.spec.sessions_list.args.active_minutes"), "minimum": 0},
                    "message_limit": {"type": "integer", "description": t("tool.spec.sessions_list.args.message_limit"), "minimum": 0},
                    "parent_id": {"type": "string", "description": "Parent session id. list/status/wait default to the current session when omitted."},
                    "session_id": {"type": "string", "description": "Exact child session id. Prefer the session_id returned by spawn. send/history require exactly one child session."},
                    "session_ids": {"type": "array", "description": "Child session ids under the current session. status/wait may use multiple targets; send/history must resolve to exactly one child session.", "items": {"type": "string"}},
                    "run_id": {"type": "string", "description": "Child run id. send/history may use a single run_id to resolve the child session; status/wait may inspect by run_id directly."},
                    "run_ids": {"type": "array", "description": "Child run ids for status/wait or multi-target inspection.", "items": {"type": "string"}},
                    "dispatch_id": {"type": "string", "description": "Dispatch id returned by batch_spawn."},
                    "strategy": {
                        "type": "string",
                        "description": "Batch dispatch strategy. first_success aligns with Codex-style early convergence.",
                        "enum": ["parallel_all", "first_success", "review_then_merge"]
                    },
                    "remaining_action": {
                        "type": "string",
                        "description": "How to handle unfinished sibling subagents after early convergence. first_success defaults to interrupt; wait keeps siblings unless specified.",
                        "enum": ["keep", "interrupt", "close"]
                    },
                    "include_tools": {"type": "boolean", "description": t("tool.spec.sessions_history.args.include_tools")},
                    "message": {"type": "string", "description": "Message content for a follow-up turn on an existing child session. Do not use action=send immediately after spawn unless you are continuing the same child conversation."},
                    "timeout_seconds": {"type": "number", "description": t("tool.spec.sessions_send.args.timeout")},
                    "task": {"type": "string", "description": "Initial task or first prompt for the child session. action=spawn/batch_spawn dispatches this task immediately and starts the first child turn; do not repeat the same content with send unless you intentionally want a follow-up turn."},
                    "tasks": {
                        "type": "array",
                        "description": "Batch child tasks.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task": {"type": "string", "description": t("tool.spec.sessions_spawn.args.task")},
                                "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                                "agent_id": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                                "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                                "run_timeout_seconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                                "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]}
                            },
                            "required": ["task"]
                        }
                    },
                    "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                    "agent_id": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                    "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                    "run_timeout_seconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                    "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]},
                    "wait_seconds": {"type": "number", "description": "Wait time for batch/status polling."},
                    "poll_interval_seconds": {"type": "number", "description": "Polling interval for wait."},
                    "wait_mode": {
                        "type": "string",
                        "description": "Wait completion mode for subagent wait. all waits every target, any returns on first terminal target, first_success returns on the first success or when all targets finish.",
                        "enum": ["all", "any", "first_success"]
                    },
                    "dispatch_label": {"type": "string", "description": "Optional label for the dispatch batch."},
                    "cascade": {"type": "boolean", "description": "Apply close/resume recursively to descendants."}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: thread_control_tool::TOOL_THREAD_CONTROL.to_string(),
            description: t("tool.spec.thread_control.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.thread_control.args.action"),
                        "enum": [
                            "list",
                            "info",
                            "create",
                            "switch",
                            "back",
                            "update_title",
                            "archive",
                            "restore",
                            "set_main"
                        ]
                    },
                    "session_id": {"type": "string", "description": t("tool.spec.thread_control.args.session_id")},
                    "parent_session_id": {"type": "string", "description": t("tool.spec.thread_control.args.parent_session_id")},
                    "title": {"type": "string", "description": t("tool.spec.thread_control.args.title")},
                    "scope": {
                        "type": "string",
                        "description": t("tool.spec.thread_control.args.scope"),
                        "enum": ["branch", "children", "roots", "all"]
                    },
                    "status": {
                        "type": "string",
                        "description": t("tool.spec.thread_control.args.status"),
                        "enum": ["active", "archived", "all"]
                    },
                    "limit": {"type": "integer", "description": t("tool.spec.thread_control.args.limit"), "minimum": 1, "maximum": 200},
                    "set_main": {"type": "boolean", "description": t("tool.spec.thread_control.args.set_main")}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: "智能体蜂群".to_string(),
            description: "蜂群协作工具，只管理已存在的其他智能体。使用 canonical 字段 agent_id/agent_name/session_id/run_ids。spawn 必须提供 agent_id 或 agent_name；临时子会话请改用子智能体控制。".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "send(发单目标)/batch_send(并发)/wait(等结果)/status(看状态)/history(看会话)/spawn(派生到已存在智能体)/list(列成员)。必须显式提供，不要留空。",
                        "enum": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
                    },
                    "agent_id": {"type": "string", "description": "目标智能体 ID。"},
                    "agent_name": {"type": "string", "description": "目标智能体名称。"},
                    "session_id": {"type": "string", "description": "目标会话 ID。"},
                    "message": {"type": "string", "description": "消息内容。", "minLength": 1},
                    "task": {"type": "string", "description": "任务描述。spawn 仅在已提供 agent_id/agent_name 时有效；临时子会话请用 subagent_control.spawn。", "minLength": 1},
                    "limit": {"type": "integer", "description": "Maximum number of items to return for list/status.", "minimum": 1},
                    "wait_seconds": {"type": "number", "description": "Optional wait duration in seconds for wait/batch_send."},
                    "poll_interval_seconds": {"type": "number", "description": "Polling interval in seconds while waiting."},
                    "include_current": {"type": "boolean", "description": "Whether the current agent can also be selected as a target."},
                    "tasks": {
                        "type": "array",
                        "description": "batch_send 任务列表。每个 task 都必须指定一个目标(agent_id/agent_name/session_id 之一)；message 建议每个 task 都显式填写，不要传空对象 {}。",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "agent_id": {"type": "string", "description": "目标智能体 ID。"},
                                "agent_name": {"type": "string", "description": "目标智能体名称。"},
                                "session_id": {"type": "string", "description": "目标会话 ID。"},
                                "message": {"type": "string", "description": "任务消息。", "minLength": 1}
                            },
                            "required": ["message"]
                        }
                    },
                    "run_ids": {"type": "array", "description": t("tool.spec.agent_swarm.args.run_ids"), "items": {"type": "string"}, "minItems": 1}
                },
                "required": ["action"],
                "examples": [
                    {"action": "send", "agent_name": "worker_a", "message": "请完成指定任务。"},
                    {"action": "batch_send", "tasks": [{"agent_name": "worker_a", "message": "请完成任务 A。"}, {"agent_name": "worker_b", "message": "请完成任务 B。"}]},
                    {"action": "wait", "run_ids": ["run_demo_1"]}
                ]
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
                    "timeout_s": {"type": "number", "description": t("tool.spec.node_invoke.args.timeout")}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: web_fetch_tool::TOOL_WEB_FETCH.to_string(),
            description: t("tool.spec.web_fetch.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": t("tool.spec.web_fetch.args.url") },
                    "extract_mode": {
                        "type": "string",
                        "description": t("tool.spec.web_fetch.args.extract_mode"),
                        "enum": ["markdown", "text"]
                    },
                    "max_chars": {
                        "type": "integer",
                        "minimum": 100,
                        "description": t("tool.spec.web_fetch.args.max_chars")
                    }
                },
                "required": ["url"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: browser_tool::TOOL_BROWSER.to_string(),
            description: t("tool.spec.browser.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.browser.args.action"),
                        "enum": [
                            "status",
                            "profiles",
                            "start",
                            "stop",
                            "tabs",
                            "open",
                            "focus",
                            "close",
                            "navigate",
                            "snapshot",
                            "click",
                            "type",
                            "press",
                            "hover",
                            "wait",
                            "screenshot",
                            "read_page"
                        ]
                    },
                    "profile": { "type": "string", "description": t("tool.spec.browser.args.profile") },
                    "browser_session_id": { "type": "string", "description": t("tool.spec.browser.args.browser_session_id") },
                    "target_id": { "type": "string", "description": t("tool.spec.browser.args.target_id") },
                    "url": { "type": "string", "description": t("tool.spec.browser.args.url") },
                    "format": { "type": "string", "description": t("tool.spec.browser.args.format") },
                    "ref": { "type": "string", "description": t("tool.spec.browser.args.ref") },
                    "selector": { "type": "string", "description": t("tool.spec.browser.args.selector") },
                    "text": { "type": "string", "description": t("tool.spec.browser.args.text") },
                    "key": { "type": "string", "description": t("tool.spec.browser.args.key") },
                    "full_page": { "type": "boolean", "description": t("tool.spec.browser.args.full_page") },
                    "max_chars": { "type": "integer", "minimum": 1, "description": t("tool.spec.browser.args.max_chars") }
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: desktop_control::TOOL_DESKTOP_CONTROLLER.to_string(),
            description: t("tool.spec.desktop_controller.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "bbox": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "anyOf": [
                            {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4},
                            {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}
                        ],
                        "description": t("tool.spec.desktop_controller.args.bbox")
                    },
                    "action": {
                        "type": "string",
                        "description": t("tool.spec.desktop_controller.args.action"),
                        "enum": [
                            "left_click",
                            "left_double_click",
                            "right_click",
                            "middle_click",
                            "left_hold",
                            "right_hold",
                            "middle_hold",
                            "left_release",
                            "right_release",
                            "middle_release",
                            "scroll_down",
                            "scroll_up",
                            "press_key",
                            "type_text",
                            "delay",
                            "move_mouse",
                            "drag_drop"
                        ]
                    },
                    "key": {"type": "string", "description": t("tool.spec.desktop_controller.args.key")},
                    "text": {"type": "string", "description": t("tool.spec.desktop_controller.args.text")},
                    "delay_ms": {"type": "integer", "minimum": 0, "description": t("tool.spec.desktop_controller.args.delay_ms")},
                    "duration_ms": {"type": "integer", "minimum": 0, "description": t("tool.spec.desktop_controller.args.duration_ms")},
                    "scroll_steps": {"type": "integer", "minimum": 1, "description": t("tool.spec.desktop_controller.args.scroll_steps")},
                    "to_bbox": {
                        "type": "array",
                        "items": {"type": "integer"},
                        "anyOf": [
                            {"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4},
                            {"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}
                        ],
                        "description": t("tool.spec.desktop_controller.args.to_bbox")
                    }
                },
                "required": ["bbox", "action"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: desktop_control::TOOL_DESKTOP_MONITOR.to_string(),
            description: t("tool.spec.desktop_monitor.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "wait_ms": {"type": "integer", "minimum": 0, "maximum": 30000, "description": t("tool.spec.desktop_monitor.args.wait_ms")},
                    "note": {"type": "string", "description": t("tool.spec.desktop_monitor.args.note")}
                },
                "required": ["wait_ms"],
                "additionalProperties": false
            }),
        },
        ToolSpec {
            name: self_status_tool::TOOL_SELF_STATUS.to_string(),
            description: t("tool.spec.self_status.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "detail_level": {
                        "type": "string",
                        "description": t("tool.spec.self_status.args.detail_level"),
                        "enum": ["basic", "standard", "full"]
                    },
                    "include_events": {
                        "type": "boolean",
                        "description": t("tool.spec.self_status.args.include_events")
                    },
                    "events_limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": t("tool.spec.self_status.args.events_limit")
                    },
                    "include_system_metrics": {
                        "type": "boolean",
                        "description": t("tool.spec.self_status.args.include_system_metrics")
                    }
                },
                "additionalProperties": false
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
    map.insert(
        self_status_tool::TOOL_SELF_STATUS_ALIAS.to_string(),
        self_status_tool::TOOL_SELF_STATUS.to_string(),
    );
    map.insert(
        sessions_yield_tool::TOOL_SESSIONS_YIELD_ALIAS.to_string(),
        sessions_yield_tool::TOOL_SESSIONS_YIELD.to_string(),
    );
    map.insert(
        sessions_yield_tool::TOOL_SESSIONS_YIELD_ALIAS_ALT.to_string(),
        sessions_yield_tool::TOOL_SESSIONS_YIELD.to_string(),
    );
    map.insert("final_response".to_string(), "最终回复".to_string());
    map.insert("update_plan".to_string(), "计划面板".to_string());
    map.insert("question_panel".to_string(), "问询面板".to_string());
    map.insert("ask_panel".to_string(), "问询面板".to_string());
    map.insert("schedule_task".to_string(), "定时任务".to_string());
    map.insert(
        sleep_tool::TOOL_SLEEP_ALIAS.to_string(),
        sleep_tool::TOOL_SLEEP_WAIT.to_string(),
    );
    map.insert(
        sleep_tool::TOOL_SLEEP_WAIT_ALIAS.to_string(),
        sleep_tool::TOOL_SLEEP_WAIT.to_string(),
    );
    map.insert(
        sleep_tool::TOOL_SLEEP_PAUSE_ALIAS.to_string(),
        sleep_tool::TOOL_SLEEP_WAIT.to_string(),
    );
    map.insert("user_world".to_string(), "用户世界工具".to_string());
    map.insert(
        "channel_tool".to_string(),
        channel_tool::TOOL_CHANNEL.to_string(),
    );
    map.insert(
        "channel_send".to_string(),
        channel_tool::TOOL_CHANNEL.to_string(),
    );
    map.insert(
        "channel_contacts".to_string(),
        channel_tool::TOOL_CHANNEL.to_string(),
    );
    map.insert("memory_manager".to_string(), "记忆管理".to_string());
    map.insert("memory_manage".to_string(), "记忆管理".to_string());
    map.insert("a2a_observe".to_string(), "a2a观察".to_string());
    map.insert("a2a_wait".to_string(), "a2a等待".to_string());
    map.insert("execute_command".to_string(), "执行命令".to_string());
    map.insert("programmatic_tool_call".to_string(), "ptc".to_string());
    map.insert("list_files".to_string(), "列出文件".to_string());
    map.insert("search_content".to_string(), "搜索内容".to_string());
    map.insert("read_file".to_string(), "读取文件".to_string());
    map.insert(
        read_image_tool::TOOL_READ_IMAGE_ALIAS.to_string(),
        read_image_tool::TOOL_READ_IMAGE.to_string(),
    );
    map.insert(
        read_image_tool::TOOL_VIEW_IMAGE_ALIAS.to_string(),
        read_image_tool::TOOL_READ_IMAGE.to_string(),
    );
    map.insert("skill_call".to_string(), "技能调用".to_string());
    map.insert("skill_get".to_string(), "技能调用".to_string());
    map.insert("write_file".to_string(), "写入文件".to_string());
    map.insert("apply_patch".to_string(), "应用补丁".to_string());
    map.insert("lsp".to_string(), "LSP查询".to_string());
    map.insert("subagent_control".to_string(), "子智能体控制".to_string());
    map.insert(
        thread_control_tool::TOOL_THREAD_CONTROL_ALIAS.to_string(),
        thread_control_tool::TOOL_THREAD_CONTROL.to_string(),
    );
    map.insert(
        thread_control_tool::TOOL_THREAD_CONTROL_ALIAS_ALT.to_string(),
        thread_control_tool::TOOL_THREAD_CONTROL.to_string(),
    );
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
    map.insert(
        web_fetch_tool::TOOL_WEB_FETCH_ALIAS.to_string(),
        web_fetch_tool::TOOL_WEB_FETCH.to_string(),
    );
    map.insert(
        "browser".to_string(),
        browser_tool::TOOL_BROWSER.to_string(),
    );
    map.insert(
        "browser_tool".to_string(),
        browser_tool::TOOL_BROWSER.to_string(),
    );
    map.insert(
        desktop_control::TOOL_DESKTOP_CONTROLLER_ALIAS.to_string(),
        desktop_control::TOOL_DESKTOP_CONTROLLER.to_string(),
    );
    map.insert(
        desktop_control::TOOL_DESKTOP_CONTROLLER_ALIAS_SHORT.to_string(),
        desktop_control::TOOL_DESKTOP_CONTROLLER.to_string(),
    );
    map.insert(
        desktop_control::TOOL_DESKTOP_MONITOR_ALIAS.to_string(),
        desktop_control::TOOL_DESKTOP_MONITOR.to_string(),
    );
    map.insert(
        desktop_control::TOOL_DESKTOP_MONITOR_ALIAS_SHORT.to_string(),
        desktop_control::TOOL_DESKTOP_MONITOR.to_string(),
    );
    map.insert(
        "browser_navigate".to_string(),
        browser_tool::TOOL_BROWSER_NAVIGATE.to_string(),
    );
    map.insert(
        "browser_click".to_string(),
        browser_tool::TOOL_BROWSER_CLICK.to_string(),
    );
    map.insert(
        "browser_type".to_string(),
        browser_tool::TOOL_BROWSER_TYPE.to_string(),
    );
    map.insert(
        "browser_screenshot".to_string(),
        browser_tool::TOOL_BROWSER_SCREENSHOT.to_string(),
    );
    map.insert(
        "browser_read_page".to_string(),
        browser_tool::TOOL_BROWSER_READ_PAGE.to_string(),
    );
    map.insert(
        "browser_close".to_string(),
        browser_tool::TOOL_BROWSER_CLOSE.to_string(),
    );
    map
}

pub fn is_browser_tool_name(name: &str) -> bool {
    browser_tool::is_browser_tool_name(name)
}

pub fn browser_tools_available(config: &Config) -> bool {
    browser_tool::browser_tools_enabled(config)
}

pub fn desktop_tools_available(config: &Config) -> bool {
    desktop_control::desktop_tools_enabled(config)
}

pub fn is_desktop_control_tool_name(name: &str) -> bool {
    desktop_control::is_desktop_control_tool_name(name)
}

pub async fn build_desktop_followup_user_message(result: &Value) -> Result<Option<Value>> {
    desktop_control::build_followup_user_message(result).await
}

pub fn is_read_image_tool_name(name: &str) -> bool {
    read_image_tool::is_read_image_tool_name(name)
}

pub async fn build_read_image_followup_user_message(result: &Value) -> Result<Option<Value>> {
    read_image_tool::build_followup_user_message(result).await
}

pub fn is_sleep_tool_name(name: &str) -> bool {
    sleep_tool::is_sleep_tool_name(name)
}

pub fn extract_sleep_seconds(args: &Value) -> Option<f64> {
    sleep_tool::extract_sleep_seconds(args)
}

fn is_desktop_mode(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
}

fn runtime_builtin_tool_allowed(config: &Config, canonical: &str) -> bool {
    if web_fetch_tool::is_web_fetch_tool_name(canonical)
        && !web_fetch_tool::web_fetch_enabled(config)
    {
        return false;
    }
    if browser_tool::is_browser_tool_name(canonical) && !browser_tool::browser_tools_enabled(config)
    {
        return false;
    }
    if desktop_control::is_desktop_control_tool_name(canonical)
        && !desktop_control::desktop_tools_enabled(config)
    {
        return false;
    }
    true
}

fn desktop_builtin_tool_names() -> &'static HashSet<String> {
    static BUILTIN_NAMES: OnceLock<HashSet<String>> = OnceLock::new();
    BUILTIN_NAMES.get_or_init(|| {
        builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .map(|spec| spec.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect()
    })
}

pub fn filter_tool_names_by_model_capability(
    allowed_tool_names: HashSet<String>,
    support_vision: bool,
) -> HashSet<String> {
    if support_vision {
        return allowed_tool_names;
    }
    allowed_tool_names
        .into_iter()
        .filter(|name| {
            let canonical = resolve_tool_name(name);
            !read_image_tool::is_read_image_tool_name(&canonical)
                && !read_image_tool::is_read_image_tool_name(name)
                && !desktop_control::is_desktop_control_tool_name(&canonical)
                && !desktop_control::is_desktop_control_tool_name(name)
        })
        .collect()
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
        sessions_yield_tool::TOOL_SESSIONS_YIELD => {
            Some(sessions_yield_tool::TOOL_SESSIONS_YIELD_ALIAS)
        }
        "问询面板" => Some("question_panel"),
        "技能调用" => Some("skill_call"),
        thread_control_tool::TOOL_THREAD_CONTROL => {
            Some(thread_control_tool::TOOL_THREAD_CONTROL_ALIAS)
        }
        "智能体蜂群" => Some("agent_swarm"),
        "节点调用" => Some("node_invoke"),
        "用户世界工具" => Some("user_world"),
        channel_tool::TOOL_CHANNEL => Some("channel_tool"),
        "记忆管理" => Some("memory_manager"),
        web_fetch_tool::TOOL_WEB_FETCH => Some(web_fetch_tool::TOOL_WEB_FETCH_ALIAS),
        browser_tool::TOOL_BROWSER => Some("browser"),
        desktop_control::TOOL_DESKTOP_CONTROLLER => Some("desktop_controller"),
        desktop_control::TOOL_DESKTOP_MONITOR => Some("desktop_monitor"),
        self_status_tool::TOOL_SELF_STATUS => Some(self_status_tool::TOOL_SELF_STATUS_ALIAS),
        read_image_tool::TOOL_READ_IMAGE => Some(read_image_tool::TOOL_READ_IMAGE_ALIAS),
        sleep_tool::TOOL_SLEEP_WAIT => Some(sleep_tool::TOOL_SLEEP_ALIAS),
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

/// 汇总系统当前可用的工具名称（包含内置别名、MCP、A2A、技能与用户工具）。
pub fn collect_available_tool_names(
    config: &Config,
    skills: &SkillRegistry,
    user_tool_bindings: Option<&UserToolBindings>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut enabled_builtin = HashSet::new();
    if is_desktop_mode(config) {
        for canonical in desktop_builtin_tool_names() {
            if !runtime_builtin_tool_allowed(config, canonical) {
                continue;
            }
            enabled_builtin.insert(canonical.clone());
            names.insert(canonical.clone());
        }
    } else {
        for name in &config.tools.builtin.enabled {
            let canonical = resolve_tool_name(name);
            if canonical.is_empty() || !runtime_builtin_tool_allowed(config, &canonical) {
                continue;
            }
            enabled_builtin.insert(canonical.clone());
            names.insert(canonical);
        }
    }
    if browser_tool::browser_tools_enabled(config) {
        // Browser visibility is controlled by tools.browser.enabled, so it should not require
        // a duplicated entry in tools.builtin.enabled.
        enabled_builtin.insert(browser_tool::TOOL_BROWSER.to_string());
        names.insert(browser_tool::TOOL_BROWSER.to_string());
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
pub(crate) fn yaml_to_json(value: &YamlValue) -> Value {
    let schema = serde_json::to_value(value).unwrap_or(Value::Null);
    normalize_tool_input_schema(Some(&schema))
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

#[cfg(test)]
mod tests {
    use super::{
        builtin_tool_specs_with_language, collect_available_tool_names, resolve_tool_name,
    };
    use crate::config::Config;
    use crate::skills::SkillRegistry;

    #[test]
    fn read_file_spec_clarifies_plain_text_only_in_english() {
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "读取文件")
            .expect("read_file spec");
        assert!(spec.description.contains("plain-text"));
        assert!(spec.description.contains("search_content"));
        assert!(spec.description.contains("start_line"));
        assert!(spec.description.contains("offset+limit"));
        assert!(!spec.description.contains("read_image"));
        let path_description = spec.input_schema["properties"]["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("plain-text"));
        assert!(path_description.contains("targeted"));
        assert!(path_description.contains("binary"));
        assert!(path_description.contains("/workspaces/"));
        assert!(path_description.contains("/工作目录/"));
        let start_line_description = spec.input_schema["properties"]["start_line"]["description"]
            .as_str()
            .expect("start_line description");
        assert!(start_line_description.contains("200-line window"));
        assert!(start_line_description.contains("offset+limit"));
        assert!(spec.input_schema["properties"]["file_path"].is_null());
        assert!(spec.input_schema["properties"]["files"].is_null());
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "path")));
    }

    #[test]
    fn read_file_spec_clarifies_plain_text_only_in_chinese() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "读取文件")
            .expect("read_file spec");
        assert!(spec.description.contains("纯文本"));
        assert!(spec.description.contains("search_content"));
        assert!(spec.description.contains("默认读取 200 行"));
        assert!(spec.description.contains("offset+limit"));
        assert!(!spec.description.contains("read_image"));
        let path_description = spec.input_schema["properties"]["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("纯文本"));
        assert!(path_description.contains("定点"));
        assert!(path_description.contains("二进制"));
        assert!(path_description.contains("/工作目录/"));
        let start_line_description = spec.input_schema["properties"]["start_line"]["description"]
            .as_str()
            .expect("start_line description");
        assert!(start_line_description.contains("默认读取 200 行"));
        assert!(start_line_description.contains("offset+limit"));
        assert!(spec.input_schema["properties"]["file_path"].is_null());
        assert!(spec.input_schema["properties"]["files"].is_null());
    }

    #[test]
    fn search_spec_exposes_canonical_model_side_fields_in_english() {
        let canonical = resolve_tool_name("search_content");
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == canonical)
            .expect("search spec");
        assert!(spec.description.contains("rg"));
        assert!(spec.input_schema["properties"]["query"].is_object());
        assert!(spec.input_schema["properties"]["glob"].is_object());
        assert!(spec.input_schema["properties"]["query_mode"].is_object());
        assert!(spec.input_schema["properties"]["context_before"].is_object());
        assert!(spec.input_schema["properties"]["context_after"].is_object());
        assert!(spec.input_schema["properties"]["max_matches"].is_object());
        assert!(spec.input_schema["properties"]["pattern"].is_null());
        assert!(spec.input_schema["properties"]["-C"].is_null());
        assert!(spec.input_schema["properties"]["-i"].is_null());
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "query")));
    }

    #[test]
    fn a2ui_schema_prefers_minimal_known_message_shape() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "a2ui")
            .expect("a2ui spec");
        assert!(spec.input_schema["properties"]["uid"].is_object());
        assert_eq!(
            spec.input_schema["properties"]["a2ui"]["minItems"].as_i64(),
            Some(1)
        );
        let message_props = &spec.input_schema["properties"]["a2ui"]["items"]["properties"];
        assert!(message_props["beginRendering"].is_object());
        assert!(message_props["surfaceUpdate"].is_object());
        assert!(message_props["dataModelUpdate"].is_object());
        assert!(message_props["deleteSurface"].is_object());
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "a2ui")));
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().all(|item| item != "uid")));
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn panel_schemas_bound_route_and_plan_item_shapes() {
        let plan_canonical = resolve_tool_name("update_plan");
        let plan_spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == plan_canonical)
            .expect("plan spec");
        assert_eq!(
            plan_spec.input_schema["properties"]["plan"]["maxItems"].as_i64(),
            Some(12)
        );
        assert_eq!(
            plan_spec.input_schema["properties"]["plan"]["items"]["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            plan_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let question_panel_canonical = resolve_tool_name("question_panel");
        let question_spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == question_panel_canonical)
            .expect("question panel spec");
        assert_eq!(
            question_spec.input_schema["properties"]["routes"]["maxItems"].as_i64(),
            Some(4)
        );
        assert_eq!(
            question_spec.input_schema["properties"]["routes"]["items"]["additionalProperties"]
                .as_bool(),
            Some(false)
        );
        assert_eq!(
            question_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn lsp_schema_uses_canonical_snake_case_operations() {
        let canonical_name = resolve_tool_name("lsp");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("lsp spec");
        let operations = spec.input_schema["properties"]["operation"]["enum"]
            .as_array()
            .expect("lsp operations");
        assert!(operations.iter().any(|item| item == "document_symbol"));
        assert!(operations.iter().any(|item| item == "workspace_symbol"));
        assert!(operations.iter().any(|item| item == "call_hierarchy"));
        assert!(operations.iter().all(|item| item != "documentSymbol"));
        assert!(operations.iter().all(|item| item != "workspaceSymbol"));
        assert!(operations.iter().all(|item| item != "callHierarchy"));
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn web_fetch_schema_disallows_extra_model_side_fields() {
        let canonical_name = resolve_tool_name("web_fetch");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("web_fetch spec");
        assert!(spec.input_schema["properties"]["url"].is_object());
        assert!(spec.input_schema["properties"]["extract_mode"].is_object());
        assert!(spec.input_schema["properties"]["max_chars"].is_object());
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn agent_swarm_schema_exposes_canonical_fields() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "智能体蜂群")
            .expect("agent_swarm spec");
        assert!(spec.description.contains("子智能体控制"));
        assert!(spec
            .description
            .contains("agent_id/agent_name/session_id/run_ids"));
        assert!(spec.input_schema["properties"]["agent_id"].is_object());
        assert!(spec.input_schema["properties"]["agent_name"].is_object());
        assert!(spec.input_schema["properties"]["session_id"].is_object());
        assert!(spec.input_schema["properties"]["run_ids"].is_object());
        assert!(spec.input_schema["properties"]["agentName"].is_null());
        assert!(spec.input_schema["properties"]["name"].is_null());
        assert!(spec.input_schema["properties"]["task"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("subagent_control.spawn")));
        let task_props = &spec.input_schema["properties"]["tasks"]["items"]["properties"];
        assert!(task_props["agent_id"].is_object());
        assert!(task_props["agent_name"].is_object());
        assert!(task_props["session_id"].is_object());
        assert!(task_props["agentName"].is_null());
        assert!(spec.input_schema["allOf"].is_null());
    }

    #[test]
    fn subagent_control_schema_defaults_to_current_parent_scope() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "子智能体控制")
            .expect("subagent_control spec");
        assert!(spec.description.contains("当前会话"));
        assert!(spec.input_schema["properties"]["parent_id"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("current session")));
        assert!(spec.input_schema["properties"]["session_id"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("child session")));
        assert!(spec.input_schema["properties"]["sessionId"].is_null());
        assert!(spec.input_schema["properties"]["runId"].is_null());
    }

    #[test]
    fn schedule_task_schema_exposes_flat_model_side_fields() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "定时任务")
            .expect("schedule_task spec");
        assert!(spec.input_schema["properties"]["job_id"].is_object());
        assert!(spec.input_schema["properties"]["schedule_text"].is_object());
        assert!(spec.input_schema["properties"]["message"].is_object());
        assert!(spec.input_schema["properties"]["enabled"].is_object());
        assert!(spec.input_schema["properties"]["job"].is_null());
    }

    #[test]
    fn channel_tool_schema_prefers_content_over_legacy_text() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "渠道工具")
            .expect("channel tool spec");
        assert!(spec.input_schema["properties"]["content"].is_object());
        assert!(spec.input_schema["properties"]["attachments"].is_object());
        assert!(spec.input_schema["properties"]["text"].is_null());
        assert!(spec.input_schema["properties"]["meta"].is_null());
        assert!(spec.input_schema["allOf"].is_null());
    }

    #[test]
    fn execute_command_schema_hides_budget_compatibility_shape() {
        let canonical_name = resolve_tool_name("execute_command");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("execute command spec");
        assert!(spec.input_schema["properties"]["content"].is_object());
        assert!(spec.input_schema["properties"]["workdir"].is_object());
        assert!(spec.input_schema["properties"]["timeout_s"].is_object());
        assert!(spec.input_schema["properties"]["dry_run"].is_object());
        assert!(spec.input_schema["properties"]["time_budget_ms"].is_null());
        assert!(spec.input_schema["properties"]["output_budget_bytes"].is_null());
        assert!(spec.input_schema["properties"]["max_commands"].is_null());
        assert!(spec.input_schema["properties"]["budget"].is_null());
    }

    #[test]
    fn list_files_schema_prefers_cursor_over_offset_alias() {
        let canonical_name = resolve_tool_name("list_files");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("list files spec");
        assert!(spec.input_schema["properties"]["path"].is_object());
        assert!(spec.input_schema["properties"]["cursor"].is_object());
        assert!(spec.input_schema["properties"]["limit"].is_object());
        assert!(spec.input_schema["properties"]["offset"].is_null());
    }

    #[test]
    fn thread_control_schema_exposes_canonical_fields() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "会话线程控制")
            .expect("thread control spec");
        assert!(spec.input_schema["properties"]["session_id"].is_object());
        assert!(spec.input_schema["properties"]["parent_session_id"].is_object());
        assert!(spec.input_schema["properties"]["set_main"].is_object());
        assert!(spec.input_schema["properties"]["agentId"].is_null());
        assert!(spec.input_schema["properties"]["label"].is_null());
        assert!(spec.input_schema["properties"]["setMain"].is_null());
    }

    #[test]
    fn browser_schema_hides_generic_request_mode_from_model_side() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "浏览器")
            .expect("browser spec");
        let actions = spec.input_schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert!(actions.iter().all(|item| item != "act"));
        assert!(spec.input_schema["properties"]["request"].is_null());
        assert!(spec.input_schema["allOf"].is_null());
        assert!(spec.input_schema["properties"]["selector"].is_object());
        assert!(spec.input_schema["properties"]["url"].is_object());
    }

    #[test]
    fn user_world_schema_hides_conditional_teaching_shape() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "用户世界工具")
            .expect("user world spec");
        assert!(spec.input_schema["properties"]["user_id"].is_object());
        assert!(spec.input_schema["properties"]["user_ids"].is_object());
        assert!(spec.input_schema["properties"]["content"].is_object());
        assert!(spec.input_schema["allOf"].is_null());
    }

    #[test]
    fn a2a_schemas_hide_raw_tasks_array_from_model_side() {
        let observe = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "a2a观察")
            .expect("a2a observe spec");
        assert!(observe.input_schema["properties"]["task_ids"].is_object());
        assert!(observe.input_schema["properties"]["tasks"].is_null());

        let wait = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "a2a等待")
            .expect("a2a wait spec");
        assert!(wait.input_schema["properties"]["task_ids"].is_object());
        assert!(wait.input_schema["properties"]["wait_s"].is_object());
        assert!(wait.input_schema["properties"]["tasks"].is_null());
    }

    #[test]
    fn node_invoke_schema_requires_explicit_action_on_model_side() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "节点调用")
            .expect("node invoke spec");
        assert!(spec.input_schema["properties"]["action"].is_object());
        assert!(spec.input_schema["properties"]["node_id"].is_object());
        assert!(spec.input_schema["properties"]["command"].is_object());
        assert!(spec.input_schema["properties"]["metadata"].is_null());
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "action")));
        assert!(spec.input_schema["allOf"].is_null());
        assert!(spec.input_schema["anyOf"].is_null());
    }

    #[test]
    fn desktop_controller_schema_no_longer_requires_description_for_model_side() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "桌面控制器")
            .expect("desktop controller spec");
        assert!(spec.input_schema["properties"]["bbox"].is_object());
        assert!(spec.input_schema["properties"]["action"].is_object());
        assert!(spec.input_schema["properties"]["description"].is_null());
        let required = spec.input_schema["required"]
            .as_array()
            .expect("required array");
        assert!(required.iter().any(|item| item == "bbox"));
        assert!(required.iter().any(|item| item == "action"));
        assert!(required.iter().all(|item| item != "description"));
    }

    #[test]
    fn desktop_mode_exposes_all_builtin_tools_even_with_partial_whitelist() {
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.tools.builtin.enabled = vec!["最终回复".to_string()];
        config.tools.browser.enabled = true;
        config.tools.desktop_controller.enabled = true;

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        for spec in builtin_tool_specs_with_language("zh-CN") {
            assert!(
                available.contains(&spec.name),
                "desktop mode should include builtin tool {}",
                spec.name
            );
        }
        assert!(available.contains("read_file"));
        assert!(available.contains("update_plan"));
    }

    #[test]
    fn non_desktop_mode_still_follows_builtin_whitelist() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.tools.builtin.enabled = vec!["读取文件".to_string()];

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        assert!(available.contains("读取文件"));
        assert!(available.contains("read_file"));
        assert!(!available.contains("写入文件"));
        assert!(!available.contains("write_file"));
    }

    #[test]
    fn self_status_alias_resolves_to_builtin_tool_name() {
        assert_eq!(
            resolve_tool_name(super::self_status_tool::TOOL_SELF_STATUS_ALIAS),
            super::self_status_tool::TOOL_SELF_STATUS
        );
    }

    #[test]
    fn browser_tool_auto_registers_without_builtin_whitelist_entry() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.browser.enabled = true;
        config.tools.browser.enabled = true;

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        assert!(available.contains(super::browser_tool::TOOL_BROWSER));
        assert!(available.contains("browser"));
    }

    #[test]
    fn simple_builtin_schemas_disallow_extra_model_side_fields() {
        let specs = builtin_tool_specs_with_language("zh-CN");

        let final_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("final_response"))
            .expect("final response spec");
        assert_eq!(
            final_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let a2ui_spec = specs
            .iter()
            .find(|spec| spec.name == "a2ui")
            .expect("a2ui spec");
        assert_eq!(
            a2ui_spec.input_schema["properties"]["a2ui"]["items"]["additionalProperties"].as_bool(),
            Some(false)
        );

        let sessions_yield_spec = specs
            .iter()
            .find(|spec| spec.name == super::sessions_yield_tool::TOOL_SESSIONS_YIELD)
            .expect("sessions yield spec");
        assert_eq!(
            sessions_yield_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let schedule_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("schedule_task"))
            .expect("schedule task spec");
        assert_eq!(
            schedule_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            schedule_spec.input_schema["properties"]["schedule"]["additionalProperties"].as_bool(),
            Some(false)
        );

        let channel_spec = specs
            .iter()
            .find(|spec| spec.name == super::channel_tool::TOOL_CHANNEL)
            .expect("channel tool spec");
        assert_eq!(
            channel_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            channel_spec.input_schema["properties"]["contact"]["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            channel_spec.input_schema["properties"]["attachments"]["items"]["additionalProperties"]
                .as_bool(),
            Some(false)
        );

        let sleep_spec = specs
            .iter()
            .find(|spec| spec.name == super::sleep_tool::TOOL_SLEEP_WAIT)
            .expect("sleep spec");
        assert_eq!(
            sleep_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let memory_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("memory_manage"))
            .expect("memory manage spec");
        assert_eq!(
            memory_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let observe_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("a2a_observe"))
            .expect("a2a observe spec");
        assert_eq!(
            observe_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let wait_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("a2a_wait"))
            .expect("a2a wait spec");
        assert_eq!(
            wait_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let execute_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("execute_command"))
            .expect("execute command spec");
        assert_eq!(
            execute_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let list_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("list_files"))
            .expect("list files spec");
        assert_eq!(
            list_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let search_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("search_content"))
            .expect("search content spec");
        assert_eq!(
            search_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let read_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("read_file"))
            .expect("read file spec");
        assert_eq!(
            read_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            read_spec.input_schema["properties"]["indentation"]["additionalProperties"].as_bool(),
            Some(false)
        );

        let read_image_spec = specs
            .iter()
            .find(|spec| spec.name == super::read_image_tool::TOOL_READ_IMAGE)
            .expect("read image spec");
        assert_eq!(
            read_image_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let skill_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("skill_call"))
            .expect("skill call spec");
        assert_eq!(
            skill_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let write_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("write_file"))
            .expect("write file spec");
        assert_eq!(
            write_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }
}
