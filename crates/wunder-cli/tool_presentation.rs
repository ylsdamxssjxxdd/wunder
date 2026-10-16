//! One typed classifier behind every tool card headline.
//!
//! Tool cards used to guess their own wording from the shape of the result payload and
//! from whether the incoming tool name happened to contain CJK — so an English session
//! showed `Using 读取文件` and a Chinese session showed `Used read_file`. The name is data;
//! the session language decides the phrasing, and this table is the only place that maps
//! one to the other.
//!
//! The engine resolves every tool name to a canonical name *before* it emits the event
//! (`orchestrator/execute_support.rs` rewrites `call.name`, `orchestrator/execute.rs`
//! emits `planned.name`), and the alias table that does the resolving lives with the
//! tools themselves. Reading an alias list of our own meant the two drifted: the merged
//! edit tool arrived as `文本编辑` while this file only knew `apply_patch`, so the diff
//! card went dead. So the canonical names below are the engine's canonical names, and the
//! alias resolution is the engine's own table — never a copy of it.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde_json::Value;
use wunder_server::tools::{builtin_aliases, is_browser_group_name, is_browser_tool_name};

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

// Canonical names, exactly as `wunder-runtime` dispatches on them
// (`services/tools/dispatch.rs`). Aliases never appear here: they are resolved through
// the engine's own table before matching.
const CANONICAL_PATCH: &str = "文本编辑";
const CANONICAL_WRITE: &str = "写入文件";
const CANONICAL_COMMAND: &str = "执行命令";
const CANONICAL_COMMAND_SESSION: &str = "命令会话";
const CANONICAL_WEB_SEARCH: &str = "网页搜索";
const CANONICAL_WEB_FETCH: &str = "网页抓取";
const CANONICAL_SUBAGENT: &str = "子智能体控制";
const CANONICAL_READ_IMAGE: &str = "读图工具";
const CANONICAL_GLOB: &str = "glob";

/// The pack helper an MCP server exposes to browse and call its own remote tools.
/// Mirrors `services/tools/mcp_pack.rs:8`, whose constant is module-private.
const MCP_PACK_TOOL_NAME: &str = "__mcp_pack__";

/// Legacy spellings that durable history rows and replayed transcripts can still
/// carry: English aliases plus the pre-merge Chinese names. The engine's alias table
/// covers all of these today; keeping the list makes classification independent of a
/// future trim of that table, so an old row can never fall back to a generic card.
const PATCH_ALIASES: &[&str] = &[
    "apply_patch",
    "patch",
    "edit",
    "edit_file",
    "str_replace_editor",
    "应用补丁",
    "编辑",
];

/// The engine's alias table, taken once.
///
/// `wunder_server::tools::resolve_tool_name` is the authority, but it rebuilds a
/// ~50-entry map on every call, and classification runs per tool event and per card
/// render. `builtin_aliases()` is the very map `resolve_tool_name` reads and it is
/// static, so a single frozen snapshot keeps the same source of truth without the
/// per-call allocation.
fn alias_table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(builtin_aliases)
}

/// The canonical name the engine would have dispatched under, borrowing the table
/// when the name is an alias. Trims first: the engine resolves untrimmed names, and a
/// replayed row can carry padding.
fn canonical_ref(name: &str) -> &str {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "";
    }
    alias_table()
        .get(trimmed)
        .map(String::as_str)
        .unwrap_or(trimmed)
}

/// Canonical spelling of a tool name for anything that compares names outside this
/// module — the same resolution the engine applies before emitting the event.
pub(crate) fn canonical_tool_name(name: &str) -> String {
    canonical_ref(name).to_string()
}

/// True for the merged text-edit tool in every spelling: the diff card is keyed on
/// this, so a name that is not recognised here renders as a plain call card.
pub(crate) fn is_patch_tool_name(name: &str) -> bool {
    matches_canonical(name, CANONICAL_PATCH, PATCH_ALIASES)
}

/// True for `写入文件`, including the `write_file` alias.
pub(crate) fn is_write_tool_name(name: &str) -> bool {
    matches_canonical(name, CANONICAL_WRITE, &["write_file"])
}

/// True for `执行命令` — one-shot command execution, including `execute_command`.
pub(crate) fn is_command_tool_name(name: &str) -> bool {
    matches_canonical(name, CANONICAL_COMMAND, &["execute_command"])
}

/// True for the two web tools. They share the explore card shape, but the item vocabulary
/// a script switches on must not read a page fetch as a file search.
pub(crate) fn is_web_tool_name(name: &str) -> bool {
    matches_canonical(name, CANONICAL_WEB_SEARCH, &["web_search"])
        || matches_canonical(name, CANONICAL_WEB_FETCH, &["web_fetch"])
}

/// Mirrors the engine's `is_mcp_tool_name` (`name.contains('@')`): a user tool is
/// `<server>@<tool>`, and there is no `mcp__` prefix anywhere in the runtime. The pack
/// helper is excluded — it is an engine-side mechanism for reaching a server's remote
/// tools, not a tool a reader asked for by name.
pub(crate) fn is_mcp_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.contains('@') && !is_mcp_pack_tool_name(trimmed)
}

fn is_mcp_pack_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed == MCP_PACK_TOOL_NAME
        || trimmed
            .split_once('@')
            .is_some_and(|(_, tool)| tool.trim() == MCP_PACK_TOOL_NAME)
}

/// The legacy `mcp__server__tool` / `mcp_server` transport spelling. Old durable rows
/// were written before the `<server>@<tool>` naming, and they still have to read as an
/// external call rather than as an unknown tool.
fn is_legacy_mcp_name(name: &str) -> bool {
    let trimmed = name.trim();
    trimmed.starts_with("mcp__") || trimmed.starts_with("mcp_")
}

fn matches_canonical(name: &str, canonical: &str, legacy_aliases: &[&str]) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed == canonical
        || canonical_ref(trimmed) == canonical
        || legacy_aliases
            .iter()
            .any(|alias| trimmed.eq_ignore_ascii_case(alias))
}

/// `crm@lookup` reads as `crm.lookup`; the engine's own split is
/// `user_tool_dispatch::split_mcp_tool_name`. The doubled-underscore transport shape
/// from old durable rows keeps its reading too, because the server and the tool are two
/// names and `__` is a transport detail, not something the reader parses.
pub(crate) fn mcp_invocation_label(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((server, tool)) = trimmed.split_once('@') {
        let server = server.trim();
        let tool = tool.trim();
        if server.is_empty() || tool.is_empty() {
            return None;
        }
        return Some(format!("{server}.{tool}"));
    }
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
    matches_canonical(name, CANONICAL_SUBAGENT, &["subagent_control"])
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
    // Last: `pattern` is a secondary wording for a search-shaped argument, and the
    // `glob` tool only carries it alongside the directory it searched.
    "pattern",
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
    // Canonical first — that is what the engine puts into every tool event.
    let canonical = canonical_ref(name);
    if let Some(presentation) = classify(canonical) {
        return presentation;
    }
    // Then the spelling we actually received, for old durable and replayed rows that
    // still carry a name the engine no longer resolves.
    if canonical != name {
        if let Some(presentation) = classify(name) {
            return presentation;
        }
    }
    unknown_named(canonical)
}

/// One entry per canonical name the runtime dispatch list knows, plus the aliases and
/// display names that can still arrive from history. Catalog display names arrive
/// already localized, so both spellings are keys here.
fn classify(name: &str) -> Option<ToolPresentation> {
    let presentation = match name {
        // Workspace reads — the only calls that collapse into the `Exploring` group.
        "读取文件" => workspace_call(ToolAccess::Explore, "读取", "Read"),
        "列出文件" => workspace_call(ToolAccess::Explore, "列出", "List"),
        "搜索内容" => workspace_call(ToolAccess::Explore, "搜索", "Search"),
        CANONICAL_GLOB => workspace_call(ToolAccess::Explore, "匹配文件", "Find files"),
        CANONICAL_READ_IMAGE | "查看图片" => {
            workspace_call(ToolAccess::Explore, "查看图片", "View image")
        }
        // Web and knowledge reads: exploring without changing anything, but their own
        // result is the point of the call, so they keep their own card.
        "网页搜索" | "联网搜索" => call(ToolAccess::Explore, "联网搜索", "Web search"),
        "网页抓取" | "获取网页" => call(ToolAccess::Explore, "获取网页", "Fetch page"),
        "知识库" | "knowledge" | "knowledge_search" => {
            call(ToolAccess::Explore, "检索知识库", "Search knowledge")
        }
        // Mutations.
        CANONICAL_WRITE => call(ToolAccess::Mutate, "写入", "Write"),
        CANONICAL_PATCH => call(ToolAccess::Mutate, "编辑", "Edit"),
        // Processes and the machine.
        CANONICAL_COMMAND => call(ToolAccess::Execute, "运行", "Ran"),
        CANONICAL_COMMAND_SESSION => call(ToolAccess::Execute, "命令会话", "Command session"),
        "ptc" => call(ToolAccess::Execute, "脚本调用", "Scripted call"),
        // Browser: the aggregate entry plus the provider's native `browser_*` names.
        other if is_browser_group_name(other) || is_browser_tool_name(other) => {
            call(ToolAccess::Execute, "浏览网页", "Browse")
        }
        "桌面控制器" | "桌面控制" => call(ToolAccess::Execute, "桌面控制", "Desktop"),
        "桌面监视器" | "桌面监视" => call(ToolAccess::Execute, "桌面监视", "Desktop monitor"),
        // Work handed to another agent, skill or channel.
        "技能调用" => call(ToolAccess::Delegate, "调用技能", "Run skill"),
        CANONICAL_SUBAGENT => call(ToolAccess::Delegate, "子智能体", "Subagent"),
        "会话线程控制" | "线程控制" => {
            call(ToolAccess::Delegate, "会话线程控制", "Thread control")
        }
        "会话让出" => call(ToolAccess::Delegate, "让出会话", "Yield session"),
        // Panels, schedules and self-reporting: no target file, no output stream.
        "计划面板" => call(ToolAccess::Other, "更新计划", "Update plan"),
        "问询面板" => call(ToolAccess::Other, "问询", "Ask"),
        "定时任务" => call(ToolAccess::Other, "定时任务", "Schedule task"),
        "记忆管理" => call(ToolAccess::Other, "记忆管理", "Memory"),
        "自我状态" => call(ToolAccess::Explore, "读取状态", "Status"),
        "get_goal" => call(ToolAccess::Other, "读取目标", "Read goal"),
        "create_goal" => call(ToolAccess::Other, "创建目标", "Create goal"),
        "update_goal" | "goal" => call(ToolAccess::Other, "更新目标", "Update goal"),
        // Generated artefacts: the result is a file the reader picks up, not a diff.
        "语音生成" => call(ToolAccess::Other, "生成语音", "Generate speech"),
        "声转文" => call(ToolAccess::Other, "语音转写", "Transcribe"),
        "图像生成" | "绘图生成" => call(ToolAccess::Other, "生成图像", "Generate image"),
        "视频生成" => call(ToolAccess::Other, "生成视频", "Generate video"),
        _ => {
            // Provider names are not in the alias table, and an empty registry would
            // not know them; the `browser_` prefix keeps old rows on the same card.
            return name
                .starts_with("browser_")
                .then(|| call(ToolAccess::Execute, "浏览网页", "Browse"));
        }
    };
    Some(presentation)
}

/// Model-remote servers and user-defined tools keep a long prefixed name; the card should
/// still read as a call, and only the grouping logic needs to know it is not a file read.
/// Everything else names itself — deliberate, because an unrecognised tool is the one
/// case where the reader has to see the raw name.
fn unknown_named(name: &str) -> ToolPresentation {
    if is_mcp_pack_tool_name(name) {
        return call(ToolAccess::Delegate, "工具包", "Tool pack");
    }
    if is_mcp_tool_name(name) || is_legacy_mcp_name(name) {
        return call(ToolAccess::Delegate, "外部工具", "External tool");
    }
    UNKNOWN
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every canonical name in `services/tools/dispatch.rs` with the card it must
    /// produce: access, zh verb, en verb, workspace_read.
    const ENGINE_CANONICALS: &[(&str, ToolAccess, &str, &str, bool)] = &[
        ("读取文件", ToolAccess::Explore, "读取", "Read", true),
        ("列出文件", ToolAccess::Explore, "列出", "List", true),
        ("搜索内容", ToolAccess::Explore, "搜索", "Search", true),
        ("glob", ToolAccess::Explore, "匹配文件", "Find files", true),
        ("读图工具", ToolAccess::Explore, "查看图片", "View image", true),
        ("网页搜索", ToolAccess::Explore, "联网搜索", "Web search", false),
        ("网页抓取", ToolAccess::Explore, "获取网页", "Fetch page", false),
        ("写入文件", ToolAccess::Mutate, "写入", "Write", false),
        ("文本编辑", ToolAccess::Mutate, "编辑", "Edit", false),
        ("执行命令", ToolAccess::Execute, "运行", "Ran", false),
        (
            "命令会话",
            ToolAccess::Execute,
            "命令会话",
            "Command session",
            false,
        ),
        ("ptc", ToolAccess::Execute, "脚本调用", "Scripted call", false),
        ("浏览器", ToolAccess::Execute, "浏览网页", "Browse", false),
        ("桌面控制器", ToolAccess::Execute, "桌面控制", "Desktop", false),
        (
            "桌面监视器",
            ToolAccess::Execute,
            "桌面监视",
            "Desktop monitor",
            false,
        ),
        ("技能调用", ToolAccess::Delegate, "调用技能", "Run skill", false),
        (
            "子智能体控制",
            ToolAccess::Delegate,
            "子智能体",
            "Subagent",
            false,
        ),
        (
            "会话线程控制",
            ToolAccess::Delegate,
            "会话线程控制",
            "Thread control",
            false,
        ),
        (
            "会话让出",
            ToolAccess::Delegate,
            "让出会话",
            "Yield session",
            false,
        ),
        ("计划面板", ToolAccess::Other, "更新计划", "Update plan", false),
        ("问询面板", ToolAccess::Other, "问询", "Ask", false),
        ("定时任务", ToolAccess::Other, "定时任务", "Schedule task", false),
        ("记忆管理", ToolAccess::Other, "记忆管理", "Memory", false),
        ("自我状态", ToolAccess::Explore, "读取状态", "Status", false),
        ("get_goal", ToolAccess::Other, "读取目标", "Read goal", false),
        ("create_goal", ToolAccess::Other, "创建目标", "Create goal", false),
        ("update_goal", ToolAccess::Other, "更新目标", "Update goal", false),
        (
            "语音生成",
            ToolAccess::Other,
            "生成语音",
            "Generate speech",
            false,
        ),
        ("声转文", ToolAccess::Other, "语音转写", "Transcribe", false),
        (
            "图像生成",
            ToolAccess::Other,
            "生成图像",
            "Generate image",
            false,
        ),
        (
            "视频生成",
            ToolAccess::Other,
            "生成视频",
            "Generate video",
            false,
        ),
    ];

    #[test]
    fn every_engine_canonical_gets_its_own_card() {
        for (name, access, zh, en, workspace_read) in ENGINE_CANONICALS {
            let presentation = present(name);
            assert_eq!(presentation.access, *access, "{name} access");
            assert_eq!(presentation.verb(true), *zh, "{name} zh verb");
            assert_eq!(presentation.verb(false), *en, "{name} en verb");
            assert_eq!(presentation.workspace_read, *workspace_read, "{name} group");
        }
    }

    #[test]
    fn the_edit_tool_is_the_patch_family() {
        // The regression that killed the diff card: the merged tool dispatches as
        // `文本编辑`, and nothing matched that spelling.
        assert!(is_patch_tool_name("文本编辑"));
        assert_eq!(present("文本编辑").access, ToolAccess::Mutate);
        for alias in [
            "apply_patch",
            "patch",
            "edit",
            "edit_file",
            "str_replace_editor",
            "应用补丁",
            "编辑",
            "  apply_patch ",
        ] {
            assert!(is_patch_tool_name(alias), "{alias} is the patch family");
            assert_eq!(
                present(alias),
                present("文本编辑"),
                "{alias} reads as the edit tool"
            );
        }
        assert!(!is_patch_tool_name("写入文件"));
        assert!(!is_patch_tool_name("执行命令"));
        assert!(!is_patch_tool_name(""));
    }

    #[test]
    fn command_and_write_families_split_the_way_the_engine_splits_them() {
        assert!(is_write_tool_name("写入文件"));
        assert!(is_write_tool_name("write_file"));
        assert!(!is_write_tool_name("文本编辑"));

        assert!(is_command_tool_name("执行命令"));
        assert!(is_command_tool_name("execute_command"));
        assert!(!is_command_tool_name("命令会话"));

        assert!(is_web_tool_name("网页搜索"));
        assert!(is_web_tool_name("web_fetch"));
        // A file-content search shares the explore card shape but is not a web call, and a
        // command is neither.
        assert!(!is_web_tool_name("搜索内容"));
        assert!(!is_web_tool_name("执行命令"));

        assert!(is_subagent_tool("subagent_control"));
        assert!(is_subagent_tool("子智能体控制"));
    }

    #[test]
    fn english_aliases_still_classify_after_the_rename() {
        // Names are canonicalized, so the alias table is the only list to maintain.
        for (alias, canonical) in [
            ("read_file", "读取文件"),
            ("list_files", "列出文件"),
            ("search_content", "搜索内容"),
            ("read_image", "读图工具"),
            ("view_image", "读图工具"),
            ("web_search", "网页搜索"),
            ("web_fetch", "网页抓取"),
            ("write_file", "写入文件"),
            ("execute_command", "执行命令"),
            ("command_session", "命令会话"),
            ("programmatic_tool_call", "ptc"),
            ("skill_call", "技能调用"),
            ("subagent_control", "子智能体控制"),
            ("thread_control", "会话线程控制"),
            ("session_thread", "会话线程控制"),
            ("sessions_yield", "会话让出"),
            ("self_status", "自我状态"),
            ("memory_manager", "记忆管理"),
            ("update_plan", "计划面板"),
            ("ask_panel", "问询面板"),
            ("schedule_task", "定时任务"),
            ("desktop_controller", "桌面控制器"),
            ("desktop_monitor", "桌面监视器"),
            ("generate_speech", "语音生成"),
            ("transcribe_speech", "声转文"),
            ("generate_image", "图像生成"),
            ("generate_video", "视频生成"),
            ("browser", "浏览器"),
            ("goal", "update_goal"),
        ] {
            assert_eq!(canonical_tool_name(alias), canonical, "{alias}");
            assert_eq!(
                present(alias),
                present(canonical),
                "{alias} renders as {canonical}"
            );
        }
    }

    #[test]
    fn canonical_tool_name_is_the_engines_resolution() {
        // The snapshot must stay byte-for-byte faithful to `resolve_tool_name`.
        for (alias, canonical) in builtin_aliases() {
            assert_eq!(
                canonical_tool_name(&alias),
                wunder_server::tools::resolve_tool_name(&alias),
                "{alias} resolves to {canonical}"
            );
        }
        assert_eq!(canonical_tool_name("读取文件"), "读取文件");
        assert_eq!(canonical_tool_name("  apply_patch "), "文本编辑");
        assert_eq!(canonical_tool_name("crm@lookup"), "crm@lookup");
        assert_eq!(canonical_tool_name(""), "");
    }

    #[test]
    fn mcp_names_are_the_engine_shape() {
        assert!(is_mcp_tool_name("crm@lookup"));
        assert!(is_mcp_tool_name(" crm@lookup "), "padded rows still count");
        assert!(!is_mcp_tool_name("crm.lookup"));
        assert!(!is_mcp_tool_name("文本编辑"));
        // The pack helper is engine plumbing, not a tool the reader named.
        assert!(!is_mcp_tool_name("crm@__mcp_pack__"));
        assert!(!is_mcp_tool_name("__mcp_pack__"));

        let external = present("crm@lookup");
        assert_eq!(external.access, ToolAccess::Delegate);
        assert_eq!(external.verb(false), "External tool");
        assert_eq!(external.verb(true), "外部工具");
        assert_eq!(
            present("crm@__mcp_pack__").verb(false),
            "Tool pack",
            "the pack helper reads as the server's tool pack"
        );
    }

    #[test]
    fn an_mcp_call_reads_as_server_dot_tool() {
        assert_eq!(
            mcp_invocation_label("crm@lookup").as_deref(),
            Some("crm.lookup")
        );
        assert_eq!(
            mcp_invocation_label(" crm @ lookup ").as_deref(),
            Some("crm.lookup")
        );
        // Legacy transport shape from old durable rows.
        assert_eq!(
            mcp_invocation_label("mcp__crm__lookup").as_deref(),
            Some("crm.lookup")
        );
        assert_eq!(mcp_invocation_label("mcp_crm").as_deref(), Some("crm"));
        assert_eq!(mcp_invocation_label("crm@").as_deref(), None);
        assert_eq!(mcp_invocation_label("@lookup").as_deref(), None);
        assert_eq!(mcp_invocation_label("read_file").as_deref(), None);
        assert_eq!(present("mcp__crm__lookup").access, ToolAccess::Delegate);
    }

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
        assert_eq!(presentation.verb(false), "Using");
        assert_eq!(present("").verb(false), "Using");
    }

    #[test]
    fn only_workspace_reads_join_the_exploring_group() {
        assert!(present("read_file").workspace_read);
        assert!(present("搜索内容").workspace_read);
        assert!(present("list_files").workspace_read);
        // Image reads and glob patterns are exploring, same as the reference cards.
        assert!(present("读图工具").workspace_read);
        assert!(present("read_image").workspace_read);
        assert!(present("glob").workspace_read);
        // Web and knowledge reads keep their own card; their result is the point.
        assert!(!present("web_search").workspace_read);
        assert!(!present("网页搜索").workspace_read);
        assert!(!present("knowledge").workspace_read);
        assert!(!present("knowledge_search").workspace_read);
        assert!(!present("知识库").workspace_read);
        assert!(!present("execute_command").workspace_read);
        assert!(!present("write_file").workspace_read);
        assert!(!present("文本编辑").workspace_read);
        assert!(!present("crm@lookup").workspace_read);
    }

    #[test]
    fn browser_and_desktop_names_classify_whichever_side_arrives() {
        // The aggregate entry is display-side; the native names are what gets emitted.
        for name in ["浏览器", "browser", "browser_navigate"] {
            assert_eq!(present(name).access, ToolAccess::Execute, "{name}");
            assert_eq!(present(name).verb(false), "Browse", "{name}");
        }
        assert_eq!(present("桌面控制器").access, ToolAccess::Execute);
        assert_eq!(present("desktop_monitor").verb(false), "Desktop monitor");
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
        let glob = serde_json::json!({ "pattern": "**/*.rs" });
        assert_eq!(
            target_argument(&glob),
            Some(("pattern", "**/*.rs".to_string()))
        );
        let no_strings = serde_json::json!({ "limit": 3 });
        assert_eq!(target_argument(&no_strings), None);
    }
}
