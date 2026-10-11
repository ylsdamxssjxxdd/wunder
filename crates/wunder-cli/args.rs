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
    ///
    /// Deliberately not `global`: the propagated `--attach` would occupy the
    /// same long name inside `cloud send`, where it means "stream the thread".
    /// `exec` and `resume` declare the same flag for their own position.
    #[arg(long = "attach", value_name = "FILE")]
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

    /// List the account's interlink nodes / 列出本账号互通节点。
    Devices,

    /// Show the cloud workspace / 查看云端工作区。
    Ws(CloudWsCommand),

    /// List the cloud account's threads / 列出云端线程。
    Threads(CloudThreadsCommand),

    /// Inspect one cloud thread / 查看单个云端线程。
    Thread(CloudThreadCommand),

    /// Drive a cloud or local thread through the interlink ledger / 经互通台账驱动线程。
    Send(CloudSendCommand),

    /// Watch one thread's live events through the server relay / 经舰体中继旁观线程事件流。
    Watch(CloudWatchCommand),

    /// Show the account's interlink audit trail / 查看本账号互通审计。
    Audit(CloudAuditCommand),

    /// Decide a pending interlink approval / 处理待决定审批。
    Approve(CloudApproveCommand),
}

/// `cloud ws …`: the workspace actions. The bare `cloud ws <PATH>` spelling is
/// kept so scripts written before `ws ls` exists still run.
#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct CloudWsCommand {
    /// Legacy positional path, equal to `ws ls <PATH>` / 旧写法：等价于 `ws ls <PATH>`。
    #[arg(value_name = "PATH")]
    pub path: Option<String>,

    #[command(subcommand)]
    pub action: Option<CloudWsAction>,
}

#[derive(Debug, Subcommand)]
pub enum CloudWsAction {
    /// List one directory / 列出目录。
    Ls(CloudWsLsCommand),
    /// Preview one file / 预览文件（有界）。
    Cat(CloudWsCatCommand),
    /// Download one file into the local workspace / 拉取文件到本地工作区。
    Pull(CloudWsPullCommand),
    /// Upload one text file into the cloud workspace / 上传文本文件到云端工作区。
    Push(CloudWsPushCommand),
}

#[derive(Debug, Args)]
pub struct CloudWsLsCommand {
    /// Relative path; defaults to the workspace root / 相对路径，缺省列根目录。
    #[arg(value_name = "PATH")]
    pub path: Option<String>,

    /// Skip the first N entries / 跳过前 N 条。
    #[arg(long)]
    pub offset: Option<u64>,

    /// Page size (server caps at 500) / 每页条数（服务端上限 500）。
    #[arg(long)]
    pub limit: Option<u64>,
}

#[derive(Debug, Args)]
pub struct CloudWsCatCommand {
    /// Relative path inside the cloud workspace / 云端工作区内相对路径。
    #[arg(value_name = "PATH")]
    pub path: String,

    /// Maximum preview lines / 预览最多行数。
    #[arg(long, default_value_t = 200)]
    pub lines: usize,
}

#[derive(Debug, Args)]
pub struct CloudWsPullCommand {
    /// Relative path inside the cloud workspace / 云端工作区内相对路径。
    #[arg(value_name = "REMOTE_PATH")]
    pub remote: String,

    /// Landing path, always resolved inside the local workspace / 本地落点（只在工作区内解析）。
    #[arg(short = 'o', long, value_name = "LOCAL_PATH")]
    pub output: Option<PathBuf>,

    /// Overwrite an existing local file / 覆盖已存在的本地文件。
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct CloudWsPushCommand {
    /// Local file to upload (text only) / 要上传的本地文件（仅文本）。
    #[arg(value_name = "LOCAL_PATH")]
    pub local: PathBuf,

    /// Destination path inside the cloud workspace / 云端工作区内目标路径。
    #[arg(short = 'd', long, value_name = "REMOTE_PATH")]
    pub dest: Option<String>,
}

#[derive(Debug, Args)]
pub struct CloudThreadsCommand {
    /// Target node: `cloud` or `device:<id>` / 目标节点：cloud 或 device:<id>。
    #[arg(long, default_value = "cloud")]
    pub to: String,

    /// Page size (server caps at 200) / 每页条数（服务端上限 200）。
    #[arg(long, default_value_t = 50)]
    pub limit: u64,

    /// Emit JSON instead of a table / 输出 JSON。
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct CloudThreadCommand {
    #[command(subcommand)]
    pub action: CloudThreadAction,
}

#[derive(Debug, Subcommand)]
pub enum CloudThreadAction {
    /// Show one thread's key points and recent items / 查看线程要点与最近条目。
    Show(CloudThreadShowCommand),
}

#[derive(Debug, Args)]
pub struct CloudThreadShowCommand {
    /// Thread id / 线程 id。
    #[arg(value_name = "THREAD_ID")]
    pub id: String,

    /// Target node: `cloud` or `device:<id>` / 目标节点：cloud 或 device:<id>。
    #[arg(long, default_value = "cloud")]
    pub to: String,

    /// Recent items to load (server caps at 200) / 加载最近条目数（服务端上限 200）。
    #[arg(long, default_value_t = 20)]
    pub limit: i64,

    /// Print item bodies in full / 输出条目完整正文。
    #[arg(long, default_value_t = false)]
    pub raw: bool,
}

#[derive(Debug, Args)]
pub struct CloudSendCommand {
    /// Target node: `cloud` or `device:<id>` / 目标节点：cloud 或 device:<id>。
    #[arg(long, default_value = "cloud")]
    pub to: String,

    /// Existing thread id; omitted with --title creates one / 既有线程 id；与 --title 同用可新建。
    #[arg(long)]
    pub thread: Option<String>,

    /// Title for a new thread / 新线程标题。
    #[arg(long)]
    pub title: Option<String>,

    /// Stream the thread's output as it arrives / 投递后流式回显线程输出。
    ///
    /// The name is shared with the local attachment flag on purpose; that flag
    /// stopped being a global argument so `cloud send` can own it here.
    #[arg(long = "attach", default_value_t = false)]
    pub attach: bool,

    /// Streaming window in seconds with --attach / --attach 的流式窗口秒数。
    #[arg(long, default_value_t = 180)]
    pub seconds: u64,

    /// Message text (prompt) / 消息文本。
    #[arg(value_name = "MESSAGE")]
    pub message: String,
}

#[derive(Debug, Args)]
pub struct CloudWatchCommand {
    /// Device owning the thread (`device:<id>` or `cloud`) / 线程所属节点。
    #[arg(long, default_value = "cloud")]
    pub to: String,

    /// Thread id to watch / 要旁观的线程 id。
    #[arg(long)]
    pub thread: String,

    /// Stop after N seconds / N 秒后自动退出。
    #[arg(long, default_value_t = 300)]
    pub seconds: u64,
}

#[derive(Debug, Args)]
pub struct CloudAuditCommand {
    /// Page size (server caps at 500) / 每页条数（服务端上限 500）。
    #[arg(long)]
    pub limit: Option<i64>,

    /// Skip the first N rows / 跳过前 N 条。
    #[arg(long)]
    pub offset: Option<i64>,

    /// Filter by audit action, e.g. `command.issue` / 按动作筛选，如 `command.issue`。
    #[arg(long)]
    pub action: Option<String>,

    /// Filter by node: bare id or `device:<id>` / 按节点筛选：裸 id 或 device:<id>。
    #[arg(long = "device", alias = "device-id")]
    pub device: Option<String>,

    /// Lower time bound: unix seconds/millis or RFC3339 / 起始时间：epoch 秒/毫秒或 RFC3339。
    #[arg(long)]
    pub since: Option<String>,

    /// Upper time bound: unix seconds/millis or RFC3339 / 截止时间：epoch 秒/毫秒或 RFC3339。
    #[arg(long)]
    pub until: Option<String>,

    /// Emit CSV instead of a table / 输出 CSV。
    #[arg(long, default_value_t = false)]
    pub csv: bool,
}

#[derive(Debug, Args)]
pub struct CloudApproveCommand {
    /// Command id carrying the pending ticket / 携带待审工单的命令 id。
    #[arg(value_name = "COMMAND_ID")]
    pub command_id: String,

    /// Approve instead of reject / 批准而非拒绝。
    #[arg(long, default_value_t = false)]
    pub yes: bool,
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

    /// Attach local file/image for that prompt (repeatable) / 为该提问附加本地文件或图片（可重复）。
    #[arg(long = "attach", value_name = "FILE")]
    pub attachments: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ExecCommand {
    /// Prompt to run; omit or pass '-' to read from stdin / 提问内容；省略或传 '-' 从 stdin 读取。
    #[arg(value_name = "PROMPT")]
    pub prompt: Option<String>,

    /// Attach local file/image for this run (repeatable) / 为本次运行附加本地文件或图片（可重复）。
    #[arg(long = "attach", value_name = "FILE")]
    pub attachments: Vec<String>,

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

        // Interlink subcommands (plan I10): defaults and required arguments.
        let cli = parse(&["wunder-cli", "cloud", "devices"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        assert!(matches!(command.command, CloudSubcommand::Devices));

        let cli = parse(&["wunder-cli", "cloud", "ws", "docs"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Ws(ws) = command.command else {
            panic!("cloud ws expected");
        };
        // The legacy `ws <PATH>` spelling still parses: no action, positional path.
        assert_eq!(ws.path.as_deref(), Some("docs"));
        assert!(ws.action.is_none());

        let cli = parse(&[
            "wunder-cli",
            "cloud",
            "ws",
            "ls",
            "docs",
            "--offset",
            "20",
            "--limit",
            "50",
        ]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Ws(ws) = command.command else {
            panic!("cloud ws expected");
        };
        assert_eq!(ws.path, None, "the action carries its own path");
        match ws.action {
            Some(CloudWsAction::Ls(ls)) => {
                assert_eq!(ls.path.as_deref(), Some("docs"));
                assert_eq!(ls.offset, Some(20));
                assert_eq!(ls.limit, Some(50));
            }
            other => panic!("unexpected cloud ws action {other:?}"),
        }

        for (argv, expected) in [
            (vec!["wunder-cli", "cloud", "ws", "cat", "a.md"], "cat"),
            (
                vec!["wunder-cli", "cloud", "ws", "pull", "a.md", "-o", "out/a.md", "--force"],
                "pull",
            ),
            (
                vec!["wunder-cli", "cloud", "ws", "push", "local.txt", "-d", "in/a.txt"],
                "push",
            ),
        ] {
            let cli = parse(&argv);
            let Some(Command::Cloud(command)) = cli.command else {
                panic!("cloud subcommand expected for {expected}");
            };
            let CloudSubcommand::Ws(ws) = command.command else {
                panic!("cloud ws expected for {expected}");
            };
            match ws.action {
                Some(CloudWsAction::Cat(cat)) => {
                    assert_eq!(expected, "cat");
                    assert_eq!(cat.path, "a.md");
                    assert_eq!(cat.lines, 200);
                }
                Some(CloudWsAction::Pull(pull)) => {
                    assert_eq!(expected, "pull");
                    assert_eq!(pull.remote, "a.md");
                    assert_eq!(pull.output.as_deref(), Some(std::path::Path::new("out/a.md")));
                    assert!(pull.force);
                }
                Some(CloudWsAction::Push(push)) => {
                    assert_eq!(expected, "push");
                    assert_eq!(push.local, PathBuf::from("local.txt"));
                    assert_eq!(push.dest.as_deref(), Some("in/a.txt"));
                }
                other => panic!("unexpected cloud ws action {other:?} for {expected}"),
            }
        }

        let cli = parse(&[
            "wunder-cli",
            "cloud",
            "send",
            "--to",
            "device:abc",
            "--thread",
            "t1",
            "hello",
        ]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Send(send) = command.command else {
            panic!("cloud send expected");
        };
        assert_eq!(send.to, "device:abc");
        assert_eq!(send.thread.as_deref(), Some("t1"));
        assert_eq!(send.message, "hello");
        assert!(!send.attach, "the default stays the polled one-shot behaviour");

        // `--attach` on `cloud send` is this command's streaming flag; `exec`
        // keeps the attachment spelling for its own position, and the root form
        // still works for the default/TUI run.
        let cli = parse(&["wunder-cli", "cloud", "send", "--attach", "--seconds", "60", "hello"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Send(send) = command.command else {
            panic!("cloud send expected");
        };
        assert!(send.attach);
        assert_eq!(send.seconds, 60);
        assert_eq!(send.message, "hello");
        assert!(cli.global.attachments.is_empty());

        let cli = parse(&["wunder-cli", "--attach", "a.png", "exec", "hello"]);
        assert_eq!(cli.global.attachments, vec!["a.png".to_string()]);

        let cli = parse(&["wunder-cli", "exec", "--attach", "b.png", "hello"]);
        let Some(Command::Exec(exec)) = &cli.command else {
            panic!("exec subcommand expected");
        };
        assert_eq!(exec.attachments, vec!["b.png".to_string()]);
        assert!(cli.global.attachments.is_empty());

        let cli = parse(&["wunder-cli", "cloud", "threads", "--limit", "10"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Threads(threads) = command.command else {
            panic!("cloud threads expected");
        };
        assert_eq!(threads.to, "cloud");
        assert_eq!(threads.limit, 10);

        let cli = parse(&["wunder-cli", "cloud", "thread", "show", "t1", "--raw"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Thread(thread) = command.command else {
            panic!("cloud thread expected");
        };
        match thread.action {
            CloudThreadAction::Show(show) => {
                assert_eq!(show.id, "t1");
                assert!(show.raw);
                assert_eq!(show.limit, 20);
            }
        }

        let cli = parse(&["wunder-cli", "cloud", "watch", "--thread", "t1"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Watch(watch) = command.command else {
            panic!("cloud watch expected");
        };
        assert_eq!(watch.to, "cloud");
        assert_eq!(watch.thread, "t1");
        assert_eq!(watch.seconds, 300);

        let cli = parse(&[
            "wunder-cli",
            "cloud",
            "audit",
            "--csv",
            "--limit",
            "20",
            "--offset",
            "40",
            "--action",
            "command.issue",
            "--device",
            "abc",
            "--since",
            "2026-01-01T00:00:00Z",
            "--until",
            "1700000000",
        ]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Audit(audit) = command.command else {
            panic!("cloud audit expected");
        };
        assert_eq!(audit.limit, Some(20));
        assert_eq!(audit.offset, Some(40));
        assert_eq!(audit.action.as_deref(), Some("command.issue"));
        assert_eq!(audit.device.as_deref(), Some("abc"));
        assert_eq!(audit.since.as_deref(), Some("2026-01-01T00:00:00Z"));
        assert_eq!(audit.until.as_deref(), Some("1700000000"));
        assert!(audit.csv);

        let cli = parse(&["wunder-cli", "cloud", "approve", "cmd-1", "--yes"]);
        let Some(Command::Cloud(command)) = cli.command else {
            panic!("cloud subcommand expected");
        };
        let CloudSubcommand::Approve(approve) = command.command else {
            panic!("cloud approve expected");
        };
        assert_eq!(approve.command_id, "cmd-1");
        assert!(approve.yes);
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
