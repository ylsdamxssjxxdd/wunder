use std::collections::HashSet;

pub const DEFAULT_TOOL_PROFILE_VERSION: &str = "authorized-v1";

/// A stable identity for the default tool surface. Defaults deliberately use
/// the caller's whole authorized set; named smaller profiles are explicit
/// routing choices and never silently revoke an existing authorization.
pub fn authorized_profile_revision(allowed: &HashSet<String>) -> String {
    let specs = crate::tools::builtin_tool_specs()
        .into_iter()
        .filter(|spec| {
            let canonical = crate::tools::resolve_tool_name(&spec.name);
            allowed.contains(&spec.name) || allowed.contains(&canonical)
        })
        .collect::<Vec<_>>();
    crate::tools::tool_profile_snapshot(&specs).revision
}

const DEFAULT_SKILL_NAMES: &[&str] = &["技能创建器"];

/// 桌面端不对智能体开放选择的内置工具：goal 由运行时按会话自动注入，
/// 其余为会话调度等系统级内部工具。桌面模式的
/// 内置工具集合以此过滤，保持与网页版"管理员未开放即不展示"的语义一致。
const DESKTOP_HIDDEN_TOOL_NAMES: &[&str] = &["goal", "会话线程控制", "会话让出"];

/// 默认选择直接来自内置工具目录；浏览器成员统一投影为可选择的聚合项。
pub fn default_tool_candidates() -> Vec<String> {
    static CANDIDATES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    CANDIDATES
        .get_or_init(|| {
            let mut seen = HashSet::new();
            let mut names = Vec::new();
            for spec in crate::tools::builtin_tool_specs() {
                if crate::services::goal::is_goal_tool_name(&spec.name)
                    || is_temporarily_hidden_tool_name(&spec.name)
                {
                    continue;
                }
                let name = if crate::tools::is_browser_tool_name(&spec.name) {
                    crate::tools::BROWSER_GROUP_NAME.to_string()
                } else {
                    spec.name
                };
                if seen.insert(name.clone()) {
                    names.push(name);
                }
            }
            names.extend(DEFAULT_SKILL_NAMES.iter().map(|name| name.to_string()));
            names
        })
        .clone()
}

/// 仅选择授权集合里的内置工具；权限只放行浏览器成员时保留成员边界。
pub fn default_builtin_tool_names(allowed: &HashSet<String>) -> Vec<String> {
    let canonical_allowed: HashSet<String> = allowed
        .iter()
        .map(|name| crate::tools::resolve_tool_name(name.trim()))
        .collect();
    let mut selected = Vec::new();
    for name in default_tool_candidates() {
        if DEFAULT_SKILL_NAMES.contains(&name.as_str()) {
            continue;
        }
        if canonical_allowed.contains(&name) {
            selected.push(name);
        } else if crate::tools::is_browser_group_name(&name) {
            selected.extend(
                crate::tools::browser_tool_names()
                    .into_iter()
                    .filter(|member| canonical_allowed.contains(member)),
            );
        }
    }
    selected
}

/// 三种执行形态共用默认选择：全部可用内置工具，加上既有默认技能。
pub fn default_tool_names(allowed: &HashSet<String>) -> Vec<String> {
    let mut selected = default_builtin_tool_names(allowed);
    selected.extend(
        DEFAULT_SKILL_NAMES
            .iter()
            .filter(|name| allowed.contains(**name))
            .map(|name| name.to_string()),
    );
    // `allowed` can include dynamic/user capabilities. Default-agent creation
    // intentionally selects only shipped defaults, but it must be deterministic.
    selected.sort();
    selected
}

/// 已有默认智能体补齐首次可用的内置工具；记住已处理集合，保留后续手动取消。
pub(crate) fn initialize_default_builtin_tools(
    store: &crate::user_store::UserStore,
    user_id: &str,
    config: &mut crate::services::default_agent_protocol::DefaultAgentConfig,
    allowed: &HashSet<String>,
    skill_names: &HashSet<String>,
) -> anyhow::Result<()> {
    let key = format!("default_agent_builtin_baseline_v1:{}", user_id.trim());
    let previous = store
        .get_meta(&key)?
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok());
    let mut known: HashSet<String> = previous
        .as_deref()
        .unwrap_or_default()
        .iter()
        .cloned()
        .collect();
    let defaults = default_builtin_tool_names(allowed);
    let additions: Vec<String> = defaults
        .into_iter()
        .filter(|name| known.insert(name.clone()))
        .collect();
    if !additions.is_empty() {
        use crate::services::agent_abilities::{
            resolve_agent_runtime_tool_names, resolve_record_ability_items,
            resolve_selected_declared_names,
        };
        let mut selected = resolve_agent_runtime_tool_names(
            &config.tool_names,
            &config.declared_tool_names,
            &config.declared_skill_names,
        );
        selected.extend(additions);
        config.tool_names =
            crate::services::user_agent_presets::filter_allowed_tools(&selected, allowed);
        let (tools, skills) = resolve_selected_declared_names(
            &config.tool_names,
            &config.declared_tool_names,
            &config.declared_skill_names,
            skill_names,
        );
        config.ability_items =
            resolve_record_ability_items(&[], &config.tool_names, &tools, &skills, skill_names);
        config.declared_tool_names = tools;
        config.declared_skill_names = skills;
        store.set_meta(
            &crate::services::default_agent_protocol::default_agent_meta_key(user_id),
            &serde_json::to_string(config)?,
        )?;
        if store.get_user_agent(user_id, "__default__")?.is_some() {
            store.upsert_user_agent(
                &crate::services::default_agent_protocol::record_from_default_agent_config(
                    "__default__",
                    user_id,
                    "A",
                    config,
                ),
            )?;
        }
    }
    let mut baseline: Vec<String> = known.into_iter().collect();
    baseline.sort();
    if previous.as_ref() != Some(&baseline) {
        store.set_meta(&key, &serde_json::to_string(&baseline)?)?;
    }
    Ok(())
}

/// 判断内置工具名是否在桌面端隐藏（不可选择、不默认启用）。
pub fn is_desktop_hidden_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    !cleaned.is_empty() && DESKTOP_HIDDEN_TOOL_NAMES.contains(&cleaned)
}

/// 产品侧临时下架的工具：暂不对智能体与用户可见（网页搜索与 ptc）。
/// 属临时隐藏，恢复时移除对应条目即可。按规范名匹配，别名先归一化。
const TEMPORARILY_HIDDEN_TOOL_NAMES: &[&str] = &["网页搜索", "ptc"];

/// 判断工具是否处于"临时隐藏"状态：无论配置是否启用，智能体与用户都不可见。
pub fn is_temporarily_hidden_tool_name(name: &str) -> bool {
    let cleaned = name.trim();
    if cleaned.is_empty() {
        return false;
    }
    let canonical = crate::tools::resolve_tool_name(cleaned);
    TEMPORARILY_HIDDEN_TOOL_NAMES
        .iter()
        .any(|hidden| *hidden == canonical || *hidden == cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn defaults_include_every_available_builtin_without_external_tools() {
        let mut allowed: HashSet<String> = default_tool_candidates().into_iter().collect();
        allowed.extend(
            [
                "sample_mcp@tool",
                "sample_skill",
                "sample_knowledge",
                "ptc",
                "goal",
            ]
            .into_iter()
            .map(str::to_string),
        );
        let selected = default_tool_names(&allowed);
        for name in [
            "浏览器",
            "文本编辑",
            "计划面板",
            "问询面板",
            "self_status",
            "执行命令",
            "命令会话",
        ] {
            let canonical = crate::tools::resolve_tool_name(name);
            assert!(selected.contains(&canonical), "missing {canonical}");
        }
        for name in [
            "sample_mcp@tool",
            "sample_skill",
            "sample_knowledge",
            "ptc",
            "goal",
        ] {
            assert!(!selected.iter().any(|selected| selected == name));
        }
    }

    #[test]
    fn defaults_resolve_aliases_and_preserve_partial_browser_permissions() {
        let allowed = ["edit", "update_plan", "question_panel", "browser"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let selected = default_tool_names(&allowed);
        assert_eq!(
            selected.into_iter().collect::<HashSet<_>>(),
            ["文本编辑", "计划面板", "问询面板", "browser"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[test]
    fn defaults_follow_runtime_gates_in_all_forms() {
        for mode in ["api", "desktop", "cli"] {
            let mut config = Config::default();
            config.server.mode = mode.to_string();
            config.tools.builtin.enabled = default_tool_candidates();
            config.browser.enabled = true;
            config.tools.browser.enabled = true;
            let skills = crate::skills::SkillRegistry::default();
            let allowed = crate::tools::collect_available_tool_names(&config, &skills, None);
            let selected = default_tool_names(&allowed);
            for name in ["浏览器", "文本编辑", "计划面板", "问询面板"] {
                assert!(
                    selected.iter().any(|selected| selected == name),
                    "{mode}: missing {name}"
                );
            }
            assert!(!selected.iter().any(|name| name == "browser_navigate"));
            config.tools.browser.enabled = false;
            let allowed = crate::tools::collect_available_tool_names(&config, &skills, None);
            assert!(!default_tool_names(&allowed)
                .iter()
                .any(|name| name == "浏览器"));
        }
    }

    #[test]
    fn temporarily_hidden_tools_cover_web_search_and_ptc() {
        for name in ["网页搜索", "web_search", "ptc", "programmatic_tool_call"] {
            assert!(is_temporarily_hidden_tool_name(name));
        }
        assert!(!is_temporarily_hidden_tool_name("读取文件"));
    }

    #[test]
    fn authorized_profile_revision_is_stable_for_permission_order() {
        let first = HashSet::from(["读取文件".to_string(), "执行命令".to_string()]);
        let second = HashSet::from(["执行命令".to_string(), "读取文件".to_string()]);
        assert_eq!(
            authorized_profile_revision(&first),
            authorized_profile_revision(&second)
        );
        assert!(!authorized_profile_revision(&first).is_empty());
    }
}
