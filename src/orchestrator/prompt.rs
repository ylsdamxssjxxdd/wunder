use super::*;

const MAX_FUNCTION_NAME_LEN: usize = 64;

pub(super) struct FunctionTooling {
    pub(super) tools: Vec<Value>,
    pub(super) name_map: HashMap<String, String>,
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

    pub(super) fn build_function_tooling(
        &self,
        config: &Config,
        skills: &SkillRegistry,
        allowed_tool_names: &HashSet<String>,
        user_tool_bindings: Option<&UserToolBindings>,
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
        let mut name_map = HashMap::new();
        for spec in specs {
            let preferred = select_preferred_tool_name(&spec.name, &canonical_aliases);
            let sanitized = sanitize_function_name(&preferred);
            let function_name =
                ensure_unique_function_name(&sanitized, &spec.name, &mut used_names);
            name_map.insert(function_name.clone(), spec.name.clone());
            tools.push(json!({
                "type": "function",
                "function": {
                    "name": function_name,
                    "description": spec.description,
                    "parameters": spec.input_schema,
                }
            }));
        }
        Some(FunctionTooling { tools, name_map })
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
        agent_prompt: Option<&str>,
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
                &workdir,
                config_overrides,
                allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                agent_prompt,
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
        agent_prompt: Option<&str>,
    ) -> String {
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
        if let Some(prompt) = stored {
            if stored_prompt_matches_workdir(
                &prompt,
                &expected_public_workdir,
                &expected_local_workdir,
            ) {
                return prompt;
            }
        }
        let prompt = self
            .build_system_prompt_with_allowed(
                config,
                config_overrides,
                allowed_tool_names,
                tool_call_mode,
                skills,
                user_tool_bindings,
                user_id,
                workspace_id,
                agent_prompt,
            )
            .await;
        let _ = self
            .workspace
            .save_session_system_prompt(user_id, session_id, &prompt, language);
        prompt
    }
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
