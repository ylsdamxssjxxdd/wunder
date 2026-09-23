use super::NativeDesktop;
use anyhow::Result;
use wunder_server::{agent_management, tools, user_access};

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
}

#[derive(Clone, Debug)]
pub struct ToolRecord {
    pub name: String,
    pub description: String,
    pub category: String,
}

impl NativeDesktop {
    pub fn list_agents(&self) -> Result<Vec<AgentRecord>> {
        self.runtime.block_on(async {
            let records = agent_management::list(self.state(), self.user_id()).await?;
            let config = self.state().config_store.get().await;
            Ok(records
                .into_iter()
                .map(|record| agent_record(record, &config.llm.default))
                .collect())
        })
    }

    pub fn create_agent(&self, name: &str) -> Result<AgentRecord> {
        self.runtime.block_on(async {
            let record = agent_management::create(self.state(), self.user_id(), name).await?;
            let config = self.state().config_store.get().await;
            Ok(agent_record(record, &config.llm.default))
        })
    }

    pub fn update_agent(
        &self,
        id: &str,
        name: &str,
        description: &str,
        system_prompt: &str,
        model: &str,
        icon_name: &str,
        icon_color: &str,
    ) -> Result<AgentRecord> {
        self.runtime.block_on(async {
            let record = agent_management::update(
                self.state(),
                self.user_id(),
                id,
                name,
                description,
                system_prompt,
                model,
                icon_name,
                icon_color,
            )
            .await?;
            let config = self.state().config_store.get().await;
            Ok(agent_record(record, &config.llm.default))
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
                    } else if spec.name.starts_with("a2a@") {
                        "A2A 工具"
                    } else if spec.name.contains('@') {
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
            records.sort_by(|a, b| (&a.category, &a.name).cmp(&(&b.category, &b.name)));
            records.truncate(200);
            Ok(records)
        })
    }
}

fn agent_record(
    record: wunder_server::storage::UserAgentRecord,
    default_model: &str,
) -> AgentRecord {
    AgentRecord {
        id: record.agent_id,
        name: record.name,
        description: record.description,
        system_prompt: record.system_prompt,
        model: record.model_name.unwrap_or_else(|| default_model.into()),
        status: record.status,
        icon_name: {
            let (name, _) = wunder_server::worker_card_settings::normalize_preset_icon_parts(
                record.icon.as_deref(),
            );
            name
        },
        icon_color: {
            let (_, color) = wunder_server::worker_card_settings::normalize_preset_icon_parts(
                record.icon.as_deref(),
            );
            color
        },
        icon_glyph: icon_glyph(record.icon.as_deref()),
    }
}

fn icon_glyph(raw: Option<&str>) -> String {
    let (name, _) = wunder_server::worker_card_settings::normalize_preset_icon_parts(raw);
    match name.as_str() {
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
