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
            name: sessions_yield_tool::TOOL_SESSIONS_YIELD.to_string(),
            description: t("tool.spec.sessions_yield.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": t("tool.spec.sessions_yield.args.message")
                    }
                }
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
                            "schedule_text": {"type": "string", "description": t("tool.spec.schedule_task.args.job.schedule_text")},
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
            name: sleep_tool::TOOL_SLEEP_WAIT.to_string(),
            description: t("tool.spec.sleep.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "seconds": {"type": "number", "description": t("tool.spec.sleep.args.seconds"), "minimum": 0.001},
                    "reason": {"type": "string", "description": t("tool.spec.sleep.args.reason")}
                },
                "required": ["seconds"]
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
                    "refresh": {"type": "boolean", "description": t("tool.spec.channel_tool.args.refresh")},
                    "contact": {
                        "type": "object",
                        "description": t("tool.spec.channel_tool.args.contact"),
                        "properties": {
                            "channel": {"type": "string"},
                            "account_id": {"type": "string"},
                            "to": {"type": "string"},
                            "peer_kind": {"type": "string", "enum": ["user", "group"]},
                            "thread_id": {"type": "string"}
                        }
                    },
                    "to": {"type": "string", "description": t("tool.spec.channel_tool.args.to")},
                    "peer_kind": {"type": "string", "enum": ["user", "group"], "description": t("tool.spec.channel_tool.args.peer_kind")},
                    "thread_id": {"type": "string", "description": t("tool.spec.channel_tool.args.thread_id")},
                    "text": {"type": "string", "description": t("tool.spec.channel_tool.args.text")},
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
                            }
                        }
                    },
                    "meta": {"type": "object", "description": t("tool.spec.channel_tool.args.meta")},
                    "wait": {"type": "boolean", "description": t("tool.spec.channel_tool.args.wait")},
                    "wait_timeout_s": {"type": "number", "minimum": 1, "maximum": 30, "description": t("tool.spec.channel_tool.args.wait_timeout_s")}
                },
                "required": ["action"],
                "allOf": [
                    {
                        "if": {"properties": {"action": {"const": "send_message"}}},
                        "then": {
                            "anyOf": [
                                {"required": ["text"]},
                                {"required": ["content"]},
                                {"required": ["attachments"]}
                            ]
                        }
                    }
                ]
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
                "required": ["action"]
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
                    "timeout_s": {"type": "number", "description": t("tool.spec.exec.args.timeout")},
                    "dry_run": {"type": "boolean", "description": "Validate command and budgets without execution."},
                    "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 600000, "description": "Optional execution time budget in milliseconds."},
                    "output_budget_bytes": {"type": "integer", "minimum": 4096, "maximum": 4194304, "description": "Optional command output capture budget in bytes."},
                    "max_commands": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Optional limit for number of commands parsed from content."},
                    "budget": {
                        "type": "object",
                        "properties": {
                            "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 600000},
                            "output_budget_bytes": {"type": "integer", "minimum": 4096, "maximum": 4194304},
                            "max_commands": {"type": "integer", "minimum": 1, "maximum": 200}
                        }
                    }
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
                    "max_depth": {"type": "integer", "minimum": 0},
                    "cursor": {"type": "string", "description": t("tool.spec.list.args.cursor")},
                    "offset": {"type": "integer", "minimum": 0, "description": t("tool.spec.list.args.offset")},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 500, "description": t("tool.spec.list.args.limit")}
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
                    "pattern": {"type": "string", "description": t("tool.spec.search.args.pattern")},
                    "path": {"type": "string", "description": t("tool.spec.search.args.path")},
                    "file_pattern": {"type": "string", "description": t("tool.spec.search.args.file_pattern")},
                    "glob": {"type": "string", "description": t("tool.spec.search.args.glob")},
                    "type": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ],
                        "description": t("tool.spec.search.args.type")
                    },
                    "query_mode": {"type": "string", "enum": ["literal", "regex"], "description": t("tool.spec.search.args.query_mode")},
                    "regex": {"type": "boolean", "description": t("tool.spec.search.args.regex")},
                    "fixed_strings": {"type": "boolean", "description": t("tool.spec.search.args.fixed_strings")},
                    "-F": {"type": "boolean", "description": t("tool.spec.search.args.fixed_strings")},
                    "case_sensitive": {"type": "boolean", "description": t("tool.spec.search.args.case_sensitive")},
                    "ignore_case": {"type": "boolean", "description": t("tool.spec.search.args.ignore_case")},
                    "-i": {"type": "boolean", "description": t("tool.spec.search.args.ignore_case")},
                    "max_depth": {"type": "integer", "minimum": 0, "description": t("tool.spec.search.args.max_depth")},
                    "max_files": {"type": "integer", "minimum": 0, "description": t("tool.spec.search.args.max_files")},
                    "max_matches": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Maximum number of matches to return (default 200)."},
                    "max_count": {"type": "integer", "minimum": 1, "maximum": 2000, "description": t("tool.spec.search.args.max_count")},
                    "head_limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": t("tool.spec.search.args.max_count")},
                    "max_candidates": {"type": "integer", "minimum": 1, "maximum": 20000, "description": "Maximum candidate files produced by fast search engine (default 4000)."},
                    "timeout_ms": {"type": "integer", "minimum": 1, "maximum": 120000, "description": "Search timeout in milliseconds (default 30000)."},
                    "engine": {"type": "string", "enum": ["auto", "rg", "rust"], "description": "Search engine strategy: auto prefers rg and falls back to rust scanner."},
                    "dry_run": {"type": "boolean", "description": "Validate search plan and resolved budget without scanning files."},
                    "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 120000, "description": "Optional time budget cap in milliseconds for this search call."},
                    "output_budget_bytes": {"type": "integer", "minimum": 2048, "maximum": 4194304, "description": "Optional cap for returned hits payload size in bytes."},
                    "budget": {
                        "type": "object",
                        "properties": {
                            "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 120000},
                            "output_budget_bytes": {"type": "integer", "minimum": 2048, "maximum": 4194304},
                            "max_files": {"type": "integer", "minimum": 1, "maximum": 20000},
                            "max_matches": {"type": "integer", "minimum": 1, "maximum": 2000},
                            "max_candidates": {"type": "integer", "minimum": 1, "maximum": 20000}
                        }
                    },
                    "context": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context")},
                    "-C": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context_alias")},
                    "context_before": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context_before")},
                    "context_after": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.context_after")},
                    "-B": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.before_alias")},
                    "-A": {"type": "integer", "minimum": 0, "maximum": 20, "description": t("tool.spec.search.args.after_alias")}
                },
                "anyOf": [
                    {"required": ["query"]},
                    {"required": ["pattern"]}
                ]
            }),
        },
        ToolSpec {
            name: "读取文件".to_string(),
            description: t("tool.spec.read.description"),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "dry_run": {"type": "boolean", "description": "Resolve targets and metadata without reading full file content."},
                    "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 600000, "description": "Optional time budget cap in milliseconds for this read call."},
                    "output_budget_bytes": {"type": "integer", "minimum": 1024, "maximum": 2097152, "description": "Optional cap for aggregated read output text bytes."},
                    "max_files": {"type": "integer", "minimum": 1, "maximum": 20, "description": "Optional cap for number of files processed in this call."},
                    "path": {"type": "string", "description": t("tool.spec.read.args.files.path")},
                    "file_path": {"type": "string", "description": t("tool.spec.read.args.files.path")},
                    "start_line": {"type": "integer", "description": t("tool.spec.read.args.files.start_line")},
                    "end_line": {"type": "integer", "description": t("tool.spec.read.args.files.end_line")},
                    "offset": {"type": "integer", "minimum": 1, "description": "Codex-compatible alias of start_line. Use with limit to read a line window."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Codex-compatible line window size used with offset."},
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
                        }
                    },
                    "budget": {
                        "type": "object",
                        "properties": {
                            "time_budget_ms": {"type": "integer", "minimum": 1, "maximum": 600000},
                            "output_budget_bytes": {"type": "integer", "minimum": 1024, "maximum": 2097152},
                            "max_files": {"type": "integer", "minimum": 1, "maximum": 20}
                        }
                    },
                    "files": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
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
                                    }
                                }
                            },
                            "required": ["path"]
                        }
                    }
                },
                "anyOf": [
                    {"required": ["files"]},
                    {"required": ["path"]},
                    {"required": ["file_path"]}
                ]
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
                "required": ["path"]
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
                    "content": {"type": "string", "description": t("tool.spec.write.args.content")},
                    "dry_run": {"type": "boolean", "description": "Preview write target and size changes without writing to disk."}
                },
                "required": ["path", "content"]
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
                        "description": format!("{} {}", t("tool.spec.subagent_control.args.action"), "Important: spawn already sends the first turn. Do not call send immediately after spawn unless you are deliberately continuing the same child conversation."),
                        "enum": ["list", "history", "send", "spawn", "batch_spawn", "status", "wait", "interrupt", "close", "resume"]
                    },
                    "limit": {"type": "integer", "description": t("tool.spec.sessions_list.args.limit"), "minimum": 1},
                    "activeMinutes": {"type": "number", "description": t("tool.spec.sessions_list.args.active_minutes"), "minimum": 0},
                    "messageLimit": {"type": "integer", "description": t("tool.spec.sessions_list.args.message_limit"), "minimum": 0},
                    "parentId": {"type": "string", "description": "Parent session id. list/status/wait default to the current session when omitted."},
                    "session_id": {"type": "string", "description": "Exact child session id. Prefer the session_id returned by spawn. send/history require exactly one child session."},
                    "sessionId": {"type": "string", "description": "Exact child session id. Prefer the session_id returned by spawn. send/history require exactly one child session."},
                    "sessionIds": {"type": "array", "description": "Child session ids under the current session. status/wait may use multiple targets; send/history must resolve to exactly one child session.", "items": {"type": "string"}},
                    "sessionKey": {"type": "string", "description": t("tool.spec.sessions_history.args.session_id")},
                    "runId": {"type": "string", "description": "Child run id. send/history may use a single runId to resolve the child session; status/wait may inspect by runId directly."},
                    "runIds": {"type": "array", "description": "Child run ids for status/wait or multi-target inspection.", "items": {"type": "string"}},
                    "dispatchId": {"type": "string", "description": "Dispatch id returned by batch_spawn."},
                    "strategy": {
                        "type": "string",
                        "description": "Batch dispatch strategy. first_success aligns with Codex-style early convergence.",
                        "enum": ["parallel_all", "first_success", "review_then_merge"]
                    },
                    "remainingAction": {
                        "type": "string",
                        "description": "How to handle unfinished sibling subagents after early convergence. first_success defaults to interrupt; wait keeps siblings unless specified.",
                        "enum": ["keep", "interrupt", "close"]
                    },
                    "includeTools": {"type": "boolean", "description": t("tool.spec.sessions_history.args.include_tools")},
                    "message": {"type": "string", "description": "Message content for a follow-up turn on an existing child session. Do not use action=send immediately after spawn unless you are continuing the same child conversation."},
                    "timeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_send.args.timeout")},
                    "task": {"type": "string", "description": "Initial task or first prompt for the child session. action=spawn/batch_spawn dispatches this task immediately and starts the first child turn; do not repeat the same content with send unless you intentionally want a follow-up turn."},
                    "tasks": {
                        "type": "array",
                        "description": "Batch child tasks.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "task": {"type": "string", "description": t("tool.spec.sessions_spawn.args.task")},
                                "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                                "agentId": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                                "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                                "runTimeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                                "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]}
                            },
                            "required": ["task"]
                        }
                    },
                    "label": {"type": "string", "description": t("tool.spec.sessions_spawn.args.label")},
                    "agentId": {"type": "string", "description": t("tool.spec.sessions_spawn.args.agent_id")},
                    "model": {"type": "string", "description": t("tool.spec.sessions_spawn.args.model")},
                    "runTimeoutSeconds": {"type": "number", "description": t("tool.spec.sessions_spawn.args.timeout")},
                    "cleanup": {"type": "string", "description": t("tool.spec.sessions_spawn.args.cleanup"), "enum": ["keep", "delete"]},
                    "waitSeconds": {"type": "number", "description": "Wait time for batch/status polling."},
                    "pollIntervalSeconds": {"type": "number", "description": "Polling interval for wait."},
                    "waitMode": {
                        "type": "string",
                        "description": "Wait completion mode for subagent wait. all waits every target, any returns on first terminal target, first_success returns on the first success or when all targets finish.",
                        "enum": ["all", "any", "first_success"]
                    },
                    "dispatchLabel": {"type": "string", "description": "Optional label for the dispatch batch."},
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
                    "agentId": {"type": "string", "description": t("tool.spec.thread_control.args.agent_id")},
                    "title": {"type": "string", "description": t("tool.spec.thread_control.args.title")},
                    "label": {"type": "string", "description": t("tool.spec.thread_control.args.label")},
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
                    "switch": {"type": "boolean", "description": t("tool.spec.thread_control.args.switch")},
                    "setMain": {"type": "boolean", "description": t("tool.spec.thread_control.args.set_main")}
                },
                "required": ["action"]
            }),
        },
        ToolSpec {
            name: "智能体蜂群".to_string(),
            description: "蜂群协作工具，只管理已存在的其他智能体。spawn 必须提供 agentId/agentName；临时子会话请改用子智能体控制。".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "description": "send(发单目标)/batch_send(并发)/wait(等结果)/status(看状态)/history(看会话)/spawn(派生到已存在智能体)/list(列成员)。",
                        "enum": ["list", "status", "send", "history", "spawn", "batch_send", "wait"]
                    },
                    "agentId": {"type": "string", "description": "目标智能体 ID。"},
                    "agentName": {"type": "string", "description": "目标智能体名称。"},
                    "name": {"type": "string", "description": "agentName 的简写别名。"},
                    "sessionKey": {"type": "string", "description": "目标会话 ID。"},
                    "message": {"type": "string", "description": "消息内容。", "minLength": 1},
                    "task": {"type": "string", "description": "任务描述。spawn 仅在已提供 agentId/agentName 时有效；临时子会话请用 subagent_control.spawn。", "minLength": 1},
                    "tasks": {
                        "type": "array",
                        "description": "batch_send 任务列表。",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "properties": {
                                "agentId": {"type": "string", "description": "目标智能体 ID。"},
                                "agentName": {"type": "string", "description": "目标智能体名称。"},
                                "name": {"type": "string", "description": "agentName 的简写别名。"},
                                "sessionKey": {"type": "string", "description": "目标会话 ID。"},
                                "message": {"type": "string", "description": "任务消息。", "minLength": 1}
                            },
                            "required": ["message"],
                            "anyOf": [
                                {"required": ["agentId"]},
                                {"required": ["agentName"]},
                                {"required": ["name"]},
                                {"required": ["sessionKey"]}
                            ]
                        }
                    },
                    "runIds": {"type": "array", "description": t("tool.spec.agent_swarm.args.run_ids"), "items": {"type": "string"}, "minItems": 1}
                },
                "required": ["action"],
                "allOf": [
                    {
                        "if": {"properties": {"action": {"const": "send"}}},
                        "then": {
                            "required": ["message"],
                            "anyOf": [
                                {"required": ["agentId"]},
                                {"required": ["agentName"]},
                                {"required": ["name"]},
                                {"required": ["sessionKey"]}
                            ]
                        }
                    },
                    {
                        "if": {"properties": {"action": {"const": "history"}}},
                        "then": {"required": ["sessionKey"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "spawn"}}},
                        "then": {
                            "required": ["task"],
                            "anyOf": [
                                {"required": ["agentId"]},
                                {"required": ["agentName"]},
                                {"required": ["name"]}
                            ]
                        }
                    },
                    {
                        "if": {"properties": {"action": {"const": "batch_send"}}},
                        "then": {"required": ["tasks"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "wait"}}},
                        "then": {"required": ["runIds"]}
                    }
                ],
                "examples": [
                    {"action": "send", "agentName": "", "message": ""},
                    {"action": "batch_send", "tasks": [{"agentName": "", "message": ""}]},
                    {"action": "wait", "runIds": [""]}
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
                "required": ["url"]
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
                            "act",
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
                    "max_chars": { "type": "integer", "minimum": 1, "description": t("tool.spec.browser.args.max_chars") },
                    "request": { "type": "object", "description": t("tool.spec.browser.args.request") }
                },
                "required": ["action"],
                "allOf": [
                    {
                        "if": {"properties": {"action": {"const": "navigate"}}},
                        "then": {"required": ["url"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "click"}}},
                        "then": {"required": ["selector"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "type"}}},
                        "then": {"required": ["selector", "text"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "focus"}}},
                        "then": {"required": ["target_id"]}
                    },
                    {
                        "if": {"properties": {"action": {"const": "snapshot"}}},
                        "then": {"properties": {"format": {"enum": ["role", "aria", "ai"]}}}
                    },
                    {
                        "if": {"properties": {"action": {"const": "act"}}},
                        "then": {"required": ["request"]}
                    }
                ]
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
                    "description": {"type": "string", "description": t("tool.spec.desktop_controller.args.description")},
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
                "required": ["bbox", "action", "description"],
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
        assert!(!spec.description.contains("read_image"));
        let path_description = spec.input_schema["properties"]["files"]["items"]["properties"]
            ["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("plain-text"));
        assert!(path_description.contains("targeted"));
        assert!(path_description.contains("binary"));
    }

    #[test]
    fn read_file_spec_clarifies_plain_text_only_in_chinese() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "读取文件")
            .expect("read_file spec");
        assert!(spec.description.contains("纯文本"));
        assert!(spec.description.contains("search_content"));
        assert!(!spec.description.contains("read_image"));
        let path_description = spec.input_schema["properties"]["files"]["items"]["properties"]
            ["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("纯文本"));
        assert!(path_description.contains("定点"));
        assert!(path_description.contains("二进制"));
    }

    #[test]
    fn search_spec_exposes_rg_style_aliases_in_english() {
        let canonical = resolve_tool_name("search_content");
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == canonical)
            .expect("search spec");
        assert!(spec.description.contains("rg"));
        assert!(spec.input_schema["properties"]["pattern"].is_object());
        assert!(spec.input_schema["properties"]["glob"].is_object());
        assert!(spec.input_schema["properties"]["type"].is_object());
        assert!(spec.input_schema["properties"]["-C"].is_object());
        assert!(spec.input_schema["properties"]["-i"].is_object());
        assert!(spec.input_schema["properties"]["-F"].is_object());
        assert!(spec.input_schema["properties"]["max_count"].is_object());
        assert!(spec.input_schema["anyOf"].is_array());
    }

    #[test]
    fn agent_swarm_schema_accepts_agent_name_variants() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "智能体蜂群")
            .expect("agent_swarm spec");
        assert!(spec.description.contains("子智能体控制"));
        assert!(spec.input_schema["properties"]["agentName"].is_object());
        assert!(spec.input_schema["properties"]["name"].is_object());
        assert!(spec.input_schema["properties"]["task"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("subagent_control.spawn")));
        let task_props = &spec.input_schema["properties"]["tasks"]["items"]["properties"];
        assert!(task_props["agentName"].is_object());
        assert!(task_props["name"].is_object());
        let all_of = spec.input_schema["allOf"]
            .as_array()
            .expect("allOf should be array");
        let send_rule = all_of
            .iter()
            .find(|item| item["if"]["properties"]["action"]["const"] == "send")
            .expect("send rule");
        let send_any_of = send_rule["then"]["anyOf"].as_array().expect("send anyOf");
        assert!(send_any_of
            .iter()
            .any(|item| item["required"][0] == "agentName"));
        let spawn_rule = all_of
            .iter()
            .find(|item| item["if"]["properties"]["action"]["const"] == "spawn")
            .expect("spawn rule");
        let spawn_any_of = spawn_rule["then"]["anyOf"].as_array().expect("spawn anyOf");
        assert!(spawn_any_of
            .iter()
            .any(|item| item["required"][0] == "name"));
    }

    #[test]
    fn subagent_control_schema_defaults_to_current_parent_scope() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "子智能体控制")
            .expect("subagent_control spec");
        assert!(spec.description.contains("当前会话"));
        assert!(spec.input_schema["properties"]["parentId"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("current session")));
        assert!(spec.input_schema["properties"]["sessionId"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("child session")));
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
}
