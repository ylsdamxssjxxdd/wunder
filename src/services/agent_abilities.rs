use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Debug, Clone, Default)]
pub struct AgentAbilitySelection {
    pub tool_names: Vec<String>,
    pub declared_tool_names: Vec<String>,
    pub declared_skill_names: Vec<String>,
    pub ability_items: Vec<AbilityDescriptor>,
}

fn ability_source_key(source: AbilitySourceKey) -> &'static str {
    match source {
        AbilitySourceKey::Builtin => "builtin",
        AbilitySourceKey::Mcp => "mcp",
        AbilitySourceKey::A2a => "a2a",
        AbilitySourceKey::Skill => "skill",
        AbilitySourceKey::Knowledge => "knowledge",
        AbilitySourceKey::UserMcp => "user_mcp",
        AbilitySourceKey::UserSkill => "user_skill",
        AbilitySourceKey::UserKnowledge => "user_knowledge",
        AbilitySourceKey::Shared => "shared",
    }
}

fn ability_group_and_source(kind: AbilityKind) -> (AbilityGroupKey, AbilitySourceKey) {
    match kind {
        AbilityKind::Tool => (AbilityGroupKey::Builtin, AbilitySourceKey::Builtin),
        AbilityKind::Skill => (AbilityGroupKey::Skills, AbilitySourceKey::Skill),
    }
}

fn build_ability_id(
    source: AbilitySourceKey,
    owner_id: Option<&str>,
    runtime_name: &str,
) -> String {
    let runtime_name = runtime_name.trim();
    match owner_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(owner_id) => format!("{}:{owner_id}:{runtime_name}", ability_source_key(source)),
        None => format!("{}:{runtime_name}", ability_source_key(source)),
    }
}

pub fn normalize_names<I, S>(values: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for raw in values {
        let cleaned = raw.as_ref().trim();
        if cleaned.is_empty() {
            continue;
        }
        let owned = cleaned.to_string();
        if seen.insert(owned.clone()) {
            output.push(owned);
        }
    }
    output
}

fn split_requested_tool_names(
    requested_tool_names: &[String],
    skill_name_keys: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let mut tool_names = Vec::new();
    let mut skill_names = Vec::new();
    for name in normalize_names(requested_tool_names.iter().map(String::as_str)) {
        if skill_name_keys.contains(&name) {
            skill_names.push(name);
        } else {
            tool_names.push(name);
        }
    }
    (tool_names, skill_names)
}

pub fn resolve_selected_declared_names(
    requested_tool_names: &[String],
    explicit_declared_tool_names: &[String],
    explicit_declared_skill_names: &[String],
    skill_name_keys: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    let selected_names = normalize_names(requested_tool_names.iter().map(String::as_str));
    let selected_name_set: HashSet<String> = selected_names.iter().cloned().collect();
    let mut covered = HashSet::new();
    let mut declared_tool_names = Vec::new();
    let mut declared_skill_names = Vec::new();

    let mut push_declared_name = |name: String| {
        if !selected_name_set.contains(&name) || !covered.insert(name.clone()) {
            return;
        }
        if skill_name_keys.contains(&name) {
            declared_skill_names.push(name);
        } else {
            declared_tool_names.push(name);
        }
    };

    for name in normalize_names(explicit_declared_skill_names.iter().map(String::as_str)) {
        push_declared_name(name);
    }
    for name in normalize_names(explicit_declared_tool_names.iter().map(String::as_str)) {
        push_declared_name(name);
    }
    for name in selected_names {
        push_declared_name(name);
    }

    (declared_tool_names, declared_skill_names)
}

fn synthesize_ability_descriptor(runtime_name: &str, kind: AbilityKind) -> AbilityDescriptor {
    let runtime_name = runtime_name.trim().to_string();
    let (group, source) = ability_group_and_source(kind);
    AbilityDescriptor {
        id: build_ability_id(source, None, &runtime_name),
        name: runtime_name.clone(),
        runtime_name: runtime_name.clone(),
        display_name: runtime_name.clone(),
        description: String::new(),
        input_schema: Value::Null,
        group,
        source,
        kind,
        owner_id: None,
        available: true,
        selected: true,
    }
}

pub fn normalize_ability_items(items: Vec<AbilityDescriptor>) -> Vec<AbilityDescriptor> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();
    for item in items {
        let kind = item.kind;
        let runtime_name = {
            let runtime_name = item.runtime_name.trim();
            if !runtime_name.is_empty() {
                Some(runtime_name.to_string())
            } else {
                let name = item.name.trim();
                (!name.is_empty()).then(|| name.to_string())
            }
        };
        let Some(runtime_name) = runtime_name else {
            continue;
        };
        let owner_id = item
            .owner_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let key = format!(
            "{}:{}:{runtime_name}",
            ability_source_key(item.source),
            owner_id.as_deref().unwrap_or_default()
        );
        if !seen.insert(key) {
            continue;
        }
        let name = item.name.trim();
        let display_name = item.display_name.trim();
        let description = item.description.trim();
        output.push(AbilityDescriptor {
            id: item.id.trim().to_string().if_empty_then(|| {
                build_ability_id(item.source, owner_id.as_deref(), &runtime_name)
            }),
            name: if name.is_empty() {
                runtime_name.clone()
            } else {
                name.to_string()
            },
            runtime_name: runtime_name.clone(),
            display_name: if display_name.is_empty() {
                runtime_name.clone()
            } else {
                display_name.to_string()
            },
            description: description.to_string(),
            input_schema: item.input_schema,
            group: item.group,
            source: item.source,
            kind,
            owner_id,
            available: true,
            selected: true,
        });
    }
    output
}

pub fn split_ability_item_names(items: &[AbilityDescriptor]) -> (Vec<String>, Vec<String>) {
    let mut tool_names = Vec::new();
    let mut skill_names = Vec::new();
    for item in items {
        let runtime_name = item.runtime_name.trim();
        let fallback_name = item.name.trim();
        let name = if runtime_name.is_empty() {
            fallback_name
        } else {
            runtime_name
        };
        if name.is_empty() {
            continue;
        }
        match item.kind {
            AbilityKind::Tool => tool_names.push(name.to_string()),
            AbilityKind::Skill => skill_names.push(name.to_string()),
        }
    }
    (
        normalize_names(tool_names.iter().map(String::as_str)),
        normalize_names(skill_names.iter().map(String::as_str)),
    )
}

pub fn build_ability_items_from_names(
    tool_names: &[String],
    skill_names: &[String],
) -> Vec<AbilityDescriptor> {
    let mut items = Vec::new();
    for name in normalize_names(tool_names.iter().map(String::as_str)) {
        items.push(synthesize_ability_descriptor(&name, AbilityKind::Tool));
    }
    for name in normalize_names(skill_names.iter().map(String::as_str)) {
        items.push(synthesize_ability_descriptor(&name, AbilityKind::Skill));
    }
    items
}

pub fn build_ability_items_from_legacy(
    tool_names: &[String],
    declared_tool_names: &[String],
    declared_skill_names: &[String],
    skill_name_keys: &HashSet<String>,
) -> Vec<AbilityDescriptor> {
    let (resolved_tool_names, resolved_skill_names) =
        if !declared_tool_names.is_empty() || !declared_skill_names.is_empty() {
            (
                normalize_names(declared_tool_names.iter().map(String::as_str)),
                normalize_names(declared_skill_names.iter().map(String::as_str)),
            )
        } else {
            split_requested_tool_names(tool_names, skill_name_keys)
        };
    build_ability_items_from_names(&resolved_tool_names, &resolved_skill_names)
}

pub fn resolve_record_ability_items(
    stored_ability_items: &[AbilityDescriptor],
    tool_names: &[String],
    declared_tool_names: &[String],
    declared_skill_names: &[String],
    skill_name_keys: &HashSet<String>,
) -> Vec<AbilityDescriptor> {
    let ability_items = normalize_ability_items(stored_ability_items.to_vec());
    if ability_items.is_empty() {
        build_ability_items_from_legacy(
            tool_names,
            declared_tool_names,
            declared_skill_names,
            skill_name_keys,
        )
    } else {
        ability_items
    }
}

pub fn resolve_record_declared_names(
    stored_ability_items: &[AbilityDescriptor],
    tool_names: &[String],
    declared_tool_names: &[String],
    declared_skill_names: &[String],
    skill_name_keys: &HashSet<String>,
) -> (Vec<String>, Vec<String>) {
    if !declared_tool_names.is_empty() || !declared_skill_names.is_empty() {
        return (
            normalize_names(declared_tool_names.iter().map(String::as_str)),
            normalize_names(declared_skill_names.iter().map(String::as_str)),
        );
    }
    let ability_items = normalize_ability_items(stored_ability_items.to_vec());
    if ability_items.is_empty() {
        split_requested_tool_names(tool_names, skill_name_keys)
    } else {
        split_ability_item_names(&ability_items)
    }
}

pub fn resolve_agent_ability_selection(
    requested_tool_names: &[String],
    requested_ability_items: Option<Vec<AbilityDescriptor>>,
    explicit_declared_tool_names: Option<Vec<String>>,
    explicit_declared_skill_names: Option<Vec<String>>,
    skill_name_keys: &HashSet<String>,
) -> AgentAbilitySelection {
    let normalized_requested_tool_names =
        normalize_names(requested_tool_names.iter().map(String::as_str));
    let explicit_declared_tool_names =
        explicit_declared_tool_names.map(|items| normalize_names(items.iter().map(String::as_str)));
    let explicit_declared_skill_names = explicit_declared_skill_names
        .map(|items| normalize_names(items.iter().map(String::as_str)));

    if let Some(raw_ability_items) = requested_ability_items {
        let mut ability_items = normalize_ability_items(raw_ability_items);
        let mut tool_names =
            normalize_names(ability_items.iter().map(|item| item.runtime_name.as_str()));
        if tool_names.is_empty() {
            tool_names = normalized_requested_tool_names.clone();
        }
        let (default_declared_tool_names, default_declared_skill_names) =
            if ability_items.is_empty() {
                split_requested_tool_names(&tool_names, skill_name_keys)
            } else {
                split_ability_item_names(&ability_items)
            };
        let declared_tool_names =
            explicit_declared_tool_names.unwrap_or(default_declared_tool_names);
        let declared_skill_names =
            explicit_declared_skill_names.unwrap_or(default_declared_skill_names);
        if ability_items.is_empty() {
            ability_items = build_ability_items_from_legacy(
                &tool_names,
                &declared_tool_names,
                &declared_skill_names,
                skill_name_keys,
            );
        }
        return AgentAbilitySelection {
            tool_names,
            declared_tool_names,
            declared_skill_names,
            ability_items,
        };
    }

    let declared_tool_names = explicit_declared_tool_names.unwrap_or_default();
    let declared_skill_names = explicit_declared_skill_names.unwrap_or_default();
    AgentAbilitySelection {
        tool_names: normalized_requested_tool_names.clone(),
        ability_items: build_ability_items_from_legacy(
            &normalized_requested_tool_names,
            &declared_tool_names,
            &declared_skill_names,
            skill_name_keys,
        ),
        declared_tool_names,
        declared_skill_names,
    }
}

trait IfEmptyThen {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String;
}

impl IfEmptyThen for String {
    fn if_empty_then(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() {
            fallback()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_ability_items_from_legacy, normalize_ability_items, resolve_agent_ability_selection,
        resolve_record_declared_names, resolve_selected_declared_names, split_ability_item_names,
    };
    use crate::schemas::{AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey};
    use serde_json::json;
    use std::collections::HashSet;

    fn sample_skill_keys() -> HashSet<String> {
        HashSet::from(["planner".to_string()])
    }

    fn sample_item(name: &str, kind: AbilityKind) -> AbilityDescriptor {
        AbilityDescriptor {
            id: String::new(),
            name: name.to_string(),
            runtime_name: name.to_string(),
            display_name: format!("Display {name}"),
            description: format!("desc:{name}"),
            input_schema: json!({"type": "object"}),
            group: match kind {
                AbilityKind::Tool => AbilityGroupKey::Builtin,
                AbilityKind::Skill => AbilityGroupKey::Skills,
            },
            source: match kind {
                AbilityKind::Tool => AbilitySourceKey::Builtin,
                AbilityKind::Skill => AbilitySourceKey::Skill,
            },
            kind,
            owner_id: None,
            available: false,
            selected: false,
        }
    }

    #[test]
    fn normalize_ability_items_dedupes_and_marks_selected() {
        let items = normalize_ability_items(vec![
            sample_item("read_file", AbilityKind::Tool),
            sample_item("read_file", AbilityKind::Tool),
            sample_item("planner", AbilityKind::Skill),
        ]);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item.available));
        assert!(items.iter().all(|item| item.selected));
        assert_eq!(items[0].id, "builtin:read_file");
        assert_eq!(items[1].id, "skill:planner");
    }

    #[test]
    fn build_ability_items_from_legacy_splits_skill_names() {
        let items = build_ability_items_from_legacy(
            &["read_file".to_string(), "planner".to_string()],
            &[],
            &[],
            &sample_skill_keys(),
        );
        let (tool_names, skill_names) = split_ability_item_names(&items);
        assert_eq!(tool_names, vec!["read_file".to_string()]);
        assert_eq!(skill_names, vec!["planner".to_string()]);
    }

    #[test]
    fn resolve_agent_ability_selection_prefers_structured_items() {
        let selection = resolve_agent_ability_selection(
            &["ignored".to_string()],
            Some(vec![
                sample_item("read_file", AbilityKind::Tool),
                sample_item("planner", AbilityKind::Skill),
            ]),
            None,
            None,
            &sample_skill_keys(),
        );
        assert_eq!(
            selection.tool_names,
            vec!["read_file".to_string(), "planner".to_string()]
        );
        assert_eq!(selection.declared_tool_names, vec!["read_file".to_string()]);
        assert_eq!(selection.declared_skill_names, vec!["planner".to_string()]);
        assert_eq!(selection.ability_items.len(), 2);
    }

    #[test]
    fn resolve_record_declared_names_prefers_existing_legacy_arrays() {
        let (tool_names, skill_names) = resolve_record_declared_names(
            &[sample_item("read_file", AbilityKind::Tool)],
            &["read_file".to_string(), "planner".to_string()],
            &["list_files".to_string()],
            &["writer".to_string()],
            &sample_skill_keys(),
        );
        assert_eq!(tool_names, vec!["list_files".to_string()]);
        assert_eq!(skill_names, vec!["writer".to_string()]);
    }

    #[test]
    fn resolve_selected_declared_names_prunes_stale_items_and_fills_gaps() {
        let (tool_names, skill_names) = resolve_selected_declared_names(
            &[
                "read_file".to_string(),
                "planner".to_string(),
                "write_file".to_string(),
            ],
            &["stale_tool".to_string(), "write_file".to_string()],
            &["planner".to_string(), "stale_skill".to_string()],
            &sample_skill_keys(),
        );
        assert_eq!(
            tool_names,
            vec!["write_file".to_string(), "read_file".to_string()]
        );
        assert_eq!(skill_names, vec!["planner".to_string()]);
    }

    #[test]
    fn resolve_selected_declared_names_reclassifies_legacy_skill_entries() {
        let (tool_names, skill_names) = resolve_selected_declared_names(
            &["planner".to_string(), "read_file".to_string()],
            &["planner".to_string(), "read_file".to_string()],
            &[],
            &sample_skill_keys(),
        );
        assert_eq!(tool_names, vec!["read_file".to_string()]);
        assert_eq!(skill_names, vec!["planner".to_string()]);
    }
}
