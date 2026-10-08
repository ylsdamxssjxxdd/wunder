use crate::locale;

/// The personal core command set (codex-shaped). Governance, multi-agent and
/// duplicated helpers are gone: threads are switched from the `←` command
/// center, files are mentioned with `@`, notifications live in config.toml.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Help,
    Status,
    Resume,
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
    Mcp,
    Exit,
    Quit,
}

#[derive(Debug, Clone, Copy)]
pub struct ParsedSlashCommand<'a> {
    pub command: SlashCommand,
    pub args: &'a str,
}

#[derive(Debug, Clone, Copy)]
struct SlashCommandDoc {
    command: SlashCommand,
    usage: &'static str,
    description: &'static str,
}

const SLASH_COMMAND_DOCS: [SlashCommandDoc; 21] = [
    SlashCommandDoc {
        command: SlashCommand::Help,
        usage: "/help",
        description: "show slash command help",
    },
    SlashCommandDoc {
        command: SlashCommand::Status,
        usage: "/status",
        description: "show session config and token usage",
    },
    SlashCommandDoc {
        command: SlashCommand::Resume,
        usage: "/resume [--all|session_id]",
        description: "list and resume sessions (current workspace, --all widens)",
    },
    SlashCommandDoc {
        command: SlashCommand::New,
        usage: "/new",
        description: "start a new thread in the current workspace",
    },
    SlashCommandDoc {
        command: SlashCommand::Clear,
        usage: "/clear",
        description: "clear the screen and start a new thread (Ctrl+L)",
    },
    SlashCommandDoc {
        command: SlashCommand::Config,
        usage: "/config [show|edit|<base_url> <api_key> <model> [max_context|auto]]",
        description: "show or edit config.toml, or set up a model endpoint",
    },
    SlashCommandDoc {
        command: SlashCommand::Model,
        usage: "/model [name] [effort]",
        description: "show or switch the model and its reasoning effort",
    },
    SlashCommandDoc {
        command: SlashCommand::Permissions,
        usage: "/permissions [never|on-request|suggest|read-only|workspace-write|danger-full-access]",
        description: "show or switch the sandbox and approval policy (alias: /approvals)",
    },
    SlashCommandDoc {
        command: SlashCommand::Plan,
        usage: "/plan [topic]",
        description: "ask model for a step-by-step plan first",
    },
    SlashCommandDoc {
        command: SlashCommand::Goal,
        usage: "/goal [pause|resume|clear|edit <objective>|<objective>]",
        description: "enter or manage persistent goal mode",
    },
    SlashCommandDoc {
        command: SlashCommand::Edit,
        usage: "/edit [draft]",
        description: "edit the draft in an external editor (Ctrl+G)",
    },
    SlashCommandDoc {
        command: SlashCommand::Init,
        usage: "/init [force]",
        description: "create an AGENTS.md template in the current workspace",
    },
    SlashCommandDoc {
        command: SlashCommand::Attach,
        usage: "/attach [list|clear|drop <index>|<path>]",
        description: "queue local file/image attachments for the next turn",
    },
    SlashCommandDoc {
        command: SlashCommand::Diff,
        usage: "/diff [summary|files|show <path>|hunks <path>|stage <path>]",
        description: "show current git changes (including untracked)",
    },
    SlashCommandDoc {
        command: SlashCommand::Review,
        usage: "/review [focus]",
        description: "review current git changes with the model",
    },
    SlashCommandDoc {
        command: SlashCommand::Skills,
        usage: "/skills [list|enable <name>|disable <name>|root]",
        description: "list and toggle local skills",
    },
    SlashCommandDoc {
        command: SlashCommand::Rename,
        usage: "/rename <title>",
        description: "rename the current thread",
    },
    SlashCommandDoc {
        command: SlashCommand::Compact,
        usage: "/compact",
        description: "compact the current thread into a summary",
    },
    SlashCommandDoc {
        command: SlashCommand::Mcp,
        usage: "/mcp [list|get <name>|add <name> <endpoint> [transport]|enable <name>|disable <name>|remove <name>|login <name> [bearer-token|token|api-key] <secret>|logout <name>|test <name>|<name>]",
        description: "list/manage MCP servers and auth status",
    },
    SlashCommandDoc {
        command: SlashCommand::Exit,
        usage: "/exit",
        description: "exit interactive mode",
    },
    SlashCommandDoc {
        command: SlashCommand::Quit,
        usage: "/quit",
        description: "exit interactive mode",
    },
];

impl SlashCommand {
    pub fn available_during_task(self) -> bool {
        matches!(
            self,
            SlashCommand::Help
                | SlashCommand::Status
                | SlashCommand::New
                | SlashCommand::Clear
                | SlashCommand::Resume
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

pub fn help_lines_with_language(language: &str) -> Vec<String> {
    let width = SLASH_COMMAND_DOCS
        .iter()
        .map(|entry| entry.usage.len())
        .max()
        .unwrap_or(0);

    SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
        .map(|entry| {
            format!(
                "{usage:<width$}  {description}",
                usage = entry.usage,
                description = localized_description(entry, language),
                width = width,
            )
        })
        .collect()
}

pub fn popup_lines_with_language(prefix: &str, limit: usize, language: &str) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let cleaned = prefix.trim();
    let (head, tail) = split_head(cleaned);

    if !tail.is_empty() {
        if let Some(entry) = command_doc_by_name(head) {
            return vec![format_popup_line(entry, language)];
        }
        return Vec::new();
    }

    command_entries_for_lookup(head, limit)
        .into_iter()
        .map(|entry| format_popup_line(entry, language))
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
        .map(|entry| command_token(entry).trim_start_matches('/').to_string())
        .collect()
}

fn command_doc_by_name(name: &str) -> Option<&'static SlashCommandDoc> {
    let normalized = name.trim().trim_start_matches('/').to_ascii_lowercase();
    let command = match normalized.as_str() {
        "help" | "h" => SlashCommand::Help,
        "status" => SlashCommand::Status,
        "resume" | "r" => SlashCommand::Resume,
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
        let token = command_token(entry)
            .trim_start_matches('/')
            .to_ascii_lowercase();
        if normalized.is_empty() || token.starts_with(normalized.as_str()) {
            prefix_matches.push(entry);
        } else if token.contains(normalized.as_str()) {
            contains_matches.push(entry);
        }
    }

    prefix_matches.extend(contains_matches);
    prefix_matches.truncate(limit);
    prefix_matches
}

fn popup_usage(entry: &SlashCommandDoc) -> &'static str {
    match entry.command {
        SlashCommand::Mcp => {
            "/mcp [list|get <name>|add <name> <endpoint>|enable|disable|remove|login|logout|test]"
        }
        SlashCommand::Permissions => {
            "/permissions [never|on-request|suggest|read-only|workspace-write|danger-full-access]"
        }
        SlashCommand::Config => "/config [show|edit|<base_url> <api_key> <model>]",
        _ => entry.usage,
    }
}

fn format_popup_line(entry: &SlashCommandDoc, language: &str) -> String {
    let usage = popup_usage(entry);
    let description = localized_description(entry, language).replace(['\n', '\r'], " ");
    format!("{usage}  {description}")
}

fn command_token(entry: &SlashCommandDoc) -> &str {
    entry.usage.split_whitespace().next().unwrap_or(entry.usage)
}

fn localized_description(entry: &SlashCommandDoc, language: &str) -> String {
    let zh = match entry.command {
        SlashCommand::Help => "显示 slash 命令帮助",
        SlashCommand::Status => "显示会话配置与 token 用量",
        SlashCommand::Resume => "列出并恢复历史会话（默认限当前工作区，--all 放开）",
        SlashCommand::New => "在当前工作区新建线程",
        SlashCommand::Clear => "清屏并新建线程（Ctrl+L）",
        SlashCommand::Config => "查看或编辑 config.toml，或配置模型服务",
        SlashCommand::Model => "查看或切换模型与推理强度",
        SlashCommand::Permissions => "查看或切换沙箱与审批策略（别名：/approvals）",
        SlashCommand::Plan => "先让模型输出步骤化执行计划",
        SlashCommand::Goal => "进入或管理持续目标态",
        SlashCommand::Edit => "用外部编辑器编辑并回填输入草稿（Ctrl+G）",
        SlashCommand::Init => "在当前工作区生成 AGENTS.md 模板",
        SlashCommand::Attach => "为下一轮请求挂载本地文件/图片附件",
        SlashCommand::Diff => "显示当前工作区 git 变更（含未跟踪文件）",
        SlashCommand::Review => "基于当前 git 变更发起评审",
        SlashCommand::Skills => "列出并管理本地技能",
        SlashCommand::Rename => "重命名当前线程",
        SlashCommand::Compact => "压缩当前线程上下文为摘要",
        SlashCommand::Mcp => "列出并管理 MCP 配置与鉴权状态",
        SlashCommand::Exit => "退出交互模式",
        SlashCommand::Quit => "退出交互模式",
    };
    locale::tr(language, zh, entry.description)
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
    fn parse_config_with_inline_args_keeps_arguments() {
        let parsed = parse_slash_command("/config https://example.com/v1 sk-test demo-model 32000")
            .expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Config);
        assert_eq!(
            parsed.args,
            "https://example.com/v1 sk-test demo-model 32000"
        );
    }

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
    fn popup_lines_show_usage_for_argument_entry() {
        let lines = popup_lines_with_language("permissions never", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/permissions [never|on-request|suggest|"));
    }

    #[test]
    fn popup_lines_show_mcp_usage_for_argument_entry() {
        let lines = popup_lines_with_language("mcp list", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/mcp [list|get <name>"));
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
    fn help_lists_every_command_but_quit() {
        let lines = help_lines_with_language("en-US");
        assert_eq!(lines.len(), SLASH_COMMAND_DOCS.len() - 1);
        assert!(lines.iter().any(|line| line.contains("/permissions")));
        assert!(lines.iter().any(|line| line.contains("/clear")));
        assert!(!lines.iter().any(|line| line.contains("/statusline")));
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
}
