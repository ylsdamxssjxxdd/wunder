use serde_json::{json, Value};

const MIN_TIME_BUDGET_MS: u64 = 1;
const MAX_TIME_BUDGET_MS: u64 = 10 * 60 * 1000;
const MIN_OUTPUT_BUDGET_BYTES: usize = 4 * 1024;
const MAX_OUTPUT_BUDGET_BYTES: usize = 4 * 1024 * 1024;
const MIN_COMMAND_LIMIT: usize = 1;
const MAX_COMMAND_LIMIT: usize = 200;
pub(crate) const MAX_COMMAND_SESSION_POLL_MS: u64 = 60_000;
/// 命令会话默认的等待窗口：取一眼新输出就返回，不阻塞线程。
///
/// 设计对齐 codex 的 unified exec：后台会话的默认动作是「读一下」，
/// 需要阻塞到退出时才由调用方显式声明 `wait_for=exit`。这样模型连续
/// 轮询时每一步都是毫秒级返回，不会因为默认 60s 空等把一轮对话拖死。
pub(crate) const DEFAULT_COMMAND_SESSION_POLL_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandSessionWaitOptions {
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
    /// 写入 stdin 后再读取输出。
    Text(&'a str),
}

impl CommandSessionInput<'_> {
    pub(crate) fn writes_stdin(self) -> bool {
        matches!(self, Self::Text(_))
    }
}

/// 模型的输入是一次「写 stdin」而不是独立的 action：空 input 就是轮询。
pub(crate) fn parse_command_session_input(args: &Value) -> CommandSessionInput<'_> {
    match args.get("input").and_then(Value::as_str).unwrap_or("") {
        "" => CommandSessionInput::Poll,
        // Ctrl-C / ETX：非 tty 会话也支持，语义是终止这条命令。
        "\x03" => CommandSessionInput::Interrupt,
        text => CommandSessionInput::Text(text),
    }
}

/// Share the same wait policy with the orchestrator's outer tool deadline.
pub(crate) fn parse_command_session_wait(args: &Value) -> CommandSessionWaitOptions {
    let wait_for_exit = args
        .get("wait_for")
        .and_then(Value::as_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("exit"));
    let yield_time_ms = args
        .get("yield_time_ms")
        .and_then(Value::as_u64)
        .unwrap_or(if wait_for_exit {
            MAX_COMMAND_SESSION_POLL_MS
        } else {
            DEFAULT_COMMAND_SESSION_POLL_MS
        })
        .min(MAX_COMMAND_SESSION_POLL_MS);
    CommandSessionWaitOptions {
        wait_for_exit,
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
                wait_for_exit: false,
                yield_time_ms: DEFAULT_COMMAND_SESSION_POLL_MS
            }
        );
        // 显式要阻塞时才给长窗口，且未知取值一律按「读一下」处理。
        for args in [
            json!({"wait_for": "exit"}),
            json!({"wait_for": "EXIT", "yield_time_ms": 90_000}),
        ] {
            assert_eq!(
                parse_command_session_wait(&args),
                CommandSessionWaitOptions {
                    wait_for_exit: true,
                    yield_time_ms: 60_000
                }
            );
        }
        assert_eq!(
            parse_command_session_wait(&json!({"wait_for": "nonsense"})).wait_for_exit,
            false
        );
        assert_eq!(
            parse_command_session_wait(&json!({"yield_time_ms": 0})).yield_time_ms,
            0
        );
        assert_eq!(
            parse_command_session_wait(&json!({"yield_time_ms": 90_000})).yield_time_ms,
            60_000
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
        assert!(parse_command_session_input(&json!({"input": "ls\n"})).writes_stdin());
        assert!(!parse_command_session_input(&json!({})).writes_stdin());
    }
}
