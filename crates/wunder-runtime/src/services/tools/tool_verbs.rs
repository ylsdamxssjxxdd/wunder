//! 工具动作的显示词表：一个动作一个词，四端（桌面版 / 安卓版 / 命令行 / 云端版）共用一份。
//!
//! 这里只放“这次调用在做什么”的动作词，和工具规范名（`命令会话`）与显示名不是一回事。
//! 它集中在这里，是因为同一个动作曾在各端各写一遍，随后就分叉了：命令行把 `命令会话`
//! 叫「续命令」，桌面版与安卓版叫「等待 / 发送输入」，云端版又是另一套键。新增动作词请
//! 加在这个文件里，由各端引用，不要各端自己拼。

use serde_json::Value;

/// 命令会话工具：接手一条仍在运行的命令，读新输出或写 stdin。
pub const COMMAND_SESSION_TOOL: &str = "命令会话";

/// 命令会话的历史别名：durable 行与旧回放仍可能带这些拼写。
const COMMAND_SESSION_ALIASES: &[&str] = &[
    COMMAND_SESSION_TOOL,
    "命令交互",
    "command_session",
    "write_stdin",
    "write_command_stdin",
];

/// 是否为命令会话工具（含历史别名）。名称比较忽略大小写与首尾空白。
pub fn is_command_session_tool(name: &str) -> bool {
    let trimmed = name.trim();
    COMMAND_SESSION_ALIASES
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(trimmed))
}

/// 这次调用在写 stdin 吗：`chars` / `input` 非空就是写，空即轮询新输出。
///
/// 判断口径与 `command_options::parse_command_session_input` 一致（那里还区分 Ctrl-C；
/// 词表只有「等待 / 发送输入」两颗词，中断只是一次输入，不再另立第三个词）。
pub fn command_session_writes_stdin(args: &Value) -> bool {
    args.get("chars")
        .or_else(|| args.get("input"))
        .and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty())
}

/// 命令会话的动作词（中文, 英文）。
pub fn command_session_verb(writes_stdin: bool) -> (&'static str, &'static str) {
    if writes_stdin {
        ("发送输入", "Send input")
    } else {
        ("等待", "Wait")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_session_alias_is_the_same_tool() {
        for alias in [
            "命令会话",
            "命令交互",
            "command_session",
            "write_stdin",
            "write_command_stdin",
            "Command_Session",
            "  命令会话  ",
        ] {
            assert!(is_command_session_tool(alias), "{alias}");
        }
        assert!(!is_command_session_tool("执行命令"));
        assert!(!is_command_session_tool(""));
    }

    #[test]
    fn empty_input_waits_and_non_empty_input_sends() {
        assert!(!command_session_writes_stdin(&json!({})));
        assert!(!command_session_writes_stdin(&json!({ "chars": "" })));
        assert!(command_session_writes_stdin(&json!({ "chars": "y\n" })));
        // `input` 是旧字段，仍按同一口径读。
        assert!(command_session_writes_stdin(&json!({ "input": "1" })));
        assert_eq!(command_session_verb(false), ("等待", "Wait"));
        assert_eq!(command_session_verb(true), ("发送输入", "Send input"));
    }
}
