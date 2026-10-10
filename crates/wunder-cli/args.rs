use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use std::path::PathBuf;

/// Wunder CLI（命令行）
///
/// With no subcommand the TUI opens in the current directory; a non-TTY run
/// falls back to one-shot execution.
/// 未指定子命令时在当前目录进入 TUI；非 TTY 环境退化为一次性执行。
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

    /// Benchmark sentinel: print WUNDER_READY to stdout and exit.
    /// 性能采集哨兵：向 stdout 打印 WUNDER_READY 后立即退出（供 scripts/form-bench 采集启动耗时）。
    #[arg(long = "bench-echo", hide = true, default_value_t = false)]
    pub bench_echo: bool,
}

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Model name / 模型名称。
    #[arg(long, short = 'm', global = true)]
    pub model: Option<String>,

    /// Tool call protocol mode / 工具调用协议模式。
    #[arg(long = "tool-call-mode", global = true, value_enum)]
    pub tool_call_mode: Option<ToolCallModeArg>,

    /// Approval policy: on-request | never | suggest (legacy words accepted).
    /// 审批策略：on-request / never / suggest（旧词表保留为别名）。
    #[arg(long = "approval-mode", global = true, value_enum)]
    pub approval_mode: Option<ApprovalModeArg>,

    /// Sandbox scope: read-only | workspace-write | danger-full-access.
    /// 沙箱范围：只读 / 只写工作区（默认） / 全盘放开。
    #[arg(long = "sandbox", short = 's', global = true, value_enum)]
    pub sandbox: Option<SandboxModeArg>,

    /// Session id / 会话 ID。
    #[arg(long, global = true)]
    pub session: Option<String>,

    /// Use DIR as the working root / 指定工作目录作为工作根。
    #[arg(long = "cd", short = 'C', global = true, value_name = "DIR")]
    pub cd: Option<PathBuf>,

    /// Layer ~/.wunder/profiles/<NAME>.toml on top of the user config.
    /// 在用户配置之上叠加 profiles/<NAME>.toml。
    #[arg(long = "profile", short = 'p', global = true, value_name = "NAME")]
    pub profile: Option<String>,

    /// Reject unknown keys in the user config files.
    /// 用户配置出现未知键时报错。
    #[arg(long = "strict-config", global = true, default_value_t = false)]
    pub strict_config: bool,

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

    /// Runtime temp root / 运行时临时目录（默认用户目录 .wunder/cli/WUNDER_TEMP）。
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

/// codex-shaped approval words. The engine keeps its three-way granularity
/// (write / execute / control), so each word maps onto one engine mode instead
/// of introducing a new judgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ApprovalModeArg {
    /// `on-request`: writes stay free, execution and controlled actions ask.
    /// The old `auto_edit` word meant exactly this, so it stays an alias.
    #[value(
        name = "on-request",
        alias = "on_request",
        alias = "auto_edit",
        alias = "auto-edit",
        alias = "auto"
    )]
    OnRequest,

    /// `never`: never ask; the workspace boundary is the guard (CLI default).
    /// The old `full_auto` word meant exactly this, so it stays an alias.
    #[value(
        name = "never",
        alias = "full_auto",
        alias = "full-auto",
        alias = "full"
    )]
    Never,

    /// Every write and execution asks; kept for users who want the friction.
    #[value(name = "suggest", alias = "suggested", alias = "untrusted")]
    Suggest,
}

impl ApprovalModeArg {
    /// The engine mode this policy selects.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OnRequest => "auto_edit",
            Self::Never => "full_auto",
            Self::Suggest => "suggest",
        }
    }

    /// The user-facing word, used by `/permissions` and `config show`.
    pub fn policy_word(self) -> &'static str {
        match self {
            Self::OnRequest => "on-request",
            Self::Never => "never",
            Self::Suggest => "suggest",
        }
    }

    /// Map an engine mode (or any accepted word) back onto a policy word.
    pub fn from_engine_mode(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "suggest" | "suggested" => Self::Suggest,
            "auto_edit" | "auto-edit" | "auto" | "on-request" | "on_request" => Self::OnRequest,
            _ => Self::Never,
        }
    }
}

/// codex sandbox words. The engine expresses the same scope through the tool
/// roots it allows and the approval gate it opens, so this stays a projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SandboxModeArg {
    /// Reads everywhere the roots allow; writes and execution ask first.
    #[value(name = "read-only", alias = "read_only")]
    ReadOnly,

    /// The default local form: free read/write/execute inside the workspace.
    #[value(
        name = "workspace-write",
        alias = "workspace_write",
        alias = "workspace"
    )]
    WorkspaceWrite,

    /// No boundary at all. High risk; the flag itself is the confirmation.
    #[value(
        name = "danger-full-access",
        alias = "danger_full_access",
        alias = "danger"
    )]
    DangerFullAccess,
}

impl SandboxModeArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }

    /// Parse a `sandbox_mode` value from any config layer.
    pub fn from_word(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "read-only" | "read_only" | "readonly" => Some(Self::ReadOnly),
            "workspace-write" | "workspace_write" | "workspace" => Some(Self::WorkspaceWrite),
            "danger-full-access" | "danger_full_access" | "danger" => Some(Self::DangerFullAccess),
            _ => None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run one non-interactive agent task / 非交互执行一次智能体任务。
    #[command(visible_alias = "e")]
    Exec(ExecCommand),

    /// Resume a previous session / 恢复历史会话。
    Resume(ResumeCommand),

    /// Run builtin/MCP/skill tools directly / 直接运行内置工具、MCP 或技能。
    Tool(ToolCommand),

    /// Manage MCP servers in local single-user config / 管理本地 MCP 服务器。
    Mcp(McpCommand),

    /// Manage local skills for current user / 管理当前用户本地技能。
    Skills(SkillsCommand),

    /// Inspect and update runtime config / 查看与修改运行配置。
    Config(ConfigCommand),

    /// Generate a synthetic thread for rendering stress tests / 生成线程渲染压测线程。
    StressThread(StressThreadCommand),

    /// Diagnose local runtime environment / 诊断本地运行环境。
    Doctor(DoctorCommand),

    /// Manage the local cloud connection / 管理本地云端连接。
    Cloud(CloudCommand),

    /// Generate shell completion scripts / 生成 Shell 补全脚本。
    Completion(CompletionCommand),
}

#[derive(Debug, Args)]
pub struct CloudCommand {
    #[command(subcommand)]
    pub command: CloudSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum CloudSubcommand {
    /// Log in to the cloud server / 登录云端舰体。
    Login(CloudLoginCommand),

    /// Log out and remove cloud models / 登出并移除云模型。
    Logout,

    /// Show cloud account status / 查看云端账户状态。
    Status,

    /// List synthesized cloud models / 列出已合成的云模型。
    Models,

    /// Refresh the account snapshot and cloud models / 刷新账户与云模型。
    Refresh,

    /// Device log reporting diagnostics / 设备日志上报诊断。
    Logs(CloudLogsCommand),
}

#[derive(Debug, Args)]
pub struct CloudLoginCommand {
    /// Cloud server address (http://ip:port) / 舰体服务地址（http://ip:port）。
    #[arg(long)]
    pub server: String,

    /// Username / 用户名；缺省时交互输入。
    #[arg(short = 'u', long)]
    pub username: Option<String>,
}

#[derive(Debug, Args)]
pub struct StressThreadCommand {
    /// User rounds to generate / 生成的用户轮次数量。
    #[arg(long, default_value_t = 1000)]
    pub user_rounds: i64,

    /// Model rounds inside each user round / 每个用户轮次内的模型轮次数量。
    #[arg(long, default_value_t = 1000)]
    pub model_rounds: i64,

    /// Optional session title / 可选会话标题。
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(Debug, Args)]
pub struct CloudLogsCommand {
    /// Flush the pending upload buffer / 冲刷待上报缓冲。
    #[arg(long, default_value_t = false)]
    pub flush: bool,
}

#[derive(Debug, Args)]
pub struct ResumeCommand {
    /// Session id / 会话 ID；留空时必须使用 --last。
    #[arg(value_name = "SESSION_ID")]
    pub session_id: Option<String>,

    /// Resume the most recent recorded session / 恢复最近会话。
    #[arg(long = "last", default_value_t = false)]
    pub last: bool,

    /// Show sessions from every workspace / 显示所有工作区的会话。
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Optional prompt after resume / 恢复后发送提问（可选，'-' 从 stdin 读取）。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,
}

#[derive(Debug, Args)]
pub struct ExecCommand {
    /// Prompt to run; omit or pass '-' to read from stdin / 提问内容；省略或传 '-' 从 stdin 读取。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Write the last assistant message to FILE / 把最后一条回复写入文件。
    #[arg(long = "output-last-message", short = 'o', value_name = "FILE")]
    pub last_message_file: Option<PathBuf>,

    /// Color setting for output / 输出颜色设置。
    #[arg(long, value_enum, default_value_t = ColorArg::Auto)]
    pub color: ColorArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum ColorArg {
    Always,
    Never,
    Auto,
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
    /// Show effective configuration and the layer behind each value / 查看生效配置及各值的来源层。
    Show,
    /// Print the user config file paths / 输出用户配置文件路径。
    Path,
    /// Validate every config layer / 校验各层配置文件。
    Validate,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(argv: &[&str]) -> Cli {
        Cli::try_parse_from(argv).expect("arguments should parse")
    }

    #[test]
    fn the_sandbox_flag_accepts_the_codex_words() {
        for (word, expected) in [
            ("read-only", SandboxModeArg::ReadOnly),
            ("workspace-write", SandboxModeArg::WorkspaceWrite),
            ("danger-full-access", SandboxModeArg::DangerFullAccess),
            // Snake-case spellings stay usable in scripts written earlier.
            ("read_only", SandboxModeArg::ReadOnly),
            ("danger", SandboxModeArg::DangerFullAccess),
        ] {
            let cli = parse(&["wunder-cli", "-s", word]);
            assert_eq!(cli.global.sandbox, Some(expected), "`-s {word}`");
            let cli = parse(&["wunder-cli", "--sandbox", word]);
            assert_eq!(cli.global.sandbox, Some(expected), "`--sandbox {word}`");
        }
        assert!(
            Cli::try_parse_from(["wunder-cli", "-s", "wide-open"]).is_err(),
            "an unknown sandbox word must be rejected by the parser"
        );
        assert_eq!(parse(&["wunder-cli"]).global.sandbox, None);
    }

    #[test]
    fn the_approval_flag_keeps_the_legacy_words_as_aliases() {
        for (word, expected) in [
            ("on-request", ApprovalModeArg::OnRequest),
            ("never", ApprovalModeArg::Never),
            ("suggest", ApprovalModeArg::Suggest),
            ("auto_edit", ApprovalModeArg::OnRequest),
            ("full_auto", ApprovalModeArg::Never),
        ] {
            let cli = parse(&["wunder-cli", "--approval-mode", word]);
            assert_eq!(cli.global.approval_mode, Some(expected), "`{word}`");
        }
    }

    #[test]
    fn the_cloud_subcommands_parse() {
        let cli = parse(&[
            "wunder-cli",
            "cloud",
            "login",
            "--server",
            "http://127.0.0.1:8000",
            "-u",
            "alice",
        ]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Login(login) = command.command else {
            panic!("cloud login expected");
        };
        assert_eq!(login.server, "http://127.0.0.1:8000");
        assert_eq!(login.username.as_deref(), Some("alice"));

        let cli = parse(&["wunder-cli", "cloud", "login", "--server", "http://s"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Login(login) = command.command else {
            panic!("cloud login expected");
        };
        assert_eq!(login.username, None, "username stays optional");

        for (argv, expected) in [
            (vec!["wunder-cli", "cloud", "logout"], "logout"),
            (vec!["wunder-cli", "cloud", "status"], "status"),
            (vec!["wunder-cli", "cloud", "models"], "models"),
            (vec!["wunder-cli", "cloud", "refresh"], "refresh"),
        ] {
            let cli = parse(&argv);
            let Some(Command::Cloud(command)) = cli.command else {
                panic!("cloud subcommand expected for {expected}");
            };
            match command.command {
                CloudSubcommand::Logout => assert_eq!(expected, "logout"),
                CloudSubcommand::Status => assert_eq!(expected, "status"),
                CloudSubcommand::Models => assert_eq!(expected, "models"),
                CloudSubcommand::Refresh => assert_eq!(expected, "refresh"),
                _ => panic!("unexpected cloud subcommand for {expected}"),
            }
        }

        let cli = parse(&["wunder-cli", "cloud", "logs", "--flush"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Logs(logs) = command.command else {
            panic!("cloud logs expected");
        };
        assert!(logs.flush);

        // Without --flush the command parses but carries no flush request.
        let cli = parse(&["wunder-cli", "cloud", "logs"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Logs(logs) = command.command else {
            panic!("cloud logs expected");
        };
        assert!(!logs.flush);
    }

    #[test]
    fn the_engine_mode_round_trips_through_the_policy_word() {
        assert_eq!(
            ApprovalModeArg::from_engine_mode("auto_edit"),
            ApprovalModeArg::OnRequest
        );
        assert_eq!(
            ApprovalModeArg::from_engine_mode("suggest"),
            ApprovalModeArg::Suggest
        );
        // Anything else - including an empty config value - reads as "never",
        // which is the local default.
        assert_eq!(
            ApprovalModeArg::from_engine_mode(""),
            ApprovalModeArg::Never
        );
        assert_eq!(
            ApprovalModeArg::from_engine_mode("full_auto"),
            ApprovalModeArg::Never
        );
    }
}
