use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

/// Wunder CLI（命令行）
///
/// If no subcommand is specified, enters TUI on TTY (or handles one-shot input).
/// 未指定子命令时，在 TTY 下进入 TUI（或处理一次性输入）。
#[derive(Debug, Parser)]
#[command(
    author,
    version,
    bin_name = "wunder-cli",
    subcommand_negates_reqs = true,
    override_usage = "wunder-cli [OPTIONS] [PROMPT]\n       wunder-cli [OPTIONS] <COMMAND> [ARGS]\n       wunder-cli [选项] [PROMPT]\n       wunder-cli [选项] <命令> [参数]"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// Initial prompt / 初始提问，留空进入 TUI/交互模式。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Model name / 模型名称。
    #[arg(long, short = 'm', global = true)]
    pub model: Option<String>,

    /// Tool call protocol mode / 工具调用协议模式。
    #[arg(long = "tool-call-mode", global = true, value_enum)]
    pub tool_call_mode: Option<ToolCallModeArg>,

    /// Approval mode for write/exec style tools / 写入与执行类工具审批模式。
    #[arg(long = "approval-mode", global = true, value_enum)]
    pub approval_mode: Option<ApprovalModeArg>,

    /// Session id / 会话 ID。
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Agent id override / 智能体 ID 覆盖（用于请求级 agent_id）。
    #[arg(long = "agent", global = true)]
    pub agent: Option<String>,

    /// Attach local file/image for next request (repeatable) / 为下一轮请求附加本地文件或图片（可重复）。
    #[arg(long = "attach", global = true)]
    pub attachments: Vec<String>,

    /// Output stream events as JSONL / 以 JSONL 输出流事件。
    #[arg(long, global = true, default_value_t = false)]
    pub json: bool,

    /// Language override (e.g. zh-CN / en-US) / 语言覆盖。
    #[arg(long = "lang", alias = "language", global = true)]
    pub language: Option<String>,

    /// Base config path / 基础配置路径（默认 <repo>/config/wunder.yaml）。
    #[arg(long = "config", global = true)]
    pub config_path: Option<PathBuf>,

    /// Runtime temp root / 运行时临时目录（默认 ./WUNDER_TEMP）。
    #[arg(long = "temp-root", global = true)]
    pub temp_root: Option<PathBuf>,

    /// Logical user id / 逻辑用户 ID（单用户默认 cli_user）。
    #[arg(long = "user", global = true)]
    pub user: Option<String>,

    /// Disable streaming output / 关闭流式输出。
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
pub enum ApprovalModeArg {
    Suggest,
    AutoEdit,
    FullAuto,
}

impl ApprovalModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::AutoEdit => "auto_edit",
            Self::FullAuto => "full_auto",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Ask one question and print the result / 单轮提问并输出结果。
    Ask(AskCommand),

    /// Start an interactive chat session / 启动交互会话。
    Chat(ChatCommand),

    /// Resume a previous session / 恢复历史会话。
    Resume(ResumeCommand),

    /// Execute shell command via builtin tool / 通过内置工具执行命令。
    #[command(visible_alias = "e")]
    Exec(ExecCommand),

    /// Run builtin/MCP/skill tools directly / 直接运行内置工具、MCP 或技能。
    Tool(ToolCommand),

    /// Manage MCP servers in local single-user config / 管理本地 MCP 服务器。
    Mcp(McpCommand),

    /// Manage local skills for current user / 管理当前用户本地技能。
    Skills(SkillsCommand),

    /// Inspect and update runtime config / 查看与修改运行配置。
    Config(ConfigCommand),

    /// Diagnose local runtime environment / 诊断本地运行环境。
    Doctor(DoctorCommand),

    /// Generate shell completion scripts / 生成 Shell 补全脚本。
    Completion(CompletionCommand),
}

#[derive(Debug, Args)]
pub struct AskCommand {
    /// Prompt to run / 提问内容；传 '-' 从 stdin 读取。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ChatCommand {
    /// Optional first prompt / 交互会话的首条提问（可选）。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ResumeCommand {
    /// Session id / 会话 ID；留空时使用 --last 或当前保存会话。
    #[arg(value_name = "SESSION_ID")]
    pub session_id: Option<String>,

    /// Resume the most recent recorded session / 恢复最近会话。
    #[arg(long = "last", default_value_t = false)]
    pub last: bool,

    /// Optional prompt after resume / 恢复后发送提问（可选，'-' 从 stdin 读取）。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ExecCommand {
    /// Command content / 要执行的命令内容。
    #[arg(
        value_name = "COMMAND",
        trailing_var_arg = true,
        allow_hyphen_values = true
    )]
    pub command: Vec<String>,

    /// Working directory / 工作目录（默认当前启动目录）。
    #[arg(long)]
    pub workdir: Option<String>,

    /// Timeout seconds / 超时时间（秒）。
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
    /// Run a tool directly / 直接运行工具。
    Run(ToolRunCommand),

    /// List available tools / 列出可用工具。
    List,
}

#[derive(Debug, Args)]
pub struct ToolRunCommand {
    /// Tool name / 工具名。
    pub name: String,

    /// JSON arguments object / JSON 参数对象。
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
    #[command(about = "List configured MCP servers / 列出已配置 MCP 服务器")]
    List(McpListCommand),

    #[command(about = "Show one MCP server / 查看单个 MCP 服务器")]
    Get(McpGetCommand),

    #[command(about = "Add or replace an MCP server / 新增或替换 MCP 服务器")]
    Add(McpAddCommand),

    #[command(about = "Remove an MCP server / 移除 MCP 服务器")]
    Remove(McpNameCommand),

    #[command(about = "Enable an MCP server / 启用 MCP 服务器")]
    Enable(McpNameCommand),

    #[command(about = "Disable an MCP server / 禁用 MCP 服务器")]
    Disable(McpNameCommand),

    #[command(about = "Save auth credentials for an MCP server / 为 MCP 服务器保存鉴权凭据")]
    Login(McpLoginCommand),

    #[command(about = "Clear auth credentials for an MCP server / 清除 MCP 服务器鉴权凭据")]
    Logout(McpNameCommand),

    #[command(about = "Test MCP server connectivity / 测试 MCP 服务器连通性")]
    Test(McpNameCommand),
}

#[derive(Debug, Args)]
pub struct McpListCommand {
    /// Output configured servers as JSON / 以 JSON 输出已配置服务器。
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct McpGetCommand {
    /// MCP server name / MCP 服务器名称。
    pub name: String,

    /// Output server config as JSON / 以 JSON 输出服务器配置。
    #[arg(long, default_value_t = false)]
    pub json: bool,
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
pub struct McpLoginCommand {
    /// MCP server name / MCP 服务器名称。
    pub name: String,

    /// Bearer token (stored as auth.bearer_token) / Bearer Token（保存到 auth.bearer_token）。
    #[arg(long, conflicts_with_all = ["token", "api_key"])]
    pub bearer_token: Option<String>,

    /// Token (stored as auth.token) / Token（保存到 auth.token）。
    #[arg(long, conflicts_with_all = ["bearer_token", "api_key"])]
    pub token: Option<String>,

    /// API key (stored as auth.api_key) / API Key（保存到 auth.api_key）。
    #[arg(long = "api-key", conflicts_with_all = ["bearer_token", "token"])]
    pub api_key: Option<String>,
}

#[derive(Debug, Args)]
pub struct SkillsCommand {
    #[command(subcommand)]
    pub command: SkillsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum SkillsSubcommand {
    /// List local skills / 列出本地技能。
    List(SkillsListCommand),
    /// Enable one skill / 启用单个技能。
    Enable(SkillNameCommand),
    /// Disable one skill / 禁用单个技能。
    Disable(SkillNameCommand),
    /// Upload skills from .zip/.skill package / 从 .zip/.skill 包上传技能。
    Upload(SkillsUploadCommand),
    /// Remove one local skill / 删除本地技能。
    Remove(SkillNameCommand),
    /// Print local skill root path / 输出本地技能根目录。
    Root,
}

#[derive(Debug, Args)]
pub struct SkillsListCommand {
    /// Output as JSON / 以 JSON 输出。
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct SkillsUploadCommand {
    /// Package path (.zip/.skill) or skill directory / 包路径（.zip/.skill）或技能目录。
    pub source: PathBuf,

    /// Replace existing files when conflict occurs / 冲突时覆盖已有文件。
    #[arg(long, default_value_t = false)]
    pub replace: bool,
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
    /// Show runtime config / 查看运行配置。
    Show,
    /// Set tool call mode / 设置工具调用模式。
    SetToolCallMode(SetToolCallModeCommand),
    /// Set approval mode / 设置审批模式。
    SetApprovalMode(SetApprovalModeCommand),
}

#[derive(Debug, Args)]
pub struct SetToolCallModeCommand {
    /// Mode value / 模式值。
    #[arg(value_enum)]
    pub mode: ToolCallModeArg,

    /// Optional model name / 可选模型名称。
    #[arg(long)]
    pub model: Option<String>,
}

#[derive(Debug, Args)]
pub struct SetApprovalModeCommand {
    /// Mode value / 模式值。
    #[arg(value_enum)]
    pub mode: ApprovalModeArg,
}

#[derive(Debug, Args)]
pub struct DoctorCommand {
    /// Print extended diagnostics / 输出扩展诊断信息。
    #[arg(long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Debug, Args)]
pub struct CompletionCommand {
    /// Target shell / 目标 Shell。
    #[arg(value_enum, default_value_t = Shell::Bash)]
    pub shell: Shell,
}
