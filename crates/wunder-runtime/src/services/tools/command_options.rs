use serde_json::{json, Value};
use std::time::Duration;

pub(crate) const DEFAULT_COMMAND_YIELD_MS: u64 = 10_000;
pub(crate) const MAX_COMMAND_YIELD_MS: u64 = 30_000;
const MIN_TIME_BUDGET_MS: u64 = 1;
const MAX_TIME_BUDGET_MS: u64 = 10 * 60 * 1000;
const MIN_OUTPUT_BUDGET_BYTES: usize = 4 * 1024;
const MAX_OUTPUT_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const MIN_COMMAND_LIMIT: usize = 1;
const MAX_COMMAND_LIMIT: usize = 200;
pub(crate) const MAX_COMMAND_SESSION_POLL_MS: u64 = 30_000;
/// 与 Codex 的 write_stdin 一致：一次调用等待一小段输出后即交还控制权。
pub(crate) const DEFAULT_COMMAND_SESSION_POLL_MS: u64 = 1_000;
/// 「阻塞到退出 / 下一条输出」的上限。渲染、构建这类分钟级任务必须能一次
/// 等得足够久，否则模型只能用几十个轮次轮询，比命令本身还慢。
pub(crate) const MAX_COMMAND_SESSION_WAIT_MS: u64 = 300_000;
/// 单条命令自身的执行时限（前台阻塞路径与超时回收共用）。
pub(crate) const DEFAULT_COMMAND_TIMEOUT_S: f64 = 120.0;
pub(crate) const MAX_COMMAND_TIMEOUT_S: f64 = 3600.0;
/// 工具层等待窗口之外再留给外层看门狗的余量：工具必须在看门狗之前
/// 自己返回，否则被丢弃 future 打断会连进程树一起带走。
pub(crate) const TOOL_WATCHDOG_GRACE_S: f64 = 30.0;

pub(crate) fn parse_command_yield_time(args: &Value) -> Duration {
    let value = args
        .get("yield_time_ms")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_COMMAND_YIELD_MS)
        .clamp(50, MAX_COMMAND_YIELD_MS);
    Duration::from_millis(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSessionWaitKind {
    /// 默认：只等一小段新输出就返回。
    Poll,
    /// 等到有新输出或进程退出，等待窗口可以很长。
    Output,
    /// 等到进程退出，等待窗口可以很长。
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandSessionWaitOptions {
    pub(crate) kind: CommandSessionWaitKind,
    pub(crate) wait_for_exit: bool,
    pub(crate) yield_time_ms: u64,
}

/// 命令会话一次调用要做什么：空输入是轮询，非空是写 stdin。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSessionInput<'a> {
    /// 只读取新增输出。
    Poll,
    /// Ctrl-C：任何会话（包括没有 stdin 的）都可以中断。
    Interrupt,
    /// Ctrl-D / EOT：关闭 stdin 写端，让读取方收到 EOF。
    Eof,
    /// 写入 stdin 后再读取输出。
    Text(&'a str),
}

impl CommandSessionInput<'_> {
    pub(crate) fn writes_stdin(self) -> bool {
        matches!(self, Self::Text(_))
    }

    pub(crate) fn uses_stdin_control(self) -> bool {
        matches!(self, Self::Text(_) | Self::Eof)
    }
}

/// `chars` 对齐 Codex；`input` 仅用于兼容已保存的调用。
pub(crate) fn parse_command_session_input(args: &Value) -> CommandSessionInput<'_> {
    let input = args
        .get("chars")
        .or_else(|| args.get("input"))
        .and_then(Value::as_str)
        .unwrap_or("");
    match input {
        "" => CommandSessionInput::Poll,
        // Accept both decoded control bytes and their literal JSON/Rust spellings.
        "\x03" | "\\x03" | "\\u0003" | "\\u{0003}" => CommandSessionInput::Interrupt,
        "\x04" | "\\x04" | "\\u0004" | "\\u{0004}" => CommandSessionInput::Eof,
        text => CommandSessionInput::Text(text),
    }
}

/// Share the same wait policy with the orchestrator's outer tool deadline.
pub(crate) fn parse_command_session_wait(args: &Value) -> CommandSessionWaitOptions {
    let kind = match args
        .get("wait_for")
        .and_then(Value::as_str)
        .map(str::trim)
    {
        Some(value) if value.eq_ignore_ascii_case("exit") => CommandSessionWaitKind::Exit,
        Some(value) if value.eq_ignore_ascii_case("output") => CommandSessionWaitKind::Output,
        _ => CommandSessionWaitKind::Poll,
    };
    // 一次「只读一眼」不该长期占住模型轮次；显式要求等退出或等下一条输出时
    // 才放开到分钟级窗口。
    let (default_ms, max_ms) = match kind {
        CommandSessionWaitKind::Poll => (
            DEFAULT_COMMAND_SESSION_POLL_MS,
            MAX_COMMAND_SESSION_POLL_MS,
        ),
        CommandSessionWaitKind::Output => (30_000, MAX_COMMAND_SESSION_WAIT_MS),
        CommandSessionWaitKind::Exit => (60_000, MAX_COMMAND_SESSION_WAIT_MS),
    };
    let yield_time_ms = args
        .get("yield_time_ms")
        .and_then(Value::as_u64)
        .unwrap_or(default_ms)
        .min(max_ms);
    CommandSessionWaitOptions {
        kind,
        wait_for_exit: kind == CommandSessionWaitKind::Exit,
        yield_time_ms,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CommandExecutionBudget {
    pub(crate) time_budget_ms: Option<u64>,
    pub(crate) output_budget_bytes: Option<usize>,
    pub(crate) max_commands: Option<usize>,
}

impl CommandExecutionBudget {
    pub(crate) fn to_json(self) -> Value {
        json!({
            "time_budget_ms": self.time_budget_ms,
            "output_budget_bytes": self.output_budget_bytes,
            "max_commands": self.max_commands,
        })
    }
}

pub(crate) fn parse_dry_run(args: &Value) -> bool {
    args.get("dry_run")
        .and_then(Value::as_bool)
        .or_else(|| args.get("preview_only").and_then(Value::as_bool))
        .unwrap_or(false)
}

pub(crate) fn parse_command_budget(args: &Value) -> CommandExecutionBudget {
    let budget_obj = args.get("budget").and_then(Value::as_object);
    let time_budget_ms = parse_u64_field(args, budget_obj, "time_budget_ms")
        .map(|value| value.clamp(MIN_TIME_BUDGET_MS, MAX_TIME_BUDGET_MS));
    let output_budget_bytes = parse_usize_field(args, budget_obj, "output_budget_bytes")
        .map(|value| value.clamp(MIN_OUTPUT_BUDGET_BYTES, MAX_OUTPUT_BUDGET_BYTES));
    let max_commands = parse_usize_field(args, budget_obj, "max_commands")
        .map(|value| value.clamp(MIN_COMMAND_LIMIT, MAX_COMMAND_LIMIT));
    CommandExecutionBudget {
        time_budget_ms,
        output_budget_bytes,
        max_commands,
    }
}

pub(crate) fn apply_time_budget_secs(base_timeout_s: f64, budget: &CommandExecutionBudget) -> f64 {
    let normalized = base_timeout_s.max(0.0);
    let Some(time_budget_ms) = budget.time_budget_ms else {
        return normalized;
    };
    let budget_timeout_s = time_budget_ms as f64 / 1000.0;
    if normalized <= 0.0 {
        budget_timeout_s
    } else {
        normalized.min(budget_timeout_s)
    }
}

fn parse_u64_field(
    args: &Value,
    budget_obj: Option<&serde_json::Map<String, Value>>,
    key: &str,
) -> Option<u64> {
    budget_obj
        .and_then(|obj| obj.get(key))
        .or_else(|| args.get(key))
        .and_then(parse_u64_value)
}

fn parse_usize_field(
    args: &Value,
    budget_obj: Option<&serde_json::Map<String, Value>>,
    key: &str,
) -> Option<usize> {
    parse_u64_field(args, budget_obj, key).map(|value| value as usize)
}

fn parse_u64_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                return Some(value);
            }
            number
                .as_i64()
                .and_then(|value| if value > 0 { Some(value as u64) } else { None })
        }
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_dry_run_accepts_primary_and_alias_fields() {
        assert!(parse_dry_run(&json!({ "dry_run": true })));
        assert!(parse_dry_run(&json!({ "preview_only": true })));
        assert!(!parse_dry_run(&json!({})));
    }

    #[test]
    fn parse_command_budget_reads_top_level_and_nested_values() {
        let budget = parse_command_budget(&json!({
            "time_budget_ms": "2000",
            "budget": {
                "output_budget_bytes": 16384,
                "max_commands": 5
            }
        }));
        assert_eq!(budget.time_budget_ms, Some(2000));
        assert_eq!(budget.output_budget_bytes, Some(16384));
        assert_eq!(budget.max_commands, Some(5));
    }

    #[test]
    fn apply_time_budget_secs_clamps_to_budget_when_needed() {
        let budget = CommandExecutionBudget {
            time_budget_ms: Some(1500),
            output_budget_bytes: None,
            max_commands: None,
        };
        assert!((apply_time_budget_secs(5.0, &budget) - 1.5).abs() < f64::EPSILON);
        assert!((apply_time_budget_secs(0.0, &budget) - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn command_session_wait_defaults_to_a_quick_read() {
        // 默认只取新输出，不再默认阻塞到退出。
        assert_eq!(
            parse_command_session_wait(&json!({})),
            CommandSessionWaitOptions {
                kind: CommandSessionWaitKind::Poll,
                wait_for_exit: false,
                yield_time_ms: DEFAULT_COMMAND_SESSION_POLL_MS
            }
        );
        // 显式退出等待：默认 60s，可按需延长到分钟级窗口。
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "exit"})),
            CommandSessionWaitOptions {
                kind: CommandSessionWaitKind::Exit,
                wait_for_exit: true,
                yield_time_ms: 60_000
            }
        );
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "EXIT", "yield_time_ms": 240_000})),
            CommandSessionWaitOptions {
                kind: CommandSessionWaitKind::Exit,
                wait_for_exit: true,
                yield_time_ms: 240_000
            }
        );
        // 等到下一条输出：返回时仍在运行，但窗口远大于一次普通轮询。
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "output", "yield_time_ms": 120_000})),
            CommandSessionWaitOptions {
                kind: CommandSessionWaitKind::Output,
                wait_for_exit: false,
                yield_time_ms: 120_000
            }
        );
        // 等待窗口有上限，模型不能靠一个超大值把线程挂死。
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "exit", "yield_time_ms": 9_000_000}))
                .yield_time_ms,
            MAX_COMMAND_SESSION_WAIT_MS
        );
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "nonsense"})).wait_for_exit,
            false
        );
        assert_eq!(
            parse_command_session_wait(&json!({"yield_time_ms": 0})).yield_time_ms,
            0
        );
        // 普通轮询仍限制在 30s 内。
        assert_eq!(
            parse_command_session_wait(&json!({"yield_time_ms": 90_000})).yield_time_ms,
            MAX_COMMAND_SESSION_POLL_MS
        );
    }

    #[test]
    fn command_session_input_separates_poll_write_and_interrupt() {
        assert_eq!(
            parse_command_session_input(&json!({})),
            CommandSessionInput::Poll
        );
        assert_eq!(
            parse_command_session_input(&json!({"input": ""})),
            CommandSessionInput::Poll
        );
        assert_eq!(
            parse_command_session_input(&json!({"input": "y\n"})),
            CommandSessionInput::Text("y\n")
        );
        assert_eq!(
            parse_command_session_input(&json!({"input": "\x03"})),
            CommandSessionInput::Interrupt
        );
        assert_eq!(
            parse_command_session_input(&json!({"chars": "\\x03"})),
            CommandSessionInput::Interrupt
        );
        assert_eq!(
            parse_command_session_input(&json!({"chars": "\\u0003"})),
            CommandSessionInput::Interrupt
        );
        assert_eq!(
            parse_command_session_input(&json!({"chars": "\\x04"})),
            CommandSessionInput::Eof
        );
        assert!(parse_command_session_input(&json!({"chars": "\\u0004"})).uses_stdin_control());
        assert!(parse_command_session_input(&json!({"input": "ls\n"})).writes_stdin());
        assert!(parse_command_session_input(&json!({"chars": "y\n"})).writes_stdin());
        assert_eq!(
            parse_command_session_input(&json!({"chars": "preferred\n", "input": "legacy\n"})),
            CommandSessionInput::Text("preferred\n")
        );
        assert!(!parse_command_session_input(&json!({})).writes_stdin());
    }
}
