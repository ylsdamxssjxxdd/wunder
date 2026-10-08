use crate::config::Config;
use std::collections::HashSet;

const DEFAULT_BUILTIN_TOOL_NAMES: &[&str] = &[
    "定时任务",
    "记忆管理",
    "执行命令",
    "命令会话",
    "ptc",
    "列出文件",
    "glob",
    "搜索内容",
    "读取文件",
    "网页抓取",
    "技能调用",
    "写入文件",
    "编辑",
    "str_replace_editor",
];

const DEFAULT_SKILL_NAMES: &[&str] = &["技能创建器"];

/// 桌面端不对智能体开放选择的内置工具：goal 三件套由运行时按会话自动注入，
/// 其余为会话调度等系统级内部工具。桌面模式的
/// 内置工具集合以此过滤，保持与网页版"管理员未开放即不展示"的语义一致。
const DESKTOP_HIDDEN_TOOL_NAMES: &[&str] = &[
    "get_goal",
    "create_goal",
    "update_goal",
    "会话线程控制",
    "会话让出",
];

/// 桌面端默认智能体在通用默认工具之外额外启用的本地能力工具。
const DESKTOP_EXTRA_DEFAULT_TOOL_NAMES: &[&str] =
    &["读图工具", "桌面监视器", "桌面控制器", "子智能体控制"];

fn dedup_names(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let cleaned = raw.trim().to_string();
        if cleaned.is_empty() || !seen.insert(cleaned.clone()) {
            continue;
        }
        output.push(cleaned);
    }
    output
}

pub fn curated_default_tool_candidates() -> Vec<String> {
    dedup_names(
        DEFAULT_BUILTIN_TOOL_NAMES
            .iter()
            .chain(DEFAULT_SKILL_NAMES.iter())
            .map(|name| (*name).to_string()),
    )
}

/// 桌面端默认智能体额外启用的工具名（未过滤可用性）。
pub fn desktop_extra_default_tool_candidates() -> Vec<String> {
    DESKTOP_EXTRA_DEFAULT_TOOL_NAMES
        .iter()
        .map(|name| name.to_string())
        .collect()
}

/// 判断内置工具名是否在桌面端隐藏（不可选择、不默认启用）。
pub fn is_desktop_hidden_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    !cleaned.is_empty() && DESKTOP_HIDDEN_TOOL_NAMES.contains(&cleaned)
}

pub fn curated_default_skill_names(allowed_tool_names: &HashSet<String>) -> Vec<String> {
    dedup_names(
        DEFAULT_SKILL_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .filter(|name| allowed_tool_names.contains(name)),
    )
}

/// 通用默认工具（服务器模式语义），与历史行为保持一致。
pub fn curated_default_tool_names(allowed_tool_names: &HashSet<String>) -> Vec<String> {
    curated_default_tool_names_with_desktop_extras(allowed_tool_names, false)
}

/// 桌面模式的默认工具：在通用默认之上追加桌面本地能力工具，均按可用集合过滤。
pub fn curated_default_tool_names_for_config(
    config: &Config,
    allowed_tool_names: &HashSet<String>,
) -> Vec<String> {
    let desktop = config.server.mode.trim().eq_ignore_ascii_case("desktop");
    curated_default_tool_names_with_desktop_extras(allowed_tool_names, desktop)
}

pub fn curated_default_tool_names_with_desktop_extras(
    allowed_tool_names: &HashSet<String>,
    include_desktop_extras: bool,
) -> Vec<String> {
    let mut output = dedup_names(
        DEFAULT_BUILTIN_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_string()),
    );
    output.extend(curated_default_skill_names(allowed_tool_names));
    if include_desktop_extras {
        output.extend(desktop_extra_default_tool_candidates());
    }
    dedup_names(output)
        .into_iter()
        .filter(|name| allowed_tool_names.contains(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        curated_default_tool_candidates, curated_default_tool_names,
        curated_default_tool_names_for_config, curated_default_tool_names_with_desktop_extras,
        is_desktop_hidden_tool_name, Config,
    };
    use crate::tools::resolve_tool_name;
    use std::collections::HashSet;

    fn allowed_set() -> HashSet<String> {
        curated_default_tool_candidates()
            .into_iter()
            .chain(super::desktop_extra_default_tool_candidates())
            .chain([
                "get_goal".to_string(),
                "create_goal".to_string(),
                "update_goal".to_string(),
            ])
            .collect()
    }

    #[test]
    fn self_status_is_not_enabled_by_default() {
        let canonical = resolve_tool_name("self_status");
        let defaults = curated_default_tool_candidates();
        assert!(!defaults.contains(&canonical));
    }

    #[test]
    fn curated_default_selection_keeps_self_status_disabled() {
        let canonical = resolve_tool_name("self_status");
        let mut allowed = curated_default_tool_candidates()
            .into_iter()
            .collect::<HashSet<_>>();
        allowed.insert(canonical.clone());
        let selected = curated_default_tool_names(&allowed);
        assert!(!selected.contains(&canonical));
    }

    #[test]
    fn curated_default_selection_includes_command_session_for_background_exec() {
        let allowed = curated_default_tool_candidates()
            .into_iter()
            .collect::<HashSet<_>>();
        let selected = curated_default_tool_names(&allowed);
        assert!(selected.iter().any(|name| name == "执行命令"));
        assert!(selected.iter().any(|name| name == "命令会话"));
    }

    #[test]
    fn desktop_extras_only_apply_when_requested() {
        let allowed = allowed_set();
        let base = curated_default_tool_names_with_desktop_extras(&allowed, false);
        assert!(!base.iter().any(|name| name == "桌面监视器"));
        let desktop = curated_default_tool_names_with_desktop_extras(&allowed, true);
        for name in super::desktop_extra_default_tool_candidates() {
            assert!(desktop.contains(&name));
        }
    }

    #[test]
    fn desktop_mode_adds_extras_and_server_mode_does_not() {
        let allowed = allowed_set();
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        let desktop = curated_default_tool_names_for_config(&config, &allowed);
        assert!(desktop.iter().any(|name| name == "读图工具"));
        assert!(desktop.iter().any(|name| name == "子智能体控制"));

        config.server.mode = "server".to_string();
        let server = curated_default_tool_names_for_config(&config, &allowed);
        assert!(!server.iter().any(|name| name == "读图工具"));
    }

    #[test]
    fn desktop_hidden_tool_names_cover_system_tools() {
        for name in [
            "get_goal",
            "create_goal",
            "update_goal",
            "会话让出",
            "会话线程控制",
        ] {
            assert!(is_desktop_hidden_tool_name(name));
        }
        assert!(!is_desktop_hidden_tool_name("读取文件"));
        assert!(!is_desktop_hidden_tool_name(""));
    }
}
