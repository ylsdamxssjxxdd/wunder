use crate::schemas::{
    AbilityDescriptor, AbilityGroupKey, AbilityKind, AbilitySourceKey, AvailableToolsResponse,
    SharedToolSpec, ToolSpec,
};
use std::collections::HashSet;

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

fn build_ability_id(
    source: AbilitySourceKey,
    owner_id: Option<&str>,
    runtime_name: &str,
) -> String {
    let normalized_runtime_name = runtime_name.trim();
    match owner_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(owner) => format!(
            "{}:{owner}:{normalized_runtime_name}",
            ability_source_key(source)
        ),
        None => format!("{}:{normalized_runtime_name}", ability_source_key(source)),
    }
}

fn build_tool_ability(
    spec: &ToolSpec,
    group: AbilityGroupKey,
    source: AbilitySourceKey,
    kind: AbilityKind,
    owner_id: Option<&str>,
    selected: bool,
) -> AbilityDescriptor {
    let runtime_name = spec.name.trim().to_string();
    AbilityDescriptor {
        id: build_ability_id(source, owner_id, &runtime_name),
        name: runtime_name.clone(),
        runtime_name: runtime_name.clone(),
        display_name: runtime_name,
        description: spec.description.clone(),
        input_schema: spec.input_schema.clone(),
        group,
        source,
        kind,
        owner_id: owner_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        available: true,
        selected,
    }
}

fn build_shared_ability(
    spec: &SharedToolSpec,
    selected_names: &HashSet<String>,
) -> AbilityDescriptor {
    let runtime_name = spec.name.trim().to_string();
    AbilityDescriptor {
        id: build_ability_id(
            AbilitySourceKey::Shared,
            Some(&spec.owner_id),
            &runtime_name,
        ),
        name: runtime_name.clone(),
        runtime_name: runtime_name.clone(),
        display_name: runtime_name,
        description: spec.description.clone(),
        input_schema: spec.input_schema.clone(),
        group: AbilityGroupKey::Shared,
        source: AbilitySourceKey::Shared,
        kind: AbilityKind::Tool,
        owner_id: Some(spec.owner_id.trim().to_string()).filter(|value| !value.is_empty()),
        available: true,
        selected: selected_names.contains(&spec.name),
    }
}

fn push_tool_specs(
    items: &mut Vec<AbilityDescriptor>,
    seen_ids: &mut HashSet<String>,
    specs: &[ToolSpec],
    group: AbilityGroupKey,
    source: AbilitySourceKey,
    kind: AbilityKind,
    owner_id: Option<&str>,
) {
    for spec in specs {
        let descriptor = build_tool_ability(spec, group, source, kind, owner_id, false);
        if seen_ids.insert(descriptor.id.clone()) {
            items.push(descriptor);
        }
    }
}

pub fn build_ability_items(response: &AvailableToolsResponse) -> Vec<AbilityDescriptor> {
    let mut items = Vec::new();
    let mut seen_ids = HashSet::new();
    let selected_shared_names: HashSet<String> = response
        .shared_tools_selected
        .as_ref()
        .into_iter()
        .flatten()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .collect();

    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.builtin_tools,
        AbilityGroupKey::Builtin,
        AbilitySourceKey::Builtin,
        AbilityKind::Tool,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.mcp_tools,
        AbilityGroupKey::Mcp,
        AbilitySourceKey::Mcp,
        AbilityKind::Tool,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.a2a_tools,
        AbilityGroupKey::A2a,
        AbilitySourceKey::A2a,
        AbilityKind::Tool,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.skills,
        AbilityGroupKey::Skills,
        AbilitySourceKey::Skill,
        AbilityKind::Skill,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.knowledge_tools,
        AbilityGroupKey::Knowledge,
        AbilitySourceKey::Knowledge,
        AbilityKind::Tool,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.user_mcp_tools,
        AbilityGroupKey::User,
        AbilitySourceKey::UserMcp,
        AbilityKind::Tool,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.user_skills,
        AbilityGroupKey::User,
        AbilitySourceKey::UserSkill,
        AbilityKind::Skill,
        None,
    );
    push_tool_specs(
        &mut items,
        &mut seen_ids,
        &response.user_knowledge_tools,
        AbilityGroupKey::User,
        AbilitySourceKey::UserKnowledge,
        AbilityKind::Tool,
        None,
    );

    for spec in &response.shared_tools {
        let descriptor = build_shared_ability(spec, &selected_shared_names);
        if seen_ids.insert(descriptor.id.clone()) {
            items.push(descriptor);
        }
    }

    items
}

pub fn populate_ability_items(response: &mut AvailableToolsResponse) {
    response.items = build_ability_items(response);
}

#[cfg(test)]
mod tests {
    use super::{build_ability_items, populate_ability_items};
    use crate::schemas::{
        AbilityGroupKey, AbilityKind, AbilitySourceKey, AvailableToolsResponse, SharedToolSpec,
        ToolSpec,
    };
    use serde_json::json;

    fn sample_spec(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.to_string(),
            description: format!("desc:{name}"),
            input_schema: json!({"type": "object"}),
        }
    }

    #[test]
    fn build_ability_items_maps_groups_and_sources() {
        let response = AvailableToolsResponse {
            builtin_tools: vec![sample_spec("read_file")],
            mcp_tools: vec![sample_spec("server@tool")],
            a2a_tools: vec![sample_spec("a2a@planner")],
            skills: vec![sample_spec("planner_skill")],
            knowledge_tools: vec![sample_spec("kb_search")],
            user_tools: vec![],
            admin_builtin_tools: vec![],
            admin_mcp_tools: vec![],
            admin_a2a_tools: vec![],
            admin_skills: vec![],
            admin_knowledge_tools: vec![],
            user_mcp_tools: vec![sample_spec("alice@server@tool")],
            user_skills: vec![sample_spec("writer_skill")],
            user_knowledge_tools: vec![sample_spec("alice@kb_search")],
            default_agent_tool_names: vec![],
            shared_tools: vec![SharedToolSpec {
                name: "bob@shared_tool".to_string(),
                description: "shared".to_string(),
                input_schema: json!({"type": "object"}),
                owner_id: "bob".to_string(),
            }],
            shared_tools_selected: Some(vec!["bob@shared_tool".to_string()]),
            items: vec![],
        };

        let items = build_ability_items(&response);
        assert_eq!(items.len(), 9);
        assert!(items.iter().any(|item| {
            item.runtime_name == "read_file"
                && item.group == AbilityGroupKey::Builtin
                && item.source == AbilitySourceKey::Builtin
                && item.kind == AbilityKind::Tool
        }));
        assert!(items.iter().any(|item| {
            item.runtime_name == "planner_skill"
                && item.group == AbilityGroupKey::Skills
                && item.kind == AbilityKind::Skill
        }));
        assert!(items.iter().any(|item| {
            item.runtime_name == "writer_skill"
                && item.group == AbilityGroupKey::User
                && item.source == AbilitySourceKey::UserSkill
                && item.kind == AbilityKind::Skill
        }));
        assert!(items.iter().any(|item| {
            item.runtime_name == "alice@kb_search"
                && item.group == AbilityGroupKey::User
                && item.source == AbilitySourceKey::UserKnowledge
                && item.kind == AbilityKind::Tool
        }));
        assert!(items.iter().any(|item| {
            item.runtime_name == "bob@shared_tool"
                && item.group == AbilityGroupKey::Shared
                && item.selected
        }));
    }

    #[test]
    fn populate_ability_items_updates_response() {
        let mut response = AvailableToolsResponse {
            builtin_tools: vec![sample_spec("read_file")],
            mcp_tools: vec![],
            a2a_tools: vec![],
            skills: vec![],
            knowledge_tools: vec![],
            user_tools: vec![],
            admin_builtin_tools: vec![],
            admin_mcp_tools: vec![],
            admin_a2a_tools: vec![],
            admin_skills: vec![],
            admin_knowledge_tools: vec![],
            user_mcp_tools: vec![],
            user_skills: vec![],
            user_knowledge_tools: vec![],
            default_agent_tool_names: vec![],
            shared_tools: vec![],
            shared_tools_selected: None,
            items: vec![],
        };

        populate_ability_items(&mut response);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].runtime_name, "read_file");
    }
}
