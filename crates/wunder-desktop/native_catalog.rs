use super::NativeDesktop;
use anyhow::Result;
use std::collections::HashSet;
use wunder_server::{agent_management, tools, user_access};

/// The desktop owns exactly one built-in agent (`__default__`). Its
/// configurable parts are edited through the settings pages; no create,
/// delete, reorder or card import/export surface exists any more.
pub const DEFAULT_AGENT_ID: &str = "__default__";

#[derive(Clone, Debug)]
pub struct AgentRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: String,
    pub status: String,
    pub icon_name: String,
    pub icon_color: String,
    pub icon_glyph: String,
    /// Full avatar configuration (static image or companion binding).
    pub icon_config: wunder_server::worker_card_settings::AgentIconConfig,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub preview_skill: bool,
    pub silent: bool,
    pub prefer_mother: bool,
    pub is_shared: bool,
}

#[derive(Clone, Debug)]
pub struct AgentSettingsEdit {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: String,
    pub icon_name: String,
    pub icon_color: String,
    /// Canonical icon payload; wins over the flattened name/color pair.
    pub icon: Option<String>,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub approval_mode: String,
    pub preview_skill: bool,
    pub silent: bool,
    pub prefer_mother: bool,
}

#[derive(Clone, Debug)]
pub struct ToolRecord {
    pub name: String,
    pub description: String,
    pub category: String,
}

impl NativeDesktop {
    /// The single built-in agent; guaranteed to exist.
    pub fn default_agent(&self) -> Result<AgentRecord> {
        self.runtime.block_on(async {
            let record =
                agent_management::owned(self.state(), self.user_id(), DEFAULT_AGENT_ID).await?;
            let config = self.state().config_store.get().await;
            Ok(agent_record(record, &config.llm))
        })
    }

    /// List of agents for generic projections; always exactly the built-in one.
    pub fn list_agents(&self) -> Result<Vec<AgentRecord>> {
        Ok(vec![self.default_agent()?])
    }

    pub fn update_agent_settings(&self, id: &str, input: AgentSettingsEdit) -> Result<AgentRecord> {
        if !id.trim().eq_ignore_ascii_case(DEFAULT_AGENT_ID) {
            anyhow::bail!("智能体不存在");
        }
        self.runtime.block_on(async {
            let input = agent_management::AgentSettingsUpdate {
                name: input.name,
                description: input.description,
                system_prompt: input.system_prompt,
                model_name: input.model_name,
                icon_name: input.icon_name,
                icon_color: input.icon_color,
                icon: input.icon,
                tool_names: input.tool_names,
                preset_questions: input.preset_questions,
                sandbox_container_id: 1,
                approval_mode: input.approval_mode,
                preview_skill: input.preview_skill,
                silent: input.silent,
                prefer_mother: input.prefer_mother,
            };
            let record = agent_management::update_settings(
                self.state(),
                self.user_id(),
                DEFAULT_AGENT_ID,
                input,
            )
            .await?;
            let config = self.state().config_store.get().await;
            Ok(agent_record(record, &config.llm))
        })
    }

    pub fn list_tools(&self) -> Result<Vec<ToolRecord>> {
        self.runtime.block_on(async {
            let user = self
                .state()
                .user_store
                .get_user_by_id(self.user_id())?
                .ok_or_else(|| anyhow::anyhow!("用户不存在"))?;
            let context = user_access::build_user_tool_context(self.state(), self.user_id()).await;
            let allowed = user_access::compute_allowed_tool_names(&user, &context);
            // Use the same resolved descriptions and permissions as model tool calls.
            let specs = tools::collect_prompt_tool_specs(
                &context.config,
                &context.skills,
                &allowed,
                Some(&context.bindings),
            );
            let skill_names = context
                .skills
                .list_specs()
                .into_iter()
                .map(|s| s.name)
                .collect::<std::collections::HashSet<_>>();
            // Model-facing MCP aliases are not guaranteed to contain '@'.
            // Classify against the runtime catalog, not display-name syntax.
            let mcp_names = tools::build_mcp_tool_alias_entries(&context.config)
                .into_iter()
                .flat_map(|entry| [entry.runtime_name, entry.display_name])
                .collect::<std::collections::HashSet<_>>();
            let mut records = specs
                .into_iter()
                .map(|spec| {
                    let category = if let Some(binding) = context.bindings.alias_map.get(&spec.name)
                    {
                        if binding.owner_id == self.user_id() {
                            "用户工具"
                        } else {
                            "共享工具"
                        }
                    } else if skill_names.contains(&spec.name) {
                        "技能"
                    } else if mcp_names.contains(&spec.name) {
                        "MCP 工具"
                    } else if context
                        .config
                        .knowledge
                        .bases
                        .iter()
                        .any(|base| base.name == spec.name)
                    {
                        "知识库"
                    } else {
                        "内置工具"
                    };
                    ToolRecord {
                        name: spec.name,
                        description: spec.description,
                        category: category.into(),
                    }
                })
                .collect::<Vec<_>>();
            // provider 只向模型提供 browser action 工具；本地清单与网页版同口径，
            // 浏览器能力只保留一条聚合条目。
            if records
                .iter()
                .any(|record| tools::is_browser_tool_name(&record.name))
            {
                records.retain(|record| !tools::is_browser_tool_name(&record.name));
                let summary = tools::browser_tool_summary_spec("zh-CN");
                records.push(ToolRecord {
                    name: summary.name,
                    description: summary.description,
                    category: "内置工具".into(),
                });
            }
            // 提示词工具规格不包含技能本体（技能经 skill_call 调度），这里与
            // 网页版工具目录对齐：把对当前用户开放的技能以"技能"分类补入，
            // 让设置页的工具区能看到并勾选技能。
            let mut seen_names: HashSet<String> =
                records.iter().map(|record| record.name.clone()).collect();
            for spec in context.skills.list_specs() {
                if !allowed.contains(&spec.name) || !seen_names.insert(spec.name.clone()) {
                    continue;
                }
                records.push(ToolRecord {
                    name: spec.name,
                    description: spec.description,
                    category: "技能".into(),
                });
            }
            records.retain(|record| {
                !wunder_server::default_tool_profile::is_desktop_hidden_tool_name(&record.name)
            });
            records.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
            records.truncate(200);
            Ok(records)
        })
    }
}

fn agent_record(
    record: wunder_server::storage::UserAgentRecord,
    llm: &wunder_server::config::LlmConfig,
) -> AgentRecord {
    let icon_config =
        wunder_server::worker_card_settings::parse_agent_icon_config(record.icon.as_deref());
    let icon_glyph = icon_glyph(&icon_config, &record.name);
    AgentRecord {
        id: record.agent_id,
        name: record.name,
        description: record.description,
        system_prompt: record.system_prompt,
        model: resolve_agent_model(record.model_name.as_deref(), llm),
        status: record.status,
        icon_name: icon_config.name.clone(),
        icon_color: icon_config.color.clone(),
        icon_glyph,
        icon_config,
        tool_names: record.tool_names,
        preset_questions: record.preset_questions,
        approval_mode: record.approval_mode,
        preview_skill: record.preview_skill,
        silent: record.silent,
        prefer_mother: record.prefer_mother,
        is_shared: record.is_shared,
    }
}

/// The model an agent projects into the composer.
///
/// A stored pointer is only meaningful while it names a configured chat model:
/// after a rename or a delete it can linger on a key the config no longer has,
/// and the picker would then label the thread with a model that does not exist
/// (while requests quietly fall back to the default). Resolve it here the same
/// way the runtime resolves an unknown key, so the projection can never show a
/// phantom entry.
fn resolve_agent_model(stored: Option<&str>, llm: &wunder_server::config::LlmConfig) -> String {
    if let Some(key) = stored.map(str::trim).filter(|key| !key.is_empty()) {
        if llm.models.contains_key(key) {
            return key.to_string();
        }
    }
    let default = llm.default.trim();
    if !default.is_empty() && llm.models.contains_key(default) {
        return default.to_string();
    }
    let mut candidates = llm
        .models
        .iter()
        .filter(|(_, model)| {
            model
                .model_type
                .as_deref()
                .unwrap_or("llm")
                .eq_ignore_ascii_case("llm")
                && model.enable != Some(false)
        })
        .map(|(key, _)| key)
        .collect::<Vec<_>>();
    candidates.sort();
    candidates
        .first()
        .map(|key| key.to_string())
        .unwrap_or_default()
}

fn icon_glyph(config: &wunder_server::worker_card_settings::AgentIconConfig, name: &str) -> String {
    if config.is_companion() {
        return "✦".to_string();
    }
    // The web catalog's `initial` option renders the agent name's first letter.
    if config.name == "initial" {
        return name
            .trim()
            .chars()
            .next()
            .map(|ch| ch.to_uppercase().collect::<String>())
            .unwrap_or_else(|| "?".to_string());
    }
    match config.name.as_str() {
        "robot" => "◉",
        "spark" => "✦",
        "leaf" => "❖",
        "heart" => "♥",
        "bolt" => "ϟ",
        "book" => "▤",
        _ => "✦",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::resolve_agent_model;
    use wunder_server::config::LlmConfig;

    fn llm(value: serde_json::Value) -> LlmConfig {
        serde_json::from_value(value).expect("llm config")
    }

    fn chat_models() -> LlmConfig {
        llm(serde_json::json!({
            "default": "beta-model",
            "models": {
                "beta-model": { "model": "beta", "model_type": "llm" },
                "ephemeral-model": { "model": "gone", "model_type": "llm" }
            }
        }))
    }

    /// 回归：模型改名或删除后，智能体里残留的旧键不能让输入区继续显示一个
    /// 配置里根本不存在的模型。
    #[test]
    fn a_pointer_to_a_missing_model_resolves_to_the_configured_default() {
        assert_eq!(
            resolve_agent_model(Some("新模型"), &chat_models()),
            "beta-model"
        );
        assert_eq!(
            resolve_agent_model(Some("  "), &chat_models()),
            "beta-model"
        );
    }

    #[test]
    fn a_configured_pointer_is_kept_verbatim() {
        assert_eq!(
            resolve_agent_model(Some("ephemeral-model"), &chat_models()),
            "ephemeral-model"
        );
    }

    /// 默认键本身失效时仍有可选项：取第一个可用对话模型，而不是留空。
    #[test]
    fn a_broken_default_falls_through_to_the_first_chat_model() {
        let config = llm(serde_json::json!({
            "default": "missing-model",
            "models": {
                "second": { "model": "b", "model_type": "llm" },
                "first": { "model": "a", "model_type": "llm" },
                "off": { "model": "c", "model_type": "llm", "enable": false },
                "embed": { "model": "d", "model_type": "embedding" }
            }
        }));
        assert_eq!(resolve_agent_model(Some("missing-model"), &config), "first");
        assert_eq!(resolve_agent_model(None, &config), "first");
    }
}
