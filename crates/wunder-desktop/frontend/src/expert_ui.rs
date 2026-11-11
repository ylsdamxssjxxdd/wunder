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
        // A live-save may legitimately be in flight when the group button is
        // clicked: toggling only dirties the form, and the pending-save timer
        // retries after the save settles, so there is no guard here.
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
            .map(|n| n.to_string())
            .collect::<Vec<_>>();
        for tool in group.tools.iter() {
            let name = tool.name.as_str();
            if group.all_selected {
                names.retain(|n| n.as_str() != name);
            } else if !names.iter().any(|n| n.as_str() == name) {
                names.push(name.to_string());
            }
        }
        // `set_vec` rewrites the backing model in place: the selection keeps
        // its identity instead of swapping the list under any binding.
        let names = names
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        match app
            .get_selected_agent_tool_names()
            .as_any()
            .downcast_ref::<VecModel<SharedString>>()
        {
            Some(selection) => selection.set_vec(names),
            None => app.set_selected_agent_tool_names(model(names)),
        }
        crate::entity_state::sync_tool_selection(&app);
        // The group toggle rewrites the selection model in place, which no
        // property-change hook would see; recompute the flag and queue the
        // live-save the same way a single switch does.
        crate::agent_editor::refresh_agent_dirty(&app);
        app.invoke_queue_agent_save();
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
        // Toggle-only refresh: a tool checkbox round-trips through the 500ms
        // live-save and this post-save re-projection. Rebuilding the picker here
        // re-renders every card and snaps its scroll to the top, which the user
        // reads as "the whole list refreshed". Build only when the picker is
        // still empty; otherwise `sync_tool_selection` already rewrote the
        // enabled flags in place and the model must be left untouched.
        if app.get_expert_tool_groups().row_count() == 0 {
            project_tools(&app);
        }
        // The active settings category is the only view selector now that the
        // expert page's own tab strip is gone: 6 memories, 13 account (runtime
        // usage, folded in from the retired activity category), 10 archives.
        let panel = app.get_settings_active_panel();
        let view = match panel {
            6 => Some(2),
            13 => Some(4),
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
                                    workspace: r.workspace_name.into(),
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
    let open_archive_api = api.clone();
    app.on_open_expert_archive(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let api = open_archive_api.clone();
        let weak = app.as_weak();
        let id = id.to_string();
        std::thread::spawn(move || {
            // Archive inspection is read-only. It deliberately bypasses the
            // normal selected-thread lease so viewing a detail modal cannot
            // block a concurrent restore/open in the desktop app or CLI.
            let result = api.get_archived_session_detail(&id);
            let _ = weak.upgrade_in_event_loop(move |app| match result {
                Ok((session, messages)) => {
                    app.set_archive_detail_title(session.title.into());
                    app.set_archive_detail_text(
                        messages
                            .into_iter()
                            .take(200)
                            .map(|m| m.text)
                            .collect::<Vec<_>>()
                            .join("\n\n")
                            .into(),
                    );
                    app.set_archive_detail_open(true);
                }
                Err(e) => app.set_expert_error(e.to_string().into()),
            });
        });
    });
    let weak = app.as_weak();
    let restore_api = api.clone();
    app.on_restore_expert_archive(move |id| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_expert_saving() {
            return;
        }
        let id = id.to_string();
        if id.trim().is_empty() {
            return;
        }
        app.set_expert_saving(true);
        let api = restore_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.restore_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_expert_saving(false);
                match result {
                    Ok(()) => {
                        app.set_archive_detail_open(false);
                        app.set_status("线程已还原".into());
                        app.invoke_refresh_expert();
                        app.invoke_refresh_chat();
                    }
                    Err(error) => app.set_expert_error(error.to_string().into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let delete_api = api.clone();
    app.on_delete_expert_archive(move |id| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_expert_saving() {
            return;
        }
        let id = id.to_string();
        if id.trim().is_empty() {
            return;
        }
        app.set_expert_saving(true);
        let api = delete_api.clone();
        let weak = app.as_weak();
        std::thread::spawn(move || {
            let result = api.delete_archived_session(&id);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.set_expert_saving(false);
                match result {
                    Ok(()) => {
                        app.set_archive_detail_open(false);
                        app.set_status("已永久删除归档线程".into());
                        app.invoke_refresh_expert();
                        app.invoke_refresh_chat();
                    }
                    Err(error) => app.set_expert_error(error.to_string().into()),
                }
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
fn duration_label(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as u64;
    format!("{}h {}m", minutes / 60, minutes % 60)
}
/// Projects one daily series into a 1000x200 command path (y flipped, 20%
/// headroom) and returns the unscaled peak so the axis label stays truthful.
fn trend_path(
    days: &[serde_json::Value],
    value_of: impl Fn(&serde_json::Value) -> f64,
) -> (String, f64) {
    let peak = days.iter().map(&value_of).fold(0.0_f64, f64::max);
    let scale = (peak * 1.2).max(1.0);
    let denominator = days.len().saturating_sub(1).max(1) as f64;
    let path = days
        .iter()
        .enumerate()
        .fold(String::new(), |mut path, (index, day)| {
            let value = (value_of(day) / scale).clamp(0.0, 1.0);
            path.push_str(&format!(
                "{} {:.2} {:.2} ",
                if index == 0 { "M" } else { "L" },
                index as f64 * 1000.0 / denominator,
                (1.0 - value) * 200.0
            ));
            path
        });
    (path, peak)
}
fn apply_runtime(app: &MainWindow, value: serde_json::Value) {
    let data = &value["data"];
    let summary = &data["summary"];
    let seconds = summary["runtime_seconds"].as_f64().unwrap_or(0.0) as u64;
    app.set_expert_duration(format!("{}h {}m", seconds / 3600, seconds % 3600 / 60).into());
    app.set_expert_tokens(compact(summary["consumed_tokens"].as_f64().unwrap_or(0.0)).into());
    app.set_expert_quota(compact(summary["quota_consumed"].as_f64().unwrap_or(0.0)).into());
    let days = data["daily"].as_array().cloned().unwrap_or_default();
    let (runtime_path, runtime_peak) = trend_path(&days, |day| {
        day["runtime_seconds"].as_f64().unwrap_or(0.0).max(0.0) / 60.0
    });
    let (tokens_path, tokens_peak) = trend_path(&days, |day| {
        day["consumed_tokens"].as_f64().unwrap_or(0.0).max(0.0)
    });
    let (quota_path, quota_peak) = trend_path(&days, |day| {
        day["quota_consumed"].as_f64().unwrap_or(0.0).max(0.0)
    });
    app.set_expert_runtime_path(runtime_path.into());
    app.set_expert_runtime_max(duration_label(runtime_peak * 60.0).into());
    app.set_expert_tokens_path(tokens_path.into());
    app.set_expert_tokens_max(compact(tokens_peak).into());
    app.set_expert_quota_path(quota_path.into());
    app.set_expert_quota_max(compact(quota_peak).into());
    let points = days
        .iter()
        .map(|day| {
            let date = day["date"].as_str().unwrap_or_default();
            RuntimePoint {
                date: date.into(),
                label: date.get(5..).unwrap_or(date).into(),
                value: day["consumed_tokens"].as_f64().unwrap_or(0.0) as f32,
            }
        })
        .collect();
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
