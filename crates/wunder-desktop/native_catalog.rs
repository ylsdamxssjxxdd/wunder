use super::NativeDesktop;
use anyhow::Result;
use wunder_server::{agent_management, tools, user_access};

/// The shared worker-card document schema exported by the web client
/// (`wunder/worker-card@2`). Export mirrors `buildWorkerCardDocument` and
/// import mirrors `normalizeWorkerCardDocument` + `workerCardToAgentPayload`.
pub const WORKER_CARD_SCHEMA_VERSION: &str = "wunder/worker-card@2";
const WORKER_CARD_MAX_DOCUMENTS: usize = 50;

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
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub sandbox_container_id: i32,
    pub approval_mode: String,
    pub preview_skill: bool,
    pub silent: bool,
    pub prefer_mother: bool,
    pub is_shared: bool,
    pub hive_id: String,
}

#[derive(Clone, Debug)]
pub struct AgentSettingsEdit {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model_name: String,
    pub icon_name: String,
    pub icon_color: String,
    pub tool_names: Vec<String>,
    pub preset_questions: Vec<String>,
    pub sandbox_container_id: i32,
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

#[derive(Clone, Debug)]
pub struct AgentImportOutcome {
    pub agent: AgentRecord,
    pub created: bool,
    pub missing_tools: Vec<String>,
    pub missing_skills: Vec<String>,
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

    pub fn update_agent_settings(&self, id: &str, input: AgentSettingsEdit) -> Result<AgentRecord> {
        self.runtime.block_on(async {
            let input = agent_management::AgentSettingsUpdate {
                name: input.name,
                description: input.description,
                system_prompt: input.system_prompt,
                model_name: input.model_name,
                icon_name: input.icon_name,
                icon_color: input.icon_color,
                tool_names: input.tool_names,
                preset_questions: input.preset_questions,
                sandbox_container_id: input.sandbox_container_id,
                approval_mode: input.approval_mode,
                preview_skill: input.preview_skill,
                silent: input.silent,
                prefer_mother: input.prefer_mother,
            };
            let record =
                agent_management::update_settings(self.state(), self.user_id(), id, input).await?;
            let config = self.state().config_store.get().await;
            Ok(agent_record(record, &config.llm.default))
        })
    }

    pub fn delete_agent(&self, id: &str) -> Result<()> {
        self.runtime
            .block_on(agent_management::delete(self.state(), self.user_id(), id))
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

    /// Build the worker-card document for one agent, secret-free by schema.
    /// Reads the stored record directly (owned) so the export always reflects
    /// what was saved, independent of the bidirectional file projection.
    pub fn export_agent_document(&self, agent_id: &str) -> Result<serde_json::Value> {
        let record = self
            .runtime
            .block_on(agent_management::owned(
                self.state(),
                self.user_id(),
                agent_id.trim(),
            ))
            .map_err(|_| anyhow::anyhow!("智能体不存在或无权访问"))?;
        Ok(worker_card_document_from(&agent_record(record, "")))
    }

    /// Write one agent's worker card into `directory` and return the file path.
    pub fn export_agent_to_file(
        &self,
        agent_id: &str,
        directory: &std::path::Path,
    ) -> Result<std::path::PathBuf> {
        let document = self.export_agent_document(agent_id)?;
        std::fs::create_dir_all(directory)?;
        let name = document
            .pointer("/metadata/name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("agent");
        let id = document
            .pointer("/metadata/agent_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("agent");
        let safe_name: String = name
            .chars()
            .map(|c| {
                if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let path = directory.join(format!("{safe_name}-{id}.json"));
        std::fs::write(&path, serde_json::to_vec_pretty(&document)?)?;
        Ok(path)
    }

    /// Import one normalized worker-card document. With `overwrite` an owned
    /// agent sharing the card name is updated; otherwise a new agent is
    /// created, mirroring the web import flow.
    pub fn import_agent_document(
        &self,
        document: &serde_json::Value,
        overwrite: bool,
    ) -> Result<AgentImportOutcome> {
        let card = normalize_worker_card(document)?;
        self.apply_worker_card(&card, overwrite)
    }

    /// Import worker cards from a JSON file: a single card, a bundle, or an
    /// array of cards, capped at 50 documents per file.
    pub fn import_agent_from_file(
        &self,
        path: &std::path::Path,
        overwrite: bool,
    ) -> Result<Vec<AgentImportOutcome>> {
        let text = std::fs::read_to_string(path)?;
        let documents = parse_worker_card_text(&text)?;
        let mut outcomes = Vec::new();
        for document in documents {
            outcomes.push(self.apply_worker_card(&normalize_worker_card(&document)?, overwrite)?);
        }
        Ok(outcomes)
    }

    /// List importable card files (capped) from a directory with display labels.
    pub fn list_agent_card_files(
        &self,
        directory: &std::path::Path,
    ) -> Result<Vec<(String, String)>> {
        let mut files = Vec::new();
        let entries = match std::fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => return Ok(files),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(documents) = parse_worker_card_text(&text) else {
                continue;
            };
            let Some(document) = documents.first() else {
                continue;
            };
            let name = document
                .pointer("/metadata/name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("未命名");
            let exported_at = document
                .pointer("/metadata/exported_at")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            files.push((
                path.display().to_string(),
                format!("{name} · {exported_at}"),
            ));
            if files.len() >= WORKER_CARD_MAX_DOCUMENTS {
                break;
            }
        }
        Ok(files)
    }

    fn apply_worker_card(
        &self,
        card: &serde_json::Value,
        overwrite: bool,
    ) -> Result<AgentImportOutcome> {
        let name = card
            .pointer("/metadata/name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let (tool_names, skill_names) = split_card_abilities(card);
        let (icon_name, icon_color) = card
            .pointer("/metadata/icon")
            .and_then(serde_json::Value::as_str)
            .map(|icon| {
                wunder_server::worker_card_settings::normalize_preset_icon_parts(Some(icon))
            })
            .unwrap_or_default();
        let edit = AgentSettingsEdit {
            name: name.clone(),
            description: card
                .pointer("/metadata/description")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            system_prompt: card
                .get("extra_prompt")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            model_name: card
                .pointer("/runtime/model_name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            icon_name,
            icon_color,
            tool_names: wunder_server::worker_card_settings::normalize_tool_list(
                tool_names
                    .iter()
                    .cloned()
                    .chain(skill_names.iter().cloned())
                    .collect(),
            ),
            preset_questions: card
                .pointer("/interaction/preset_questions")
                .and_then(serde_json::Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            sandbox_container_id: card
                .pointer("/runtime/sandbox_container_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(1)
                .max(1) as i32,
            approval_mode: card
                .pointer("/runtime/approval_mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("suggest")
                .to_string(),
            preview_skill: card
                .pointer("/runtime/preview_skill")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            silent: card
                .pointer("/runtime/silent")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            prefer_mother: card
                .pointer("/runtime/prefer_mother")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        };
        let existing = if overwrite {
            self.list_agents()?
                .into_iter()
                .find(|agent| agent.name == name && !agent.is_shared)
        } else {
            None
        };
        let (agent, created) = match existing {
            Some(existing) => (
                self.update_agent_settings(&existing.id, edit.clone())?,
                false,
            ),
            None => {
                let created = self.create_agent(&name)?;
                (self.update_agent_settings(&created.id, edit)?, true)
            }
        };
        let available = self.list_tools()?;
        let missing = |want_skill: bool, names: &[String]| -> Vec<String> {
            names
                .iter()
                .filter(|name| {
                    !available
                        .iter()
                        .any(|tool| tool.name == **name && (!want_skill || tool.category == "技能"))
                })
                .cloned()
                .collect()
        };
        Ok(AgentImportOutcome {
            agent,
            created,
            missing_tools: missing(false, &tool_names),
            missing_skills: missing(true, &skill_names),
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
        tool_names: record.tool_names,
        preset_questions: record.preset_questions,
        sandbox_container_id: record.sandbox_container_id,
        approval_mode: record.approval_mode,
        preview_skill: record.preview_skill,
        silent: record.silent,
        prefer_mother: record.prefer_mother,
        is_shared: record.is_shared,
        hive_id: record.hive_id,
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

/// Split declared dependencies into tools and skills. Records store a single
/// tool_names list where `skill:` prefixes mark skills; the prefix form is
/// produced by this module's own exports and by the web settings form.
fn split_record_abilities(record: &AgentRecord) -> (Vec<String>, Vec<String>) {
    let mut tool_names = Vec::new();
    let mut skill_names = Vec::new();
    for name in &record.tool_names {
        if let Some(skill) = name.strip_prefix("skill:") {
            skill_names.push(skill.to_string());
        } else {
            tool_names.push(name.clone());
        }
    }
    (tool_names, skill_names)
}

fn worker_card_document_from(record: &AgentRecord) -> serde_json::Value {
    let (tool_names, skill_names) = split_record_abilities(record);
    let items = tool_names
        .iter()
        .map(|name| {
            serde_json::json!({
                "id": format!("tool:{name}"), "name": name, "runtime_name": name,
                "display_name": name, "description": "", "kind": "tool"
            })
        })
        .chain(skill_names.iter().map(|name| {
            serde_json::json!({
                "id": format!("skill:{name}"), "name": name, "runtime_name": name,
                "display_name": name, "description": "", "kind": "skill"
            })
        }))
        .collect::<Vec<_>>();
    serde_json::json!({
        "schema_version": WORKER_CARD_SCHEMA_VERSION,
        "kind": "WorkerCard",
        "metadata": {
            "agent_id": record.id,
            "name": record.name,
            "description": record.description,
            "icon": wunder_server::worker_card_settings::build_icon_payload(
                &record.icon_name,
                &record.icon_color,
            ),
            "exported_at": chrono::Utc::now().to_rfc3339(),
        },
        "extra_prompt": (!record.system_prompt.is_empty()).then(|| record.system_prompt.clone()),
        "abilities": { "items": items, "tool_names": tool_names, "skills": skill_names },
        "interaction": { "preset_questions": record.preset_questions },
        "runtime": {
            "model_name": record.model,
            "approval_mode": record.approval_mode,
            "sandbox_container_id": record.sandbox_container_id,
            "is_shared": record.is_shared,
            "preview_skill": record.preview_skill,
            "silent": record.silent,
            "prefer_mother": record.prefer_mother,
        },
        "hive": { "id": record.hive_id, "name": "", "description": "" },
        "extensions": {},
    })
}

fn split_card_abilities(card: &serde_json::Value) -> (Vec<String>, Vec<String>) {
    let abilities = card
        .get("abilities")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let read_list = |key: &str| -> Vec<String> {
        abilities
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    (read_list("tool_names"), read_list("skills"))
}

fn parse_worker_card_text(text: &str) -> Result<Vec<serde_json::Value>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        anyhow::bail!("工蜂卡文件为空");
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed)?;
    let documents = match parsed {
        serde_json::Value::Array(items) => items,
        value @ serde_json::Value::Object(_) => {
            match value.get("kind").and_then(serde_json::Value::as_str) {
                Some("WorkerCardBundle") => value
                    .get("items")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                _ => vec![value],
            }
        }
        _ => anyhow::bail!("无效的工蜂卡内容"),
    };
    if documents.is_empty() {
        anyhow::bail!("工蜂卡文件为空");
    }
    if documents.len() > WORKER_CARD_MAX_DOCUMENTS {
        anyhow::bail!("工蜂卡文件包含过多条目（最多 {WORKER_CARD_MAX_DOCUMENTS} 张）");
    }
    Ok(documents)
}

fn normalize_worker_card(document: &serde_json::Value) -> Result<serde_json::Value> {
    if !document.is_object() {
        anyhow::bail!("无效的工蜂卡内容");
    }
    let schema = document
        .get("schema_version")
        .or_else(|| document.get("schemaVersion"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if !schema.is_empty() && schema != WORKER_CARD_SCHEMA_VERSION {
        anyhow::bail!("不支持的工蜂卡版本：{schema}");
    }
    let name = document
        .pointer("/metadata/name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    if name.is_empty() {
        anyhow::bail!("工蜂卡缺少名称");
    }
    Ok(document.clone())
}
