#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Model,
    ToolCallMode,
    Status,
    Session,
    System,
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

const SLASH_COMMAND_DOCS: [SlashCommandDoc; 11] = [
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
        "new" => (SlashCommand::New, remaining),
        "model" => (SlashCommand::Model, remaining),
        "tool-call-mode" | "mode" => (SlashCommand::ToolCallMode, remaining),
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

pub fn help_lines() -> Vec<String> {
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
                description = entry.description,
                width = width,
            )
        })
        .collect()
}

pub fn popup_lines(prefix: &str, limit: usize) -> Vec<String> {
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
                description = entry.description,
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
                description = entry.description,
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
        "new" => SlashCommand::New,
        "model" => SlashCommand::Model,
        "tool-call-mode" | "mode" => SlashCommand::ToolCallMode,
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
        let lines = popup_lines("tool-call-mode function_call", 7);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/tool-call-mode <tool_call|function_call> [model]"));
    }

    #[test]
    fn popup_lines_accepts_mode_alias_for_argument_entry() {
        let lines = popup_lines("mode tool_call", 7);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("/tool-call-mode <tool_call|function_call> [model]"));
    }
}
