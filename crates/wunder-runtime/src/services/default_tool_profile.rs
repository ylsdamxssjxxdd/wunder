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

/// 桌面版/命令行随包附带的默认技能：新建智能体默认勾选，用户可自行取消。
const DEFAULT_SKILL_NAMES: &[&str] = &[
    "技能创建器",
    "公文写作",
    "文稿校对",
    "3D网页",
    "知识短片",
    "图纸设计",
];

/// 桌面端不对智能体开放选择的内置工具：goal 由运行时按会话自动注入，
/// 其余为会话调度等系统级内部工具。桌面模式的
/// 内置工具集合以此过滤，保持与网页版"管理员未开放即不展示"的语义一致。
const DESKTOP_HIDDEN_TOOL_NAMES: &[&str] = &["goal", "会话线程控制"];

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
                // The phone surface only exists on the Android shell, which
                // installs its device host at startup. 桌面版/命令行/服务端 must not
                // advertise a control that has no executor.
                if crate::tools::is_phone_control_tool_name(&spec.name)
                    && !crate::tools::phone_tools_available()
                {
                    continue;
                }
                let name = if crate::tools::is_browser_tool_name(&spec.name) {
                    // 默认选择保存规范名；英文别名只给模型提示词用。
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
    // `browser` 与 `浏览器` 都代表整组浏览器能力；旧授权/默认值可能只写了
    // 英文别名，这里统一按规范名选中。
    let browser_allowed = allowed
        .iter()
        .any(|name| crate::tools::is_browser_group_name(name));
    for name in default_tool_candidates() {
        if DEFAULT_SKILL_NAMES.contains(&name.as_str()) {
            continue;
        }
        if canonical_allowed.contains(&name) {
            selected.push(name);
        } else if crate::tools::is_browser_group_name(&name) {
            if browser_allowed {
                selected.push(name);
                continue;
            }
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

/// 桌面版一次性迁移：早期默认画像或历史保存可能没有「桌面控制」，
/// 已存在的默认智能体会一直缺这个核心能力。这里只补一次；marker 写入后
/// 用户后续手动取消不会被重新加回。
pub fn migrate_local_default_agent_desktop_control(
    store: &crate::user_store::UserStore,
    user_id: &str,
) -> anyhow::Result<()> {
    let user_id = user_id.trim();
    let marker = format!("default_agent_desktop_control_migration_v1:{user_id}");
    if store.get_meta(&marker)?.as_deref() == Some("1") {
        return Ok(());
    }
    let canonical = crate::tools::resolve_tool_name("桌面控制");
    let meta_key = crate::services::default_agent_protocol::default_agent_meta_key(user_id);
    // 还没有任何默认智能体状态时不写 marker：之后从旧备份导入的智能体
    // 仍能在下一次启动时被修复。
    let mut inspected = false;
    if let Some(raw) = store.get_meta(&meta_key)? {
        inspected = true;
        if let Ok(mut config) = serde_json::from_str::<
            crate::services::default_agent_protocol::DefaultAgentConfig,
        >(raw.trim())
        {
            if ensure_desktop_control_selected(
                &mut config.tool_names,
                &mut config.declared_tool_names,
                &mut config.ability_items,
                &canonical,
            ) {
                config.updated_at = now_ts();
                store.set_meta(&meta_key, &serde_json::to_string(&config)?)?;
            }
        }
    }
    if let Some(mut record) = store.get_user_agent(user_id, "__default__")? {
        inspected = true;
        if ensure_desktop_control_selected(
            &mut record.tool_names,
            &mut record.declared_tool_names,
            &mut record.ability_items,
            &canonical,
        ) {
            record.updated_at = now_ts();
            store.upsert_user_agent(&record)?;
        }
    }
    if inspected {
        store.set_meta(&marker, "1")?;
    }
    Ok(())
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

fn contains_canonical_tool(names: &[String], canonical: &str) -> bool {
    names
        .iter()
        .any(|name| crate::tools::resolve_tool_name(name.trim()) == canonical)
}

fn ensure_desktop_control_selected(
    tool_names: &mut Vec<String>,
    declared_tool_names: &mut Vec<String>,
    ability_items: &mut Vec<crate::schemas::AbilityDescriptor>,
    canonical: &str,
) -> bool {
    use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};

    // 能力项存在时运行时会以它为准，只补 tool_names 不会进入线程工具面。
    let mut changed = false;
    if !contains_canonical_tool(tool_names, canonical) {
        tool_names.push(canonical.to_string());
        changed = true;
    }
    if !declared_tool_names.is_empty() && !contains_canonical_tool(declared_tool_names, canonical) {
        declared_tool_names.push(canonical.to_string());
        changed = true;
    }
    if let Some(item) = ability_items
        .iter_mut()
        .find(|item| crate::tools::resolve_tool_name(item.runtime_name.trim()) == canonical)
    {
        if !item.selected || !item.available {
            item.selected = true;
            item.available = true;
            changed = true;
        }
    } else if !ability_items.is_empty() {
        ability_items.push(AbilityDescriptor {
            id: format!("builtin:{canonical}"),
            name: canonical.to_string(),
            runtime_name: canonical.to_string(),
            display_name: canonical.to_string(),
            description: String::new(),
            input_schema: serde_json::Value::Null,
            group: AbilityGroupKey::Builtin,
            source: AbilitySourceKey::Builtin,
            kind: AbilityKind::Tool,
            owner_id: None,
            available: true,
            selected: true,
        });
        changed = true;
    }
    changed
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

/// 产品侧临时下架的工具：暂不对智能体与用户可见（网页搜索）。
/// 属临时隐藏，恢复时移除对应条目即可。按规范名匹配，别名先归一化。
const TEMPORARILY_HIDDEN_TOOL_NAMES: &[&str] = &["网页搜索"];

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
            "goal",
        ] {
            assert!(!selected.iter().any(|selected| selected == name));
        }
    }

    /// 桌面版/命令行默认工具面必须含「网页抓取」：本地形态不读
    /// tools.builtin.enabled，能否默认挂载只取决于 feature 是否编入与
    /// 配置开关，这里同时锁住两者，防止构建配置再把它裁掉。
    #[cfg(feature = "web-fetch")]
    #[test]
    fn desktop_defaults_include_web_fetch() {
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.tools.web.fetch.enabled = true;
        let skills = crate::skills::SkillRegistry::default();
        let allowed = crate::tools::collect_available_tool_names(&config, &skills, None);
        assert!(
            allowed.contains("网页抓取"),
            "web_fetch missing from allowed"
        );
        assert!(
            default_tool_names(&allowed).contains(&"网页抓取".to_string()),
            "web_fetch missing from default selection"
        );
        config.tools.web.fetch.enabled = false;
        let allowed = crate::tools::collect_available_tool_names(&config, &skills, None);
        assert!(!allowed.contains("网页抓取"));
    }

    #[test]
    fn desktop_control_migration_backfills_missing_tool_once() {
        use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};

        assert!(contains_canonical_tool(
            &["desktop_control".to_string()],
            "桌面控制"
        ));
        let mut tool_names = vec!["读取文件".to_string()];
        let mut declared = vec!["读取文件".to_string()];
        let mut items = vec![AbilityDescriptor {
            id: "builtin:读取文件".to_string(),
            name: "读取文件".to_string(),
            runtime_name: "读取文件".to_string(),
            display_name: "读取文件".to_string(),
            description: String::new(),
            input_schema: serde_json::Value::Null,
            group: AbilityGroupKey::Builtin,
            source: AbilitySourceKey::Builtin,
            kind: AbilityKind::Tool,
            owner_id: None,
            available: true,
            selected: true,
        }];
        assert!(ensure_desktop_control_selected(
            &mut tool_names,
            &mut declared,
            &mut items,
            "桌面控制"
        ));
        assert!(tool_names.iter().any(|name| name == "桌面控制"));
        assert!(declared.iter().any(|name| name == "桌面控制"));
        assert!(items.iter().any(|item| item.runtime_name == "桌面控制"));
        assert!(!ensure_desktop_control_selected(
            &mut tool_names,
            &mut declared,
            &mut items,
            "桌面控制"
        ));
    }

    #[cfg(feature = "sqlite-storage")]
    #[test]
    fn migration_repairs_existing_default_agent_meta_once() {
        use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};
        use crate::services::default_agent_protocol::{default_agent_meta_key, DefaultAgentConfig};

        let directory = tempfile::tempdir().expect("temp dir");
        let storage = std::sync::Arc::new(crate::storage::SqliteStorage::new(
            directory
                .path()
                .join("state.db")
                .to_string_lossy()
                .to_string(),
        ));
        let store = crate::user_store::UserStore::new(storage);
        let key = default_agent_meta_key("local_user");
        let config = DefaultAgentConfig {
            name: "默认智能体".to_string(),
            tool_names: vec!["读取文件".to_string()],
            declared_tool_names: vec!["读取文件".to_string()],
            ability_items: vec![AbilityDescriptor {
                id: "builtin:读取文件".to_string(),
                name: "读取文件".to_string(),
                runtime_name: "读取文件".to_string(),
                display_name: "读取文件".to_string(),
                description: String::new(),
                input_schema: serde_json::Value::Null,
                group: AbilityGroupKey::Builtin,
                source: AbilitySourceKey::Builtin,
                kind: AbilityKind::Tool,
                owner_id: None,
                available: true,
                selected: true,
            }],
            updated_at: 1.0,
            ..Default::default()
        };
        store
            .set_meta(&key, &serde_json::to_string(&config).expect("serialize"))
            .expect("seed meta");

        migrate_local_default_agent_desktop_control(&store, "local_user").expect("migrate");

        let repaired: DefaultAgentConfig =
            serde_json::from_str(&store.get_meta(&key).expect("read meta").expect("meta"))
                .expect("parse meta");
        assert!(repaired.tool_names.iter().any(|name| name == "桌面控制"));
        assert!(repaired
            .declared_tool_names
            .iter()
            .any(|name| name == "桌面控制"));
        assert!(repaired
            .ability_items
            .iter()
            .any(|item| item.runtime_name == "桌面控制" && item.selected));
        assert!(repaired.updated_at > 1.0);

        // 用户之后手动取消，迁移不能把它重新加回。
        let mut cleared = repaired;
        cleared.tool_names.retain(|name| name != "桌面控制");
        cleared
            .declared_tool_names
            .retain(|name| name != "桌面控制");
        cleared
            .ability_items
            .retain(|item| item.runtime_name != "桌面控制");
        store
            .set_meta(&key, &serde_json::to_string(&cleared).expect("serialize"))
            .expect("persist cleared");
        migrate_local_default_agent_desktop_control(&store, "local_user").expect("migrate again");
        let kept: DefaultAgentConfig =
            serde_json::from_str(&store.get_meta(&key).expect("read meta").expect("meta"))
                .expect("parse meta");
        assert!(!kept.tool_names.iter().any(|name| name == "桌面控制"));
    }

    #[cfg(feature = "desktop-control")]
    #[test]
    fn desktop_defaults_select_desktop_control() {
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.tools.desktop_control.enabled = true;
        let skills = crate::skills::SkillRegistry::default();
        let allowed = crate::tools::collect_available_tool_names(&config, &skills, None);
        assert!(allowed.contains("桌面控制"), "desktop control not allowed");
        assert!(
            default_tool_names(&allowed).contains(&"桌面控制".to_string()),
            "desktop control not selected by default; candidates: {:?}",
            default_tool_candidates()
        );
    }

    #[test]
    fn defaults_select_every_bundled_skill() {
        let allowed: HashSet<String> = DEFAULT_SKILL_NAMES
            .iter()
            .map(|name| name.to_string())
            .collect();
        let selected = default_tool_names(&allowed);
        for name in DEFAULT_SKILL_NAMES {
            assert!(selected.iter().any(|item| item == name), "missing {name}");
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
            ["文本编辑", "计划面板", "问询面板", "浏览器"]
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
    fn temporarily_hidden_tools_cover_web_search() {
        for name in ["网页搜索", "web_search"] {
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
