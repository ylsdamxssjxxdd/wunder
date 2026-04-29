use super::*;
use crate::tools::build_responses_freeform_tool;

const MAX_FUNCTION_NAME_LEN: usize = 64;
const THREAD_AGENTS_MD_FILE_NAME: &str = "AGENTS.md";
const THREAD_AGENTS_MD_BLOCK_BEGIN: &str = "<!-- WUNDER_THREAD_AGENTS_MD_BEGIN -->";
const THREAD_AGENTS_MD_BLOCK_END: &str = "<!-- WUNDER_THREAD_AGENTS_MD_END -->";
const MAX_THREAD_AGENTS_MD_TOKENS: i64 = 3_000;

pub(crate) fn build_prompt_tool_name_map(
    config: &Config,
    allowed_tool_names: &HashSet<String>,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for runtime_name in allowed_tool_names {
        map.entry(runtime_name.clone())
            .or_insert_with(|| runtime_name.clone());
    }
    for entry in crate::tools::build_mcp_tool_alias_entries(config) {
        if !allowed_tool_names.contains(&entry.runtime_name) {
            continue;
        }
        map.insert(entry.display_name.clone(), entry.runtime_name.clone());
        map.entry(entry.runtime_name.clone())
            .or_insert(entry.runtime_name);
    }
    map
}

pub(crate) struct FunctionTooling {
    pub(crate) tools: Vec<Value>,
    pub(crate) name_map: HashMap<String, String>,
    pub(crate) display_map: HashMap<String, String>,
}

impl Orchestrator {
    pub(super) fn resolve_allowed_tool_names(
        &self,
        config: &Config,
        requested: &[String],
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
    ) -> HashSet<String> {
        let is_default = requested.is_empty();
        let allowed = if is_default {
            collect_available_tool_names(config, skills, user_tool_bindings)
        } else {
            self.prompt_composer.resolve_allowed_tool_names(
                config,
                skills,
                requested,
                user_tool_bindings,
            )
        };
        self.apply_a2ui_tool_policy(allowed, is_default)
    }

    pub(super) fn apply_a2ui_tool_policy(
        &self,
        mut allowed_tool_names: HashSet<String>,
        default_mode: bool,
    ) -> HashSet<String> {
        if default_mode {
            allowed_tool_names.remove("a2ui");
        }
        if allowed_tool_names.contains("a2ui") {
            allowed_tool_names.remove("最终回复");
            allowed_tool_names.remove("final_response");
            allowed_tool_names.remove(&resolve_tool_name("final_response"));
        }
        allowed_tool_names
    }

    pub(super) fn filter_tools_for_model_capability(
        &self,
        allowed_tool_names: HashSet<String>,
        support_vision: bool,
    ) -> HashSet<String> {
        filter_tool_names_by_model_capability(allowed_tool_names, support_vision)
    }

    pub(super) fn apply_preview_skill_tool_policy(
        &self,
        mut allowed_tool_names: HashSet<String>,
        preview_skill: bool,
    ) -> HashSet<String> {
        if preview_skill {
            for name in ["skill_call", "skill_get", "技能调用"] {
                allowed_tool_names.remove(name);
                allowed_tool_names.remove(&resolve_tool_name(name));
            }
        }
        allowed_tool_names
    }

    pub(crate) fn build_function_tooling(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        allowed_tool_names: &HashSet<String>,
        user_tool_bindings: Option<&UserToolBindings>,
        tool_call_mode: ToolCallMode,
        _user_id: &str,
        _current_agent_id: Option<&str>,
        workspace_id: &str,
    ) -> Option<FunctionTooling> {
        if allowed_tool_names.is_empty() {
            return None;
        }
        let specs = collect_prompt_tool_specs_with_language(
            config,
            skills,
            allowed_tool_names,
            user_tool_bindings,
            "en-US",
        );
        if specs.is_empty() {
            return None;
        }
        let mut canonical_aliases: HashMap<String, Vec<String>> = HashMap::new();
        for (alias, canonical) in builtin_aliases() {
            canonical_aliases.entry(canonical).or_default().push(alias);
        }
        for aliases in canonical_aliases.values_mut() {
            aliases.sort();
        }

        let mut used_names = HashSet::new();
        let mut tools = Vec::new();
        let mut name_map = build_prompt_tool_name_map(config, allowed_tool_names);
        let mut display_map = HashMap::new();
        // Replace the generic {user_id} placeholder in builtin tool descriptions
        // with the actual scoped workspace id so the model produces correct paths.
        let ws_placeholder = "/workspaces/{user_id}/";
        let ws_actual = format!("/workspaces/{workspace_id}/");
        for spec in specs {
            let preferred = select_preferred_tool_name(&spec.name, &canonical_aliases);
            let sanitized = sanitize_function_name(&preferred);
            let function_name =
                ensure_unique_function_name(&sanitized, &spec.name, &mut used_names);
            let runtime_name = name_map
                .get(&spec.name)
                .cloned()
                .unwrap_or_else(|| spec.name.clone());
            name_map.insert(function_name.clone(), runtime_name);
            let display_name = if function_name == spec.name {
                spec.title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(spec.name.as_str())
                    .to_string()
            } else {
                spec.name.clone()
            };
            display_map.insert(function_name.clone(), display_name);
            // Resolve workspace placeholders in description.
            let description = if spec.description.contains(ws_placeholder) {
                spec.description.replace(ws_placeholder, &ws_actual)
            } else {
                spec.description
            };
            if tool_call_mode == ToolCallMode::FreeformCall {
                if let Some(tool) =
                    build_responses_freeform_tool(&spec.name, &description, &function_name)
                {
                    tools.push(tool);
                    continue;
                }
            }
            let parameters =
                crate::core::json_schema::normalize_tool_input_schema(Some(&spec.input_schema));
            tools.push(json!({
                "type": "function",
                "function": {
                    "name": function_name,
                    "description": description,
                    "parameters": parameters,
                }
            }));
        }
        Some(FunctionTooling {
            tools,
            name_map,
            display_map,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn build_system_prompt_with_allowed(
        &self,
        config: &Config,
        config_overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        tool_call_mode: ToolCallMode,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        workspace_id: &str,
        agent_id: Option<&str>,
        agent_prompt: Option<&str>,
        preview_skill: bool,
    ) -> String {
        let workdir = self
            .workspace
            .ensure_user_root(workspace_id)
            .unwrap_or_else(|_| self.workspace.root().to_path_buf());
        let config_version = self.config_store.version();
        self.prompt_composer
            .build_system_prompt_cached(
                config,
                config_version,
                &self.workspace,
                workspace_id,
                user_id,
                agent_id,
                &workdir,
                config_overrides,
                allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                agent_prompt,
                preview_skill,
            )
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn resolve_session_prompt(
        &self,
        config: &Config,
        config_overrides: Option<&Value>,
        allowed_tool_names: &HashSet<String>,
        tool_call_mode: ToolCallMode,
        skills: &SkillRegistry,
        user_tool_bindings: Option<&UserToolBindings>,
        user_id: &str,
        workspace_id: &str,
        session_id: &str,
        language: Option<&str>,
        agent_id: Option<&str>,
        _is_admin: bool,
        agent_prompt: Option<&str>,
        preview_skill: bool,
        _query_text: Option<&str>,
        round_id: Option<&str>,
    ) -> String {
        let resolved_agent_id = agent_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.storage
                    .get_chat_session(user_id, session_id)
                    .ok()
                    .flatten()
                    .and_then(|record| record.agent_id)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            });
        let workdir = self
            .workspace
            .ensure_user_root(workspace_id)
            .unwrap_or_else(|_| self.workspace.root().to_path_buf());
        let expected_public_workdir = self.workspace.display_path(workspace_id, &workdir);
        let expected_local_workdir = workdir.to_string_lossy().replace('\\', "/");
        let stored = self
            .workspace
            .load_session_system_prompt_async(user_id, session_id, None)
            .await
            .unwrap_or(None);
        if let Some(prompt) = reuse_stored_session_prompt_if_valid(
            stored.as_deref(),
            &expected_public_workdir,
            &expected_local_workdir,
        ) {
            return prompt;
        }
        // Snapshot AGENTS.md only when the thread prompt is first finalized so
        // later file edits do not mutate the prompt already cached for this thread.
        let effective_agent_prompt = merge_agent_prompt_with_thread_agents_snapshot(
            agent_prompt,
            stored.as_deref(),
            &workdir,
            true,
        );
        let base_prompt = self
            .build_system_prompt_with_allowed(
                config,
                config_overrides,
                allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                user_id,
                workspace_id,
                resolved_agent_id.as_deref(),
                effective_agent_prompt.as_deref(),
                preview_skill,
            )
            .await;
        // Freeze the initial memory snapshot together with the session prompt so
        // later turns reuse the same system prompt verbatim within the thread.
        let session_prompt = self
            .append_memory_prompt(
                user_id,
                resolved_agent_id.as_deref(),
                base_prompt,
                Some(session_id),
                round_id,
                None,
            )
            .await;
        let _ = self.workspace.save_session_system_prompt(
            user_id,
            session_id,
            &session_prompt,
            language,
        );
        session_prompt
    }
}

pub(crate) fn merge_agent_prompt_with_thread_agents_snapshot(
    base_agent_prompt: Option<&str>,
    stored_session_prompt: Option<&str>,
    workspace_root: &Path,
    allow_initial_read: bool,
) -> Option<String> {
    let base_prompt = base_agent_prompt
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let agents_snapshot = stored_session_prompt
        .and_then(extract_thread_agents_md_snapshot)
        .or_else(|| {
            if allow_initial_read {
                load_thread_agents_md_snapshot(workspace_root)
            } else {
                None
            }
        });
    match (base_prompt, agents_snapshot) {
        (None, None) => None,
        (Some(prompt), None) => Some(prompt),
        (None, Some(snapshot)) => Some(snapshot),
        (Some(prompt), Some(snapshot)) => Some(format!("{prompt}\n\n{snapshot}")),
    }
}

fn reuse_stored_session_prompt_if_valid(
    stored_prompt: Option<&str>,
    public_workdir: &str,
    local_workdir: &str,
) -> Option<String> {
    stored_prompt
        .filter(|prompt| stored_prompt_matches_workdir(prompt, public_workdir, local_workdir))
        .map(str::to_string)
}

fn stored_prompt_matches_workdir(prompt: &str, public_workdir: &str, local_workdir: &str) -> bool {
    let cleaned_prompt = prompt.trim();
    if cleaned_prompt.is_empty() {
        return false;
    }
    let public = public_workdir.trim();
    if !public.is_empty() && cleaned_prompt.contains(public) {
        return true;
    }
    let local = local_workdir.trim();
    !local.is_empty() && cleaned_prompt.contains(local)
}

fn extract_thread_agents_md_snapshot(prompt: &str) -> Option<String> {
    let start = prompt.find(THREAD_AGENTS_MD_BLOCK_BEGIN)?;
    let end_marker_start = prompt[start..].find(THREAD_AGENTS_MD_BLOCK_END)? + start;
    let end = end_marker_start + THREAD_AGENTS_MD_BLOCK_END.len();
    let block = prompt[start..end].trim();
    if block.is_empty() {
        None
    } else {
        Some(block.to_string())
    }
}

fn load_thread_agents_md_snapshot(workspace_root: &Path) -> Option<String> {
    let path = workspace_root.join(THREAD_AGENTS_MD_FILE_NAME);
    if !path.is_file() {
        return None;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            warn!(
                "failed to read {} for thread prompt snapshot: {err}",
                path.display()
            );
            return None;
        }
    };
    format_thread_agents_md_snapshot(&content)
}

fn format_thread_agents_md_snapshot(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let snapshot = trim_text_to_tokens(
        trimmed,
        MAX_THREAD_AGENTS_MD_TOKENS,
        "\n\n...(AGENTS.md truncated)",
    );
    Some(format!(
        "{THREAD_AGENTS_MD_BLOCK_BEGIN}\nWorkspace instructions snapshot loaded from AGENTS.md in the sandbox container root when this thread received its first user message. Keep following this snapshot for the lifetime of the thread.\n\n{snapshot}\n{THREAD_AGENTS_MD_BLOCK_END}"
    ))
}

fn select_preferred_tool_name(
    name: &str,
    canonical_aliases: &HashMap<String, Vec<String>>,
) -> String {
    if is_valid_function_name(name) {
        return name.to_string();
    }
    let canonical = resolve_tool_name(name);
    if let Some(aliases) = canonical_aliases.get(&canonical) {
        if let Some(alias) = aliases.iter().find(|alias| is_valid_function_name(alias)) {
            return alias.to_string();
        }
    }
    name.to_string()
}

fn is_valid_function_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }
    trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn sanitize_function_name(name: &str) -> String {
    let mut output = String::new();
    let mut last_underscore = false;
    for ch in name.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else if ch == '_' || ch == '-' {
            ch
        } else {
            '_'
        };
        if mapped == '_' {
            if last_underscore {
                continue;
            }
            last_underscore = true;
        } else {
            last_underscore = false;
        }
        output.push(mapped);
    }
    let trimmed = output.trim_matches('_').to_string();
    if trimmed.is_empty() {
        String::new()
    } else if trimmed
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("tool_{trimmed}")
    } else {
        trimmed
    }
}

fn ensure_unique_function_name(base: &str, original: &str, used: &mut HashSet<String>) -> String {
    let mut candidate = truncate_function_name(base, MAX_FUNCTION_NAME_LEN);
    if candidate.is_empty() {
        candidate = format!("tool_{}", short_hash(original));
    }
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let suffix = short_hash(original);
    candidate = format_with_suffix(base, &suffix);
    if used.insert(candidate.clone()) {
        return candidate;
    }
    let mut index = 2;
    loop {
        let extra = format!("{suffix}_{index}");
        candidate = format_with_suffix(base, &extra);
        if used.insert(candidate.clone()) {
            return candidate;
        }
        index += 1;
    }
}

fn truncate_function_name(name: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    if name.len() <= max_len {
        return name.to_string();
    }
    name[..max_len].trim_matches('_').to_string()
}

fn format_with_suffix(base: &str, suffix: &str) -> String {
    let max_base = MAX_FUNCTION_NAME_LEN.saturating_sub(suffix.len().saturating_add(1));
    let mut trimmed = truncate_function_name(base, max_base);
    if trimmed.is_empty() {
        trimmed = "tool".to_string();
    }
    format!("{trimmed}_{suffix}")
}

fn short_hash(value: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    let hash = format!("{:x}", hasher.finish());
    hash.chars().take(6).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, McpServerConfig, McpToolSpec};

    fn create_temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wunder-thread-agents-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn thread_agents_snapshot_round_trips_from_prompt() {
        let snapshot = format_thread_agents_md_snapshot("# AGENTS\n- keep tests green").unwrap();
        let prompt = format!("base prompt\n\n{snapshot}\n\nmore");
        assert_eq!(extract_thread_agents_md_snapshot(&prompt), Some(snapshot));
    }

    #[test]
    fn merge_agent_prompt_prefers_stored_thread_snapshot() {
        let dir = create_temp_dir();
        std::fs::write(dir.join("AGENTS.md"), "new rules").unwrap();
        let stored_snapshot = format_thread_agents_md_snapshot("old rules").unwrap();
        let stored_prompt = format!("system\n\n{stored_snapshot}");

        let merged = merge_agent_prompt_with_thread_agents_snapshot(
            Some("agent prompt"),
            Some(&stored_prompt),
            &dir,
            true,
        )
        .unwrap();

        assert!(merged.contains("agent prompt"));
        assert!(merged.contains("old rules"));
        assert!(!merged.contains("new rules"));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn merge_agent_prompt_reads_workspace_agents_only_when_allowed() {
        let dir = create_temp_dir();
        std::fs::write(dir.join("AGENTS.md"), "workspace rules").unwrap();

        let initial =
            merge_agent_prompt_with_thread_agents_snapshot(None, None, &dir, true).unwrap();
        assert!(initial.contains("workspace rules"));

        let skipped = merge_agent_prompt_with_thread_agents_snapshot(None, None, &dir, false);
        assert!(skipped.is_none());

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reuse_stored_session_prompt_returns_frozen_snapshot_when_workdir_matches() {
        let prompt = "system prompt /workspace/demo";
        let reused = reuse_stored_session_prompt_if_valid(
            Some(prompt),
            "/workspace/demo",
            "C:/workspace/demo",
        );
        assert_eq!(reused, Some(prompt.to_string()));
    }

    #[test]
    fn reuse_stored_session_prompt_skips_mismatched_workdir() {
        let prompt = "system prompt /workspace/other";
        let reused = reuse_stored_session_prompt_if_valid(
            Some(prompt),
            "/workspace/demo",
            "C:/workspace/demo",
        );
        assert_eq!(reused, None);
    }

    #[test]
    fn build_prompt_tool_name_map_includes_mcp_display_alias() {
        let mut config = Config::default();
        config.mcp.servers = vec![McpServerConfig {
            name: "extra_mcp".to_string(),
            endpoint: "http://127.0.0.1:9010/mcp".to_string(),
            enabled: true,
            tool_specs: vec![McpToolSpec {
                name: "db_query_人员信息".to_string(),
                title: None,
                description: String::new(),
                input_schema: serde_yaml::Value::Mapping(Default::default()),
            }],
            ..Default::default()
        }];
        let allowed = HashSet::from(["extra_mcp@db_query_人员信息".to_string()]);
        let map = build_prompt_tool_name_map(&config, &allowed);
        assert_eq!(
            map.get("db_query_人员信息").map(String::as_str),
            Some("extra_mcp@db_query_人员信息")
        );
        assert_eq!(
            map.get("extra_mcp@db_query_人员信息").map(String::as_str),
            Some("extra_mcp@db_query_人员信息")
        );
    }

    #[test]
    fn build_prompt_tool_name_map_keeps_runtime_name_distinct_from_display_alias() {
        let mut config = Config::default();
        config.mcp.servers = vec![McpServerConfig {
            name: "extra_mcp".to_string(),
            endpoint: "http://127.0.0.1:9010/mcp".to_string(),
            enabled: true,
            tool_specs: vec![McpToolSpec {
                name: "db_query_company_all_personnel".to_string(),
                title: Some("数据库查询（人员信息）".to_string()),
                description: String::new(),
                input_schema: serde_yaml::Value::Mapping(Default::default()),
            }],
            ..Default::default()
        }];
        let allowed = HashSet::from(["extra_mcp@db_query_company_all_personnel".to_string()]);
        let map = build_prompt_tool_name_map(&config, &allowed);

        assert_eq!(
            map.get("数据库查询（人员信息）").map(String::as_str),
            Some("extra_mcp@db_query_company_all_personnel")
        );
        assert_eq!(
            map.get("extra_mcp@db_query_company_all_personnel")
                .map(String::as_str),
            Some("extra_mcp@db_query_company_all_personnel")
        );
    }
}
