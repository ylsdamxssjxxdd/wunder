#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashCommand {
    Model,
    ToolCallMode,
    Status,
    Session,
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

const SLASH_COMMAND_DOCS: [SlashCommandDoc; 10] = [
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
        description: "show current session id",
    },
    SlashCommandDoc {
        command: SlashCommand::New,
        usage: "/new",
        description: "start a new chat session",
    },
    SlashCommandDoc {
        command: SlashCommand::Config,
        usage: "/config",
        description: "interactive model setup (base_url/api_key/model)",
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
    let prefix = prefix.trim().to_ascii_lowercase();
    let width = SLASH_COMMAND_DOCS
        .iter()
        .map(|entry| entry.usage.len())
        .max()
        .unwrap_or(0);

    SLASH_COMMAND_DOCS
        .iter()
        .filter(|entry| entry.command != SlashCommand::Quit)
        .filter(|entry| {
            if prefix.is_empty() {
                return true;
            }
            command_token(entry)
                .trim_start_matches('/')
                .to_ascii_lowercase()
                .contains(prefix.as_str())
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
