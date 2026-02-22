use crate::locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Model,
    ToolCallMode,
    Approvals,
    Plan,
    Personality,
    Init,
    Agent,
    Attach,
    Notify,
    Diff,
    Review,
    Mention,
    Mcp,
    Skills,
    Apps,
    Ps,
    Clean,
    Fork,
    Rename,
    Compact,
    Backtrack,
    DebugConfig,
    Statusline,
    Status,
    Session,
    System,
    Mouse,
    Resume,
    New,
    Config,
    ConfigShow,
    Help,
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

const SLASH_COMMAND_DOCS: [SlashCommandDoc; 34] = [
    SlashCommandDoc {
        command: SlashCommand::Model,
        usage: "/model [name]",
        description: "show current model or switch default model",
    },
    SlashCommandDoc {
        command: SlashCommand::ToolCallMode,
        usage: "/tool-call-mode <tool_call|function_call> [model]",
        description: "switch tool protocol mode (alias: /mode)",
    },
    SlashCommandDoc {
        command: SlashCommand::Approvals,
        usage: "/approvals [show|suggest|auto_edit|full_auto]",
        description: "show or switch approval mode",
    },
    SlashCommandDoc {
        command: SlashCommand::Plan,
        usage: "/plan [topic]",
        description: "ask model for a step-by-step plan first",
    },
    SlashCommandDoc {
        command: SlashCommand::Personality,
        usage: "/personality [show|concise|balanced|detailed|clear]",
        description: "show or switch response style preference",
    },
    SlashCommandDoc {
        command: SlashCommand::Init,
        usage: "/init [force]",
        description: "create AGENTS.md template in current workspace",
    },
    SlashCommandDoc {
        command: SlashCommand::Agent,
        usage: "/agent [show|list|clear|<agent_id>]",
        description: "show/list/switch current agent id override",
    },
    SlashCommandDoc {
        command: SlashCommand::Attach,
        usage: "/attach [list|clear|drop <index>|<path>]",
        description: "queue local file/image attachments for next turn",
    },
    SlashCommandDoc {
        command: SlashCommand::Notify,
        usage: "/notify [show|off|bell|<command...>]",
        description: "configure turn-complete notifications",
    },
    SlashCommandDoc {
        command: SlashCommand::Diff,
        usage: "/diff",
        description: "show current git diff summary",
    },
    SlashCommandDoc {
        command: SlashCommand::Review,
        usage: "/review [focus]",
        description: "review current git changes with model",
    },
    SlashCommandDoc {
        command: SlashCommand::Mention,
        usage: "/mention <query>",
        description: "search files in workspace",
    },
    SlashCommandDoc {
        command: SlashCommand::Mcp,
        usage: "/mcp [name|list]",
        description: "list configured MCP servers and auth status",
    },
    SlashCommandDoc {
        command: SlashCommand::Skills,
        usage: "/skills [list|enable <name>|disable <name>|root]",
        description: "list and toggle local skills",
    },
    SlashCommandDoc {
        command: SlashCommand::Apps,
        usage:
            "/apps [list|info|connect|install|enable|disable|disconnect|auth|logout|remove|test]",
        description: "manage app connectors (A2A/MCP surfaces)",
    },
    SlashCommandDoc {
        command: SlashCommand::Ps,
        usage: "/ps",
        description: "list active background sessions",
    },
    SlashCommandDoc {
        command: SlashCommand::Clean,
        usage: "/clean",
        description: "cancel active background sessions",
    },
    SlashCommandDoc {
        command: SlashCommand::Fork,
        usage: "/fork [title]",
        description: "fork current session into a new branch",
    },
    SlashCommandDoc {
        command: SlashCommand::Rename,
        usage: "/rename <title>",
        description: "rename current session",
    },
    SlashCommandDoc {
        command: SlashCommand::Compact,
        usage: "/compact",
        description: "compact current session into a summary-only branch",
    },
    SlashCommandDoc {
        command: SlashCommand::Backtrack,
        usage: "/backtrack [list|index]",
        description: "load recent user turns for edit/resend",
    },
    SlashCommandDoc {
        command: SlashCommand::DebugConfig,
        usage: "/debug-config",
        description: "show config layers and effective values",
    },
    SlashCommandDoc {
        command: SlashCommand::Statusline,
        usage: "/statusline [show|set <items>|reset]",
        description: "configure TUI status line items",
    },
    SlashCommandDoc {
        command: SlashCommand::Status,
        usage: "/status",
        description: "show current session runtime status",
    },
    SlashCommandDoc {
        command: SlashCommand::Session,
        usage: "/session",
        description: "show current session statistics",
    },
    SlashCommandDoc {
        command: SlashCommand::System,
        usage: "/system [set <extra_prompt>|clear]",
        description: "show current system prompt or manage extra prompt",
    },
    SlashCommandDoc {
        command: SlashCommand::Mouse,
        usage: "/mouse [auto|scroll|select]",
        description: "toggle mouse mode for wheel scroll or text selection",
    },
    SlashCommandDoc {
        command: SlashCommand::Resume,
        usage: "/resume [session_id|last|list]",
        description: "list and resume historical sessions",
    },
    SlashCommandDoc {
        command: SlashCommand::New,
        usage: "/new",
        description: "start a new chat session",
    },
    SlashCommandDoc {
        command: SlashCommand::Config,
        usage: "/config [<base_url> <api_key> <model> [max_context|auto]]",
        description: "interactive model setup or direct one-line model config",
    },
    SlashCommandDoc {
        command: SlashCommand::ConfigShow,
        usage: "/config show",
        description: "print current runtime config",
    },
    SlashCommandDoc {
        command: SlashCommand::Help,
        usage: "/help",
        description: "show slash command help",
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
                | SlashCommand::Session
                | SlashCommand::Mouse
                | SlashCommand::Diff
                | SlashCommand::Mention
                | SlashCommand::Mcp
                | SlashCommand::Skills
                | SlashCommand::Apps
                | SlashCommand::Ps
                | SlashCommand::Clean
                | SlashCommand::Backtrack
                | SlashCommand::Personality
                | SlashCommand::Attach
                | SlashCommand::Notify
                | SlashCommand::Statusline
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
        "session" => (SlashCommand::Session, remaining),
        "system" => (SlashCommand::System, remaining),
        "mouse" => (SlashCommand::Mouse, remaining),
        "resume" | "r" => (SlashCommand::Resume, remaining),
        "new" => (SlashCommand::New, remaining),
        "model" => (SlashCommand::Model, remaining),
        "tool-call-mode" | "mode" => (SlashCommand::ToolCallMode, remaining),
        "approvals" => (SlashCommand::Approvals, remaining),
        "plan" => (SlashCommand::Plan, remaining),
        "personality" | "style" => (SlashCommand::Personality, remaining),
        "init" => (SlashCommand::Init, remaining),
        "agent" => (SlashCommand::Agent, remaining),
        "attach" => (SlashCommand::Attach, remaining),
        "notify" => (SlashCommand::Notify, remaining),
        "diff" => (SlashCommand::Diff, remaining),
        "review" => (SlashCommand::Review, remaining),
        "mention" => (SlashCommand::Mention, remaining),
        "mcp" => (SlashCommand::Mcp, remaining),
        "skills" => (SlashCommand::Skills, remaining),
        "apps" => (SlashCommand::Apps, remaining),
        "ps" => (SlashCommand::Ps, remaining),
        "clean" => (SlashCommand::Clean, remaining),
        "fork" => (SlashCommand::Fork, remaining),
        "rename" => (SlashCommand::Rename, remaining),
        "compact" => (SlashCommand::Compact, remaining),
        "backtrack" | "back" => (SlashCommand::Backtrack, remaining),
        "debug-config" => (SlashCommand::DebugConfig, remaining),
        "statusline" => (SlashCommand::Statusline, remaining),
        "config" => {
            let (sub, rest) = split_head(remaining);
            if sub.eq_ignore_ascii_case("show") {
                (SlashCommand::ConfigShow, rest)
            } else {
                (SlashCommand::Config, remaining)
            }
        }
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
    let cleaned = prefix.trim();
    let (head, tail) = split_head(cleaned);
    let width = SLASH_COMMAND_DOCS
        .iter()
        .map(|entry| entry.usage.len())
        .max()
        .unwrap_or(0);

    if !tail.is_empty() {
        if let Some(entry) = command_doc_by_name(head) {
            return vec![format!(
                "{usage:<width$}  {description}",
                usage = entry.usage,
                description = localized_description(entry, language),
                width = width,
            )];
        }
        return Vec::new();
    }

    let lookup = head.to_ascii_lowercase();
    SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
        .filter(|entry| {
            if lookup.is_empty() {
                return true;
            }
            command_token(entry)
                .trim_start_matches('/')
                .to_ascii_lowercase()
                .contains(lookup.as_str())
        })
        .take(limit)
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

pub fn first_command_completion(prefix: &str) -> Option<String> {
    let prefix = prefix.trim().to_ascii_lowercase();
    SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
        .find(|entry| {
            let token = command_token(entry)
                .trim_start_matches('/')
                .to_ascii_lowercase();
            token.starts_with(prefix.as_str())
        })
        .map(|entry| command_token(entry).trim_start_matches('/').to_string())
}

fn command_doc_by_name(name: &str) -> Option<&'static SlashCommandDoc> {
    let normalized = name.trim().trim_start_matches('/').to_ascii_lowercase();
    let command = match normalized.as_str() {
        "help" | "h" => SlashCommand::Help,
        "status" => SlashCommand::Status,
        "session" => SlashCommand::Session,
        "system" => SlashCommand::System,
        "mouse" => SlashCommand::Mouse,
        "resume" | "r" => SlashCommand::Resume,
        "new" => SlashCommand::New,
        "model" => SlashCommand::Model,
        "tool-call-mode" | "mode" => SlashCommand::ToolCallMode,
        "approvals" => SlashCommand::Approvals,
        "plan" => SlashCommand::Plan,
        "personality" | "style" => SlashCommand::Personality,
        "init" => SlashCommand::Init,
        "agent" => SlashCommand::Agent,
        "attach" => SlashCommand::Attach,
        "notify" => SlashCommand::Notify,
        "diff" => SlashCommand::Diff,
        "review" => SlashCommand::Review,
        "mention" => SlashCommand::Mention,
        "mcp" => SlashCommand::Mcp,
        "skills" => SlashCommand::Skills,
        "apps" => SlashCommand::Apps,
        "ps" => SlashCommand::Ps,
        "clean" => SlashCommand::Clean,
        "fork" => SlashCommand::Fork,
        "rename" => SlashCommand::Rename,
        "compact" => SlashCommand::Compact,
        "backtrack" | "back" => SlashCommand::Backtrack,
        "debug-config" => SlashCommand::DebugConfig,
        "statusline" => SlashCommand::Statusline,
        "config" => SlashCommand::Config,
        "exit" => SlashCommand::Exit,
        "quit" | "q" => SlashCommand::Quit,
        _ => return None,
    };

    SLASH_COMMAND_DOCS
        .iter()
        .find(|entry| entry.command == command)
}

fn command_token(entry: &SlashCommandDoc) -> &str {
    entry.usage.split_whitespace().next().unwrap_or(entry.usage)
}

fn localized_description(entry: &SlashCommandDoc, language: &str) -> String {
    let zh = match entry.command {
        SlashCommand::Model => "查看当前模型或切换默认模型",
        SlashCommand::ToolCallMode => "切换工具调用协议（别名：/mode）",
        SlashCommand::Approvals => "查看或切换审批模式",
        SlashCommand::Plan => "先让模型输出步骤化执行计划",
        SlashCommand::Personality => "查看或切换回答风格偏好",
        SlashCommand::Init => "在当前目录生成 AGENTS.md 模板",
        SlashCommand::Agent => "查看或切换当前 agent_id 覆盖",
        SlashCommand::Attach => "为下一轮请求挂载本地文件/图片附件",
        SlashCommand::Notify => "配置回合完成通知方式",
        SlashCommand::Diff => "显示当前工作区 git 变更摘要",
        SlashCommand::Review => "基于当前 git 变更发起评审",
        SlashCommand::Mention => "在工作区内搜索文件",
        SlashCommand::Mcp => "列出当前 MCP 配置与鉴权状态",
        SlashCommand::Skills => "列出并管理本地技能",
        SlashCommand::Apps => "管理应用连接器（A2A/MCP）",
        SlashCommand::Ps => "列出活动中的后台会话",
        SlashCommand::Clean => "取消活动中的后台会话",
        SlashCommand::Fork => "将当前会话分叉为新会话",
        SlashCommand::Rename => "重命名当前会话",
        SlashCommand::Compact => "将当前会话压缩为摘要分支",
        SlashCommand::Backtrack => "查看并回填最近用户消息",
        SlashCommand::DebugConfig => "显示配置分层与最终生效值",
        SlashCommand::Statusline => "配置 TUI 底部状态栏显示项",
        SlashCommand::Status => "显示当前会话运行状态",
        SlashCommand::Session => "显示当前会话统计信息",
        SlashCommand::System => "查看系统提示词或管理额外提示词",
        SlashCommand::Mouse => "切换鼠标自动/滚轮/选择模式",
        SlashCommand::Resume => "列出并恢复历史会话",
        SlashCommand::New => "开始新会话",
        SlashCommand::Config => "交互式配置模型或一行直配",
        SlashCommand::ConfigShow => "显示当前运行配置",
        SlashCommand::Help => "显示 slash 命令帮助",
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
    fn parse_config_show_uses_config_show_command() {
        let parsed = parse_slash_command("/config show").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::ConfigShow);
        assert_eq!(parsed.args, "");
    }

    #[test]
    fn popup_lines_show_usage_for_argument_entry() {
        let lines = popup_lines_with_language("tool-call-mode function_call", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/tool-call-mode <tool_call|function_call> [model]"));
    }

    #[test]
    fn popup_lines_accepts_mode_alias_for_argument_entry() {
        let lines = popup_lines_with_language("mode tool_call", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/tool-call-mode <tool_call|function_call> [model]"));
    }

    #[test]
    fn parse_mouse_command_with_args() {
        let parsed = parse_slash_command("/mouse select").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Mouse);
        assert_eq!(parsed.args, "select");
    }

    #[test]
    fn parse_resume_command_with_alias_and_args() {
        let parsed = parse_slash_command("/r last").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Resume);
        assert_eq!(parsed.args, "last");
    }

    #[test]
    fn popup_lines_show_mouse_usage_for_argument_entry() {
        let lines = popup_lines_with_language("mouse scroll", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/mouse [auto|scroll|select]"));
    }

    #[test]
    fn parse_mcp_command_with_args() {
        let parsed = parse_slash_command("/mcp docs").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Mcp);
        assert_eq!(parsed.args, "docs");
    }

    #[test]
    fn popup_lines_show_mcp_usage_for_argument_entry() {
        let lines = popup_lines_with_language("mcp list", 7, "en-US");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/mcp [name|list]"));
    }

    #[test]
    fn parse_debug_config_command() {
        let parsed = parse_slash_command("/debug-config").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::DebugConfig);
        assert_eq!(parsed.args, "");
    }

    #[test]
    fn parse_rename_command_with_inline_args() {
        let parsed = parse_slash_command("/rename backend flow").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Rename);
        assert_eq!(parsed.args, "backend flow");
    }

    #[test]
    fn parse_backtrack_command_with_alias() {
        let parsed = parse_slash_command("/back 2").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Backtrack);
        assert_eq!(parsed.args, "2");
    }

    #[test]
    fn parse_personality_command_with_alias() {
        let parsed = parse_slash_command("/style concise").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Personality);
        assert_eq!(parsed.args, "concise");
    }

    #[test]
    fn parse_agent_command() {
        let parsed = parse_slash_command("/agent worker_alpha").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Agent);
        assert_eq!(parsed.args, "worker_alpha");
    }

    #[test]
    fn parse_notify_command() {
        let parsed = parse_slash_command("/notify bell").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Notify);
        assert_eq!(parsed.args, "bell");
    }

    #[test]
    fn parse_attach_command() {
        let parsed = parse_slash_command("/attach ./README.md").expect("command should parse");
        assert_eq!(parsed.command, SlashCommand::Attach);
        assert_eq!(parsed.args, "./README.md");
    }

    #[test]
    fn busy_task_availability_matrix_smoke() {
        assert!(SlashCommand::Apps.available_during_task());
        assert!(SlashCommand::Backtrack.available_during_task());
        assert!(SlashCommand::Personality.available_during_task());
        assert!(SlashCommand::Attach.available_during_task());
        assert!(SlashCommand::Notify.available_during_task());
        assert!(SlashCommand::Status.available_during_task());
        assert!(!SlashCommand::Review.available_during_task());
        assert!(!SlashCommand::Fork.available_during_task());
        assert!(!SlashCommand::Plan.available_during_task());
    }
}
