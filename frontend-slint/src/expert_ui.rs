//! Bounded expert projections. Storage work stays on a worker; stale responses are discarded.
use crate::{ArchiveCard, ExpertToolGroup, HeatCard, MainWindow, MemoryCard, RuntimePoint};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};
use wunder_desktop::NativeDesktop;

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}
pub(crate) fn agent_id(app: &MainWindow) -> Option<String> {
    usize::try_from(app.get_selected_agent())
        .ok()
        .and_then(|i| app.get_agents().row_data(i))
        .map(|a| a.id.to_string())
}
fn timestamp(value: f64) -> String {
    chrono::DateTime::from_timestamp(value as i64, 0)
        .map(|v| {
            v.with_timezone(&chrono::Local)
                .format("%Y/%m/%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}
pub(crate) fn project_tools(app: &MainWindow) {
    let query = app.get_expert_tool_keyword().trim().to_lowercase();
    let mut groups = Vec::new();
    for category in [
        "内置工具",
        "MCP 工具",
        "A2A 工具",
        "技能",
        "知识库",
        "用户工具",
        "共享工具",
    ] {
        let tools = app
            .get_tools()
            .iter()
            .filter(|t| {
                t.category == category
                    && (query.is_empty()
                        || t.name.to_lowercase().contains(&query)
                        || t.description.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        if !tools.is_empty() {
            let (title, source) = match category {
                "技能" => ("技能工具", "管理员开放工具"),
                "知识库" => ("知识库工具", "管理员开放工具"),
                "用户工具" => ("自定义工具", "用户自建工具"),
                "共享工具" => ("可用工具", "用户自建工具"),
                _ => (category, "管理员开放工具"),
            };
            groups.push(ExpertToolGroup {
                title: title.into(),
                source: source.into(),
                all_selected: tools.iter().all(|t| t.enabled),
                tools: model(tools),
            });
        }
    }
    app.set_expert_tool_groups(model(groups));
    app.set_expert_agent_names(model(app.get_agents().iter().map(|a| a.name).collect()));
}

/// Toggle-only refresh: updates enabled flags inside the existing
/// `expert_tool_groups` model so the ListView keeps its scroll position.
/// `project_tools` (full rebuild) stays reserved for keyword/agent changes.
pub(crate) fn sync_tool_group_selection(app: &MainWindow) {
    let selected = app
        .get_selected_agent_tool_names()
        .iter()
        .map(|name| name.to_string())
        .collect::<std::collections::HashSet<_>>();
    let groups = app.get_expert_tool_groups();
    if groups.row_count() == 0 {
        project_tools(app);
        return;
    }
    for index in 0..groups.row_count() {
        let Some(mut group) = groups.row_data(index) else {
            continue;
        };
        let mut all_selected = group.tools.row_count() > 0;
        for tool_index in 0..group.tools.row_count() {
            if let Some(mut tool) = group.tools.row_data(tool_index) {
                let enabled = selected.contains(tool.name.as_str());
                if tool.enabled != enabled {
                    tool.enabled = enabled;
                    group.tools.set_row_data(tool_index, tool);
                }
                all_selected &= enabled;
            }
        }
        if group.all_selected != all_selected {
            group.all_selected = all_selected;
            groups.set_row_data(index, group);
        }
    }
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let generation = Arc::new(AtomicU64::new(0));
    let busy = Arc::new(AtomicBool::new(false));
    let weak = app.as_weak();
    app.on_search_expert_tools(move || {
        if let Some(app) = weak.upgrade() {
            project_tools(&app);
        }
    });
    let weak = app.as_weak();
    app.on_toggle_expert_tool_group(move |category| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_saving() {
            return;
        }
        let Some(group) = app
            .get_expert_tool_groups()
            .iter()
            .find(|g| g.title == category)
        else {
            return;
        };
        let mut names = app
            .get_selected_agent_tool_names()
            .iter()
            .collect::<Vec<_>>();
        for tool in group.tools.iter() {
            if group.all_selected {
                names.retain(|n| n != &tool.name);
            } else if !names.contains(&tool.name) {
                names.push(tool.name);
            }
        }
        app.set_selected_agent_tool_names(model(names));
        crate::entity_state::sync_tool_selection(&app);
        // The group toggle rewrites the selection model in place, which no
        // property-change hook would see.
        crate::agent_editor::refresh_agent_dirty(&app);
    });
    let weak = app.as_weak();
    app.on_edit_agent_question(move |index, text| {
        let Some(app) = weak.upgrade() else { return };
        if !app.get_saving() {
            if let Ok(index) = usize::try_from(index) {
                if index < app.get_selected_agent_preset_questions().row_count() {
                    app.get_selected_agent_preset_questions()
                        .set_row_data(index, text);
                    // In-place model mutation does not reassign the property,
                    // so the dirty recomputation runs here explicitly.
                    crate::agent_editor::refresh_agent_dirty(&app);
                }
            }
        }
    });
    let weak = app.as_weak();
    let refresh_api = api.clone();
    app.on_refresh_expert(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(agent) = agent_id(&app) else { return };
        project_tools(&app);
        // The active settings category is the only view selector now that the
        // expert page's own tab strip is gone: 6 memories, 8 usage, 10 archives.
        let panel = app.get_settings_active_panel();
        let view = match panel {
            6 => Some(2),
            8 => Some(4),
            10 => Some(5),
            _ => None,
        };
        app.set_expert_error("".into());
        let Some(view) = view else {
            // Cron, channels and every other category own their own refresh;
            // nothing loads here, and a no-op must not cancel an in-flight pull.
            app.set_expert_loading(false);
            return;
        };
        let ticket = generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation = generation.clone();
        if busy.swap(true, Ordering::Relaxed) {
            return;
        }
        let busy = busy.clone();
        let api = refresh_api.clone();
        let weak = app.as_weak();
        let query = app.get_expert_query().to_string();
        let category = app.get_expert_category().to_string();
        let date = app.get_expert_date().to_string();
        let offset = app.get_expert_archive_page() as i64 * 50;
        app.set_expert_loading(true);
        std::thread::spawn(move || {
            enum Data {
                Memories(Vec<wunder_desktop::native::ExpertMemory>),
                Archives(Vec<wunder_desktop::NativeSession>, i64),
                Runtime(serde_json::Value),
            }
            let result = match view {
                2 => api
                    .expert_memories(
                        &agent,
                        &query,
                        if category == "全部标签" {
                            ""
                        } else {
                            &category
                        },
                    )
                    .map(Data::Memories),
                4 => api
                    .expert_runtime(&agent, (!date.is_empty()).then_some(date.as_str()))
                    .map(Data::Runtime),
                5 => api
                    .expert_archives(&agent, offset)
                    .map(|(rows, total)| Data::Archives(rows, total)),
                _ => unreachable!("view filtered before spawn"),
            };
            let _ = weak.upgrade_in_event_loop(move |app| {
                busy.store(false, Ordering::Relaxed);
                if generation.load(Ordering::Relaxed) != ticket
                    || agent_id(&app).as_deref() != Some(agent.as_str())
                    || app.get_settings_active_panel() != panel
                {
                    app.invoke_refresh_expert();
                    return;
                }
                app.set_expert_loading(false);
                match result {
                    Ok(Data::Memories(rows)) => {
                        if category == "全部标签" {
                            let mut categories = vec![SharedString::from("全部标签")];
                            for row in &rows {
                                let value = SharedString::from(row.category.as_str());
                                if !categories.contains(&value) {
                                    categories.push(value);
                                }
                            }
                            app.set_expert_categories(model(categories));
                        }
                        app.set_expert_memories(model(
                            rows.into_iter()
                                .map(|m| MemoryCard {
                                    id: m.id.into(),
                                    title: m.title.into(),
                                    content: m.content.into(),
                                    category: m.category.into(),
                                    source: if m.source == "manual" {
                                        "手动写入"
                                    } else {
                                        "模型写入"
                                    }
                                    .into(),
                                    time: timestamp(m.updated_at).into(),
                                })
                                .collect(),
                        ));
                    }
                    Ok(Data::Archives(rows, total)) => {
                        app.set_expert_archive_total(total.min(i32::MAX as i64) as i32);
                        app.set_expert_archives(model(
                            rows.into_iter()
                                .map(|r| ArchiveCard {
                                    id: r.id.into(),
                                    title: r.title.into(),
                                    time: timestamp(r.updated_at).into(),
                                })
                                .collect(),
                        ));
                    }
                    Ok(Data::Runtime(value)) => apply_runtime(&app, value),
                    Err(error) => app.set_expert_error(error.to_string().into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_open_expert_memory(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let row = usize::try_from(index)
            .ok()
            .and_then(|i| app.get_expert_memories().row_data(i))
            .unwrap_or_default();
        app.set_memory_edit_id(row.id);
        app.set_memory_edit_title(row.title);
        app.set_memory_edit_content(row.content);
        app.set_memory_edit_category(if row.category.is_empty() {
            "session_summary".into()
        } else {
            row.category
        });
        app.set_memory_editor_open(true);
    });
    let weak = app.as_weak();
    let save_api = api.clone();
    app.on_save_expert_memory(move || {
        let Some(app) = weak.upgrade() else { return };
        let Some(agent) = agent_id(&app) else { return };
        if app.get_expert_saving() {
            return;
        }
        let (id, title, content, category) = (
            app.get_memory_edit_id().to_string(),
            app.get_memory_edit_title().to_string(),
            app.get_memory_edit_content().to_string(),
            app.get_memory_edit_category().to_string(),
        );
        let api = save_api.clone();
        let weak = app.as_weak();
        app.set_expert_saving(true);
        std::thread::spawn(move || {
            let result = api.save_expert_memory(&agent, &id, &title, &content, &category);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_expert_saving(false);
                if agent_id(&app).as_deref() != Some(agent.as_str()) {
                    return;
                }
                match result {
                    Ok(()) => {
                        app.set_memory_editor_open(false);
                        app.invoke_refresh_expert();
                    }
                    Err(e) => app.set_expert_error(e.to_string().into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_expert_memory(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let Some(agent) = agent_id(&app) else { return };
        if app.get_expert_saving() {
            return;
        }
        app.set_expert_saving(true);
        let api = delete_api.clone();
        let weak = app.as_weak();
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = api.delete_expert_memory(&agent, &id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_expert_saving(false);
                if agent_id(&app).as_deref() != Some(agent.as_str()) {
                    return;
                }
                match result {
                    Ok(()) => {
                        app.set_memory_editor_open(false);
                        app.invoke_refresh_expert();
                    }
                    Err(e) => app.set_expert_error(e.to_string().into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let replicate_api = api.clone();
    app.on_replicate_expert_memory(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let Some(agent) = agent_id(&app) else { return };
        let Some(target) = usize::try_from(index)
            .ok()
            .and_then(|i| app.get_agents().row_data(i))
        else {
            return;
        };
        if app.get_expert_saving() {
            return;
        }
        app.set_expert_saving(true);
        let api = replicate_api.clone();
        let weak = app.as_weak();
        let target = target.id.to_string();
        std::thread::spawn(move || {
            let result = api.replicate_expert_memories(&agent, &target);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_expert_saving(false);
                if agent_id(&app).as_deref() != Some(agent.as_str()) {
                    return;
                }
                match result {
                    Ok(()) => app.set_status("记忆复刻完成".into()),
                    Err(e) => app.set_expert_error(e.to_string().into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    app.on_open_expert_archive(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let api = api.clone();
        let weak = app.as_weak();
        let id = id.to_string();
        std::thread::spawn(move || {
            let result = api.get_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok((session, messages)) => {
                    app.set_dialog_title(session.title.into());
                    app.set_dialog_text(
                        messages
                            .into_iter()
                            .take(200)
                            .map(|m| m.text)
                            .collect::<Vec<_>>()
                            .join("\n\n")
                            .into(),
                    );
                    app.set_dialog_open(true);
                }
                Err(e) => app.set_expert_error(e.to_string().into()),
            });
        });
    });
}

fn compact(value: f64) -> String {
    if value >= 1_000_000.0 {
        format!("{:.2}M", value / 1_000_000.0)
    } else if value >= 1_000.0 {
        format!("{:.1}K", value / 1_000.0)
    } else {
        format!("{value:.0}")
    }
}
fn apply_runtime(app: &MainWindow, value: serde_json::Value) {
    let data = &value["data"];
    let summary = &data["summary"];
    let seconds = summary["runtime_seconds"].as_f64().unwrap_or(0.0) as u64;
    app.set_expert_duration(format!("{}h {}m", seconds / 3600, seconds % 3600 / 60).into());
    app.set_expert_tokens(compact(summary["consumed_tokens"].as_f64().unwrap_or(0.0)).into());
    app.set_expert_calls(compact(summary["tool_calls"].as_f64().unwrap_or(0.0)).into());
    let days = data["daily"].as_array().cloned().unwrap_or_default();
    let max = days
        .iter()
        .filter_map(|d| d["consumed_tokens"].as_f64())
        .fold(1.0_f64, f64::max)
        * 1.2;
    app.set_expert_chart_max(compact(max).into());
    let mut path = String::new();
    let points = days
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let date = d["date"].as_str().unwrap_or_default();
            let value = d["consumed_tokens"].as_f64().unwrap_or(0.0) / max;
            path.push_str(&format!(
                "{} {:.2} {:.2} ",
                if i == 0 { "M" } else { "L" },
                i as f64 * 1000.0 / (days.len().saturating_sub(1).max(1)) as f64,
                (1.0 - value) * 200.0
            ));
            RuntimePoint {
                date: date.into(),
                label: date.get(5..).unwrap_or(date).into(),
                value: value as f32,
            }
        })
        .collect();
    app.set_expert_chart_path(path.into());
    app.set_expert_points(model(points));
    let heat = &data["heatmap"];
    app.set_expert_date(heat["date"].as_str().unwrap_or_default().into());
    let max = heat["max_calls"].as_f64().unwrap_or(1.0).max(1.0);
    app.set_expert_heat(model(
        heat["items"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .take(24)
                    .map(|i| HeatCard {
                        name: i["display_name"].as_str().unwrap_or_default().into(),
                        count: compact(i["total_calls"].as_f64().unwrap_or(0.0)).into(),
                        intensity: (i["total_calls"].as_f64().unwrap_or(0.0) / max) as f32,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    ));
}
