use crate::config::ToolVisibilityRule;
use crate::services::org_units;
use crate::storage::{OrgUnitRecord, UserAccountRecord};
use std::collections::HashSet;

pub fn normalize_visible_unit_ids(values: Vec<String>) -> Vec<String> {
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

pub fn expand_visible_unit_ids(
    visible_unit_ids: &[String],
    units: &[OrgUnitRecord],
) -> HashSet<String> {
    let roots = normalize_visible_unit_ids(visible_unit_ids.to_vec());
    org_units::collect_descendant_unit_ids(units, &roots)
}

pub fn normalize_tool_visibility_rules(rules: Vec<ToolVisibilityRule>) -> Vec<ToolVisibilityRule> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for mut rule in rules {
        let name = crate::tools::resolve_tool_name(rule.name.trim());
        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }
        rule.name = name;
        rule.visible_unit_ids = normalize_visible_unit_ids(rule.visible_unit_ids);
        output.push(rule);
    }
    output
}

pub fn filter_tool_visibility(
    allowed: HashSet<String>,
    rules: &[ToolVisibilityRule],
    units: &[OrgUnitRecord],
    user: &UserAccountRecord,
) -> HashSet<String> {
    if rules.is_empty() {
        return allowed;
    }
    let user_unit_id = user
        .unit_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let rule_map = normalize_tool_visibility_rules(rules.to_vec())
        .into_iter()
        .map(|rule| {
            (
                rule.name,
                expand_visible_unit_ids(&rule.visible_unit_ids, units),
            )
        })
        .collect::<Vec<_>>();
    let mut filtered = HashSet::new();
    for tool_name in allowed {
        let canonical = crate::tools::resolve_tool_name(&tool_name);
        let Some((_, visible_units)) = rule_map.iter().find(|(name, _)| name == &canonical) else {
            filtered.insert(tool_name);
            continue;
        };
        if visible_units.is_empty() {
            filtered.insert(tool_name);
            continue;
        }
        if let Some(unit_id) = user_unit_id {
            if visible_units.contains(unit_id) {
                filtered.insert(tool_name);
            }
        }
    }
    filtered
}
