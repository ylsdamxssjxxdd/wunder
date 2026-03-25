use std::collections::HashSet;

const DEFAULT_BUILTIN_TOOL_NAMES: &[&str] = &[
    "最终回复",
    "定时任务",
    "休眠等待",
    "记忆管理",
    "执行命令",
    "ptc",
    "列出文件",
    "搜索内容",
    "读取文件",
    "网页抓取",
    "技能调用",
    "写入文件",
    "应用补丁",
];

const DEFAULT_SKILL_NAMES: &[&str] = &["技能创建器"];

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

pub fn curated_default_skill_names(allowed_tool_names: &HashSet<String>) -> Vec<String> {
    dedup_names(
        DEFAULT_SKILL_NAMES
            .iter()
            .map(|name| (*name).to_string())
            .filter(|name| allowed_tool_names.contains(name)),
    )
}

pub fn curated_default_tool_names(allowed_tool_names: &HashSet<String>) -> Vec<String> {
    let mut output = dedup_names(
        DEFAULT_BUILTIN_TOOL_NAMES
            .iter()
            .map(|name| (*name).to_string()),
    );
    output.extend(curated_default_skill_names(allowed_tool_names));
    dedup_names(output)
        .into_iter()
        .filter(|name| allowed_tool_names.contains(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{curated_default_tool_candidates, curated_default_tool_names};
    use crate::tools::resolve_tool_name;
    use std::collections::HashSet;

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
}
