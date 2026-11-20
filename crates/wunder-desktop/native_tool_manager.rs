//! Typed tool-management projection for native clients. All calls run on the
//! caller's worker thread and reuse the runtime's stores and archive handling.
use super::{NativeDesktop, ToolRecord};
use anyhow::{anyhow, bail, Result};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};
use wunder_server::{
    config::{KnowledgeBaseType, McpServerConfig},
    skills,
    user_tools::{UserKnowledgeBase, UserMcpServer},
};

const PAGE_SIZE: usize = 200;
const MAX_TEXT: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolResourceKind {
    Mcp,
    Skill,
    Knowledge,
}

#[derive(Clone, Debug)]
pub struct NativeToolResource {
    pub name: String,
    pub description: String,
    pub detail: String,
    pub readonly: bool,
}

/// Full MCP server projection for the add/edit dialog.
#[derive(Clone, Debug, Default)]
pub struct NativeMcpConfig {
    pub name: String,
    pub display_name: String,
    pub endpoint: String,
    pub transport: String,
    pub description: String,
    pub headers: String,
}

/// Full knowledge base projection for the add/edit dialog.
#[derive(Clone, Debug, Default)]
pub struct NativeKnowledgeConfig {
    pub name: String,
    pub description: String,
    pub base_type: String,
    pub embedding_model: String,
    pub chunk_size: String,
    pub chunk_overlap: String,
    pub ragflow_dataset_id: String,
}

/// Draft submitted by the MCP server dialog; an empty `previous` creates.
#[derive(Clone, Debug, Default)]
pub struct NativeMcpServerDraft {
    pub previous: String,
    pub name: String,
    pub display_name: String,
    pub endpoint: String,
    pub transport: String,
    pub description: String,
    pub headers: String,
}

/// Draft submitted by the knowledge base dialog; an empty `previous` creates.
#[derive(Clone, Debug, Default)]
pub struct NativeKnowledgeDraft {
    pub previous: String,
    pub name: String,
    pub description: String,
    pub base_type: String,
    pub embedding_model: String,
    pub chunk_size: String,
    pub chunk_overlap: String,
    pub ragflow_dataset_id: String,
}

#[derive(Default)]
pub struct NativeToolManager {
    pub servers: Vec<NativeToolResource>,
    pub skills: Vec<NativeToolResource>,
    pub bases: Vec<NativeToolResource>,
    pub server_configs: Vec<NativeMcpConfig>,
    pub base_configs: Vec<NativeKnowledgeConfig>,
}

#[derive(Clone, Debug)]
pub struct NativeToolFile {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub size: u64,
}

pub struct NativeToolFiles {
    pub entries: Vec<NativeToolFile>,
    pub has_more: bool,
}

impl NativeDesktop {
    pub fn tool_manager(&self) -> Result<NativeToolManager> {
        let payload = self
            .state()
            .user_tool_store
            .sync_skills_from_disk(self.user_id())?;
        let mut result = NativeToolManager::default();
        result.servers = payload
            .mcp_servers
            .iter()
            .take(PAGE_SIZE)
            .map(|s| NativeToolResource {
                name: s.name.clone(),
                description: s.description.clone(),
                detail: s.endpoint.clone(),
                readonly: false,
            })
            .collect();
        result.server_configs = payload
            .mcp_servers
            .iter()
            .take(PAGE_SIZE)
            .map(|s| NativeMcpConfig {
                name: s.name.clone(),
                display_name: s.display_name.clone(),
                endpoint: s.endpoint.clone(),
                transport: s.transport.clone(),
                description: s.description.clone(),
                headers: headers_to_text(&s.headers),
            })
            .collect();
        result.skills = self
            .tool_manager_skills()
            .into_iter()
            .take(PAGE_SIZE)
            .map(|(s, readonly)| NativeToolResource {
                name: s.name,
                description: s.description,
                detail: String::new(),
                readonly,
            })
            .collect();
        result.bases = payload
            .knowledge_bases
            .iter()
            .take(PAGE_SIZE)
            .map(|b| NativeToolResource {
                name: b.name.clone(),
                description: b.description.clone(),
                detail: b.description.clone(),
                readonly: false,
            })
            .collect();
        result.base_configs = payload
            .knowledge_bases
            .iter()
            .take(PAGE_SIZE)
            .map(|b| NativeKnowledgeConfig {
                name: b.name.clone(),
                description: b.description.clone(),
                base_type: base_type_label(b.base_type.as_deref()).to_string(),
                embedding_model: b.embedding_model.clone().unwrap_or_default(),
                chunk_size: b.chunk_size.map(|v| v.to_string()).unwrap_or_default(),
                chunk_overlap: b.chunk_overlap.map(|v| v.to_string()).unwrap_or_default(),
                ragflow_dataset_id: b.ragflow_dataset_id.clone().unwrap_or_default(),
            })
            .collect();
        Ok(result)
    }

    fn tool_manager_skills(&self) -> Vec<(skills::SkillSpec, bool)> {
        let mut config = self.runtime.block_on(self.state().config_store.get());
        let global = self
            .runtime
            .block_on(async { self.state().skills.read().await.list_specs() });
        config.skills.paths = vec![self
            .state()
            .user_tool_store
            .get_skill_root(self.user_id())
            .to_string_lossy()
            .into_owned()];
        let custom = skills::load_skills(&config, false, false, false).list_specs();
        let mut names = HashSet::new();
        let mut result = custom
            .into_iter()
            .map(|s| (s, false))
            .chain(global.into_iter().map(|s| (s, true)))
            .filter(|(s, _)| names.insert(s.name.clone()))
            .collect::<Vec<_>>();
        result.sort_by(|a, b| a.0.name.cmp(&b.0.name));
        result
    }

    fn knowledge_base_type(&self, name: &str) -> KnowledgeBaseType {
        let payload = self.state().user_tool_store.load_user_tools(self.user_id());
        payload
            .knowledge_bases
            .iter()
            .find(|b| b.name == name)
            .map(|b| wunder_server::config::normalize_knowledge_base_type(b.base_type.as_deref()))
            .unwrap_or(KnowledgeBaseType::Literal)
    }

    fn tool_resource_root(
        &self,
        kind: ToolResourceKind,
        name: &str,
        write: bool,
    ) -> Result<PathBuf> {
        match kind {
            ToolResourceKind::Skill => {
                let (spec, readonly) = self
                    .tool_manager_skills()
                    .into_iter()
                    .find(|(s, _)| s.name == name)
                    .ok_or_else(|| anyhow!("技能不存在"))?;
                if write && readonly {
                    bail!("全局技能只读");
                }
                if !readonly {
                    let owner_root = self
                        .state()
                        .user_tool_store
                        .get_skill_root(self.user_id())
                        .canonicalize()?;
                    if !spec.root.canonicalize()?.starts_with(owner_root) {
                        bail!("技能目录越界");
                    }
                }
                Ok(spec.root)
            }
            ToolResourceKind::Knowledge => {
                let payload = self.state().user_tool_store.load_user_tools(self.user_id());
                let base = payload
                    .knowledge_bases
                    .iter()
                    .find(|b| b.name == name)
                    .ok_or_else(|| anyhow!("知识库不存在"))?;
                if base
                    .base_type
                    .as_deref()
                    .is_some_and(|t| !t.is_empty() && t != "literal")
                {
                    bail!("此知识库类型不支持文本文件编辑");
                }
                self.state()
                    .user_tool_store
                    .resolve_knowledge_base_root(self.user_id(), name, true)
            }
            ToolResourceKind::Mcp => bail!("MCP 服务没有文件目录"),
        }
    }

    pub fn tool_resource_files(
        &self,
        kind: ToolResourceKind,
        name: &str,
        directory: &str,
        offset: usize,
    ) -> Result<NativeToolFiles> {
        // Indexed knowledge bases keep their chunks outside the desktop file
        // browser, so they project an empty list instead of an error.
        if kind == ToolResourceKind::Knowledge
            && self.knowledge_base_type(name) != KnowledgeBaseType::Literal
        {
            return Ok(NativeToolFiles {
                entries: Vec::new(),
                has_more: false,
            });
        }
        let root = self.tool_resource_root(kind, name, false)?;
        let path = confined_path(&root, directory, false)?;
        // Read only one bounded page; no recursive directory scans.
        let mut entries = Vec::with_capacity(PAGE_SIZE + 1);
        for item in fs::read_dir(path)?.skip(offset).take(PAGE_SIZE + 1) {
            let item = item?;
            let metadata = item.metadata()?;
            let name = item.file_name().to_string_lossy().into_owned();
            let relative = Path::new(directory)
                .join(&name)
                .to_string_lossy()
                .replace('\\', "/");
            entries.push(NativeToolFile {
                name,
                path: relative,
                directory: metadata.is_dir(),
                size: metadata.len(),
            });
        }
        let has_more = entries.len() > PAGE_SIZE;
        entries.truncate(PAGE_SIZE);
        entries.sort_by(|a, b| (!a.directory, &a.name).cmp(&(!b.directory, &b.name)));
        Ok(NativeToolFiles { entries, has_more })
    }

    pub fn read_tool_resource(
        &self,
        kind: ToolResourceKind,
        name: &str,
        path: &str,
    ) -> Result<String> {
        let root = self.tool_resource_root(kind, name, false)?;
        read_text(&confined_path(&root, path, false)?)
    }

    pub fn save_tool_resource(
        &self,
        kind: ToolResourceKind,
        name: &str,
        path: &str,
        content: &str,
        create: bool,
    ) -> Result<()> {
        if content.len() as u64 > MAX_TEXT {
            bail!("文件超过编辑大小限制");
        }
        let root = self.tool_resource_root(kind, name, true)?;
        let path = confined_path(&root, path, create)?;
        if create && path.exists() {
            bail!("文件已存在");
        }
        if !create && !path.is_file() {
            bail!("文件不存在");
        }
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true);
        if create {
            options.create_new(true);
        } else {
            options.truncate(true);
        }
        options.open(path)?.write_all(content.as_bytes())?;
        self.state()
            .user_tool_manager
            .clear_skill_cache(Some(self.user_id()));
        Ok(())
    }

    pub fn tool_server_tools(&self, name: &str) -> Result<Vec<ToolRecord>> {
        let payload = self.state().user_tool_store.load_user_tools(self.user_id());
        let server = payload
            .mcp_servers
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow!("MCP 服务不存在"))?;
        Ok(server
            .tool_specs
            .iter()
            .take(PAGE_SIZE)
            .map(|s| ToolRecord {
                name: s
                    .get("name")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .into(),
                description: s
                    .get("description")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .into(),
                category: "MCP 工具".into(),
            })
            .collect())
    }

    pub fn connect_tool_server(&self, name: &str) -> Result<()> {
        let mut payload = self.state().user_tool_store.load_user_tools(self.user_id());
        let server = payload
            .mcp_servers
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| anyhow!("MCP 服务不存在"))?;
        let config = self.runtime.block_on(self.state().config_store.get());
        let mcp = McpServerConfig {
            name: server.name.clone(),
            endpoint: server.endpoint.clone(),
            enabled: true,
            transport: Some(server.transport.clone()),
            headers: server.headers.clone(),
            auth: server
                .auth
                .as_ref()
                .and_then(|v| serde_yaml::to_value(v).ok()),
            ..Default::default()
        };
        let tools = self
            .runtime
            .block_on(wunder_server::mcp::fetch_tools(&config, &mcp))?;
        server.tool_specs = tools
            .into_iter()
            .map(serde_json::to_value)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        server.enabled = true;
        self.state()
            .user_tool_store
            .update_mcp_servers(self.user_id(), payload.mcp_servers)?;
        Ok(())
    }

    /// 创建或更新一个 MCP 服务器（对话框草稿），与云端版保存路径一致：
    /// 已有服务器保留工具缓存，连接动作再刷新。
    pub fn save_mcp_server(&self, draft: &NativeMcpServerDraft) -> Result<()> {
        validate_name(&draft.name)?;
        let headers = parse_headers_text(&draft.headers)?;
        if !draft.endpoint.starts_with("http://") && !draft.endpoint.starts_with("https://") {
            bail!("MCP 地址需要使用 HTTP 或 HTTPS");
        }
        let transport = if draft.transport.trim() == "auto" {
            String::new()
        } else {
            draft.transport.trim().to_string()
        };
        let mut payload = self.state().user_tool_store.load_user_tools(self.user_id());
        if payload
            .mcp_servers
            .iter()
            .any(|s| s.name == draft.name && s.name != draft.previous)
        {
            bail!("名称已存在");
        }
        if draft.previous.is_empty() {
            payload.mcp_servers.push(UserMcpServer {
                name: draft.name.clone(),
                display_name: draft.display_name.trim().to_string(),
                endpoint: draft.endpoint.trim().to_string(),
                transport,
                description: draft.description.trim().to_string(),
                headers,
                enabled: true,
                ..Default::default()
            });
        } else {
            let server = payload
                .mcp_servers
                .iter_mut()
                .find(|s| s.name == draft.previous)
                .ok_or_else(|| anyhow!("MCP 服务不存在"))?;
            server.name = draft.name.clone();
            server.display_name = draft.display_name.trim().to_string();
            server.endpoint = draft.endpoint.trim().to_string();
            server.transport = transport;
            server.description = draft.description.trim().to_string();
            server.headers = headers;
        }
        self.state()
            .user_tool_store
            .update_mcp_servers(self.user_id(), payload.mcp_servers)?;
        Ok(())
    }

    /// 粘贴导入 MCP 服务器：接受 `{"mcpServers": {...}}`、单个服务器对象或数组，
    /// 与云端版导入同一套字段别名。同名服务器就地更新，工具缓存保留。
    /// 返回导入（新建或更新）的服务器个数；只接受 HTTP/HTTPS 地址。
    pub fn import_mcp_servers(&self, raw: &str) -> Result<usize> {
        let raw = raw.trim();
        if raw.is_empty() {
            bail!("请粘贴 MCP 服务器配置");
        }
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|_| anyhow!("粘贴内容不是合法 JSON"))?;
        let entries = paste_entries(&value)?;
        if entries.is_empty() {
            bail!("没有找到可导入的 MCP 服务器");
        }
        let mut drafts = Vec::new();
        let mut skipped = Vec::new();
        for (key, config) in entries {
            match paste_server(&key, &config) {
                Ok(draft) => drafts.push(draft),
                Err(_) => skipped.push(key),
            }
        }
        if drafts.is_empty() {
            bail!("粘贴内容里没有可用于 HTTP/HTTPS 的 MCP 服务器");
        }
        if !skipped.is_empty() {
            // 条目级的跳过不阻断整体导入：缺地址或使用 stdio 的条目在这里留痕，
            // 界面上只报成功导入的个数。
            eprintln!("import_mcp_servers skipped: {}", skipped.join(", "));
        }
        let mut payload = self.state().user_tool_store.load_user_tools(self.user_id());
        for draft in &drafts {
            match payload
                .mcp_servers
                .iter_mut()
                .find(|server| server.name == draft.name)
            {
                Some(server) => {
                    server.display_name = draft.display_name.clone();
                    server.endpoint = draft.endpoint.clone();
                    server.transport = draft.transport.clone();
                    server.description = draft.description.clone();
                    server.headers = draft.headers.clone();
                }
                None => payload.mcp_servers.push(UserMcpServer {
                    name: draft.name.clone(),
                    display_name: draft.display_name.clone(),
                    endpoint: draft.endpoint.clone(),
                    transport: draft.transport.clone(),
                    description: draft.description.clone(),
                    headers: draft.headers.clone(),
                    enabled: true,
                    ..Default::default()
                }),
            }
        }
        self.state()
            .user_tool_store
            .update_mcp_servers(self.user_id(), payload.mcp_servers)?;
        Ok(drafts.len())
    }

    /// Create or update one knowledge base from the dialog draft. The desktop
    /// keeps the web semantics: names stay fixed after creation and advanced
    /// fields only apply to the type that consumes them.
    pub fn save_knowledge_base(&self, draft: &NativeKnowledgeDraft) -> Result<()> {
        validate_name(&draft.name)?;
        let base_type = match draft.base_type.trim() {
            "vector" => "vector",
            "ragflow" => "ragflow",
            _ => "literal",
        };
        let is_vector = base_type == "vector";
        let embedding = draft.embedding_model.trim().to_string();
        let chunk_size = if is_vector {
            parse_optional_usize(&draft.chunk_size, "切块大小")?
        } else {
            None
        };
        let chunk_overlap = if is_vector {
            parse_optional_usize(&draft.chunk_overlap, "切块重叠")?
        } else {
            None
        };
        let dataset_id = if base_type == "ragflow" {
            draft.ragflow_dataset_id.trim().to_string()
        } else {
            String::new()
        };
        let mut payload = self.state().user_tool_store.load_user_tools(self.user_id());
        if draft.previous.is_empty() {
            if payload.knowledge_bases.iter().any(|b| b.name == draft.name) {
                bail!("名称已存在");
            }
            let kind = match base_type {
                "vector" => KnowledgeBaseType::Vector,
                "ragflow" => KnowledgeBaseType::Ragflow,
                _ => KnowledgeBaseType::Literal,
            };
            self.state()
                .user_tool_store
                .resolve_knowledge_base_root_with_type(self.user_id(), &draft.name, kind, true)?;
            payload.knowledge_bases.push(UserKnowledgeBase {
                name: draft.name.clone(),
                description: draft.description.trim().to_string(),
                enabled: true,
                base_type: (base_type != "literal").then(|| base_type.to_string()),
                embedding_model: (!embedding.is_empty()).then_some(embedding),
                ragflow_dataset_id: (!dataset_id.is_empty()).then_some(dataset_id),
                chunk_size,
                chunk_overlap,
                ..Default::default()
            });
        } else {
            if draft.previous != draft.name {
                bail!("请保留知识库名称，仅编辑描述");
            }
            let base = payload
                .knowledge_bases
                .iter_mut()
                .find(|b| b.name == draft.previous)
                .ok_or_else(|| anyhow!("知识库不存在"))?;
            base.description = draft.description.trim().to_string();
            if is_vector {
                base.embedding_model = (!embedding.is_empty()).then_some(embedding);
                base.chunk_size = chunk_size;
                base.chunk_overlap = chunk_overlap;
            }
            if base_type == "ragflow" {
                base.ragflow_dataset_id = (!dataset_id.is_empty()).then_some(dataset_id);
            }
        }
        self.state()
            .user_tool_store
            .update_knowledge_bases(self.user_id(), payload.knowledge_bases)?;
        Ok(())
    }

    pub fn delete_tool_resource(&self, kind: ToolResourceKind, name: &str) -> Result<()> {
        let mut payload = self.state().user_tool_store.load_user_tools(self.user_id());
        match kind {
            ToolResourceKind::Mcp => {
                payload.mcp_servers.retain(|s| s.name != name);
                self.state()
                    .user_tool_store
                    .update_mcp_servers(self.user_id(), payload.mcp_servers)?;
            }
            ToolResourceKind::Knowledge => {
                payload.knowledge_bases.retain(|b| b.name != name);
                self.state()
                    .user_tool_store
                    .update_knowledge_bases(self.user_id(), payload.knowledge_bases)?;
            }
            ToolResourceKind::Skill => {
                let root = self.tool_resource_root(kind, name, true)?.canonicalize()?;
                let owner = self
                    .state()
                    .user_tool_store
                    .get_skill_root(self.user_id())
                    .canonicalize()?;
                if root == owner || !root.starts_with(owner) {
                    bail!("技能目录越界");
                }
                fs::remove_dir_all(root)?;
                self.state()
                    .user_tool_store
                    .sync_skills_from_disk(self.user_id())?;
                self.state()
                    .user_tool_manager
                    .clear_skill_cache(Some(self.user_id()));
            }
        }
        Ok(())
    }

    pub fn import_tool_resource(&self, kind: ToolResourceKind, source: &str) -> Result<()> {
        let source = Path::new(source);
        match kind {
            ToolResourceKind::Skill => {
                if fs::metadata(source)?.len() > 200 * 1024 * 1024 {
                    bail!("技能包过大");
                }
                let root = self.state().user_tool_store.get_skill_root(self.user_id());
                fs::create_dir_all(&root)?;
                let reserved = fs::read_dir(&root)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                wunder_server::skill_archive::import_skill_archive(
                    source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default(),
                    &fs::read(source)?,
                    &root,
                    &reserved,
                )?;
                self.state()
                    .user_tool_store
                    .sync_skills_from_disk(self.user_id())?;
                self.state()
                    .user_tool_manager
                    .clear_skill_cache(Some(self.user_id()));
            }
            _ => bail!("不支持的导入类型"),
        }
        Ok(())
    }

    pub fn export_tool_skill(&self, name: &str, target: &str) -> Result<()> {
        let root = self.tool_resource_root(ToolResourceKind::Skill, name, false)?;
        let target = Path::new(target);
        if target.exists() {
            bail!("导出文件已存在");
        }
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("导出目录无效"))?
            .canonicalize()?;
        if parent.starts_with(root.canonicalize()?) {
            bail!("导出文件不能位于技能内部");
        }
        wunder_server::skill_archive::create_skill_archive(&root, name, target)
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.len() > 160
        || name.contains(['/', '\\', ':'])
        || name == "."
        || name.contains("..")
    {
        bail!("名称无效");
    }
    Ok(())
}

/// Parse the dialog headers textarea into a header map; empty means none.
fn parse_headers_text(text: &str) -> Result<HashMap<String, String>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(HashMap::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| anyhow!("请求头必须是 JSON 对象"))?;
    if !value.is_object() {
        bail!("请求头必须是 JSON 对象");
    }
    serde_json::from_value(value).map_err(|_| anyhow!("请求头必须是 JSON 对象"))
}

/// Render stored headers back to the dialog's JSON textarea form.
fn headers_to_text(headers: &HashMap<String, String>) -> String {
    if headers.is_empty() {
        return String::new();
    }
    serde_json::to_string_pretty(headers).unwrap_or_default()
}

fn parse_optional_usize(text: &str, label: &str) -> Result<Option<usize>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse::<usize>()
        .map(Some)
        .map_err(|_| anyhow!("{label}无效"))
}

fn base_type_label(value: Option<&str>) -> &'static str {
    match wunder_server::config::normalize_knowledge_base_type(value) {
        KnowledgeBaseType::Vector => "vector",
        KnowledgeBaseType::Ragflow => "ragflow",
        KnowledgeBaseType::Literal => "literal",
    }
}

fn confined_path(root: &Path, relative: &str, create: bool) -> Result<PathBuf> {
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("文件路径越界");
    }
    let root = root.canonicalize()?;
    let target = root.join(relative);
    let resolved = if create && !target.exists() {
        target
            .parent()
            .ok_or_else(|| anyhow!("路径无效"))?
            .canonicalize()?
            .join(target.file_name().ok_or_else(|| anyhow!("路径无效"))?)
    } else {
        target.canonicalize()?
    };
    if !resolved.starts_with(&root) {
        bail!("文件路径越界");
    }
    Ok(resolved)
}

fn read_text(path: &Path) -> Result<String> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_TEXT {
        bail!("文件超过编辑大小限制");
    }
    let mut bytes = Vec::new();
    file.take(MAX_TEXT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT || bytes.contains(&0) {
        bail!("仅支持有界文本文件");
    }
    Ok(String::from_utf8(bytes)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_resolution_rejects_escape_and_allows_new_child() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("sample.md"), "sample").unwrap();
        assert!(confined_path(dir.path(), "../sample.md", false).is_err());
        assert!(confined_path(dir.path(), "nested/../../sample.md", true).is_err());
        assert!(confined_path(dir.path(), "new.md", true).is_ok());
        assert_eq!(
            read_text(&confined_path(dir.path(), "sample.md", false).unwrap()).unwrap(),
            "sample"
        );
    }
    #[test]
    fn editor_rejects_binary_and_oversized_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sample.bin");
        fs::write(&path, [0, 1, 2]).unwrap();
        assert!(read_text(&path).is_err());
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_TEXT + 1)
            .unwrap();
        assert!(read_text(&path).is_err());
    }
    #[test]
    fn headers_text_roundtrip_and_rejects_non_object() {
        assert!(parse_headers_text("").unwrap().is_empty());
        let parsed = parse_headers_text("{\"Authorization\":\"Bearer demo\"}").unwrap();
        assert_eq!(
            parsed.get("Authorization").map(String::as_str),
            Some("Bearer demo")
        );
        assert!(parse_headers_text("[1,2]").is_err());
        assert!(parse_headers_text("not json").is_err());
        assert_eq!(headers_to_text(&Default::default()), "");
        let text = headers_to_text(&parsed);
        assert!(text.contains("\"Authorization\""));
    }
    #[test]
    fn chunk_sizes_parse_as_optional_usize() {
        assert_eq!(parse_optional_usize("", "切块").unwrap(), None);
        assert_eq!(parse_optional_usize(" 512 ", "切块").unwrap(), Some(512));
        assert!(parse_optional_usize("big", "切块").is_err());
    }
    #[test]
    fn base_type_label_maps_known_types() {
        assert_eq!(base_type_label(None), "literal");
        assert_eq!(base_type_label(Some("vector")), "vector");
        assert_eq!(base_type_label(Some("ragflow")), "ragflow");
        assert_eq!(base_type_label(Some("unknown")), "literal");
    }
}

/// 粘贴导入的一条服务器：字段已在文本层解析、校验过，可直接写入存储。
struct PastedMcpServer {
    name: String,
    display_name: String,
    endpoint: String,
    transport: String,
    description: String,
    headers: HashMap<String, String>,
}

/// 粘贴内容的三种形态：`{"mcpServers": {...}}`、单个服务器对象、服务器数组。
/// 返回 (名称, 服务器配置)；名称优先取容器的键，缺省时退回配置里的 name/id。
fn paste_entries(value: &serde_json::Value) -> Result<Vec<(String, serde_json::Value)>> {
    let named = |item: &serde_json::Value| -> String {
        item.get("name")
            .or_else(|| item.get("id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    match value {
        serde_json::Value::Object(map) => {
            for key in ["mcpServers", "mcp_servers", "servers"] {
                if let Some(serde_json::Value::Object(servers)) = map.get(key) {
                    return Ok(servers
                        .iter()
                        .map(|(name, config)| (name.clone(), config.clone()))
                        .collect());
                }
            }
            Ok(vec![(named(value), value.clone())])
        }
        serde_json::Value::Array(items) => Ok(items
            .iter()
            .map(|item| (named(item), item.clone()))
            .collect()),
        _ => bail!("粘贴内容需要是 JSON 对象或数组"),
    }
}

/// 一条粘贴配置 → 存储草稿。字段别名与云端版导入一致（url/endpoint/baseUrl、
/// transport/type，请求头支持对象或 JSON 字符串），地址只接受 HTTP/HTTPS。
fn paste_server(key: &str, config: &serde_json::Value) -> Result<PastedMcpServer> {
    let map = config
        .as_object()
        .ok_or_else(|| anyhow!("服务器配置必须是 JSON 对象"))?;
    let text = |names: &[&str]| -> String {
        names
            .iter()
            .find_map(|name| map.get(*name).and_then(serde_json::Value::as_str))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let name = match key.trim() {
        "" => text(&["name", "id"]),
        key => key.to_string(),
    };
    if name.is_empty() {
        bail!("缺少服务器名称");
    }
    validate_name(&name)?;
    let endpoint = text(&["url", "endpoint", "baseUrl", "base_url", "address"]);
    if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
        bail!("MCP 服务器缺少 HTTP/HTTPS 地址");
    }
    let transport = match text(&["transport", "type"]).as_str() {
        "" | "auto" => String::new(),
        other => other.to_string(),
    };
    let headers = match map.get("headers") {
        Some(serde_json::Value::String(raw)) => parse_headers_text(raw)?,
        Some(serde_json::Value::Null) | None => HashMap::new(),
        Some(value) => {
            serde_json::from_value(value.clone()).map_err(|_| anyhow!("请求头必须是 JSON 对象"))?
        }
    };
    Ok(PastedMcpServer {
        name,
        display_name: text(&["display_name", "displayName"]),
        endpoint,
        transport,
        description: text(&["description"]),
        headers,
    })
}

#[cfg(test)]
mod paste_tests {
    use super::{paste_entries, paste_server};

    fn value(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn mcp_servers_wrapper_yields_one_entry_per_key() {
        let raw = value(
            r#"{"mcpServers":{"docs":{"url":"http://127.0.0.1:9000/mcp"},"gh":{"endpoint":"https://m.example.com/mcp"}}}"#,
        );
        let entries = paste_entries(&raw).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "docs");
        assert_eq!(entries[1].0, "gh");
    }

    #[test]
    fn single_object_and_array_are_both_accepted() {
        let single = paste_entries(&value(
            r#"{"name":"docs","url":"http://127.0.0.1:9000/mcp"}"#,
        ))
        .unwrap();
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].0, "docs");
        let list = paste_entries(&value(
            r#"[{"name":"a","url":"http://x/mcp"},{"id":"b","baseUrl":"https://y/mcp"}]"#,
        ))
        .unwrap();
        assert_eq!(list.len(), 2);
        let server = paste_server(&list[1].0, &list[1].1).unwrap();
        assert_eq!(server.name, "b");
        assert!(server.endpoint.starts_with("https://"));
    }

    #[test]
    fn headers_accept_an_object_or_a_json_string() {
        let from_object = paste_server(
            "docs",
            &value(r#"{"url":"http://x/mcp","headers":{"Authorization":"Bearer t"}}"#),
        )
        .unwrap();
        assert_eq!(
            from_object.headers.get("Authorization").map(String::as_str),
            Some("Bearer t")
        );
        let from_text = paste_server(
            "docs",
            &value(r#"{"url":"http://x/mcp","headers":"{\"X-Key\":\"v\"}"}"#),
        )
        .unwrap();
        assert_eq!(
            from_text.headers.get("X-Key").map(String::as_str),
            Some("v")
        );
    }

    #[test]
    fn stdio_and_nameless_entries_are_rejected() {
        assert!(paste_server("tool", &value(r#"{"command":"npx","args":["x"]}"#)).is_err());
        assert!(paste_server("", &value(r#"{"url":"http://x/mcp"}"#)).is_err());
    }
}
