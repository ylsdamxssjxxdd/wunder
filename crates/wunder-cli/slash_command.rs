use crate::locale;

/// The personal core command set (codex-shaped). Governance, multi-agent and
/// duplicated helpers are gone: threads are switched from the `←` command
/// center, files are mentioned with `@`, notifications live in config.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Status,
    Resume,
    Trajectory,
    New,
    Clear,
    Config,
    Model,
    Permissions,
    Plan,
    Goal,
    Edit,
    Init,
    Attach,
    Diff,
    Review,
    Skills,
    Rename,
    Compact,
    Context,
    Mcp,
    Exit,
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub struct ParsedSlashCommand<'a> {
    pub command: SlashCommand,
    pub args: &'a str,
}

/// What the command list shows for one command: the spelling to type and one
/// plain sentence about what it does.
///
/// The list deliberately carries **no argument grammar**. Every command either
/// opens a picker (resume, model, permissions, skills, MCP) or asks for the one
/// value it needs once selected, so a `[list|add <name> <endpoint> ...]` string
/// only ever taught users to type forms the UI no longer asks for.
#[derive(Debug, Clone, Copy)]
struct SlashCommandDoc {
    command: SlashCommand,
    /// Canonical spelling, without the leading `/`. Presentation order follows
    /// this table; there is no sorting pass on top of it.
    name: &'static str,
    description: &'static str,
    description_zh: &'static str,
}

const SLASH_COMMAND_DOCS: [SlashCommandDoc; 23] = [
    SlashCommandDoc {
        command: SlashCommand::Help,
        name: "help",
        description: "list the commands you can run",
        description_zh: "查看可用命令",
    },
    SlashCommandDoc {
        command: SlashCommand::Status,
        name: "status",
        description: "show this session's config and token usage",
        description_zh: "查看会话配置与用量",
    },
    SlashCommandDoc {
        command: SlashCommand::Resume,
        name: "resume",
        description: "resume an earlier thread",
        description_zh: "恢复历史线程",
    },
    SlashCommandDoc {
        command: SlashCommand::New,
        name: "new",
        description: "start a new thread",
        description_zh: "新建线程",
    },
    SlashCommandDoc {
        command: SlashCommand::Clear,
        name: "clear",
        description: "clear the screen and start a new thread",
        description_zh: "清屏并新建线程",
    },
    SlashCommandDoc {
        command: SlashCommand::Model,
        name: "model",
        description: "choose the model and reasoning effort",
        description_zh: "选择模型与推理强度",
    },
    SlashCommandDoc {
        command: SlashCommand::Permissions,
        name: "permissions",
        description: "choose what the assistant may do",
        description_zh: "选择助手的权限范围",
    },
    SlashCommandDoc {
        command: SlashCommand::Plan,
        name: "plan",
        description: "ask for a step-by-step plan first",
        description_zh: "先让助手给出计划",
    },
    SlashCommandDoc {
        command: SlashCommand::Goal,
        name: "goal",
        description: "set or manage the long-running goal",
        description_zh: "设置或管理持续目标",
    },
    SlashCommandDoc {
        command: SlashCommand::Diff,
        name: "diff",
        description: "show the workspace's git changes",
        description_zh: "查看工作区 Git 变更",
    },
    SlashCommandDoc {
        command: SlashCommand::Review,
        name: "review",
        description: "review the current changes",
        description_zh: "评审当前变更",
    },
    SlashCommandDoc {
        command: SlashCommand::Attach,
        name: "attach",
        description: "queue local files for the next turn",
        description_zh: "为下一轮挂载本地文件",
    },
    SlashCommandDoc {
        command: SlashCommand::Skills,
        name: "skills",
        description: "list and toggle local skills",
        description_zh: "列出并开关本地技能",
    },
    SlashCommandDoc {
        command: SlashCommand::Mcp,
        name: "mcp",
        description: "manage MCP servers",
        description_zh: "管理 MCP 服务器",
    },
    SlashCommandDoc {
        command: SlashCommand::Config,
        name: "config",
        description: "show or edit config.toml",
        description_zh: "查看或编辑配置文件",
    },
    SlashCommandDoc {
        command: SlashCommand::Edit,
        name: "edit",
        description: "edit the draft in your editor (Ctrl+G)",
        description_zh: "在编辑器里修改草稿（Ctrl+G）",
    },
    SlashCommandDoc {
        command: SlashCommand::Init,
        name: "init",
        description: "create an AGENTS.md template here",
        description_zh: "生成 AGENTS.md 模板",
    },
    SlashCommandDoc {
        command: SlashCommand::Compact,
        name: "compact",
        description: "compact the thread into a summary",
        description_zh: "把上下文压缩成摘要",
    },
    SlashCommandDoc {
        command: SlashCommand::Context,
        name: "context",
        description: "show the thread's context window composition",
        description_zh: "查看上下文窗口构成",
    },
    SlashCommandDoc {
        command: SlashCommand::Rename,
        name: "rename",
        description: "rename the current thread",
        description_zh: "重命名当前线程",
    },
    SlashCommandDoc {
        command: SlashCommand::Trajectory,
        name: "trajectory",
        description: "inspect this thread's activity log",
        description_zh: "查看线程活动记录",
    },
    SlashCommandDoc {
        command: SlashCommand::Exit,
        name: "exit",
        description: "exit wunder",
        description_zh: "退出 wunder",
    },
    SlashCommandDoc {
        command: SlashCommand::Quit,
        name: "quit",
        description: "exit wunder",
        description_zh: "退出 wunder",
    },
];

/// One row of the command list. `label` is what the user types (`/model`);
/// `detail` is the sentence shown to its right. File, app and skill rows fill
/// `label` only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSuggestion {
    pub label: String,
    pub detail: String,
}

impl SlashCommand {
    pub fn available_during_task(self) -> bool {
        matches!(
            self,
            SlashCommand::Help
                | SlashCommand::Status
                | SlashCommand::New
                | SlashCommand::Clear
                | SlashCommand::Resume
                | SlashCommand::Trajectory
                | SlashCommand::Context
                | SlashCommand::Diff
                | SlashCommand::Mcp
                | SlashCommand::Skills
                | SlashCommand::Goal
                | SlashCommand::Edit
                | SlashCommand::Attach
                | SlashCommand::Permissions
                | SlashCommand::Exit
                | SlashCommand::Quit
        )
    }
}

pub fn parse_slash_command(input: &str) -> Option<ParsedSlashCommand<'_>> {
    let trimmed = input.trim();
    let body = trimmed.strip_prefix('/')?.trim();
    if body.is_empty() {
        return None;
    }

    let (name, remaining) = split_head(body);
    let lowered = name.to_ascii_lowercase();
    let (command, args) = match lowered.as_str() {
        "help" | "h" => (SlashCommand::Help, remaining),
        "status" => (SlashCommand::Status, remaining),
        "resume" | "r" => (SlashCommand::Resume, remaining),
        "trajectory" | "trace" => (SlashCommand::Trajectory, remaining),
        "new" => (SlashCommand::New, remaining),
        "clear" | "cls" => (SlashCommand::Clear, remaining),
        "model" => (SlashCommand::Model, remaining),
        "permissions" | "permission" | "perm" | "approvals" | "approval" => {
            (SlashCommand::Permissions, remaining)
        }
        "plan" => (SlashCommand::Plan, remaining),
        "goal" => (SlashCommand::Goal, remaining),
        "edit" => (SlashCommand::Edit, remaining),
        "init" => (SlashCommand::Init, remaining),
        "attach" => (SlashCommand::Attach, remaining),
        "diff" => (SlashCommand::Diff, remaining),
        "review" => (SlashCommand::Review, remaining),
        "skills" => (SlashCommand::Skills, remaining),
        "rename" => (SlashCommand::Rename, remaining),
        "compact" => (SlashCommand::Compact, remaining),
        "context" => (SlashCommand::Context, remaining),
        "mcp" => (SlashCommand::Mcp, remaining),
        // `/config show` is a sub-command form of `/config`, not a command of
        // its own (codex shape: one entry point, sub-verbs as arguments).
        "config" => (SlashCommand::Config, remaining),
        "exit" => (SlashCommand::Exit, remaining),
        "quit" | "q" => (SlashCommand::Quit, remaining),
        _ => return None,
    };

    Some(ParsedSlashCommand {
        command,
        args: args.trim(),
    })
}

/// `/help`: the same rows as the popup, laid out as two columns.
pub fn help_lines_with_language(language: &str) -> Vec<String> {
    let entries: Vec<&SlashCommandDoc> = SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
        .collect();
    let width = entries
        .iter()
        .map(|entry| entry.name.len() + 1)
        .max()
        .unwrap_or(0);

    entries
        .into_iter()
        .map(|entry| {
            format!(
                "/{name:<width$}  {detail}",
                name = entry.name,
                detail = description(entry, language),
                width = width,
            )
        })
        .collect()
}

/// The popup list while the user is typing a slash command.
pub fn suggestions_with_language(
    prefix: &str,
    limit: usize,
    language: &str,
) -> Vec<CommandSuggestion> {
    if limit == 0 {
        return Vec::new();
    }

    let cleaned = prefix.trim();
    let (head, tail) = split_head(cleaned);

    // `/model something` is still `/model`: keep one row so the popup says what
    // the command does instead of vanishing.
    if !tail.is_empty() {
        return command_doc_by_name(head)
            .map(|entry| vec![suggestion(entry, language)])
            .unwrap_or_default();
    }

    command_entries_for_lookup(head, limit)
        .into_iter()
        .map(|entry| suggestion(entry, language))
        .collect()
}

pub fn command_completions(prefix: &str, limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let cleaned = prefix.trim();
    let (head, tail) = split_head(cleaned);
    if !tail.is_empty() {
        return Vec::new();
    }

    command_entries_for_lookup(head, limit)
        .into_iter()
        .map(|entry| entry.name.to_string())
        .collect()
}

fn suggestion(entry: &SlashCommandDoc, language: &str) -> CommandSuggestion {
    CommandSuggestion {
        label: format!("/{}", entry.name),
        detail: description(entry, language),
    }
}

fn description(entry: &SlashCommandDoc, language: &str) -> String {
    locale::tr(language, entry.description_zh, entry.description)
}

fn command_doc_by_name(name: &str) -> Option<&'static SlashCommandDoc> {
    let normalized = name.trim().trim_start_matches('/').to_ascii_lowercase();
    let command = match normalized.as_str() {
        "help" | "h" => SlashCommand::Help,
        "status" => SlashCommand::Status,
        "resume" | "r" => SlashCommand::Resume,
        "trajectory" | "trace" => SlashCommand::Trajectory,
        "new" => SlashCommand::New,
        "clear" | "cls" => SlashCommand::Clear,
        "config" => SlashCommand::Config,
        "model" => SlashCommand::Model,
        "permissions" | "permission" | "perm" | "approvals" | "approval" => {
            SlashCommand::Permissions
        }
        "plan" => SlashCommand::Plan,
        "goal" => SlashCommand::Goal,
        "edit" => SlashCommand::Edit,
        "init" => SlashCommand::Init,
        "attach" => SlashCommand::Attach,
        "diff" => SlashCommand::Diff,
        "review" => SlashCommand::Review,
        "skills" => SlashCommand::Skills,
        "rename" => SlashCommand::Rename,
        "compact" => SlashCommand::Compact,
        "context" => SlashCommand::Context,
        "mcp" => SlashCommand::Mcp,
        "exit" => SlashCommand::Exit,
        "quit" | "q" => SlashCommand::Quit,
        _ => return None,
    };

    SLASH_COMMAND_DOCS
        .iter()
        .find(|entry| entry.command == command)
}

fn command_entries_for_lookup(lookup: &str, limit: usize) -> Vec<&'static SlashCommandDoc> {
    let normalized = lookup.trim().trim_start_matches('/').to_ascii_lowercase();
    let mut prefix_matches = Vec::new();
    let mut contains_matches = Vec::new();

    for entry in SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
    {
        if normalized.is_empty() || entry.name.starts_with(normalized.as_str()) {
            prefix_matches.push(entry);
        } else if entry.name.contains(normalized.as_str()) {
            contains_matches.push(entry);
        }
    }

    prefix_matches.extend(contains_matches);
    prefix_matches.truncate(limit);
    prefix_matches
}

fn split_head(input: &str) -> (&str, &str) {
    let cleaned = input.trim_start();
    if cleaned.is_empty() {
        return ("", "");
    }
    if let Some(index) = cleaned.find(char::is_whitespace) {
        let head = &cleaned[..index];
        let tail = cleaned[index..].trim_start();
        (head, tail)
    } else {
        (cleaned, "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_show_is_an_argument_not_a_separate_command() {
        let parsed = parse_slash_command("/config show").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Config);
        assert_eq!(parsed.args, "show");
    }

    #[test]
    fn permissions_answers_to_the_codex_word_names() {
        for input in ["/permissions", "/permissions never", "/approvals"] {
            let parsed = parse_slash_command(input).expect("command should parse");
            assert_eq!(parsed.command, SlashCommand::Permissions);
        }
    }

    #[test]
    fn clear_parses_with_its_alias() {
        let parsed = parse_slash_command("/cls").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Clear);
    }

    #[test]
    fn removed_commands_are_no_longer_recognized() {
        for input in [
            "/agent demo",
            "/personality warm",
            "/apps list",
            "/branches",
            "/fork",
            "/backtrack",
            "/ps",
            "/clean",
            "/notify bell",
            "/mention readme",
            "/mouse scroll",
            "/statusline reset",
            "/debug-config",
            "/session",
            "/system show",
            "/tool-call-mode tool_call",
            "/threads",
        ] {
            assert!(
                parse_slash_command(input).is_none(),
                "{input} must not parse after the command-surface convergence"
            );
        }
    }

    #[test]
    fn the_list_shows_a_plain_description_instead_of_argument_grammar() {
        for language in ["en-US", "zh-CN"] {
            let rows = suggestions_with_language("", 32, language);
            assert_eq!(rows.len(), SLASH_COMMAND_DOCS.len() - 1);
            for row in &rows {
                assert!(row.label.starts_with('/'), "{row:?}");
                assert!(!row.detail.is_empty(), "{row:?}");
                for forbidden in ['[', ']', '<', '>', '|'] {
                    assert!(
                        !row.detail.contains(forbidden),
                        "{language}: {row:?} still teaches a command form"
                    );
                }
            }
        }
    }

    #[test]
    fn a_finished_command_keeps_one_row_that_explains_it() {
        let rows = suggestions_with_language("permissions never", 7, "zh-CN");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "/permissions");
        assert_eq!(rows[0].detail, "选择助手的权限范围");

        let rows = suggestions_with_language("mcp list", 7, "en-US");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "/mcp");
        assert_eq!(rows[0].detail, "manage MCP servers");
    }

    #[test]
    fn prefix_matches_come_before_contains_matches() {
        let rows = suggestions_with_language("mod", 7, "en-US");
        assert_eq!(rows[0].label, "/model");

        let rows = suggestions_with_language("del", 7, "en-US");
        assert_eq!(rows[0].label, "/model", "contains matches still resolve");
    }

    #[test]
    fn parse_mcp_command_with_args() {
        let parsed = parse_slash_command("/mcp docs").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Mcp);
        assert_eq!(parsed.args, "docs");
    }

    #[test]
    fn parse_rename_command_with_inline_args() {
        let parsed = parse_slash_command("/rename backend flow").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Rename);
        assert_eq!(parsed.args, "backend flow");
    }

    #[test]
    fn parse_attach_command() {
        let parsed = parse_slash_command("/attach ./README.md").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Attach);
        assert_eq!(parsed.args, "./README.md");
    }

    #[test]
    fn parse_edit_command() {
        let parsed = parse_slash_command("/edit fix parser flow").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Edit);
        assert_eq!(parsed.args, "fix parser flow");
    }

    #[test]
    fn parse_resume_command_with_alias_and_args() {
        let parsed = parse_slash_command("/r last").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Resume);
        assert_eq!(parsed.args, "last");
    }

    #[test]
    fn parse_trajectory_command_with_alias_and_session() {
        let parsed = parse_slash_command("/trace thread-fixture").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Trajectory);
        assert_eq!(parsed.args, "thread-fixture");
        assert!(SlashCommand::Trajectory.available_during_task());
    }

    #[test]
    fn parse_context_command_with_args_and_unknown_subcommand() {
        let parsed = parse_slash_command("/context").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Context);
        assert_eq!(parsed.args, "");
        assert!(SlashCommand::Context.available_during_task());

        let parsed = parse_slash_command("/context detail").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Context);
        assert_eq!(parsed.args, "detail");

        let parsed = parse_slash_command("/context bogus").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Context);
        assert_eq!(parsed.args, "bogus");
    }

    #[test]
    fn help_lists_every_command_but_quit() {
        let lines = help_lines_with_language("en-US");
        assert_eq!(lines.len(), SLASH_COMMAND_DOCS.len() - 1);
        assert!(lines.iter().any(|line| line.starts_with("/permissions")));
        assert!(lines.iter().any(|line| line.starts_with("/clear")));
        assert!(!lines.iter().any(|line| line.contains("/statusline")));
    }

    #[test]
    fn help_never_prints_an_argument_form() {
        for language in ["en-US", "zh-CN"] {
            for line in help_lines_with_language(language) {
                for forbidden in ['[', ']', '<', '>', '|'] {
                    assert!(
                        !line.contains(forbidden),
                        "{language}: {line} still teaches a command form"
                    );
                }
            }
        }
    }

    #[test]
    fn busy_task_availability_matrix_smoke() {
        assert!(SlashCommand::Edit.available_during_task());
        assert!(SlashCommand::Attach.available_during_task());
        assert!(SlashCommand::Status.available_during_task());
        assert!(SlashCommand::Permissions.available_during_task());
        // Starting or switching threads only moves the running one to the background, so
        // neither may wait on the visible thread's turn.
        assert!(SlashCommand::New.available_during_task());
        assert!(SlashCommand::Resume.available_during_task());
        assert!(!SlashCommand::Review.available_during_task());
        assert!(!SlashCommand::Plan.available_during_task());
        assert!(!SlashCommand::Init.available_during_task());
    }

    #[test]
    fn the_endpoint_form_is_never_advertised_in_the_list() {
        // The endpoint wizard asks one field at a time (`/model add`), so no
        // surface may go back to spelling out `<base_url> <api_key> <name>`.
        for language in ["en-US", "zh-CN"] {
            for line in help_lines_with_language(language) {
                assert!(
                    !line.contains("base_url") && !line.contains("api_key"),
                    "{language}: {line} must not advertise the endpoint arguments"
                );
            }
            for row in suggestions_with_language("model", 7, language) {
                assert!(!row.detail.contains("base_url"), "{row:?}");
            }
        }
    }

    #[test]
    fn parse_model_add_keeps_the_endpoint_arguments() {
        let parsed = parse_slash_command("/model add https://example.test/v1 sk-x demo-model")
            .expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Model);
        assert_eq!(parsed.args, "add https://example.test/v1 sk-x demo-model");
    }
}
