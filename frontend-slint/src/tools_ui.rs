//! Native tools controller. Resource selection has a generation guard so slow
//! filesystem reads never populate another tab; only one mutation runs at once.
use crate::{
    FileCard, MainWindow, McpServerDraft, KnowledgeInfo, ToolCard, ToolResource, ToolsDialog,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wunder_desktop::native::{
    NativeKnowledgeConfig, NativeMcpConfig, NativeToolManager, NativeToolResource,
    ToolResourceKind,
};
use wunder_desktop::{NativeDesktop, ToolRecord};

fn model<T: Clone + 'static>(items: Vec<T>) -> ModelRc<T> {
    Rc::new(VecModel::from(items)).into()
}
fn card(tool: ToolRecord) -> ToolCard {
    let visual = crate::tool_icons::tool_card_visual(&tool.name, &tool.description, &tool.category);
    ToolCard {
        name: tool.name.into(),
        description: tool.description.into(),
        category: tool.category.into(),
        enabled: false,
        icon: visual.icon.into(),
        tone: visual.tone.into(),
    }
}
fn resource(item: NativeToolResource) -> ToolResource {
    ToolResource {
        name: item.name.into(),
        description: item.description.into(),
        detail: item.detail.into(),
        readonly: item.readonly,
    }
}
fn mcp_config(config: &NativeMcpConfig) -> McpServerDraft {
    McpServerDraft {
        name: config.name.clone().into(),
        display_name: config.display_name.clone().into(),
        endpoint: config.endpoint.clone().into(),
        transport: config.transport.clone().into(),
        description: config.description.clone().into(),
        headers: config.headers.clone().into(),
    }
}
fn kb_config(config: &NativeKnowledgeConfig) -> KnowledgeInfo {
    KnowledgeInfo {
        name: config.name.clone().into(),
        description: config.description.clone().into(),
        base_type: config.base_type.clone().into(),
        embedding_model: config.embedding_model.clone().into(),
        chunk_size: config.chunk_size.clone().into(),
        chunk_overlap: config.chunk_overlap.clone().into(),
        ragflow_dataset_id: config.ragflow_dataset_id.clone().into(),
    }
}
fn kind(tab: i32) -> Option<ToolResourceKind> {
    match tab {
        1 => Some(ToolResourceKind::Mcp),
        2 => Some(ToolResourceKind::Skill),
        3 => Some(ToolResourceKind::Knowledge),
        _ => None,
    }
}
fn selected(app: &MainWindow) -> Option<(ToolResourceKind, String)> {
    let rows = match app.get_tools_tab() {
        1 => app.get_tools_servers(),
        2 => app.get_tools_skill_resources(),
        3 => app.get_tools_bases(),
        _ => return None,
    };
    let row = rows.row_data(usize::try_from(app.get_tools_selected()).ok()?)?;
    Some((kind(app.get_tools_tab())?, row.name.to_string()))
}
fn error(app: &MainWindow, error: impl std::fmt::Display) {
    app.set_status("工具操作失败".into());
    app.set_dialog_title("工具操作失败".into());
    app.set_dialog_text(error.to_string().into());
    app.set_dialog_open(true);
}
fn apply(app: &MainWindow, catalog: Vec<ToolRecord>, manager: NativeToolManager) {
    app.set_tools_builtin(model(
        catalog
            .iter()
            .filter(|t| t.category == "内置工具")
            .cloned()
            .map(card)
            .collect(),
    ));
    app.set_tools_mcp(model(
        catalog
            .iter()
            .filter(|t| t.category == "MCP 工具")
            .cloned()
            .map(card)
            .collect(),
    ));
    app.set_tools_skills(model(
        catalog
            .iter()
            .filter(|t| t.category == "技能")
            .cloned()
            .map(card)
            .collect(),
    ));
    app.set_tools_knowledge(model(
        catalog
            .iter()
            .filter(|t| t.category == "知识库")
            .cloned()
            .map(card)
            .collect(),
    ));
    app.set_tools(model(catalog.into_iter().map(card).collect()));
    crate::entity_state::sync_tool_selection(app);
    app.set_tools_servers(model(manager.servers.into_iter().map(resource).collect()));
    app.set_tools_skill_resources(model(manager.skills.into_iter().map(resource).collect()));
    app.set_tools_bases(model(manager.bases.into_iter().map(resource).collect()));
    let dialog = app.global::<ToolsDialog>();
    dialog.set_server_configs(model(manager.server_configs.iter().map(mcp_config).collect()));
    dialog.set_base_configs(model(manager.base_configs.iter().map(kb_config).collect()));
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    app.on_search_tool_files(move |query| {
        if let Some(app) = weak.upgrade() {
            filter_files(&app, query.as_str());
        }
    });
    let generation = Arc::new(AtomicU64::new(0));
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_tools(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_loading() || app.get_tools_working() {
            return;
        }
        app.set_tools_loading(true);
        let previous = selected(&app).map(|(_, name)| name);
        let weak = weak.clone();
        let api = refresh_api.clone();
        std::thread::spawn(move || {
            let result = api
                .tool_manager()
                .and_then(|manager| Ok((api.list_tools()?, manager)));
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_loading(false);
                match result {
                    Ok((catalog, manager)) => {
                        apply(&app, catalog, manager);
                        let rows = match app.get_tools_tab() {
                            1 => app.get_tools_servers(),
                            2 => app.get_tools_skill_resources(),
                            _ => app.get_tools_bases(),
                        };
                        let index = previous
                            .and_then(|name| rows.iter().position(|r| r.name == name))
                            .map(|i| i as i32)
                            .unwrap_or(if rows.row_count() > 0 && app.get_tools_tab() == 2 {
                                0
                            } else {
                                -1
                            });
                        app.set_tools_selected(index);
                        app.invoke_select_tool_resource(app.get_tools_tab(), index);
                        app.set_status("工具目录已同步".into());
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    let weak = app.as_weak();
    let select_api = api.clone();
    let gen = generation.clone();
    app.on_select_tool_resource(move |tab, index| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() {
            return;
        }
        let token = gen.fetch_add(1, Ordering::Relaxed) + 1;
        app.set_tools_files(ModelRc::default());
        app.set_tools_server_tools(ModelRc::default());
        app.set_tools_all_files(ModelRc::default());
        app.set_tools_search("".into());
        app.set_tools_file_path("".into());
        app.set_tools_content("".into());
        app.set_tools_directory("".into());
        app.set_tools_has_more(false);
        app.set_tools_tab(tab);
        let index = if index < 0 && tab == 2 && app.get_tools_skill_resources().row_count() > 0 {
            0
        } else {
            index
        };
        app.set_tools_selected(index);
        let Some((kind, name)) = selected(&app) else {
            return;
        };
        app.set_tools_working(true);
        let gen = gen.clone();
        let weak = weak.clone();
        let api = select_api.clone();
        std::thread::spawn(move || {
            let result = if kind == ToolResourceKind::Mcp {
                api.tool_server_tools(&name).map(Selection::Tools)
            } else {
                api.tool_resource_files(kind, &name, "", 0)
                    .map(Selection::Files)
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                if gen.load(Ordering::Relaxed) != token {
                    return;
                }
                if selected(&app) != Some((kind, name)) {
                    return;
                }
                match result {
                    Ok(Selection::Tools(rows)) => {
                        app.set_tools_server_tools(model(rows.into_iter().map(card).collect()))
                    }
                    Ok(Selection::Files(files)) => apply_files(&app, files, false),
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    let weak = app.as_weak();
    let file_api = api.clone();
    app.on_open_tool_file(move |path, directory| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() {
            return;
        }
        let Some((kind, name)) = selected(&app) else {
            return;
        };
        let path = if path == ".." {
            std::path::Path::new(app.get_tools_directory().as_str())
                .parent()
                .unwrap_or(std::path::Path::new(""))
                .to_string_lossy()
                .into_owned()
        } else {
            path.to_string()
        };
        app.set_tools_working(true);
        let weak = weak.clone();
        let api = file_api.clone();
        std::thread::spawn(move || {
            let result = if directory {
                api.tool_resource_files(kind, &name, &path, 0)
                    .map(FileResult::Directory)
            } else {
                api.read_tool_resource(kind, &name, &path)
                    .map(FileResult::Text)
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                if selected(&app) != Some((kind, name)) {
                    return;
                }
                match result {
                    Ok(FileResult::Directory(files)) => {
                        app.set_tools_directory(path.into());
                        app.set_tools_file_path("".into());
                        apply_files(&app, files, false);
                    }
                    Ok(FileResult::Text(content)) => {
                        app.set_tools_file_path(path.into());
                        app.set_tools_content(content.into());
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    let weak = app.as_weak();
    let more_api = api.clone();
    app.on_more_tool_files(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() {
            return;
        }
        let Some((kind, name)) = selected(&app) else {
            return;
        };
        let offset = app.get_tools_all_files().row_count();
        if offset >= 2000 {
            return;
        }
        let directory = app.get_tools_directory().to_string();
        let api = more_api.clone();
        let weak = weak.clone();
        app.set_tools_working(true);
        std::thread::spawn(move || {
            let result = api.tool_resource_files(kind, &name, &directory, offset);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                if selected(&app) != Some((kind, name)) || app.get_tools_directory() != directory {
                    return;
                }
                match result {
                    Ok(files) => apply_files(&app, files, true),
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    let weak = app.as_weak();
    let action_api = api.clone();
    app.on_tool_manager_action(move |operation, name, value| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() || app.get_tools_loading() {
            return;
        }
        let Some(kind) = kind(app.get_tools_tab()) else {
            return;
        };
        let current = selected(&app).map(|(_, n)| n).unwrap_or_default();
        let directory = app.get_tools_directory().to_string();
        let api = action_api.clone();
        let weak = weak.clone();
        app.set_tools_working(true);
        std::thread::spawn(move || {
            let result = match operation.as_str() {
                "delete" => api.delete_tool_resource(kind, &current),
                "connect" => api.connect_tool_server(&current),
                "export" => api.export_tool_skill(&current, &name),
                "save" => api.save_tool_resource(kind, &current, &name, &value, false),
                "file" => api.save_tool_resource(kind, &current, &name, &value, true),
                "upload" => (|| {
                    let source = std::path::Path::new(name.as_str());
                    if std::fs::metadata(source)?.len() > 2 * 1024 * 1024 {
                        anyhow::bail!("文件超过上传大小限制");
                    }
                    let text = std::fs::read_to_string(source)?;
                    let filename = source
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| anyhow::anyhow!("文件名无效"))?;
                    let target = std::path::Path::new(&directory).join(filename);
                    api.save_tool_resource(kind, &current, &target.to_string_lossy(), &text, true)
                })(),
                _ => Err(anyhow::anyhow!("未知操作")),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                match result {
                    Ok(()) => {
                        app.set_status("工具操作完成".into());
                        if operation != "save" && operation != "export" {
                            app.invoke_refresh_tools();
                        }
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    install_dialog_actions(app, &api);
}

/// Dialog-form actions live on the shared ToolsDialog global: the MCP server
/// and knowledge base forms plus the native skill-archive import.
fn install_dialog_actions(app: &MainWindow, api: &Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let save_api = api.clone();
    app.global::<ToolsDialog>().on_save_mcp(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() || app.get_tools_loading() {
            return;
        }
        let dialog = app.global::<ToolsDialog>();
        let draft = wunder_desktop::native::NativeMcpServerDraft {
            previous: dialog.get_mcp_previous().to_string(),
            name: dialog.get_mcp_name().to_string(),
            display_name: dialog.get_mcp_display_name().to_string(),
            endpoint: dialog.get_mcp_endpoint().to_string(),
            transport: dialog.get_mcp_transport().to_string(),
            description: dialog.get_mcp_description().to_string(),
            headers: dialog.get_mcp_headers().to_string(),
        };
        app.set_tools_working(true);
        let weak = weak.clone();
        let api = save_api.clone();
        std::thread::spawn(move || {
            let result = api.save_mcp_server(&draft);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                match result {
                    Ok(()) => {
                        app.set_status("工具操作完成".into());
                        app.invoke_refresh_tools();
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    let weak = app.as_weak();
    let kb_api = api.clone();
    app.global::<ToolsDialog>().on_save_kb(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() || app.get_tools_loading() {
            return;
        }
        let dialog = app.global::<ToolsDialog>();
        let base_type = match dialog.get_kb_type_index() {
            1 => "literal",
            2 => "ragflow",
            _ => "vector",
        };
        let draft = wunder_desktop::native::NativeKnowledgeDraft {
            previous: dialog.get_kb_previous().to_string(),
            name: dialog.get_kb_name().to_string(),
            description: dialog.get_kb_description().to_string(),
            base_type: base_type.to_string(),
            embedding_model: dialog.get_kb_embedding_model().to_string(),
            chunk_size: dialog.get_kb_chunk_size().to_string(),
            chunk_overlap: dialog.get_kb_chunk_overlap().to_string(),
            ragflow_dataset_id: dialog.get_kb_dataset_id().to_string(),
        };
        app.set_tools_working(true);
        let weak = weak.clone();
        let api = kb_api.clone();
        std::thread::spawn(move || {
            let result = api.save_knowledge_base(&draft);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                match result {
                    Ok(()) => {
                        app.set_status("工具操作完成".into());
                        app.invoke_refresh_tools();
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });

    // Live struct preview and headers validation for the MCP form. Everything
    // stays on the UI thread: the draft is a few short strings.
    let weak = app.as_weak();
    app.global::<ToolsDialog>().on_refresh_mcp_feedback(move || {
        let Some(app) = weak.upgrade() else { return };
        let dialog = app.global::<ToolsDialog>();
        let headers = dialog.get_mcp_headers().to_string();
        let headers_error = match validate_headers_text(&headers) {
            Ok(()) => "",
            Err(message) => message,
        };
        dialog.set_mcp_headers_error(headers_error.into());
        let preview = build_mcp_struct_preview(
            &dialog.get_mcp_name(),
            &dialog.get_mcp_display_name(),
            &dialog.get_mcp_endpoint(),
            &dialog.get_mcp_transport(),
            &dialog.get_mcp_description(),
            &headers,
        );
        dialog.set_mcp_preview(preview.into());
    });

    let weak = app.as_weak();
    let import_api = api.clone();
    app.global::<ToolsDialog>().on_import_skill(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_tools_working() || app.get_tools_loading() {
            return;
        }
        // The native picker runs modally on the UI thread, matching the
        // settings page supplement import; extraction runs on a worker.
        let Some(archive) = crate::file_dialog::pick_skill_archive() else {
            return;
        };
        app.set_tools_working(true);
        app.set_status("正在导入技能包…".into());
        let weak = weak.clone();
        let api = import_api.clone();
        std::thread::spawn(move || {
            let result = api.import_tool_resource(ToolResourceKind::Skill, &archive);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_tools_working(false);
                match result {
                    Ok(()) => {
                        app.set_status("工具操作完成".into());
                        app.invoke_refresh_tools();
                    }
                    Err(e) => error(&app, e),
                }
            });
        });
    });
}

fn validate_headers_text(text: &str) -> std::result::Result<(), &'static str> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(value) if value.is_object() => Ok(()),
        _ => Err("请求头必须是 JSON 对象"),
    }
}

/// Mirror the web struct preview: an MCP config envelope for the draft, or an
/// empty string so the field falls back to its placeholder.
fn build_mcp_struct_preview(
    name: &str,
    display_name: &str,
    endpoint: &str,
    transport: &str,
    description: &str,
    headers: &str,
) -> String {
    let name = name.trim();
    let endpoint = endpoint.trim();
    if name.is_empty() || endpoint.is_empty() {
        return String::new();
    }
    let mut config = serde_json::Map::new();
    let transport = transport.trim();
    if !transport.is_empty() && transport != "auto" {
        config.insert("type".into(), transport.into());
    }
    let description = description.trim();
    if !description.is_empty() {
        config.insert("description".into(), description.into());
    }
    let display_name = display_name.trim();
    config.insert(
        "name".into(),
        if display_name.is_empty() { name } else { display_name }.into(),
    );
    config.insert("baseUrl".into(), endpoint.into());
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(headers)
    {
        if !map.is_empty() {
            config.insert("headers".into(), map.into());
        }
    }
    let preview = serde_json::json!({ "mcpServers": { name: config } });
    serde_json::to_string_pretty(&preview).unwrap_or_default()
}

enum Selection {
    Tools(Vec<ToolRecord>),
    Files(wunder_desktop::native::NativeToolFiles),
}
enum FileResult {
    Text(String),
    Directory(wunder_desktop::native::NativeToolFiles),
}
fn apply_files(app: &MainWindow, files: wunder_desktop::native::NativeToolFiles, append: bool) {
    let mut rows = if append {
        app.get_tools_all_files().iter().collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    rows.extend(files.entries.into_iter().map(|f| {
        let entry_type = if f.directory { "dir" } else { "file" };
        FileCard {
            icon: crate::file_icons::workspace_file_icon(&f.name, entry_type),
            name: f.name.into(),
            path: f.path.into(),
            entry_type: entry_type.into(),
            size: format!("{:.1} KB", f.size as f64 / 1024.0).into(),
            modified: Default::default(),
            depth: 0,
            expanded: false,
        }
    }));
    app.set_tools_has_more(files.has_more && rows.len() < 2000);
    app.set_tools_all_files(model(rows));
    filter_files(app, app.get_tools_search().as_str());
}

fn filter_files(app: &MainWindow, query: &str) {
    let query = query.trim().to_lowercase();
    let source = app.get_tools_all_files();
    if query.is_empty() {
        app.set_tools_files(source);
        return;
    }
    app.set_tools_files(model(
        source
            .iter()
            .filter(|row| row.name.to_lowercase().contains(&query))
            .collect(),
    ));
}
