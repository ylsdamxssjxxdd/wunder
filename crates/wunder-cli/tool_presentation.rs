//! One typed classifier behind every tool card headline.
//!
//! Tool cards used to guess their own wording from the shape of the result payload and
//! from whether the incoming tool name happened to contain CJK — so an English session
//! showed `Using 读取文件` and a Chinese session showed `Used read_file`. The name is data;
//! the session language decides the phrasing, and this table is the only place that maps
//! one to the other.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ToolAccess {
    /// Reads the workspace or the web without changing anything.
    Explore,
    /// Writes, edits or patches.
    Mutate,
    /// Runs a process or drives the machine.
    Execute,
    /// Hands work to another agent, skill or channel.
    Delegate,
    /// Everything else, including tools we only know by name.
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ToolPresentation {
    pub(crate) access: ToolAccess,
    /// Reads a file, a directory listing or a search over the local workspace. Only these
    /// collapse into an `Exploring` group; web and knowledge reads keep their own card
    /// because their result is the point of the call.
    pub(crate) workspace_read: bool,
    zh: &'static str,
    en: &'static str,
    /// What the card reads once the call settles, when the settled state is a
    /// different sentence: codex says "Waiting for" while it waits and
    /// "Finished waiting" when the wait is over.
    zh_done: Option<&'static str>,
    en_done: Option<&'static str>,
}

impl ToolPresentation {
    pub(crate) fn verb(&self, is_zh: bool) -> &'static str {
        if is_zh {
            self.zh
        } else {
            self.en
        }
    }

    pub(crate) fn verb_done(&self, is_zh: bool) -> &'static str {
        if is_zh {
            self.zh_done.unwrap_or(self.zh)
        } else {
            self.en_done.unwrap_or(self.en)
        }
    }
}

const fn call(access: ToolAccess, zh: &'static str, en: &'static str) -> ToolPresentation {
    ToolPresentation {
        access,
        workspace_read: false,
        zh,
        en,
        zh_done: None,
        en_done: None,
    }
}

/// A call that reads differently once it settles.
const fn call_done(
    access: ToolAccess,
    zh: &'static str,
    en: &'static str,
    zh_done: &'static str,
    en_done: &'static str,
) -> ToolPresentation {
    ToolPresentation {
        access,
        workspace_read: false,
        zh,
        en,
        zh_done: Some(zh_done),
        en_done: Some(en_done),
    }
}

const fn workspace_call(
    access: ToolAccess,
    zh: &'static str,
    en: &'static str,
) -> ToolPresentation {
    ToolPresentation {
        access,
        workspace_read: true,
        zh,
        en,
        zh_done: None,
        en_done: None,
    }
}

/// `mcp__server__tool` reads as `server.tool`: the server and the tool are two
/// names, and the doubled underscore is a transport detail, not something the
/// reader is meant to parse.
pub(crate) fn mcp_invocation_label(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let rest = trimmed
        .strip_prefix("mcp__")
        .or_else(|| trimmed.strip_prefix("mcp_"))?;
    let mut parts = rest.split("__").filter(|part| !part.trim().is_empty());
    let server = parts.next()?;
    match parts.next() {
        Some(tool) => Some(format!("{server}.{tool}")),
        None => Some(server.to_string()),
    }
}

/// True for the sub-agent control tool in either spelling, so a card can read
/// its action out of the arguments instead of guessing from the name.
pub(crate) fn is_subagent_tool(name: &str) -> bool {
    matches!(name.trim(), "subagent_control" | "子智能体控制")
}

/// A sub-agent call names what it did to the child in `args.action`. Reading it
/// out of the arguments keeps the card honest: `Waiting for` and `Closed` are
/// different sentences, and the tool name alone cannot tell them apart.
pub(crate) fn present_subagent_action(args: &Value) -> Option<ToolPresentation> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let presentation = match action {
        "spawn" | "batch_spawn" => call(ToolAccess::Delegate, "已派生子智能体", "Spawned"),
        "send" => call(ToolAccess::Delegate, "已发送输入给", "Sent input to"),
        "wait" => call_done(
            ToolAccess::Delegate,
            "正在等待",
            "Waiting for",
            "等待结束",
            "Finished waiting",
        ),
        "close" => call(ToolAccess::Delegate, "已关闭子智能体", "Closed"),
        "resume" => call_done(
            ToolAccess::Delegate,
            "正在恢复子智能体",
            "Resuming",
            "已恢复子智能体",
            "Resumed",
        ),
        "interrupt" => call(ToolAccess::Delegate, "已中断子智能体", "Interrupted"),
        "status" => call(ToolAccess::Delegate, "已查询子智能体", "Checked"),
        "list" => call(ToolAccess::Delegate, "已列出子智能体", "Listed"),
        "history" | "report" => call(ToolAccess::Delegate, "已读取子智能体", "Read"),
        _ => return None,
    };
    Some(presentation)
}

/// Unknown tools keep their own name in the card and this verb in front of it.
const UNKNOWN: ToolPresentation = call(ToolAccess::Other, "使用", "Using");

/// Argument keys that name what a call is about, most useful first. Cards and the
/// `Exploring` group read the same list, so a tool never summarises two different
/// arguments depending on which card renders it.
const TARGET_KEYS: &[&str] = &[
    "path",
    "file_path",
    "filePath",
    "query",
    "q",
    "url",
    "location",
    "ticker",
    "command",
    "content",
    "text",
    "prompt",
    "name",
];

pub(crate) fn target_argument(args: &Value) -> Option<(&'static str, String)> {
    let object = args.as_object()?;
    TARGET_KEYS.iter().find_map(|key| {
        let value = object.get(*key)?.as_str()?.trim();
        (!value.is_empty()).then(|| (*key, value.to_string()))
    })
}

pub(crate) fn present(tool_name: &str) -> ToolPresentation {
    let name = tool_name.trim();
    if name.is_empty() {
        return UNKNOWN;
    }
    // Catalog display names arrive already localized, so both spellings are keys here.
    let presentation = match name {
        "read_file" | "读取文件" => workspace_call(ToolAccess::Explore, "读取", "Read"),
        "list_files" | "列出文件" | "list_dir" => {
            workspace_call(ToolAccess::Explore, "列出", "List")
        }
        "search_content" | "搜索内容" | "search" | "grep" => {
            workspace_call(ToolAccess::Explore, "搜索", "Search")
        }
        "read_image" | "查看图片" => {
            workspace_call(ToolAccess::Explore, "查看图片", "View image")
        }
        "web_search" | "联网搜索" => call(ToolAccess::Explore, "联网搜索", "Web search"),
        "web_fetch" | "获取网页" => call(ToolAccess::Explore, "获取网页", "Fetch page"),
        "knowledge" | "知识库" | "knowledge_search" => {
            call(ToolAccess::Explore, "检索知识库", "Search knowledge")
        }
        "write_file" | "写入文件" => call(ToolAccess::Mutate, "写入", "Write"),
        "apply_patch" | "应用补丁" => call(ToolAccess::Mutate, "应用补丁", "Apply patch"),
        "execute_command" | "执行命令" => call(ToolAccess::Execute, "运行", "Ran"),
        "command_session" | "命令会话" => {
            call(ToolAccess::Execute, "命令会话", "Command session")
        }
        "write_command_stdin" => call(ToolAccess::Execute, "写入命令输入", "Write stdin"),
        "programmatic_tool_call" | "ptc" => call(ToolAccess::Execute, "脚本调用", "Scripted call"),
        "desktop_control" | "桌面控制" => call(ToolAccess::Execute, "桌面控制", "Desktop"),
        other if other.starts_with("browser_") => {
            call(ToolAccess::Execute, "浏览网页", "Browse")
        }
        "skill_call" | "skill_get" | "技能调用" => {
            call(ToolAccess::Delegate, "调用技能", "Run skill")
        }
        "subagent_control" | "子智能体控制" => {
            call(ToolAccess::Delegate, "子智能体", "Subagent")
        }
        "thread_control" | "线程控制" => {
            call(ToolAccess::Delegate, "线程控制", "Thread control")
        }
        "memory_manager" | "memory_manage" | "记忆管理" => {
            call(ToolAccess::Other, "记忆管理", "Memory")
        }
        "goal" => call(ToolAccess::Other, "目标", "Goal"),
        _ => return unknown_named(name),
    };
    presentation
}

/// Model-remote servers and user-defined tools keep a long prefixed name; the card should
/// still read as a call, and only the grouping logic needs to know it is not a file read.
fn unknown_named(name: &str) -> ToolPresentation {
    if name.starts_with("mcp__") || name.starts_with("mcp_") {
        return call(ToolAccess::Delegate, "外部工具", "External tool");
    }
    UNKNOWN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_tool_gets_a_short_action_verb() {
        assert_eq!(present("read_file").verb(false), "Read");
        assert_eq!(present("read_file").verb(true), "读取");
        assert_eq!(present("读取文件").verb(false), "Read");
        assert_eq!(present("  execute_command ").verb(false), "Ran");
    }

    #[test]
    fn reading_and_writing_and_running_are_separated_by_access() {
        assert_eq!(present("search_content").access, ToolAccess::Explore);
        assert_eq!(present("列出文件").access, ToolAccess::Explore);
        assert_ne!(present("write_file").access, ToolAccess::Explore);
        assert_eq!(present("apply_patch").access, ToolAccess::Mutate);
        assert_eq!(present("execute_command").access, ToolAccess::Execute);
        assert_eq!(present("subagent_control").access, ToolAccess::Delegate);
    }

    #[test]
    fn an_unknown_tool_falls_back_to_its_own_name() {
        let presentation = present("stock_quote");
        assert_eq!(presentation.access, ToolAccess::Other);
        assert_eq!(presentation.verb(true), "使用");
        assert_eq!(present("mcp__crm__lookup").access, ToolAccess::Delegate);
        assert_eq!(present("").verb(false), "Using");
    }

    #[test]
    fn only_workspace_reads_join_the_exploring_group() {
        assert!(present("read_file").workspace_read);
        assert!(present("搜索内容").workspace_read);
        assert!(present("list_files").workspace_read);
        assert!(!present("web_search").workspace_read);
        assert!(!present("knowledge_search").workspace_read);
        assert!(!present("execute_command").workspace_read);
        assert!(!present("write_file").workspace_read);
    }

    #[test]
    fn the_target_is_the_one_argument_a_reader_wants() {
        let args = serde_json::json!({ "path": "src/demo.rs", "reason": "check the gate" });
        assert_eq!(
            target_argument(&args),
            Some(("path", "src/demo.rs".to_string()))
        );
        let search = serde_json::json!({ "pattern": "x", "query": " flaky " });
        assert_eq!(
            target_argument(&search),
            Some(("query", "flaky".to_string()))
        );
        let no_strings = serde_json::json!({ "limit": 3 });
        assert_eq!(target_argument(&no_strings), None);
    }
}
