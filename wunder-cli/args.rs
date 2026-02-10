use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    bin_name = "wunder-cli",
    subcommand_negates_reqs = true,
    override_usage = "wunder-cli [OPTIONS] [PROMPT]\n       wunder-cli [OPTIONS] <COMMAND> [ARGS]"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Initial prompt. When omitted, enters interactive mode.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Model name.
    #[arg(long, short = 'm', global = true)]
    pub model: Option<String>,

    /// Tool call protocol mode.
    #[arg(long = "tool-call-mode", global = true, value_enum)]
    pub tool_call_mode: Option<ToolCallModeArg>,

    /// Session id.
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Output stream events as JSONL.
    #[arg(long, global = true, default_value_t = false)]
    pub json: bool,

    /// Language override, e.g. zh-CN / en-US.
    #[arg(long = "lang", global = true)]
    pub language: Option<String>,

    /// Base config path, defaults to <repo>/config/wunder.yaml.
    #[arg(long = "config", global = true)]
    pub config_path: Option<PathBuf>,

    /// Runtime temp root, defaults to ./WUNDER_TEMP.
    #[arg(long = "temp-root", global = true)]
    pub temp_root: Option<PathBuf>,

    /// Logical user id (single-user mode defaults to cli_user).
    #[arg(long = "user", global = true)]
    pub user: Option<String>,

    /// Disable streaming output.
    #[arg(long = "no-stream", global = true, default_value_t = false)]
    pub no_stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ToolCallModeArg {
    ToolCall,
    FunctionCall,
}

impl ToolCallModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolCall => "tool_call",
            Self::FunctionCall => "function_call",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Ask one question and print the result.
    Ask(AskCommand),

    /// Start an interactive chat session.
    Chat(ChatCommand),

    /// Execute shell command via builtin tool.
    Exec(ExecCommand),

    /// Run builtin/MCP/skill tools directly.
    Tool(ToolCommand),

    /// Manage MCP servers in local single-user config.
    Mcp(McpCommand),

    /// Manage local skills for current user.
    Skills(SkillsCommand),

    /// Inspect and update runtime config.
    Config(ConfigCommand),

    /// Diagnose local runtime environment.
    Doctor(DoctorCommand),
}

#[derive(Debug, Args)]
pub struct AskCommand {
    /// Prompt to run. Use '-' to read from stdin.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ChatCommand {
    /// Optional first prompt for the interactive session.
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ExecCommand {
    /// Command content to execute.
    #[arg(value_name = "COMMAND", trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,

    /// Working directory, defaults to current launch dir.
    #[arg(long)]
    pub workdir: Option<String>,

    /// Timeout seconds.
    #[arg(long = "timeout-s")]
    pub timeout_s: Option<f64>,
}

#[derive(Debug, Args)]
pub struct ToolCommand {
    #[command(subcommand)]
    pub command: ToolSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ToolSubcommand {
    /// Run a tool directly.
    Run(ToolRunCommand),

    /// List available tools.
    List,
}

#[derive(Debug, Args)]
pub struct ToolRunCommand {
    /// Tool name.
    pub name: String,

    /// JSON arguments object.
    #[arg(long, default_value = "{}")]
    pub args: String,
}

#[derive(Debug, Args)]
pub struct McpCommand {
    #[command(subcommand)]
    pub command: McpSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum McpSubcommand {
    List,
    Add(McpAddCommand),
    Remove(McpNameCommand),
    Enable(McpNameCommand),
    Disable(McpNameCommand),
}

#[derive(Debug, Args)]
pub struct McpAddCommand {
    pub name: String,

    #[arg(long)]
    pub endpoint: String,

    #[arg(long, default_value = "streamable-http")]
    pub transport: String,

    #[arg(long = "allow-tools", value_delimiter = ',')]
    pub allow_tools: Vec<String>,

    #[arg(long = "description")]
    pub description: Option<String>,

    #[arg(long = "display-name")]
    pub display_name: Option<String>,

    #[arg(long, default_value_t = true)]
    pub enabled: bool,
}

#[derive(Debug, Args)]
pub struct McpNameCommand {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct SkillsCommand {
    #[command(subcommand)]
    pub command: SkillsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillsSubcommand {
    List,
    Enable(SkillNameCommand),
    Disable(SkillNameCommand),
}

#[derive(Debug, Args)]
pub struct SkillNameCommand {
    pub name: String,
}

#[derive(Debug, Args)]
pub struct ConfigCommand {
    #[command(subcommand)]
    pub command: ConfigSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    Show,
    SetToolCallMode(SetToolCallModeCommand),
}

#[derive(Debug, Args)]
pub struct SetToolCallModeCommand {
    #[arg(value_enum)]
    pub mode: ToolCallModeArg,

    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Args)]
pub struct DoctorCommand {
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}
