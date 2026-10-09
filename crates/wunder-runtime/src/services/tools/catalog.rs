use super::mcp_pack;
use super::{
    browser_tool, desktop_control, multimodal_generation_tool, read_image_tool, self_status_tool,
    sessions_yield_tool, thread_control_tool, web_fetch_tool, web_search_tool,
};
use crate::config::Config;
use crate::core::json_schema::normalize_tool_input_schema;
use crate::i18n;
use crate::schemas::ToolSpec;
use crate::services::goal;
use crate::services::tools::context::ToolContext;
use crate::skills::SkillRegistry;
use crate::user_tools::UserToolBindings;
use anyhow::Result;
use serde_json::{json, Value};
use serde_yaml::Value as YamlValue;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolAliasEntry {
    pub runtime_name: String,
    pub display_name: String,
    pub server_name: String,
    pub tool_name: String,
    pub tool_title: Option<String>,
}

fn sanitize_mcp_display_segment(value: &str) -> String {
    let mut output = String::new();
    let mut last_underscore = false;
    for ch in value.trim().chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if ch == '_' || ch == '-' {
            ch
        } else if ch.is_alphanumeric() {
            ch
        } else {
            '_'
        };
        if mapped == '_' {
            if last_underscore {
                continue;
            }
            last_underscore = true;
        } else {
            last_underscore = false;
        }
        output.push(mapped);
    }
    output.trim_matches('_').to_string()
}

fn short_hash_suffix(value: &str) -> String {
    let mut state = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut state);
    let hash = format!("{:x}", state.finish());
    hash.chars().take(6).collect()
}

fn build_mcp_tool_alias_entries_from_raw(
    raw_entries: Vec<(String, String, String, Option<String>)>,
) -> Vec<McpToolAliasEntry> {
    let mut tool_counts: HashMap<String, usize> = HashMap::new();
    let mut tool_server_counts: HashMap<(String, String), usize> = HashMap::new();
    for (_, server_name, tool_name, _) in &raw_entries {
        *tool_counts.entry(tool_name.clone()).or_default() += 1;
        *tool_server_counts
            .entry((tool_name.clone(), server_name.clone()))
            .or_default() += 1;
    }

    let mut used_display_names = HashSet::new();
    let mut output = Vec::new();
    for (runtime_name, server_name, tool_name, tool_title) in raw_entries {
        let tool_segment = sanitize_mcp_display_segment(&tool_name);
        let server_segment = sanitize_mcp_display_segment(&server_name);
        let base = if tool_counts.get(&tool_name).copied().unwrap_or_default() <= 1 {
            tool_segment.clone()
        } else {
            format!("{tool_segment}__{server_segment}")
        };
        let same_server_count = tool_server_counts
            .get(&(tool_name.clone(), server_name.clone()))
            .copied()
            .unwrap_or_default();
        let preferred_title = tool_title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let mut display_name = preferred_title.unwrap_or_else(|| {
            if same_server_count <= 1 {
                base.clone()
            } else {
                format!("{base}__{}", short_hash_suffix(&runtime_name))
            }
        });
        if display_name.is_empty() {
            display_name = format!("mcp_tool__{}", short_hash_suffix(&runtime_name));
        }
        if !used_display_names.insert(display_name.clone()) {
            let candidate = format!("{display_name}__{}", short_hash_suffix(&runtime_name));
            used_display_names.insert(candidate.clone());
            display_name = candidate;
        }
        output.push(McpToolAliasEntry {
            runtime_name,
            display_name,
            server_name,
            tool_name,
            tool_title,
        });
    }
    output
}

pub fn build_mcp_tool_alias_entries_for_names<I, S>(names: I) -> Vec<McpToolAliasEntry>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut raw_entries = Vec::new();
    for raw_name in names {
        let runtime_name = raw_name.as_ref().trim();
        let Some((server_name, tool_name)) = runtime_name.split_once('@') else {
            continue;
        };
        let server_name = server_name.trim();
        let tool_name = tool_name.trim();
        if server_name.is_empty() || tool_name.is_empty() {
            continue;
        }
        raw_entries.push((
            runtime_name.to_string(),
            server_name.to_string(),
            tool_name.to_string(),
            None,
        ));
    }
    build_mcp_tool_alias_entries_from_raw(raw_entries)
}

pub fn build_mcp_tool_alias_entries(config: &Config) -> Vec<McpToolAliasEntry> {
    let mut raw_entries = Vec::new();
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        if server.packaged {
            let server_name = server.name.trim();
            if server_name.is_empty() {
                continue;
            }
            let tool_title = server
                .display_name
                .as_ref()
                .map(|value| format!("{} MCP package", value.trim()))
                .filter(|value| !value.trim().is_empty());
            raw_entries.push((
                mcp_pack::runtime_name(server_name),
                server_name.to_string(),
                "mcp_package".to_string(),
                tool_title,
            ));
            continue;
        }
        let allow: HashSet<String> = server.allow_tools.iter().cloned().collect();
        for tool in &server.tool_specs {
            let tool_name = tool.name.trim();
            if tool_name.is_empty() {
                continue;
            }
            if !allow.is_empty() && !allow.contains(&tool.name) {
                continue;
            }
            raw_entries.push((
                format!("{}@{}", server.name, tool_name),
                server.name.trim().to_string(),
                tool_name.to_string(),
                tool.title.clone(),
            ));
        }
    }
    build_mcp_tool_alias_entries_from_raw(raw_entries)
}

/// Build the execute_command tool description, appending a note describing the
/// active shell so the model emits the correct command syntax.
fn exec_tool_description(t: &impl Fn(&str) -> String) -> String {
    let mut desc = t("tool.spec.exec.description");
    let env = crate::core::python_runtime::resolve_desktop_command_env();
    use crate::core::command_utils::ShellKind;
    let note_key = match env.shell.kind {
        ShellKind::Bash => Some("tool.spec.exec.shell_bash"),
        ShellKind::PowerShell => Some("tool.spec.exec.shell_powershell"),
        ShellKind::Cmd => None,
    };
    if let Some(key) = note_key {
        desc.push('\n');
        desc.push_str(&t(key));
    }
    desc
}

fn localized_tool_title(name: &str, language: &str) -> Option<String> {
    let key = format!("tool.title.{name}");
    let value = i18n::t_in_language(&key, language);
    let value = value.trim().to_string();
    if value.is_empty() || value == key {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn builtin_tool_specs_with_language(language: &str) -> Vec<ToolSpec> {
    let t = |key: &str| i18n::t_in_language(key, language);
    let mut specs = vec![
        ToolSpec {
            name: "计划面板".to_string(),
            title: None,
            description: t("tool.spec.plan.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("explanation")
                    .desc(t("tool.spec.plan.args.explanation"))
                    .optional(),
                super::schema::array("plan")
                    .desc(t("tool.spec.plan.args.plan"))
                    .min_items(1)
                    .max_items(12)
                    .items(
                        super::schema::object_param("_")
                            .props(vec![
                                super::schema::string("step")
                                    .desc(t("tool.spec.plan.args.plan.step")),
                                super::schema::string("status")
                                    .desc(t("tool.spec.plan.args.plan.status"))
                                    .enums(vec!["pending", "in_progress", "completed"]),
                            ])
                            .closed(),
                    ),
            ]),
        },
        ToolSpec {
            name: "问询面板".to_string(),
            title: None,
            description: t("tool.spec.question_panel.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("question")
                    .desc(t("tool.spec.question_panel.args.question"))
                    .optional(),
                super::schema::array("routes")
                    .desc(t("tool.spec.question_panel.args.routes"))
                    .min_items(1)
                    .max_items(4)
                    .items(
                        super::schema::object_param("_")
                            .props(vec![
                                super::schema::string("label")
                                    .desc(t("tool.spec.question_panel.args.routes.label")),
                                super::schema::string("description")
                                    .desc(t("tool.spec.question_panel.args.routes.description"))
                                    .optional(),
                                super::schema::boolean("recommended")
                                    .desc(t("tool.spec.question_panel.args.routes.recommended"))
                                    .optional(),
                            ])
                            .closed(),
                    ),
                super::schema::boolean("multiple")
                    .desc(t("tool.spec.question_panel.args.multiple"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: sessions_yield_tool::TOOL_SESSIONS_YIELD.to_string(),
            title: None,
            description: t("tool.spec.sessions_yield.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("message")
                    .desc(t("tool.spec.sessions_yield.args.message"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "定时任务".to_string(),
            title: None,
            description: t("tool.spec.schedule_task.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.schedule_task.args.action"))
                    .enums(vec!["add", "update", "remove", "enable", "disable", "get", "list", "run", "status"]),
                super::schema::string("job_id")
                    .desc(t("tool.spec.schedule_task.args.job.job_id"))
                    .optional(),
                super::schema::string("name")
                    .desc(t("tool.spec.schedule_task.args.job.name"))
                    .optional(),
                super::schema::object_param("schedule")
                    .props(vec![
                        super::schema::string("kind")
                            .desc(t("tool.spec.schedule_task.args.job.schedule.kind"))
                            .enums(vec!["at", "every", "cron"]),
                        super::schema::string("at")
                            .desc(t("tool.spec.schedule_task.args.job.schedule.at"))
                            .optional(),
                        super::schema::integer("every_ms")
                            .desc(t("tool.spec.schedule_task.args.job.schedule.every_ms"))
                            .min(1000)
                            .optional(),
                        super::schema::string("cron")
                            .desc(t("tool.spec.schedule_task.args.job.schedule.cron"))
                            .optional(),
                        super::schema::string("tz")
                            .desc(t("tool.spec.schedule_task.args.job.schedule.tz"))
                            .optional(),
                    ])
                    .closed()
                    .optional(),
                super::schema::string("schedule_text")
                    .desc(t("tool.spec.schedule_task.args.job.schedule_text"))
                    .optional(),
                super::schema::string("session")
                    .desc(t("tool.spec.schedule_task.args.job.session"))
                    .enums(vec!["main", "isolated"])
                    .optional(),
                super::schema::string("message")
                    .desc(t("tool.spec.schedule_task.args.job.payload.message"))
                    .optional(),
                super::schema::boolean("enabled")
                    .desc(t("tool.spec.schedule_task.args.job.enabled"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "记忆管理".to_string(),
            title: None,
            description: t("tool.spec.memory_manager.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.memory_manager.args.action"))
                    .enums(vec!["list", "search", "get", "add", "update", "remove", "clear"]),
                super::schema::string("memory_id")
                    .desc(t("tool.spec.memory_manager.args.memory_id"))
                    .optional(),
                super::schema::string("title")
                    .desc(t("tool.spec.memory_manager.args.title"))
                    .optional(),
                super::schema::string("content")
                    .desc(t("tool.spec.memory_manager.args.content"))
                    .optional(),
                super::schema::string("tag")
                    .desc(t("tool.spec.memory_manager.args.tag"))
                    .optional(),
                super::schema::string("related_memory_id")
                    .desc(t("tool.spec.memory_manager.args.related_memory_id"))
                    .optional(),
                super::schema::string("memory_time")
                    .desc(t("tool.spec.memory_manager.args.memory_time"))
                    .optional(),
                super::schema::string("query")
                    .desc(t("tool.spec.memory_manager.args.query"))
                    .optional(),
                super::schema::integer("limit")
                    .desc(t("tool.spec.memory_manager.args.limit"))
                    .min(1)
                    .max(200)
                    .optional(),
                super::schema::string("order")
                    .desc(t("tool.spec.memory_manager.args.order"))
                    .enums(vec!["desc", "asc"])
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "执行命令".to_string(),
            title: None,
            description: exec_tool_description(&t),
            input_schema: super::schema::object(vec![
                super::schema::string("description").desc(t("tool.spec.exec.args.description")),
                super::schema::string("content").desc(t("tool.spec.exec.args.content")),
                super::schema::string("workdir")
                    .desc(t("tool.spec.exec.args.workdir"))
                    .optional(),
                super::schema::number("timeout_s")
                    .desc(t("tool.spec.exec.args.timeout"))
                    .optional(),
                super::schema::boolean("run_in_background")
                    .desc(t("tool.spec.exec.args.run_in_background"))
                    .optional(),
                super::schema::integer("yield_time_ms")
                    .desc("Only with run_in_background=true: brief wait before returning the background command session; default 750ms.")
                    .min(50)
                    .max(10000)
                    .optional(),
                super::schema::boolean("dry_run")
                    .desc("Validate command only without execution.")
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "命令会话".to_string(),
            title: None,
            description: t("tool.spec.command_session.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.command_session.args.action"))
                    .enums(vec!["poll", "write_stdin"])
                    .optional(),
                super::schema::string("command_session_id")
                    .desc(t("tool.spec.command_session.args.id")),
                super::schema::string("input")
                    .desc(t("tool.spec.command_session.args.input"))
                    .optional(),
                super::schema::integer("after_seq")
                    .desc(t("tool.spec.command_session.args.after_seq"))
                    .min(0)
                    .optional(),
                super::schema::integer("yield_time_ms")
                    .desc(t("tool.spec.command_session.args.yield_time_ms"))
                    .min(0)
                    .max(60000)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "ptc".to_string(),
            title: None,
            description: t("tool.spec.ptc.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("filename")
                    .desc(t("tool.spec.ptc.args.filename")),
                super::schema::string("workdir")
                    .desc(t("tool.spec.ptc.args.workdir"))
                    .optional(),
                super::schema::string("content")
                    .desc(t("tool.spec.ptc.args.content")),
            ]),
        },
        ToolSpec {
            name: "列出文件".to_string(),
            title: None,
            description: t("tool.spec.list.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("path")
                    .desc(t("tool.spec.list.args.path"))
                    .optional(),
                super::schema::integer("max_depth").min(0).optional(),
                super::schema::string("cursor")
                    .desc(t("tool.spec.list.args.cursor"))
                    .optional(),
                super::schema::integer("limit")
                    .desc(t("tool.spec.list.args.limit"))
                    .min(1)
                    .max(500)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "glob".to_string(),
            title: None,
            description: t("tool.spec.glob.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("pattern").desc(t("tool.spec.glob.args.pattern")),
                super::schema::string("path")
                    .desc(t("tool.spec.glob.args.path"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "搜索内容".to_string(),
            title: None,
            description: t("tool.spec.search.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("query").desc(t("tool.spec.search.args.query")),
                super::schema::string("pattern")
                    .desc(t("tool.spec.search.args.pattern"))
                    .optional(),
                super::schema::string("path")
                    .desc(t("tool.spec.search.args.path"))
                    .optional(),
                super::schema::string("glob")
                    .desc(t("tool.spec.search.args.glob"))
                    .optional(),
                super::schema::string("include")
                    .desc(t("tool.spec.search.args.include"))
                    .optional(),
                super::schema::string("query_mode")
                    .desc(t("tool.spec.search.args.query_mode"))
                    .enums(vec!["literal", "regex"])
                    .optional(),
                super::schema::boolean("case_sensitive")
                    .desc(t("tool.spec.search.args.case_sensitive"))
                    .optional(),
                super::schema::integer("max_matches")
                    .desc("Maximum number of matches to return (default 200).")
                    .min(1)
                    .max(2000)
                    .optional(),
                super::schema::integer("timeout_ms")
                    .desc("Search timeout in milliseconds (default 30000).")
                    .min(1)
                    .max(120000)
                    .optional(),
                super::schema::integer("context_before")
                    .desc(t("tool.spec.search.args.context_before"))
                    .min(0)
                    .max(20)
                    .optional(),
                super::schema::integer("context_after")
                    .desc(t("tool.spec.search.args.context_after"))
                    .min(0)
                    .max(20)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "读取文件".to_string(),
            title: None,
            description: t("tool.spec.read.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("path").desc(t("tool.spec.read.args.path")),
                super::schema::integer("start_line")
                    .desc(t("tool.spec.read.args.start_line"))
                    .optional(),
                super::schema::integer("end_line")
                    .desc(t("tool.spec.read.args.end_line"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: read_image_tool::TOOL_READ_IMAGE.to_string(),
            title: None,
            description: t("tool.spec.read_image.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("path").desc(t("tool.spec.read_image.args.path")),
                super::schema::number("frame_rate")
                    .desc(t("tool.spec.read_image.args.frame_rate"))
                    .optional(),
                super::schema::integer("frame_step")
                    .desc(t("tool.spec.read_image.args.frame_step"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: multimodal_generation_tool::TOOL_GENERATE_SPEECH.to_string(),
            title: None,
            description: t("tool.spec.generate_speech.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("text").desc(t("tool.spec.generate_speech.args.text")),
                super::schema::string("path")
                    .desc(t("tool.spec.generate_speech.args.path"))
                    .optional(),
                super::schema::string("model_name")
                    .desc(t("tool.spec.generate_speech.args.model_name"))
                    .optional(),
                super::schema::string("voice")
                    .desc(t("tool.spec.generate_speech.args.voice"))
                    .optional(),
                super::schema::string("instructions")
                    .desc(t("tool.spec.generate_speech.args.instructions"))
                    .optional(),
                super::schema::string("response_format")
                    .desc(t("tool.spec.generate_speech.args.response_format"))
                    .optional(),
                super::schema::number("speed")
                    .desc(t("tool.spec.generate_speech.args.speed"))
                    .optional(),
                super::schema::string("reference_path")
                    .desc(t("tool.spec.generate_speech.args.reference_path"))
                    .optional(),
                super::schema::string("ref_audio")
                    .desc(t("tool.spec.generate_speech.args.ref_audio"))
                    .optional(),
                super::schema::string("ref_text")
                    .desc(t("tool.spec.generate_speech.args.ref_text"))
                    .optional(),
                super::schema::object_param("model_specific_params")
                    .desc(t("tool.spec.generate_speech.args.model_specific_params"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH.to_string(),
            title: None,
            description: t("tool.spec.transcribe_speech.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("path").desc(t("tool.spec.transcribe_speech.args.path")),
                super::schema::string("source_public_path")
                    .desc(t("tool.spec.transcribe_speech.args.source_public_path"))
                    .optional(),
                super::schema::string("model_name")
                    .desc(t("tool.spec.transcribe_speech.args.model_name"))
                    .optional(),
                super::schema::string("language")
                    .desc(t("tool.spec.transcribe_speech.args.language"))
                    .optional(),
                super::schema::string("prompt")
                    .desc(t("tool.spec.transcribe_speech.args.prompt"))
                    .optional(),
                super::schema::string("response_format")
                    .desc(t("tool.spec.transcribe_speech.args.response_format"))
                    .optional(),
                super::schema::number("temperature")
                    .desc(t("tool.spec.transcribe_speech.args.temperature"))
                    .optional(),
                super::schema::string("filename")
                    .desc(t("tool.spec.transcribe_speech.args.filename"))
                    .optional(),
                super::schema::string("content_type")
                    .desc(t("tool.spec.transcribe_speech.args.content_type"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: multimodal_generation_tool::TOOL_GENERATE_IMAGE.to_string(),
            title: None,
            description: t("tool.spec.generate_image.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("prompt").desc(t("tool.spec.generate_image.args.prompt")),
                super::schema::string("path")
                    .desc(t("tool.spec.generate_image.args.path"))
                    .optional(),
                super::schema::string("model_name")
                    .desc(t("tool.spec.generate_image.args.model_name"))
                    .optional(),
                super::schema::string("size")
                    .desc(t("tool.spec.generate_image.args.size"))
                    .optional(),
                super::schema::string("output_format")
                    .desc(t("tool.spec.generate_image.args.output_format"))
                    .optional(),
                super::schema::string("negative_prompt")
                    .desc(t("tool.spec.generate_image.args.negative_prompt"))
                    .optional(),
                super::schema::integer("num_inference_steps")
                    .desc(t("tool.spec.generate_image.args.num_inference_steps"))
                    .optional(),
                super::schema::number("guidance_scale")
                    .desc(t("tool.spec.generate_image.args.guidance_scale"))
                    .optional(),
                super::schema::integer("seed")
                    .desc(t("tool.spec.generate_image.args.seed"))
                    .optional(),
                super::schema::string("input_path")
                    .desc(t("tool.spec.generate_image.args.input_path"))
                    .optional(),
                super::schema::array("input_paths")
                    .desc(t("tool.spec.generate_image.args.input_paths"))
                    .items(super::schema::string("_"))
                    .optional(),
                super::schema::string("mask_path")
                    .desc(t("tool.spec.generate_image.args.mask_path"))
                    .optional(),
                super::schema::string("reference_path")
                    .desc(t("tool.spec.generate_image.args.reference_path"))
                    .optional(),
                super::schema::number("strength")
                    .desc(t("tool.spec.generate_image.args.strength"))
                    .optional(),
                super::schema::number("true_cfg_scale")
                    .desc(t("tool.spec.generate_image.args.true_cfg_scale"))
                    .optional(),
                super::schema::integer("output_compression")
                    .desc(t("tool.spec.generate_image.args.output_compression"))
                    .optional(),
                super::schema::integer("layers")
                    .desc(t("tool.spec.generate_image.args.layers"))
                    .optional(),
                super::schema::integer("resolution")
                    .desc(t("tool.spec.generate_image.args.resolution"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: multimodal_generation_tool::TOOL_GENERATE_VIDEO.to_string(),
            title: None,
            description: t("tool.spec.generate_video.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("prompt").desc(t("tool.spec.generate_video.args.prompt")),
                super::schema::string("path")
                    .desc(t("tool.spec.generate_video.args.path"))
                    .optional(),
                super::schema::string("model_name")
                    .desc(t("tool.spec.generate_video.args.model_name"))
                    .optional(),
                super::schema::string("size")
                    .desc(t("tool.spec.generate_video.args.size"))
                    .optional(),
                super::schema::number("seconds")
                    .desc(t("tool.spec.generate_video.args.seconds"))
                    .optional(),
                super::schema::integer("fps")
                    .desc(t("tool.spec.generate_video.args.fps"))
                    .optional(),
                super::schema::integer("num_frames")
                    .desc(t("tool.spec.generate_video.args.num_frames"))
                    .optional(),
                super::schema::string("negative_prompt")
                    .desc(t("tool.spec.generate_video.args.negative_prompt"))
                    .optional(),
                super::schema::integer("num_inference_steps")
                    .desc(t("tool.spec.generate_video.args.num_inference_steps"))
                    .optional(),
                super::schema::number("guidance_scale")
                    .desc(t("tool.spec.generate_video.args.guidance_scale"))
                    .optional(),
                super::schema::number("guidance_scale_2")
                    .desc(t("tool.spec.generate_video.args.guidance_scale_2"))
                    .optional(),
                super::schema::number("boundary_ratio")
                    .desc(t("tool.spec.generate_video.args.boundary_ratio"))
                    .optional(),
                super::schema::number("flow_shift")
                    .desc(t("tool.spec.generate_video.args.flow_shift"))
                    .optional(),
                super::schema::integer("seed")
                    .desc(t("tool.spec.generate_video.args.seed"))
                    .optional(),
                super::schema::boolean("enable_frame_interpolation")
                    .desc(t("tool.spec.generate_video.args.enable_frame_interpolation"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "技能调用".to_string(),
            title: None,
            description: t("tool.spec.skill_call.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("name")
                    .desc(t("tool.spec.skill_call.args.name")),
            ]),
        },
        ToolSpec {
            name: "写入文件".to_string(),
            title: None,
            description: t("tool.spec.write.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("path")
                    .desc(t("tool.spec.write.args.path")),
                super::schema::string("content")
                    .desc(t("tool.spec.write.args.content")),
                super::schema::boolean("dry_run")
                    .desc("Preview write target and size changes without writing to disk.")
                    .optional(),
            ]),
        },
        ToolSpec {
            // 由原 `编辑` 与 `str_replace_editor` 合并：字面替换为主，
            // 兼容补丁（input）与子命令（command）两种形态。仅 file_path 必填，
            // 其余按形态在运行时校验。
            name: "文本编辑".to_string(),
            title: None,
            description: t("tool.spec.text_edit.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("file_path").desc(t("tool.spec.text_edit.args.file_path")),
                super::schema::string("old_string")
                    .desc(t("tool.spec.text_edit.args.old_string"))
                    .optional(),
                super::schema::string("new_string")
                    .desc(t("tool.spec.text_edit.args.new_string"))
                    .optional(),
                super::schema::boolean("replace_all")
                    .desc(t("tool.spec.text_edit.args.replace_all"))
                    .optional(),
                super::schema::string("input")
                    .desc(t("tool.spec.text_edit.args.input"))
                    .optional(),
                super::schema::boolean("dry_run")
                    .desc(t("tool.spec.text_edit.args.dry_run"))
                    .optional(),
                super::schema::string("command")
                    .desc(t("tool.spec.text_edit.args.command"))
                    .enums(vec!["view", "create", "str_replace", "insert"])
                    .optional(),
                super::schema::string("file_text")
                    .desc(t("tool.spec.text_edit.args.file_text"))
                    .optional(),
                super::schema::integer("insert_line")
                    .desc(t("tool.spec.text_edit.args.insert_line"))
                    .optional(),
                super::schema::string("new_str")
                    .desc(t("tool.spec.text_edit.args.new_str"))
                    .optional(),
                super::schema::string("old_str")
                    .desc(t("tool.spec.text_edit.args.old_str"))
                    .optional(),
                super::schema::array("view_range")
                    .desc(t("tool.spec.text_edit.args.view_range"))
                    .items(super::schema::integer("_"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: "子智能体控制".to_string(),
            title: None,
            description: t("tool.spec.subagent_control.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.subagent_control.args.action"))
                    .enums(vec!["list", "history", "send", "report", "spawn", "batch_spawn", "status", "wait", "interrupt", "close", "resume"]),
                super::schema::integer("limit")
                    .desc(t("tool.spec.sessions_list.args.limit"))
                    .min(1)
                    .optional(),
                super::schema::string("parent_id")
                    .desc(t("tool.spec.subagent_control.args.parent_id"))
                    .optional(),
                super::schema::string("session_id")
                    .desc(t("tool.spec.subagent_control.args.session_id"))
                    .optional(),
                super::schema::array("session_ids")
                    .desc(t("tool.spec.subagent_control.args.session_ids"))
                    .items(super::schema::string("_"))
                    .optional(),
                super::schema::string("run_id")
                    .desc(t("tool.spec.subagent_control.args.run_id"))
                    .optional(),
                super::schema::array("run_ids")
                    .desc(t("tool.spec.subagent_control.args.run_ids"))
                    .items(super::schema::string("_"))
                    .optional(),
                super::schema::string("dispatch_id")
                    .desc(t("tool.spec.subagent_control.args.dispatch_id"))
                    .optional(),
                super::schema::string("strategy")
                    .desc(t("tool.spec.subagent_control.args.strategy"))
                    .enums(vec!["parallel_all", "first_success", "review_then_merge"])
                    .optional(),
                super::schema::string("remaining_action")
                    .desc(t("tool.spec.subagent_control.args.remaining_action"))
                    .enums(vec!["keep", "interrupt", "close"])
                    .optional(),
                super::schema::boolean("include_tools")
                    .desc(t("tool.spec.sessions_history.args.include_tools"))
                    .optional(),
                super::schema::string("message")
                    .desc(t("tool.spec.subagent_control.args.message"))
                    .min_len(1)
                    .max_len(20000)
                    .optional(),
                super::schema::string("message_id")
                    .desc("Optional retry id for running-turn messages or reports. Reuse the same id only for the same message.")
                    .max_len(128)
                    .optional(),
                super::schema::number("timeout_seconds")
                    .desc(t("tool.spec.sessions_send.args.timeout"))
                    .optional(),
                super::schema::integer("fork_turns")
                    .desc("For spawn/batch_spawn: copy up to N recent user turns as quoted background, default 0. At most 256 history items and 64 KiB; truncation is recorded. Does not alter the system prompt.")
                    .min(0)
                    .max(16)
                    .optional(),
                super::schema::string("context_summary")
                    .desc("Optional selected background for spawn/batch_spawn, at most 16384 UTF-8 bytes. Reference data, not system instructions; persists with the first task only.")
                    .max_len(16384)
                    .optional(),
                super::schema::string("task")
                    .desc(t("tool.spec.subagent_control.args.task"))
                    .optional(),
                super::schema::array("tasks")
                    .desc(t("tool.spec.subagent_control.args.tasks"))
                    .max_items(64)
                    .items(
                        super::schema::object_param("_")
                            .props(vec![
                                super::schema::integer("fork_turns")
                                    .desc("Override the batch context turn count for this worker.")
                                    .min(0)
                                    .max(16)
                                    .optional(),
                                super::schema::string("context_summary")
                                    .desc("Override the batch background summary for this worker.")
                                    .max_len(16384)
                                    .optional(),
                                super::schema::string("task")
                                    .desc(t("tool.spec.sessions_spawn.args.task")),
                                super::schema::string("label")
                                    .desc(t("tool.spec.sessions_spawn.args.label"))
                                    .optional(),
                                super::schema::string("agent_id")
                                    .desc(t("tool.spec.sessions_spawn.args.agent_id"))
                                    .optional(),
                                super::schema::string("model")
                                    .desc(t("tool.spec.sessions_spawn.args.model"))
                                    .optional(),
                                super::schema::number("run_timeout_seconds")
                                    .desc(t("tool.spec.sessions_spawn.args.timeout"))
                                    .optional(),
                                super::schema::string("cleanup")
                                    .desc(t("tool.spec.sessions_spawn.args.cleanup"))
                                    .enums(vec!["keep", "delete"])
                                    .optional(),
                            ])
                            .closed(),
                    )
                    .optional(),
                super::schema::string("label")
                    .desc(t("tool.spec.sessions_spawn.args.label"))
                    .optional(),
                super::schema::string("agent_id")
                    .desc(t("tool.spec.sessions_spawn.args.agent_id"))
                    .optional(),
                super::schema::string("model")
                    .desc(t("tool.spec.sessions_spawn.args.model"))
                    .optional(),
                super::schema::number("run_timeout_seconds")
                    .desc(t("tool.spec.sessions_spawn.args.timeout"))
                    .optional(),
                super::schema::string("cleanup")
                    .desc(t("tool.spec.sessions_spawn.args.cleanup"))
                    .enums(vec!["keep", "delete"])
                    .optional(),
                super::schema::number("wait_seconds")
                    .desc(t("tool.spec.subagent_control.args.wait_seconds"))
                    .optional(),
                super::schema::number("poll_interval_seconds")
                    .desc(t("tool.spec.subagent_control.args.poll_interval_seconds"))
                    .optional(),
                super::schema::string("wait_mode")
                    .desc(t("tool.spec.subagent_control.args.wait_mode"))
                    .enums(vec!["all", "any", "first_success"])
                    .optional(),
            ]),
        },
        ToolSpec {
            name: thread_control_tool::TOOL_THREAD_CONTROL.to_string(),
            title: None,
            description: t("tool.spec.thread_control.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.thread_control.args.action"))
                    .enums(vec![
                        "list",
                        "info",
                        "create",
                        "switch",
                        "back",
                        "update_title",
                        "archive",
                        "restore",
                    ]),
                super::schema::string("session_id")
                    .desc(t("tool.spec.thread_control.args.session_id"))
                    .optional(),
                super::schema::string("parent_session_id")
                    .desc(t("tool.spec.thread_control.args.parent_session_id"))
                    .optional(),
                super::schema::string("title")
                    .desc(t("tool.spec.thread_control.args.title"))
                    .optional(),
                super::schema::string("scope")
                    .desc(t("tool.spec.thread_control.args.scope"))
                    .enums(vec!["branch", "children", "roots", "all"])
                    .optional(),
                super::schema::string("status")
                    .desc(t("tool.spec.thread_control.args.status"))
                    .enums(vec!["active", "archived", "all"])
                    .optional(),
                super::schema::integer("limit")
                    .desc(t("tool.spec.thread_control.args.limit"))
                    .min(1)
                    .max(200)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: web_search_tool::TOOL_WEB_SEARCH.to_string(),
            title: None,
            description: t("tool.spec.web_search.description"),
            input_schema: super::schema::object(vec![
                super::schema::array("queries")
                    .desc(t("tool.spec.web_search.args.queries"))
                    .items(super::schema::string("_"))
                    .min_items(1)
                    .max_items(4),
                super::schema::string("query")
                    .desc(t("tool.spec.web_search.args.query"))
                    .optional(),
                super::schema::integer("count")
                    .desc(t("tool.spec.web_search.args.count"))
                    .min(1)
                    .max(10)
                    .optional(),
                super::schema::string("site")
                    .desc(t("tool.spec.web_search.args.site"))
                    .optional(),
                super::schema::array("sites")
                    .desc(t("tool.spec.web_search.args.sites"))
                    .items(super::schema::string("_"))
                    .optional(),
                super::schema::boolean("scrape_results")
                    .desc(t("tool.spec.web_search.args.scrape_results"))
                    .optional(),
                super::schema::integer("max_result_chars")
                    .desc(t("tool.spec.web_search.args.max_result_chars"))
                    .min(120)
                    .max(4000)
                    .optional(),
                super::schema::array("sources")
                    .desc(t("tool.spec.web_search.args.sources"))
                    .items(super::schema::string("_"))
                    .optional(),
                super::schema::array("categories")
                    .desc(t("tool.spec.web_search.args.categories"))
                    .items(super::schema::string("_"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: web_fetch_tool::TOOL_WEB_FETCH.to_string(),
            title: None,
            description: t("tool.spec.web_fetch.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("url")
                    .desc(t("tool.spec.web_fetch.args.url")),
                super::schema::string("extract_mode")
                    .desc(t("tool.spec.web_fetch.args.extract_mode"))
                    .enums(vec!["markdown", "text"])
                    .optional(),
                super::schema::integer("max_chars")
                    .desc(t("tool.spec.web_fetch.args.max_chars"))
                    .min(100)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: browser_tool::TOOL_BROWSER.to_string(),
            title: None,
            description: t("tool.spec.browser.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("action")
                    .desc(t("tool.spec.browser.args.action"))
                    .enums(vec![
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
                        "read_page",
                    ]),
                super::schema::string("browser_session_id")
                    .desc(t("tool.spec.browser.args.browser_session_id"))
                    .optional(),
                super::schema::string("target_id")
                    .desc(t("tool.spec.browser.args.target_id"))
                    .optional(),
                super::schema::string("url")
                    .desc(t("tool.spec.browser.args.url"))
                    .optional(),
                super::schema::string("path")
                    .desc(t("tool.spec.browser.args.path"))
                    .optional(),
                super::schema::string("format")
                    .desc(t("tool.spec.browser.args.format"))
                    .optional(),
                super::schema::string("ref")
                    .desc(t("tool.spec.browser.args.ref"))
                    .optional(),
                super::schema::string("selector")
                    .desc(t("tool.spec.browser.args.selector"))
                    .optional(),
                super::schema::string("text")
                    .desc(t("tool.spec.browser.args.text"))
                    .optional(),
                super::schema::string("key")
                    .desc(t("tool.spec.browser.args.key"))
                    .optional(),
                super::schema::boolean("full_page")
                    .desc(t("tool.spec.browser.args.full_page"))
                    .optional(),
                super::schema::integer("max_chars")
                    .desc(t("tool.spec.browser.args.max_chars"))
                    .min(1)
                    .optional(),
                super::schema::integer("timeout_ms")
                    .desc(t("tool.spec.browser.args.timeout_ms"))
                    .min(1)
                    .max(120000)
                    .optional(),
                super::schema::integer("timeout_secs")
                    .desc(t("tool.spec.browser.args.timeout_secs"))
                    .min(1)
                    .max(120)
                    .optional(),
            ]),
        },
        ToolSpec {
            name: desktop_control::TOOL_DESKTOP_CONTROLLER.to_string(),
            title: None,
            description: t("tool.spec.desktop_controller.description"),
            input_schema: super::schema::object(vec![
                super::schema::array("bbox")
                    .desc(t("tool.spec.desktop_controller.args.bbox"))
                    .items(super::schema::integer("_"))
                    .any_of(vec![
                        json!({"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4}),
                        json!({"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}),
                    ]),
                super::schema::string("action")
                    .desc(t("tool.spec.desktop_controller.args.action"))
                    .enums(vec![
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
                        "drag_drop",
                    ]),
                super::schema::string("key")
                    .desc(t("tool.spec.desktop_controller.args.key"))
                    .optional(),
                super::schema::string("text")
                    .desc(t("tool.spec.desktop_controller.args.text"))
                    .optional(),
                super::schema::integer("delay_ms")
                    .desc(t("tool.spec.desktop_controller.args.delay_ms"))
                    .min(0)
                    .optional(),
                super::schema::integer("duration_ms")
                    .desc(t("tool.spec.desktop_controller.args.duration_ms"))
                    .min(0)
                    .optional(),
                super::schema::integer("scroll_steps")
                    .desc(t("tool.spec.desktop_controller.args.scroll_steps"))
                    .min(1)
                    .optional(),
                super::schema::array("to_bbox")
                    .desc(t("tool.spec.desktop_controller.args.to_bbox"))
                    .items(super::schema::integer("_"))
                    .any_of(vec![
                        json!({"type": "array", "items": {"type": "integer"}, "minItems": 4, "maxItems": 4}),
                        json!({"type": "array", "items": {"type": "integer"}, "minItems": 2, "maxItems": 2}),
                    ])
                    .optional(),
            ]),
        },
        ToolSpec {
            name: desktop_control::TOOL_DESKTOP_MONITOR.to_string(),
            title: None,
            description: t("tool.spec.desktop_monitor.description"),
            input_schema: super::schema::object(vec![
                super::schema::integer("wait_ms")
                    .desc(t("tool.spec.desktop_monitor.args.wait_ms"))
                    .min(0)
                    .max(30000),
                super::schema::string("note")
                    .desc(t("tool.spec.desktop_monitor.args.note"))
                    .optional(),
            ]),
        },
        ToolSpec {
            name: self_status_tool::TOOL_SELF_STATUS.to_string(),
            title: None,
            description: t("tool.spec.self_status.description"),
            input_schema: super::schema::object(vec![
                super::schema::string("detail_level")
                    .desc(t("tool.spec.self_status.args.detail_level"))
                    .enums(vec!["basic", "standard", "full"])
                    .optional(),
                super::schema::boolean("include_events")
                    .desc(t("tool.spec.self_status.args.include_events"))
                    .optional(),
                super::schema::integer("events_limit")
                    .desc(t("tool.spec.self_status.args.events_limit"))
                    .min(1)
                    .max(200)
                    .optional(),
            ]),
        },
    ];
    specs.extend(goal::goal_tool_specs());
    for spec in specs.iter_mut() {
        if spec.title.is_none() {
            if let Some(title) = localized_tool_title(&spec.name, language) {
                spec.title = Some(title);
            }
        }
    }
    specs
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
    map.insert("update_plan".to_string(), "计划面板".to_string());
    map.insert("question_panel".to_string(), "问询面板".to_string());
    map.insert("ask_panel".to_string(), "问询面板".to_string());
    map.insert("schedule_task".to_string(), "定时任务".to_string());
    map.insert("memory_manager".to_string(), "记忆管理".to_string());
    map.insert("memory_manage".to_string(), "记忆管理".to_string());
    map.insert("goal".to_string(), goal::TOOL_UPDATE_GOAL.to_string());
    map.insert("execute_command".to_string(), "执行命令".to_string());
    map.insert("command_session".to_string(), "命令会话".to_string());
    map.insert("write_command_stdin".to_string(), "命令会话".to_string());
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
    map.insert(
        multimodal_generation_tool::TOOL_GENERATE_SPEECH_ALIAS.to_string(),
        multimodal_generation_tool::TOOL_GENERATE_SPEECH.to_string(),
    );
    map.insert(
        multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH_ALIAS.to_string(),
        multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH.to_string(),
    );
    map.insert(
        multimodal_generation_tool::TOOL_GENERATE_IMAGE_ALIAS.to_string(),
        multimodal_generation_tool::TOOL_GENERATE_IMAGE.to_string(),
    );
    map.insert(
        multimodal_generation_tool::TOOL_GENERATE_IMAGE_LEGACY.to_string(),
        multimodal_generation_tool::TOOL_GENERATE_IMAGE.to_string(),
    );
    map.insert(
        multimodal_generation_tool::TOOL_GENERATE_VIDEO_ALIAS.to_string(),
        multimodal_generation_tool::TOOL_GENERATE_VIDEO.to_string(),
    );
    map.insert("skill_call".to_string(), "技能调用".to_string());
    map.insert("skill_get".to_string(), "技能调用".to_string());
    map.insert("write_file".to_string(), "写入文件".to_string());
    map.insert("apply_patch".to_string(), "文本编辑".to_string());
    map.insert("应用补丁".to_string(), "文本编辑".to_string());
    map.insert("patch".to_string(), "文本编辑".to_string());
    map.insert("edit".to_string(), "文本编辑".to_string());
    map.insert("edit_file".to_string(), "文本编辑".to_string());
    // 合并前的两个规范名保留为别名，保证历史调用与旧前端仍可解析。
    map.insert("编辑".to_string(), "文本编辑".to_string());
    map.insert("str_replace_editor".to_string(), "文本编辑".to_string());
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
        web_search_tool::TOOL_WEB_SEARCH_ALIAS.to_string(),
        web_search_tool::TOOL_WEB_SEARCH.to_string(),
    );
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

pub async fn build_read_image_followup_user_message(
    context: &ToolContext<'_>,
    result: &Value,
) -> Result<Option<Value>> {
    read_image_tool::build_followup_user_message(context, result).await
}

fn is_desktop_mode(config: &Config) -> bool {
    config.server.mode.trim().eq_ignore_ascii_case("desktop")
}

fn runtime_builtin_tool_allowed(config: &Config, canonical: &str) -> bool {
    // 临时下架（网页搜索 / ptc）：无论配置是否启用，都不对智能体与用户暴露。
    if crate::services::default_tool_profile::is_temporarily_hidden_tool_name(canonical) {
        return false;
    }
    if web_fetch_tool::is_web_fetch_tool_name(canonical)
        && !web_fetch_tool::web_fetch_enabled(config)
    {
        return false;
    }
    if web_search_tool::is_web_search_tool_name(canonical)
        && !web_search_tool::web_search_enabled(config)
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
    if canonical == multimodal_generation_tool::TOOL_GENERATE_SPEECH
        && !multimodal_generation_tool::speech_tool_available(config)
    {
        return false;
    }
    if canonical == multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH
        && !multimodal_generation_tool::transcribe_tool_available(config)
    {
        return false;
    }
    if canonical == multimodal_generation_tool::TOOL_GENERATE_IMAGE
        && !multimodal_generation_tool::image_tool_available(config)
    {
        return false;
    }
    if canonical == multimodal_generation_tool::TOOL_GENERATE_VIDEO
        && !multimodal_generation_tool::video_tool_available(config)
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
            // goal 等系统级工具不对桌面端开放：goal 由运行时按会话自动注入，
            // 其余为内部协调工具，均不应出现在桌面工具选择器或默认启用集合中。
            .filter(|name| {
                !crate::services::default_tool_profile::is_desktop_hidden_tool_name(name)
            })
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

pub fn build_runtime_tool_display_map(config: &Config) -> HashMap<String, String> {
    let language = i18n::get_language();
    let prefer_alias = language.to_lowercase().starts_with("en");
    let aliases_by_name = {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        for (alias, canonical) in builtin_aliases() {
            map.entry(canonical).or_default().push(alias);
        }
        for aliases in map.values_mut() {
            aliases.sort();
        }
        map
    };
    let mut display_map = HashMap::new();
    for spec in builtin_tool_specs() {
        let runtime_name = spec.name.trim().to_string();
        if runtime_name.is_empty() {
            continue;
        }
        let display_name = if prefer_alias {
            aliases_by_name
                .get(&runtime_name)
                .and_then(|aliases| aliases.first())
                .cloned()
                .unwrap_or_else(|| runtime_name.clone())
        } else {
            localized_tool_title(&runtime_name, &language).unwrap_or_else(|| runtime_name.clone())
        };
        display_map.insert(runtime_name, display_name);
    }
    for entry in build_mcp_tool_alias_entries(config) {
        display_map.insert(entry.runtime_name, entry.display_name);
    }
    display_map
}

pub fn resolve_runtime_tool_display_name(config: &Config, runtime_name: &str) -> String {
    let trimmed = runtime_name.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    build_runtime_tool_display_map(config)
        .get(trimmed)
        .cloned()
        .unwrap_or_else(|| trimmed.to_string())
}

fn preferred_english_alias(canonical: &str) -> Option<&'static str> {
    match canonical {
        sessions_yield_tool::TOOL_SESSIONS_YIELD => {
            Some(sessions_yield_tool::TOOL_SESSIONS_YIELD_ALIAS)
        }
        "问询面板" => Some("question_panel"),
        "技能调用" => Some("skill_call"),
        // 合并后以字面替换为主，模型侧优先暴露 `edit`。
        "文本编辑" => Some("edit"),
        thread_control_tool::TOOL_THREAD_CONTROL => {
            Some(thread_control_tool::TOOL_THREAD_CONTROL_ALIAS)
        }
        "记忆管理" => Some("memory_manager"),
        web_search_tool::TOOL_WEB_SEARCH => Some(web_search_tool::TOOL_WEB_SEARCH_ALIAS),
        web_fetch_tool::TOOL_WEB_FETCH => Some(web_fetch_tool::TOOL_WEB_FETCH_ALIAS),
        browser_tool::TOOL_BROWSER => Some("browser"),
        desktop_control::TOOL_DESKTOP_CONTROLLER => Some("desktop_controller"),
        desktop_control::TOOL_DESKTOP_MONITOR => Some("desktop_monitor"),
        self_status_tool::TOOL_SELF_STATUS => Some(self_status_tool::TOOL_SELF_STATUS_ALIAS),
        read_image_tool::TOOL_READ_IMAGE => Some(read_image_tool::TOOL_READ_IMAGE_ALIAS),
        multimodal_generation_tool::TOOL_GENERATE_SPEECH => {
            Some(multimodal_generation_tool::TOOL_GENERATE_SPEECH_ALIAS)
        }
        multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH => {
            Some(multimodal_generation_tool::TOOL_TRANSCRIBE_SPEECH_ALIAS)
        }
        multimodal_generation_tool::TOOL_GENERATE_IMAGE => {
            Some(multimodal_generation_tool::TOOL_GENERATE_IMAGE_ALIAS)
        }
        multimodal_generation_tool::TOOL_GENERATE_VIDEO => {
            Some(multimodal_generation_tool::TOOL_GENERATE_VIDEO_ALIAS)
        }
        canonical if goal::is_goal_tool_name(canonical) => Some("goal"),
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

/// 汇总系统当前可用的工具名称（包含内置别名、MCP、技能与用户工具）。
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
        if server.packaged {
            if let Some(spec) = mcp_pack::spec_for_server(server) {
                names.insert(spec.name);
            }
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

pub fn collect_enabled_tool_names_for_catalog(
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
        enabled_builtin.insert(browser_tool::TOOL_BROWSER.to_string());
        names.insert(browser_tool::TOOL_BROWSER.to_string());
    }
    for server in &config.mcp.servers {
        if !server.enabled {
            continue;
        }
        if server.packaged {
            if let Some(spec) = mcp_pack::spec_for_server(server) {
                names.insert(spec.name);
            }
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
            title: None,
            description: spec.description.clone(),
            input_schema: spec.input_schema.clone(),
        });
    }
    let mcp_alias_entries = build_mcp_tool_alias_entries(config);
    let mut mcp_tool_lookup: HashMap<String, ToolSpec> = HashMap::new();
    for server in config.mcp.servers.iter().filter(|server| server.enabled) {
        if server.packaged {
            if let Some(spec) = mcp_pack::spec_for_server(server) {
                mcp_tool_lookup.insert(spec.name.clone(), spec);
            }
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
            mcp_tool_lookup.insert(
                format!("{}@{}", server.name, tool.name),
                ToolSpec {
                    name: tool.name.clone(),
                    title: tool.title.clone(),
                    description: tool.description.clone(),
                    input_schema: yaml_to_json(&tool.input_schema),
                },
            );
        }
    }
    for entry in mcp_alias_entries {
        if !allowed_names.contains(&entry.runtime_name) || !seen.insert(entry.display_name.clone())
        {
            continue;
        }
        let Some(spec) = mcp_tool_lookup.get(&entry.runtime_name) else {
            continue;
        };
        output.push(ToolSpec {
            name: entry.display_name.clone(),
            title: spec.title.clone(),
            description: spec.description.clone(),
            input_schema: spec.input_schema.clone(),
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
            title: None,
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

#[cfg(test)]
mod tests {
    use super::{
        build_mcp_tool_alias_entries, builtin_tool_specs_with_language,
        collect_available_tool_names, collect_enabled_tool_names_for_catalog,
        collect_prompt_tool_specs_with_language, resolve_tool_name,
    };
    use crate::config::Config;

    #[test]
    fn text_edit_tool_spec_is_registered() {
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "文本编辑")
            .expect("text edit spec");
        assert!(spec.description.to_lowercase().contains("edit"));
        // 仅 file_path 必填；old_string/new_string 在子命令与补丁形态下并非必需。
        let required = spec.input_schema["required"].as_array().expect("required");
        assert_eq!(required.len(), 1, "only file_path is required");
        assert!(required.iter().any(|v| v == "file_path"));
        assert_eq!(spec.input_schema["additionalProperties"], false);
        assert_eq!(
            spec.input_schema["properties"]["replace_all"]["type"],
            "boolean"
        );
        assert!(spec.input_schema["properties"]["file_path"].is_object());
        assert!(spec.input_schema["properties"]["command"].is_object());
        assert!(spec.input_schema["properties"]["view_range"].is_object());
        assert!(spec.input_schema["properties"]["edits"].is_null());
        // 旧名与旧规范名全部回落到合并后的规范名。
        for alias in [
            "edit",
            "edit_file",
            "apply_patch",
            "应用补丁",
            "patch",
            "编辑",
            "str_replace_editor",
            "文本编辑",
        ] {
            assert_eq!(resolve_tool_name(alias), "文本编辑", "alias {alias}");
        }
    }
    use crate::i18n;
    use crate::skills::SkillRegistry;
    use serde_json::Value;
    use std::collections::HashSet;

    #[test]
    fn command_session_catalog_is_localized_and_removed_tools_are_not_exposed() {
        let zh = builtin_tool_specs_with_language("zh-CN");
        let zh_session = zh
            .iter()
            .find(|spec| spec.name == "命令会话")
            .expect("Chinese command session spec");
        assert!(zh_session.description.contains("轮询后台命令"));
        assert!(
            zh_session.input_schema["properties"]["action"]["description"]
                .as_str()
                .is_some_and(|value| value.contains("操作"))
        );
        assert!(zh
            .iter()
            .all(|spec| spec.name != "LSP查询" && spec.name != "节点调用"));

        let en_session = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "命令会话")
            .expect("English command session spec");
        assert!(en_session.description.contains("Poll a background command"));
        assert!(
            en_session.input_schema["properties"]["action"]["description"]
                .as_str()
                .is_some_and(|value| value.starts_with("Action:"))
        );
        assert_eq!(resolve_tool_name("lsp"), "lsp");
        assert_eq!(resolve_tool_name("node_invoke"), "node_invoke");
    }

    #[test]
    fn read_file_spec_clarifies_plain_text_only_in_english() {
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "读取文件")
            .expect("read_file spec");
        assert!(spec.description.contains("plain-text"));
        assert!(spec.description.contains("cat"));
        assert!(spec.description.contains("read_image"));
        assert!(spec.description.contains(">>> path"));
        assert!(spec.description.contains("apply_patch"));
        let path_description = spec.input_schema["properties"]["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("plain-text"));
        assert!(path_description.contains("read_image"));
        assert!(path_description.contains(">>> path"));
        assert!(path_description.contains("N: "));
        assert!(spec.input_schema["properties"]["start_line"].is_object());
        assert!(spec.input_schema["properties"]["end_line"].is_object());
        assert!(spec.input_schema["properties"]["files"].is_null());
        assert!(spec.input_schema["properties"]["line_ranges"].is_null());
        assert!(spec.input_schema["properties"]["mode"].is_null());
        assert!(spec.input_schema["properties"]["indentation"].is_null());
        assert!(spec.input_schema["properties"]["file_path"].is_null());
        assert!(spec.input_schema["properties"]["dry_run"].is_null());
        assert!(spec.input_schema["anyOf"].is_null());
        assert_eq!(
            spec.input_schema["required"]
                .as_array()
                .map(|items| items.len()),
            Some(1)
        );
    }

    #[test]
    fn mcp_alias_prefers_plain_tool_name_when_unique() {
        let mut config = Config::default();
        config.mcp.servers = vec![crate::config::McpServerConfig {
            name: "extra_mcp".to_string(),
            endpoint: "http://127.0.0.1:9010/mcp".to_string(),
            enabled: true,
            tool_specs: vec![crate::config::McpToolSpec {
                name: "db_query_人员信息".to_string(),
                title: None,
                description: String::new(),
                input_schema: serde_yaml::Value::Mapping(Default::default()),
            }],
            ..Default::default()
        }];
        let aliases = build_mcp_tool_alias_entries(&config);
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].runtime_name, "extra_mcp@db_query_人员信息");
        assert_eq!(aliases[0].display_name, "db_query_人员信息");
    }

    #[test]
    fn mcp_alias_adds_server_suffix_when_tool_names_conflict() {
        let mut config = Config::default();
        config.mcp.servers = vec![
            crate::config::McpServerConfig {
                name: "extra_mcp".to_string(),
                endpoint: "http://127.0.0.1:9010/mcp".to_string(),
                enabled: true,
                tool_specs: vec![crate::config::McpToolSpec {
                    name: "search".to_string(),
                    title: None,
                    description: String::new(),
                    input_schema: serde_yaml::Value::Mapping(Default::default()),
                }],
                ..Default::default()
            },
            crate::config::McpServerConfig {
                name: "ragflow".to_string(),
                endpoint: "http://127.0.0.1:9380/mcp".to_string(),
                enabled: true,
                tool_specs: vec![crate::config::McpToolSpec {
                    name: "search".to_string(),
                    title: None,
                    description: String::new(),
                    input_schema: serde_yaml::Value::Mapping(Default::default()),
                }],
                ..Default::default()
            },
        ];
        let aliases = build_mcp_tool_alias_entries(&config);
        assert_eq!(aliases.len(), 2);
        assert!(aliases
            .iter()
            .any(|item| item.display_name == "search__extra_mcp"));
        assert!(aliases
            .iter()
            .any(|item| item.display_name == "search__ragflow"));
    }

    #[test]
    fn packaged_mcp_exposes_single_runtime_tool() {
        let mut config = Config::default();
        config.mcp.servers = vec![crate::config::McpServerConfig {
            name: "extra_mcp".to_string(),
            endpoint: "http://127.0.0.1:9010/mcp".to_string(),
            enabled: true,
            packaged: true,
            tool_specs: vec![crate::config::McpToolSpec {
                name: "search".to_string(),
                title: None,
                description: String::new(),
                input_schema: serde_yaml::Value::Mapping(Default::default()),
            }],
            ..Default::default()
        }];

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        assert!(available.contains("extra_mcp@__mcp_pack__"));
        assert!(!available.contains("extra_mcp@search"));
    }

    #[test]
    fn packaged_mcp_prompt_spec_uses_package_schema() {
        let mut config = Config::default();
        config.mcp.servers = vec![crate::config::McpServerConfig {
            name: "extra_mcp".to_string(),
            endpoint: "http://127.0.0.1:9010/mcp".to_string(),
            enabled: true,
            packaged: true,
            tool_specs: vec![crate::config::McpToolSpec {
                name: "search".to_string(),
                title: None,
                description: String::new(),
                input_schema: serde_yaml::Value::Mapping(Default::default()),
            }],
            ..Default::default()
        }];
        let allowed = HashSet::from(["extra_mcp@__mcp_pack__".to_string()]);

        let specs = collect_prompt_tool_specs_with_language(
            &config,
            &SkillRegistry::default(),
            &allowed,
            None,
            "en-US",
        );

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].name, "mcp_package");
        assert_eq!(specs[0].input_schema["required"][0], "action");
    }

    #[test]
    fn read_file_spec_clarifies_plain_text_only_in_chinese() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "读取文件")
            .expect("read_file spec");
        assert!(spec.description.contains("纯文本"));
        assert!(spec.description.contains("cat"));
        assert!(spec.description.contains("read_image"));
        let path_description = spec.input_schema["properties"]["path"]["description"]
            .as_str()
            .expect("path description");
        assert!(path_description.contains("纯文本"));
        assert!(path_description.contains("read_image"));
        assert!(spec.input_schema["properties"]["start_line"].is_object());
        assert!(spec.input_schema["properties"]["end_line"].is_object());
        assert!(spec.input_schema["properties"]["files"].is_null());
        assert!(spec.input_schema["properties"]["line_ranges"].is_null());
        assert!(spec.input_schema["properties"]["mode"].is_null());
        assert!(spec.input_schema["properties"]["indentation"].is_null());
        assert!(spec.input_schema["properties"]["file_path"].is_null());
        assert!(spec.input_schema["properties"]["dry_run"].is_null());
    }

    #[test]
    fn web_fetch_spec_discourages_search_and_guessed_urls() {
        let en = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == "网页抓取")
            .expect("web_fetch spec");
        assert!(en.description.contains("not a search provider"));
        assert!(en.description.contains("guessed URLs"));
        assert!(en
            .description
            .contains("search result pages may also be fetched"));
        let en_url = en.input_schema["properties"]["url"]["description"]
            .as_str()
            .expect("url description");
        assert!(en_url.contains("exact public URL"));
        assert!(en_url.contains("model-guessed site addresses"));

        assert!(
            i18n::t_in_language("tool.spec.web_fetch.description", "en-US")
                .contains("not a search provider")
        );
        assert!(i18n::t_in_language("tool.spec.web_fetch.args.url", "en-US")
            .contains("model-guessed site addresses"));
    }

    #[test]
    fn search_spec_exposes_canonical_model_side_fields_in_english() {
        let canonical = resolve_tool_name("search_content");
        let spec = builtin_tool_specs_with_language("en-US")
            .into_iter()
            .find(|spec| spec.name == canonical)
            .expect("search spec");
        assert!(spec.description.contains("local filesystem text files"));
        assert!(spec.description.contains("never the web"));
        assert!(spec.input_schema["properties"]["query"].is_object());
        assert!(spec.input_schema["properties"]["glob"].is_object());
        assert!(spec.input_schema["properties"]["query_mode"].is_object());
        assert!(spec.input_schema["properties"]["context_before"].is_object());
        assert!(spec.input_schema["properties"]["context_after"].is_object());
        assert!(spec.input_schema["properties"]["max_matches"].is_object());
        assert!(spec.input_schema["properties"]["context"].is_null());
        assert!(spec.input_schema["properties"]["pattern"].is_object());
        assert!(spec.input_schema["properties"]["include"].is_object());
        assert!(spec.input_schema["properties"]["-C"].is_null());
        assert!(spec.input_schema["properties"]["-i"].is_null());
        assert!(spec.input_schema["required"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "query")));
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
    fn web_fetch_schema_disallows_extra_model_side_fields() {
        let canonical_name = resolve_tool_name("web_fetch");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("web_fetch spec");
        assert!(spec.description.contains("抓取单个公开网页"));
        assert!(spec.input_schema["properties"]["url"].is_object());
        assert!(spec.input_schema["properties"]["extract_mode"].is_object());
        assert!(spec.input_schema["properties"]["max_chars"].is_object());
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn web_search_schema_exposes_query_not_url() {
        let canonical_name = resolve_tool_name("web_search");
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == canonical_name)
            .expect("web_search spec");
        assert!(spec.description.contains("自然语言关键词"));
        assert!(spec.description.contains("web_fetch"));
        assert!(spec.input_schema["properties"]["queries"].is_object());
        assert!(spec.input_schema["properties"]["query"].is_object());
        assert!(spec.input_schema["properties"]["count"].is_object());
        assert!(spec.input_schema["properties"]["site"].is_object());
        assert!(spec.input_schema["properties"]["sites"].is_object());
        assert!(spec.input_schema["properties"]["scrape_results"].is_object());
        assert!(spec.input_schema["properties"]["url"].is_null());
        assert_eq!(
            spec.input_schema["properties"]["queries"]["minItems"].as_u64(),
            Some(1)
        );
        assert_eq!(
            spec.input_schema["properties"]["queries"]["maxItems"].as_u64(),
            Some(4)
        );
        assert_eq!(
            spec.input_schema["required"]
                .as_array()
                .and_then(|items| items.first())
                .and_then(Value::as_str),
            Some("queries")
        );
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn subagent_control_schema_defaults_to_current_parent_scope() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "子智能体控制")
            .expect("subagent_control spec");
        assert!(spec.description.contains("当前会话"));
        assert!(spec
            .description
            .contains("spawn(task) -> wait/status -> history"));
        assert!(spec.input_schema["properties"]["parent_id"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("当前会话")));
        assert!(spec.input_schema["properties"]["session_id"]["description"]
            .as_str()
            .is_some_and(|value| value.contains("子会话")));
        assert!(spec.input_schema["properties"]["active_minutes"].is_null());
        assert!(spec.input_schema["properties"]["message_limit"].is_null());
        assert!(spec.input_schema["properties"]["dispatch_label"].is_null());
        assert!(spec.input_schema["properties"]["cascade"].is_null());
        assert!(spec.input_schema["properties"]["sessionId"].is_null());
        assert!(spec.input_schema["properties"]["runId"].is_null());
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
        assert_eq!(
            spec.input_schema["properties"]["tasks"]["items"]["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn schedule_task_schema_exposes_flat_model_side_fields() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "定时任务")
            .expect("schedule_task spec");
        assert!(spec.description.contains("优先使用扁平字段"));
        assert!(spec.input_schema["properties"]["job_id"].is_object());
        assert!(spec.input_schema["properties"]["schedule_text"].is_object());
        assert!(spec.input_schema["properties"]["message"].is_object());
        assert!(spec.input_schema["properties"]["enabled"].is_object());
        assert!(spec.input_schema["properties"]["delete_after_run"].is_null());
        assert!(spec.input_schema["properties"]["dedupe_key"].is_null());
        assert!(spec.input_schema["properties"]["job"].is_null());
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
        assert!(spec.description.contains("list/info"));
        assert!(spec.input_schema["properties"]["session_id"].is_object());
        assert!(spec.input_schema["properties"]["parent_session_id"].is_object());
        assert!(spec.input_schema["properties"]["agentId"].is_null());
        assert!(spec.input_schema["properties"]["label"].is_null());
        assert!(spec.input_schema["properties"]["setMain"].is_null());
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn browser_schema_hides_generic_request_mode_from_model_side() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "浏览器")
            .expect("browser spec");
        assert!(spec.description.contains("start -> open/navigate"));
        let actions = spec.input_schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert!(actions.iter().all(|item| item != "act"));
        assert!(spec.input_schema["properties"]["profile"].is_null());
        assert!(spec.input_schema["properties"]["request"].is_null());
        assert!(spec.input_schema["allOf"].is_null());
        assert!(spec.input_schema["properties"]["selector"].is_object());
        assert!(spec.input_schema["properties"]["url"].is_object());
        assert!(spec.input_schema["properties"]["path"].is_object());
        assert!(spec.input_schema["properties"]["timeout_ms"].is_object());
        assert!(spec.input_schema["properties"]["timeout_secs"].is_object());
        assert!(spec.description.contains("先用 start"));
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn desktop_controller_schema_no_longer_requires_description_for_model_side() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == "桌面控制器")
            .expect("desktop controller spec");
        assert!(spec.description.contains("bbox"));
        assert!(spec.description.contains("scroll_steps"));
        assert!(spec.description.contains("to_bbox"));
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
        config.tools.builtin.enabled = vec!["执行命令".to_string()];
        config.tools.browser.enabled = true;
        config.tools.desktop_controller.enabled = true;
        config.tools.web.search.enabled = true;
        config.tools.web.search.provider = "firecrawl".to_string();

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        for spec in builtin_tool_specs_with_language("zh-CN") {
            // Runtime feature gates (web, browser, desktop, multimodal) apply
            // on top of the desktop whitelist bypass; they filter 语音生成 etc.
            // when the matching provider/model is not configured.
            if !super::runtime_builtin_tool_allowed(&config, &spec.name) {
                continue;
            }
            if crate::services::default_tool_profile::is_desktop_hidden_tool_name(&spec.name) {
                assert!(
                    !available.contains(&spec.name),
                    "desktop mode should hide system tool {}",
                    spec.name
                );
                continue;
            }
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
    fn web_search_is_hidden_by_default_when_disabled() {
        let config = Config::default();
        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        assert!(!available.contains("web_search"));
        assert!(!available.contains("网页搜索"));
    }

    #[test]
    fn catalog_available_tools_hide_runtime_disabled_web_search_by_default() {
        let config = Config::default();
        let available =
            collect_enabled_tool_names_for_catalog(&config, &SkillRegistry::default(), None);
        assert!(!available.contains("web_search"));
        assert!(!available.contains("网页搜索"));
    }

    #[test]
    fn temporarily_hidden_tools_stay_hidden_even_when_enabled() {
        let mut config = Config::default();
        config.server.mode = "api".to_string();
        config.tools.builtin.enabled = vec![
            "网页搜索".to_string(),
            "web_search".to_string(),
            "ptc".to_string(),
            "读取文件".to_string(),
        ];
        config.tools.web.search.enabled = true;
        config.tools.web.search.provider = "firecrawl".to_string();

        let available = collect_available_tool_names(&config, &SkillRegistry::default(), None);
        for hidden in ["网页搜索", "web_search", "ptc"] {
            assert!(!available.contains(hidden), "{hidden} should stay hidden");
        }
        assert!(available.contains("读取文件"));

        let enabled =
            collect_enabled_tool_names_for_catalog(&config, &SkillRegistry::default(), None);
        for hidden in ["网页搜索", "web_search", "ptc"] {
            assert!(
                !enabled.contains(hidden),
                "{hidden} should stay hidden in catalog"
            );
        }
    }

    #[cfg(not(feature = "web-fetch"))]
    #[test]
    fn catalog_available_tools_hide_web_fetch_when_feature_is_disabled() {
        let config = Config::default();
        let available =
            collect_enabled_tool_names_for_catalog(&config, &SkillRegistry::default(), None);
        assert!(!available.contains("web_fetch"));
        assert!(!available.contains(super::web_fetch_tool::TOOL_WEB_FETCH));
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

        let sessions_yield_spec = specs
            .iter()
            .find(|spec| spec.name == super::sessions_yield_tool::TOOL_SESSIONS_YIELD)
            .expect("sessions yield spec");
        assert!(sessions_yield_spec.description.contains("不是最终回复"));
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

        assert!(specs.iter().all(|spec| spec.name != "休眠等待"));

        let memory_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("memory_manage"))
            .expect("memory manage spec");
        assert!(memory_spec.description.contains("长期记忆"));
        assert!(memory_spec.description.contains("memory_id"));
        assert!(memory_spec.description.contains("list/search"));
        assert_eq!(
            memory_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
        assert!(memory_spec.input_schema["properties"]["title"].is_object());
        assert!(memory_spec.input_schema["properties"]["content"].is_object());
        assert!(memory_spec.input_schema["properties"]["tag"].is_object());
        assert!(memory_spec.input_schema["properties"]["related_memory_id"].is_object());
        assert!(memory_spec.input_schema["properties"]["memory_time"].is_object());
        assert!(memory_spec.input_schema["properties"]["category"].is_null());
        assert!(memory_spec.input_schema["properties"]["summary"].is_null());
        assert!(memory_spec.input_schema["properties"]["tags"].is_null());
        assert!(memory_spec.input_schema["properties"]["entities"].is_null());

        let self_status_spec = specs
            .iter()
            .find(|spec| spec.name == super::self_status_tool::TOOL_SELF_STATUS)
            .expect("self status spec");
        assert!(self_status_spec.description.contains("不要高频调用"));
        assert!(self_status_spec.input_schema["properties"]["include_system_metrics"].is_null());
        assert_eq!(
            self_status_spec.input_schema["additionalProperties"].as_bool(),
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
        assert!(list_spec.description.contains("search_content"));
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
        assert!(read_spec.input_schema["properties"]["indentation"].is_null());
        assert!(read_spec.input_schema["properties"]["files"].is_null());
        assert!(read_spec.input_schema["properties"]["dry_run"].is_null());

        let read_image_spec = specs
            .iter()
            .find(|spec| spec.name == super::read_image_tool::TOOL_READ_IMAGE)
            .expect("read image spec");
        assert!(read_image_spec.description.contains("本地图片"));
        assert!(read_image_spec.input_schema["properties"]["path"].is_object());
        assert!(read_image_spec.input_schema["properties"]["frame_rate"].is_object());
        assert!(read_image_spec.input_schema["properties"]["frame_step"].is_object());
        assert!(read_image_spec.input_schema["properties"]["prompt"].is_null());
        assert_eq!(
            read_image_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let skill_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("skill_call"))
            .expect("skill call spec");
        assert!(skill_spec.description.contains("SKILL.md"));
        assert_eq!(
            skill_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        let write_spec = specs
            .iter()
            .find(|spec| spec.name == resolve_tool_name("write_file"))
            .expect("write file spec");
        assert!(write_spec.description.contains("已存在文件会被覆盖"));
        assert_eq!(
            write_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );

        // 「应用补丁」已并入「文本编辑」：patch 输入作为可选兼容参数保留。
        let edit_spec = specs
            .iter()
            .find(|spec| spec.name == "文本编辑")
            .expect("edit spec (merged from apply_patch)");
        let patch_input_description = edit_spec.input_schema["properties"]["input"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(patch_input_description.contains("*** Begin Patch"));
        assert!(patch_input_description.contains("*** End Patch"));
        assert!(patch_input_description.contains("dry_run"));
        assert!(patch_input_description.contains("read_file"));
        assert_eq!(
            edit_spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }

    #[test]
    fn read_image_schema_exposes_no_prompt_parameter() {
        let spec = builtin_tool_specs_with_language("zh-CN")
            .into_iter()
            .find(|spec| spec.name == super::read_image_tool::TOOL_READ_IMAGE)
            .expect("read image spec");

        assert!(spec.description.contains("本地图片"));
        assert!(spec.input_schema["properties"]["path"].is_object());
        assert!(spec.input_schema["properties"]["frame_rate"].is_object());
        assert!(spec.input_schema["properties"]["frame_step"].is_object());
        assert!(spec.input_schema["properties"]["prompt"].is_null());
        assert_eq!(spec.input_schema["required"][0], "path");
        assert_eq!(
            spec.input_schema["additionalProperties"].as_bool(),
            Some(false)
        );
    }
}
