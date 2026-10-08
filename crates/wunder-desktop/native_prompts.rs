//! User prompt-pack management for the desktop settings shell. Mirrors the
//! server prompt-template API semantics: built-in packs are readonly, user
//! packs are editable, and changing the active pack bumps the prompt cache
//! revision so running turns pick it up.

use super::NativeDesktop;
use anyhow::{anyhow, bail, Result};
use wunder_server::prompting;
use wunder_server::user_prompt_templates;

const MAX_SEGMENT_CHARS: usize = 65_536;

#[derive(Clone, Debug)]
pub struct PromptPackInfo {
    pub id: String,
    pub readonly: bool,
    pub builtin: bool,
    pub locale: String,
    pub is_system_language_default: bool,
}

#[derive(Clone, Debug)]
pub struct PromptSegmentContent {
    pub key: String,
    pub content: String,
    pub readonly: bool,
    pub exists: bool,
    pub source_pack_id: String,
}

impl NativeDesktop {
    /// List the active pack id, all packs (built-ins first) and the editable
    /// segment keys, mirroring the server list endpoint.
    pub fn list_prompt_packs(
        &self,
    ) -> Result<(String, Vec<PromptPackInfo>, Vec<(String, String)>)> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let active = user_prompt_templates::load_user_active_pack_id(&config, self.user_id());
        let packs_root = user_prompt_templates::resolve_user_packs_root(&config, self.user_id());
        let system_language_default = user_prompt_templates::resolve_default_user_pack_id();

        let mut packs = Vec::new();
        for pack_id in [
            user_prompt_templates::DEFAULT_ZH_PACK_ID,
            user_prompt_templates::DEFAULT_EN_PACK_ID,
        ] {
            packs.push(PromptPackInfo {
                id: pack_id.to_string(),
                readonly: true,
                builtin: true,
                locale: user_prompt_templates::builtin_user_pack_locale(pack_id)
                    .unwrap_or("zh")
                    .to_string(),
                is_system_language_default: pack_id.eq_ignore_ascii_case(&system_language_default),
            });
        }
        if let Ok(entries) = std::fs::read_dir(&packs_root) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().trim().to_string();
                if name.is_empty()
                    || user_prompt_templates::is_builtin_user_pack_id(&name)
                    || user_prompt_templates::validate_pack_id(&name).is_err()
                {
                    continue;
                }
                packs.push(PromptPackInfo {
                    id: name,
                    readonly: false,
                    builtin: false,
                    locale: String::new(),
                    is_system_language_default: false,
                });
            }
        }
        packs.sort_by(|a, b| {
            b.builtin
                .cmp(&a.builtin)
                .then(
                    b.is_system_language_default
                        .cmp(&a.is_system_language_default),
                )
                .then(a.id.to_lowercase().cmp(&b.id.to_lowercase()))
        });
        let segments = user_prompt_templates::SYSTEM_SEGMENTS
            .iter()
            .map(|(key, file)| (key.to_string(), file.to_string()))
            .collect();
        Ok((active, packs, segments))
    }

    pub fn set_active_prompt_pack(&self, pack_id: &str) -> Result<()> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let pack_id = user_prompt_templates::normalize_pack_id(Some(pack_id));
        user_prompt_templates::validate_pack_id(&pack_id).map_err(|err| anyhow!("{err}"))?;
        if !user_prompt_templates::is_builtin_user_pack_id(&pack_id) {
            let root =
                user_prompt_templates::resolve_user_pack_root(&config, self.user_id(), &pack_id);
            if !root.is_dir() {
                bail!("提示词包不存在");
            }
        }
        user_prompt_templates::save_user_active_pack_id(&config, self.user_id(), &pack_id)
            .map_err(|err| anyhow!("{err}"))?;
        prompting::bump_system_prompt_templates_revision();
        Ok(())
    }

    /// Read one segment with the same fallback chain as the server: builtin
    /// packs read the active system pack (falling back to the default system
    /// pack); user packs read their own file, falling back to the system
    /// content as the editable starting point.
    pub fn read_prompt_segment(&self, pack_id: &str, key: &str) -> Result<PromptSegmentContent> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let pack_id = user_prompt_templates::normalize_pack_id(Some(pack_id));
        user_prompt_templates::validate_pack_id(&pack_id).map_err(|err| anyhow!("{err}"))?;
        let key = key.trim();
        if key.is_empty() {
            bail!("分段标识为空");
        }
        let locale = user_prompt_templates::normalize_locale(None);
        let system_pack_id = user_prompt_templates::resolve_system_active_pack_id(&config);
        let system_root = user_prompt_templates::resolve_system_pack_root(&config, &system_pack_id);
        let default_root = user_prompt_templates::resolve_system_pack_root(
            &config,
            user_prompt_templates::DEFAULT_PACK_ID,
        );

        if user_prompt_templates::is_builtin_user_pack_id(&pack_id) {
            let segment_path =
                user_prompt_templates::resolve_segment_path(&system_root, &locale, key)
                    .map_err(|err| anyhow!("{err}"))?;
            let mut content = std::fs::read_to_string(&segment_path).unwrap_or_default();
            let mut source = system_pack_id.clone();
            if content.is_empty()
                && !system_pack_id.eq_ignore_ascii_case(user_prompt_templates::DEFAULT_PACK_ID)
            {
                let default_segment_path =
                    user_prompt_templates::resolve_segment_path(&default_root, &locale, key)
                        .map_err(|err| anyhow!("{err}"))?;
                content = std::fs::read_to_string(&default_segment_path).unwrap_or_default();
                source = user_prompt_templates::DEFAULT_PACK_ID.to_string();
            }
            let exists = !content.is_empty();
            return Ok(PromptSegmentContent {
                key: key.to_string(),
                content,
                readonly: true,
                exists,
                source_pack_id: source,
            });
        }

        let pack_root =
            user_prompt_templates::resolve_user_pack_root(&config, self.user_id(), &pack_id);
        if !pack_root.is_dir() {
            bail!("提示词包不存在");
        }
        let path = user_prompt_templates::resolve_segment_path(&pack_root, &locale, key)
            .map_err(|err| anyhow!("{err}"))?;
        if path.is_file() {
            return Ok(PromptSegmentContent {
                key: key.to_string(),
                content: std::fs::read_to_string(&path).unwrap_or_default(),
                readonly: false,
                exists: true,
                source_pack_id: pack_id,
            });
        }
        let mut content = std::fs::read_to_string(
            user_prompt_templates::resolve_segment_path(&system_root, &locale, key)
                .map_err(|err| anyhow!("{err}"))?,
        )
        .unwrap_or_default();
        let mut source = system_pack_id.clone();
        if content.is_empty()
            && !system_pack_id.eq_ignore_ascii_case(user_prompt_templates::DEFAULT_PACK_ID)
        {
            content = std::fs::read_to_string(
                user_prompt_templates::resolve_segment_path(&default_root, &locale, key)
                    .map_err(|err| anyhow!("{err}"))?,
            )
            .unwrap_or_default();
            source = user_prompt_templates::DEFAULT_PACK_ID.to_string();
        }
        Ok(PromptSegmentContent {
            key: key.to_string(),
            content,
            readonly: false,
            exists: false,
            source_pack_id: source,
        })
    }

    /// Write one editable segment. Built-in packs are readonly by definition.
    pub fn write_prompt_segment(&self, pack_id: &str, key: &str, content: &str) -> Result<()> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let pack_id = user_prompt_templates::normalize_pack_id(Some(pack_id));
        user_prompt_templates::validate_pack_id(&pack_id).map_err(|err| anyhow!("{err}"))?;
        if user_prompt_templates::is_builtin_user_pack_id(&pack_id) {
            bail!("内置提示词包只读");
        }
        if content.chars().count() > MAX_SEGMENT_CHARS {
            bail!("分段内容过长（最多 {MAX_SEGMENT_CHARS} 字）");
        }
        let pack_root =
            user_prompt_templates::resolve_user_pack_root(&config, self.user_id(), &pack_id);
        if !pack_root.is_dir() {
            bail!("提示词包不存在");
        }
        let locale = user_prompt_templates::normalize_locale(None);
        let path = user_prompt_templates::resolve_segment_path(&pack_root, &locale, key)
            .map_err(|err| anyhow!("{err}"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        prompting::bump_system_prompt_templates_revision();
        Ok(())
    }

    pub fn create_prompt_pack(&self, pack_id: &str) -> Result<()> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let pack_id = user_prompt_templates::normalize_pack_id(Some(pack_id));
        user_prompt_templates::validate_pack_id(&pack_id).map_err(|err| anyhow!("{err}"))?;
        if user_prompt_templates::is_builtin_user_pack_id(&pack_id) {
            bail!("不能使用内置提示词包名称");
        }
        let pack_root =
            user_prompt_templates::resolve_user_pack_root(&config, self.user_id(), &pack_id);
        if pack_root.exists() {
            bail!("提示词包已存在");
        }
        std::fs::create_dir_all(&pack_root)?;
        Ok(())
    }

    pub fn delete_prompt_pack(&self, pack_id: &str) -> Result<()> {
        let config = self.runtime.block_on(self.state().config_store.get());
        let pack_id = user_prompt_templates::normalize_pack_id(Some(pack_id));
        user_prompt_templates::validate_pack_id(&pack_id).map_err(|err| anyhow!("{err}"))?;
        if user_prompt_templates::is_builtin_user_pack_id(&pack_id) {
            bail!("内置提示词包不能删除");
        }
        let pack_root =
            user_prompt_templates::resolve_user_pack_root(&config, self.user_id(), &pack_id);
        if !pack_root.is_dir() {
            bail!("提示词包不存在");
        }
        std::fs::remove_dir_all(&pack_root)?;
        let active = user_prompt_templates::load_user_active_pack_id(&config, self.user_id());
        if active.eq_ignore_ascii_case(pack_id.trim()) {
            user_prompt_templates::save_user_active_pack_id(
                &config,
                self.user_id(),
                user_prompt_templates::DEFAULT_PACK_ID,
            )
            .map_err(|err| anyhow!("{err}"))?;
            prompting::bump_system_prompt_templates_revision();
        }
        Ok(())
    }

    /// Build a realtime system prompt preview, mirroring the server
    /// `/chat/system-prompt` endpoint semantics for the desktop user.
    pub fn preview_system_prompt(&self) -> Result<String> {
        let state = self.state().clone();
        let user_id = self.user_id().to_string();
        self.runtime.block_on(async move {
            let user_context =
                wunder_server::user_access::build_user_tool_context(&state, &user_id).await;
            let user = state
                .user_store
                .get_user_by_id(&user_id)?
                .ok_or_else(|| anyhow!("desktop user unavailable"))?;
            let allowed =
                wunder_server::user_access::compute_allowed_tool_names(&user, &user_context);
            let mut tool_names: Vec<String> = allowed.into_iter().collect();
            tool_names.sort();
            if tool_names.is_empty() {
                tool_names.push("__no_tools__".to_string());
            }
            let agent_record = state
                .user_store
                .get_user_agent_by_id("__default__")
                .unwrap_or(None);
            let agent_prompt = agent_record
                .as_ref()
                .map(|record| record.system_prompt.trim().to_string())
                .filter(|value| !value.is_empty());
            let preview_skill = agent_record
                .as_ref()
                .map(|record| record.preview_skill)
                .unwrap_or(false);
            let workspace_id = state
                .workspace
                .ensure_user_root(&user_id)
                .unwrap_or_else(|_| state.workspace.root().to_path_buf());
            let prompt = state
                .kernel
                .orchestrator
                .build_system_prompt(
                    &user_context.config,
                    &tool_names,
                    &user_context.skills,
                    Some(&user_context.bindings),
                    &user_id,
                    None,
                    wunder_server::user_store::UserStore::is_admin(&user),
                    workspace_id.to_string_lossy().as_ref(),
                    None,
                    agent_prompt.as_deref(),
                    preview_skill,
                )
                .await;
            Ok(prompt)
        })
    }
}
