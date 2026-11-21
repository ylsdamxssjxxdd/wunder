//! Desktop mirror of the web tool icon resolvers (`web/shared/tool-visuals.js`
//! and `frontend/src/utils/abilityVisuals.ts`). Message workflow entries use
//! the shared tool rules; the agent settings tool list uses the ability rules
//! so icons and badge tones match the web client entry for entry.
//!
//! Returned icon keys map 1:1 to `assets/icons/<key>.svg` and are resolved to
//! images by the `ToolIcons` global in `ui/tool_icons.slint` because Slint
//! needs compile-time `@image-url` literals.

/// Icon key plus badge tone, mirroring `resolveAbilityVisual`.
pub struct AbilityVisual {
    pub icon: &'static str,
    pub tone: &'static str,
}

/// Keyword rules from `tool-visuals.js` (`TOOL_ICON_RULES`).
const TOOL_RULES: &[(&[&str], &str)] = &[
    (&["用户世界工具", "user_world", "user world"], "earth-asia"),
    (
        &["桌面控制", "desktop_control", "desktop control"],
        "computer-mouse",
    ),
    (
        &[
            "手机控制",
            "phone_control",
            "phone control",
            "mobile_control",
        ],
        "smartphone",
    ),
    (
        &[
            "安卓屏幕",
            "android_screen",
            "android screen",
            "screen_capture",
        ],
        "display",
    ),
    (
        &[
            "安卓通知",
            "android_notifications",
            "android notifications",
            "notification_listener",
        ],
        "bell",
    ),
    (
        &["计划面板", "计划看板", "update_plan", "plan board"],
        "table-columns",
    ),
    (
        &["问询面板", "question_panel", "ask_panel", "question panel"],
        "circle-question",
    ),
    (&["目标态", "目标工具", "goal", "goal mode"], "bullseye"),
    (&["浏览器", "browser"], "window-maximize"),
    (&["技能调用", "skill_call", "skill_get"], "book-open"),
    (&["智能体蜂群", "agent_swarm", "swarm_control"], "bee"),
    (&["子智能体控制", "subagent_control"], "diagram-project"),
    (
        &["会话线程控制", "thread_control", "session_thread"],
        "code-branch",
    ),
    (
        &["网页搜索", "web_search", "web search", "websearch"],
        "magnifying-glass",
    ),
    (&["网页抓取", "web_fetch", "web fetch", "webfetch"], "globe"),
    (&["a2a观察", "a2a_observe"], "glasses"),
    (&["a2a等待", "a2a_wait"], "clock"),
    (
        &[
            "记忆管理",
            "memory_manager",
            "memory_manage",
            "memory manager",
        ],
        "memory",
    ),
    (&["a2ui"], "image"),
    (
        &[
            "读图工具",
            "read_image",
            "read image",
            "view_image",
            "view image",
        ],
        "eye",
    ),
    (
        &[
            "声转文",
            "语音转文",
            "transcribe_speech",
            "transcribe speech",
            "speech_to_text",
            "speech to text",
            "asr",
            "audio transcription",
        ],
        "microphone-lines",
    ),
    (
        &[
            "文转声",
            "语音生成",
            "generate_speech",
            "speech generation",
            "text_to_speech",
            "text to speech",
            "tts",
        ],
        "wave-square",
    ),
    (
        &[
            "图像生成",
            "绘图生成",
            "generate_image",
            "image generation",
            "text_to_image",
            "text to image",
        ],
        "paintbrush",
    ),
    (
        &[
            "视频生成",
            "generate_video",
            "video generation",
            "text_to_video",
            "text to video",
        ],
        "film",
    ),
    (
        &[
            "渠道工具",
            "channel_tool",
            "channel tool",
            "channel_send",
            "channel_contacts",
        ],
        "comments",
    ),
    (
        &["列出文件", "list files", "list_file", "list_files"],
        "folder-open",
    ),
    (&["读取文件", "read file", "read_file"], "file-lines"),
    (
        &["写入文件", "write file", "write_file"],
        "file-circle-plus",
    ),
    (
        &[
            "文本编辑",
            "text edit",
            "text_editor",
            "text editor",
            "edit_file2",
        ],
        "file-pen",
    ),
    (&["应用补丁", "apply patch", "apply_patch"], "pen-to-square"),
    (
        &[
            "命令会话",
            "command_session",
            "command session",
            "write_command_stdin",
        ],
        "terminal",
    ),
    (&["programmatic_tool_call", "ptc"], "code"),
    (
        &[
            "执行命令",
            "运行命令",
            "run command",
            "execute command",
            "execute_command",
            "shell",
        ],
        "terminal",
    ),
    (
        &[
            "定时任务",
            "计划任务",
            "cron",
            "schedule",
            "scheduled",
            "timer",
            "schedule_task",
        ],
        "clock",
    ),
    (
        &["搜索内容", "search_content", "search content"],
        "magnifying-glass",
    ),
    (
        &[
            "知识",
            "knowledge",
            "rag",
            "vector",
            "embedding",
            "document",
            "kb",
        ],
        "database",
    ),
    (&["mcp", "connector", "integration", "endpoint"], "plug"),
    (&["shared", "share"], "wrench"),
    (
        &[
            "最终回复",
            "final answer",
            "final response",
            "final_response",
        ],
        "paper-plane",
    ),
];

/// Tone-tagged rules from `abilityVisuals.ts` (`ABILITY_RULES`). Order and
/// keyword sets follow the web file; tones drive the badge palette.
const ABILITY_RULES: &[(&[&str], &str, &str)] = &[
    (
        &["用户世界工具", "user_world", "user world"],
        "earth-asia",
        "general",
    ),
    (&["桌面控制"], "computer-mouse", "general"),
    (
        &[
            "手机控制",
            "phone_control",
            "phone control",
            "mobile_control",
        ],
        "smartphone",
        "general",
    ),
    (
        &[
            "安卓屏幕",
            "android_screen",
            "android screen",
            "screen_capture",
        ],
        "display",
        "general",
    ),
    (
        &[
            "安卓通知",
            "android_notifications",
            "android notifications",
            "notification_listener",
        ],
        "bell",
        "general",
    ),
    (
        &[
            "渠道工具",
            "channel_tool",
            "channel tool",
            "channel_send",
            "channel_contacts",
        ],
        "comments",
        "general",
    ),
    (
        &[
            "最终回复",
            "final response",
            "final answer",
            "final reply",
            "final_response",
        ],
        "paper-plane",
        "general",
    ),
    (
        &["desktop_control", "desktop control"],
        "computer-mouse",
        "general",
    ),
    (
        &["update_plan", "plan board", "计划面板", "计划看板"],
        "table-columns",
        "automation",
    ),
    (
        &["question_panel", "ask_panel", "question panel", "问询面板"],
        "circle-question",
        "general",
    ),
    (&["browser", "浏览器"], "window-maximize", "search"),
    (
        &["a2a_observe", "a2a observe", "a2a观察"],
        "glasses",
        "automation",
    ),
    (&["a2a_wait", "a2a wait", "a2a等待"], "clock", "automation"),
    (
        &["agent_swarm", "swarm_control", "智能体蜂群"],
        "bee",
        "automation",
    ),
    (
        &["subagent_control", "子智能体控制"],
        "diagram-project",
        "automation",
    ),
    (
        &["thread_control", "session_thread", "会话线程控制"],
        "code-branch",
        "automation",
    ),
    (
        &["skill_call", "skill_get", "技能调用"],
        "book-open",
        "skill",
    ),
    (
        &["cron", "schedule_task", "scheduled task", "timer"],
        "clock",
        "automation",
    ),
    (&["计划任务", "定时任务"], "clock", "automation"),
    (
        &["sleep_wait", "sleep", "pause"],
        "hourglass-half",
        "automation",
    ),
    (&["休眠等待"], "hourglass-half", "automation"),
    (
        &[
            "memory_manager",
            "memory_manage",
            "memory manager",
            "memory",
        ],
        "memory",
        "automation",
    ),
    (&["记忆管理"], "memory", "automation"),
    (
        &["thread_control", "session_thread", "thread"],
        "code-branch",
        "automation",
    ),
    (
        &["subagent_control", "a2a", "subagent", "swarm"],
        "diagram-project",
        "automation",
    ),
    (
        &["web_fetch", "web fetch", "webfetch", "browse"],
        "globe",
        "search",
    ),
    (&["网页抓取"], "globe", "search"),
    (
        &["list_files", "list_file", "list files"],
        "folder-open",
        "file",
    ),
    (&["列出文件"], "folder-open", "file"),
    (
        &["搜索内容", "search_content", "search content"],
        "magnifying-glass",
        "search",
    ),
    (&["read_file", "read file"], "file-lines", "file"),
    (&["读取文件"], "file-lines", "file"),
    (&["write_file", "write file"], "file-circle-plus", "file"),
    (&["写入文件"], "file-circle-plus", "file"),
    (
        &[
            "edit_file2",
            "text edit",
            "text_editor",
            "text editor",
            "文本编辑",
        ],
        "file-pen",
        "file",
    ),
    (&["apply_patch", "apply patch"], "pen-to-square", "file"),
    (&["应用补丁"], "pen-to-square", "file"),
    (
        &[
            "命令会话",
            "command_session",
            "command session",
            "write_command_stdin",
        ],
        "terminal",
        "terminal",
    ),
    (&["programmatic_tool_call", "ptc"], "code", "file"),
    (
        &[
            "skill",
            "skills",
            "prompt",
            "workflow",
            "template",
            "agent preset",
            "preset",
        ],
        "book",
        "skill",
    ),
    (
        &["knowledge", "rag", "vector", "embedding", "document", "kb"],
        "database",
        "knowledge",
    ),
    (&["知识"], "database", "knowledge"),
    (
        &["mcp", "connector", "integration", "endpoint"],
        "plug",
        "mcp",
    ),
    (&["shared", "share"], "wrench", "shared"),
    (
        &[
            "shell",
            "terminal",
            "command",
            "powershell",
            "bash",
            "cmd",
            "execute_command",
            "run command",
            "execute command",
            "执行命令",
            "运行命令",
        ],
        "terminal",
        "terminal",
    ),
    (
        &[
            "file",
            "files",
            "read",
            "write",
            "patch",
            "edit",
            "folder",
            "workspace",
        ],
        "file-lines",
        "file",
    ),
    (
        &["image", "vision", "camera", "screenshot"],
        "image",
        "search",
    ),
];

fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

/// `normalizeMatchKey`: lowercase with separator characters removed so
/// `user-world`, `user_world` and `userworld` all match the same keyword.
fn match_key(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '.' | '-' | ':' | '/' | '\\' | '@'))
        .collect()
}

fn rule_matches(text: &str, normalized: &str, keyword: &str) -> bool {
    let lower = normalize(keyword);
    if !lower.is_empty() && text.contains(&lower) {
        return true;
    }
    let key = match_key(keyword);
    !key.is_empty() && normalized.contains(&key)
}

/// Canonical builtin names no keyword rule covers; mirrors the web
/// `TOOL_NAME_ICON_OVERRIDES` map in `web/shared/tool-visuals.js` and
/// `abilityVisuals.ts`. All entries carry the `file` badge tone.
fn name_icon_override(name: &str) -> Option<AbilityVisual> {
    let icon = match normalize(name).as_str() {
        "编辑" | "str_replace_editor" => "file-pen",
        "文件名匹配" | "文件名通配匹配" | "file glob match" | "glob" | "glob_files" => {
            "folder-tree"
        }
        _ => return None,
    };
    Some(AbilityVisual { icon, tone: "file" })
}

/// `resolveToolIconClass` from `web/shared/tool-visuals.js`. `category` maps a
/// desktop tool category onto the web-side group/source vocabulary; pass an
/// empty slice for workflow entries (they carry no category).
fn shared_tool_icon(name: &str, description: &str, category: &str) -> &'static str {
    let name_text = normalize(name);
    let mut text = name_text.clone();
    let description = normalize(description);
    if !description.is_empty() {
        if text.is_empty() {
            text = description.clone();
        } else {
            text.push(' ');
            text.push_str(&description);
        }
    }
    let normalized = match_key(&text);
    let name_normalized = match_key(&name_text);
    let group = group_for_category(category);
    if text.is_empty() && group.is_empty() {
        return "toolbox";
    }
    if group == "mcp" {
        return "plug";
    }
    if text == "wunder@excute" || text.ends_with("@wunder@excute") {
        return "dragon";
    }
    if text == "wunder@doc2md" || text.ends_with("@wunder@doc2md") {
        return "file-lines";
    }
    if let Some(visual) = name_icon_override(name) {
        return visual.icon;
    }
    // 名称优先：网页抓取的描述里会提到 browser，若先按整段文本匹配会被
    // 浏览器规则抢走图标。只有名称没有任何规则命中时才回退到描述。
    for (keywords, icon) in TOOL_RULES {
        if keywords
            .iter()
            .any(|k| rule_matches(&name_text, &name_normalized, k))
        {
            return icon;
        }
    }
    for (keywords, icon) in TOOL_RULES {
        if keywords.iter().any(|k| rule_matches(&text, &normalized, k)) {
            return icon;
        }
    }
    if name.contains('@') || description.contains('@') {
        return "plug";
    }
    match group.as_str() {
        "knowledge" => "database",
        "skill" => "book",
        "shared" | "user" => "wrench",
        _ => "toolbox",
    }
}

/// Desktop tool categories onto the web group/source vocabulary used by
/// `resolveAbilityVisual` when the agent settings page passes its group key.
fn group_for_category(category: &str) -> String {
    match normalize(category).as_str() {
        "mcp 工具" => "mcp".into(),
        "a2a 工具" => "a2a".into(),
        "技能" => "skill".into(),
        "知识库" => "knowledge".into(),
        "用户工具" => "user".into(),
        "共享工具" => "shared".into(),
        _ => String::new(),
    }
}

fn default_icon_for_tone(tone: &str) -> &'static str {
    match tone {
        "skill" => "book",
        "mcp" => "plug",
        "knowledge" => "database",
        "shared" => "wrench",
        "automation" => "diagram-project",
        "search" => "magnifying-glass",
        "file" => "file-lines",
        "terminal" => "terminal",
        _ => "toolbox",
    }
}

/// Icon for one agent-loop workflow entry: the compaction marker wins, then
/// the shared tool rules on the raw tool name with builtin grouping — the
/// same inputs `resolveWorkflowToolIconClass` passes on the web.
pub fn workflow_icon(title: &str) -> &'static str {
    let text = normalize(title);
    if text.is_empty() {
        return "toolbox";
    }
    // Web `isCompactionToolName` plus the desktop "上下文压缩" marker.
    if text.contains("compact") || title.contains("压缩") {
        return "compress";
    }
    shared_tool_icon(title, "", "")
}

/// Icon and badge tone for the agent settings tool list, mirroring
/// `resolveAbilityVisual` as invoked by `AgentToolOptionLabel`.
pub fn tool_card_visual(name: &str, description: &str, category: &str) -> AbilityVisual {
    let group = group_for_category(category);
    let kind_skill = group == "skill";
    let preferred_tone = match group.as_str() {
        "knowledge" => "knowledge",
        "mcp" => "mcp",
        "shared" => "shared",
        "a2a" => "automation",
        "skill" if kind_skill => "skill",
        _ => "",
    };
    if preferred_tone == "mcp" {
        return AbilityVisual {
            icon: "plug",
            tone: "mcp",
        };
    }
    // A skill is represented as a readable, reusable instruction package.
    // Its category is authoritative: name keywords such as "网页" or
    // "气象" describe the skill subject and must not turn it into a browser
    // or search tool in the settings catalogue.
    if kind_skill {
        return AbilityVisual {
            icon: "book",
            tone: "skill",
        };
    }
    if let Some(visual) = name_icon_override(name) {
        return visual;
    }
    let name_text = normalize(name);
    let mut text = name_text.clone();
    for part in [description, category] {
        let part = normalize(part);
        if !part.is_empty() {
            if text.is_empty() {
                text = part;
            } else {
                text.push(' ');
                text.push_str(&part);
            }
        }
    }
    let normalized = match_key(&text);
    let name_normalized = match_key(&name_text);
    // read_image / generate_image 的专属图标只按工具身份判定：读取文件的
    // 描述会写明「图片请用 read_image」，把描述算进来会让它也变成眼睛。
    if name_text.contains("read_image")
        || name_text.contains("view_image")
        || name_text.contains("读图工具")
        || name_text.contains("view image")
    {
        return AbilityVisual {
            icon: "eye",
            tone: "search",
        };
    }
    if name_text.contains("generate_image")
        || name_text.contains("text_to_image")
        || name_text.contains("image generation")
        || name_text.contains("图像生成")
        || name_text.contains("绘图生成")
    {
        return AbilityVisual {
            icon: "paintbrush",
            tone: "search",
        };
    }
    // 名称优先：网页抓取的描述会提到 browser，按整段文本先匹配会被浏览器
    // 规则抢走图标，和云端保持同一判定顺序。
    let mut matched: Option<(&'static str, &'static str)> = None;
    for (keywords, icon, tone) in ABILITY_RULES {
        if keywords
            .iter()
            .any(|k| rule_matches(&name_text, &name_normalized, k))
        {
            matched = Some((icon, tone));
            break;
        }
    }
    if matched.is_none() {
        for (keywords, icon, tone) in ABILITY_RULES {
            if keywords.iter().any(|k| rule_matches(&text, &normalized, k)) {
                matched = Some((icon, tone));
                break;
            }
        }
    }
    let tone = if !preferred_tone.is_empty() {
        preferred_tone
    } else if let Some((_, matched_tone)) = matched {
        matched_tone
    } else {
        "general"
    };
    let icon = match matched {
        Some((icon, _)) => icon,
        None => {
            let shared = shared_tool_icon(name, description, category);
            if shared != "toolbox" {
                shared
            } else {
                default_icon_for_tone(tone)
            }
        }
    };
    AbilityVisual { icon, tone }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_entries_match_web_icons() {
        assert_eq!(workflow_icon("上下文压缩"), "compress");
        assert_eq!(workflow_icon("context_compaction"), "compress");
        assert_eq!(workflow_icon("execute_command"), "terminal");
        assert_eq!(workflow_icon("执行命令"), "terminal");
        assert_eq!(workflow_icon("read_file"), "file-lines");
        assert_eq!(workflow_icon("列出文件"), "folder-open");
        assert_eq!(workflow_icon("web_search"), "magnifying-glass");
        assert_eq!(workflow_icon("generate_image"), "paintbrush");
        assert_eq!(workflow_icon("编辑"), "file-pen");
        assert_eq!(workflow_icon("str_replace_editor"), "file-pen");
        assert_eq!(workflow_icon("文件名匹配"), "folder-tree");
        assert_eq!(workflow_icon("glob"), "folder-tree");
        assert_eq!(workflow_icon("glob_files"), "folder-tree");
        assert_eq!(workflow_icon("wunder@excute"), "dragon");
        assert_eq!(workflow_icon("手机控制"), "smartphone");
        assert_eq!(workflow_icon("phone_control"), "smartphone");
        assert_eq!(workflow_icon("mcp_server.query"), "plug");
        assert_eq!(workflow_icon("未知工具"), "toolbox");
    }

    #[test]
    fn tool_cards_match_web_icons_and_tones() {
        let mcp = tool_card_visual("mcp-a", "连接器", "MCP 工具");
        assert_eq!((mcp.icon, mcp.tone), ("plug", "mcp"));
        let skill = tool_card_visual("写作技能", "长文工作流", "技能");
        assert_eq!((skill.icon, skill.tone), ("book", "skill"));
        for name in ["3D网页", "技能创建器", "通用气象知识"] {
            let skill = tool_card_visual(name, "", "技能");
            assert_eq!((skill.icon, skill.tone), ("book", "skill"), "{name}");
        }
        let kb = tool_card_visual("产品知识库", "文档检索", "知识库");
        assert_eq!((kb.icon, kb.tone), ("database", "knowledge"));
        let term = tool_card_visual("执行命令", "运行 shell 命令", "内置工具");
        assert_eq!((term.icon, term.tone), ("terminal", "terminal"));
        let a2a = tool_card_visual("a2a_wait", "等待其他智能体", "A2A 工具");
        assert_eq!((a2a.icon, a2a.tone), ("clock", "automation"));
        let user = tool_card_visual("我的工具", "", "用户工具");
        assert_eq!((user.icon, user.tone), ("wrench", "general"));
        let phone = tool_card_visual("手机控制", "手机控制、界面读取与屏幕截图", "内置工具");
        assert_eq!((phone.icon, phone.tone), ("smartphone", "general"));
        // 读取文件的描述会提示「图片请用 read_image」，不能因此变成眼睛图标。
        let read = tool_card_visual(
            "读取文件",
            "读取本地纯文本文件；图片请用 read_image。",
            "内置工具",
        );
        assert_eq!((read.icon, read.tone), ("file-lines", "file"));
        let image = tool_card_visual("读图工具", "读取本地图片、GIF 或视频。", "内置工具");
        assert_eq!((image.icon, image.tone), ("eye", "search"));
    }

    #[test]
    fn canonical_builtin_tools_match_the_cloud_glyph_table() {
        let cases = [
            ("执行命令", "terminal", "terminal"),
            ("运行命令", "terminal", "terminal"),
            ("命令会话", "terminal", "terminal"),
            ("读取文件", "file-lines", "file"),
            ("列出文件", "folder-open", "file"),
            ("写入文件", "file-circle-plus", "file"),
            ("编辑", "file-pen", "file"),
            ("应用补丁", "pen-to-square", "file"),
            ("文件名匹配", "folder-tree", "file"),
            ("文件名通配匹配", "folder-tree", "file"),
            ("File Glob Match", "folder-tree", "file"),
            ("网页抓取", "globe", "search"),
            ("浏览器", "window-maximize", "search"),
            ("网页搜索", "magnifying-glass", "general"),
            ("读图工具", "eye", "search"),
            ("图像生成", "paintbrush", "search"),
            ("桌面控制", "computer-mouse", "general"),
            ("手机控制", "smartphone", "general"),
            ("安卓屏幕", "display", "general"),
            ("安卓通知", "bell", "general"),
            ("计划任务", "clock", "automation"),
            ("技能调用", "book-open", "skill"),
        ];
        for (name, icon, tone) in cases {
            let visual = tool_card_visual(name, "", "内置工具");
            assert_eq!((visual.icon, visual.tone), (icon, tone), "{name}");
        }
    }

    #[test]
    fn web_fetch_name_wins_over_browser_wording_in_description() {
        let description =
            "抓取单个公开网页 URL 并提取正文。登录、注册、填表或点击等交互任务直接使用 browser 的 action=navigate。";
        let fetch = tool_card_visual("网页抓取", description, "内置工具");
        assert_eq!((fetch.icon, fetch.tone), ("globe", "search"));
        let browser = tool_card_visual("浏览器", description, "内置工具");
        assert_eq!((browser.icon, browser.tone), ("window-maximize", "search"));
    }
}
