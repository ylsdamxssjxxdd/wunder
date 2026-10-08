//! Durable thread log controller. Only one catalog page and one item page are retained.
use crate::{LogMetric, MainWindow, ThreadLogEvent};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use wunder_desktop::NativeDesktop;

#[derive(Default)]
struct State {
    session: String,
    agent: String,
    generation: u64,
    page: usize,
    cursors: Vec<Option<i64>>,
    turns: Vec<Value>,
    selected: usize,
    after: i64,
    next_after: i64,
    events: Vec<ThreadLogEvent>,
    overview: Value,
}
fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let state = Rc::new(RefCell::new(State::default()));
    let (weak, current) = (app.as_weak(), state.clone());
    app.on_filter_thread_log(move || {
        if let Some(app) = weak.upgrade() {
            filter(&app, &current.borrow());
        }
    });
    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_select_log_round(move |index| {
        let Some(app) = weak.upgrade() else { return };
        if index < 0 || index as usize >= current.borrow().turns.len() {
            return;
        }
        current.borrow_mut().selected = index as usize;
        load_items(&app, current.clone(), backend.clone(), -1);
    });
    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_log_page(move |delta| {
        let Some(app) = weak.upgrade() else { return };
        let page = (current.borrow().page as i32 + delta).max(0) as usize;
        if page < current.borrow().cursors.len() {
            load_page(&app, current.clone(), backend.clone(), page);
        }
    });
    let (weak, current, backend) = (app.as_weak(), state.clone(), api.clone());
    app.on_more_log_items(move || {
        let Some(app) = weak.upgrade() else { return };
        let after = current.borrow().next_after;
        load_items(&app, current.clone(), backend.clone(), after);
    });
    let weak = app.as_weak();
    app.on_export_thread_log(move || {
        let current = state.borrow();
        let session = current.session.clone();
        let api = api.clone();
        let weak = weak.clone();
        std::thread::spawn(move || {
            let target = crate::file_dialog::save_file("导出线程日志", "thread-log.json");
            if let Some(target) = target {
                let result = api.export_thread_log(&session, std::path::Path::new(&target));
                let _ = weak.upgrade_in_event_loop(move |app| {
                    app.set_status(match result {
                        Ok(()) => "线程日志已导出".into(),
                        Err(e) => format!("导出失败：{e}").into(),
                    })
                });
            }
        });
    });
}

fn load_page(app: &MainWindow, state: Rc<RefCell<State>>, api: Arc<NativeDesktop>, page: usize) {
    let (session, before, generation) = {
        let mut s = state.borrow_mut();
        s.generation += 1;
        (s.session.clone(), s.cursors[page], s.generation)
    };
    app.set_thread_log_loading(true);
    app.set_thread_log_error("".into());
    app.set_thread_log_events(model(vec![]));
    let weak = app.as_weak();
    // Rc UI state stays in the UI callback; only a SendWeakRc crosses the worker.
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let backend = api.clone();
    std::thread::spawn(move || {
        let _ = tx.send(backend.thread_log_page(&session, before));
    });
    poll(move || match rx.try_recv() {
        Ok(result) => {
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if state.borrow().generation != generation || !app.get_thread_log_open() {
                return true;
            }
            match result {
                Ok(data) => {
                    let options = data
                        .turns
                        .iter()
                        .map(|r| format!("用户轮次 {}", r["user_turn_index"]).into())
                        .collect();
                    let mut s = state.borrow_mut();
                    s.page = page;
                    s.turns = data.turns;
                    s.selected = 0;
                    s.overview = data.overview;
                    s.agent = data.agent_name;
                    s.cursors.truncate(page + 1);
                    if data.has_more {
                        s.cursors.push(data.next_before);
                    }
                    app.set_thread_log_round_options(model(options));
                    app.set_thread_log_round(0);
                    app.set_thread_log_previous(page > 0);
                    app.set_thread_log_next(data.has_more);
                    app.set_thread_log_page_info(
                        format!(
                            "第 {} 页 · {}-{} / {} 轮",
                            page + 1,
                            if s.turns.is_empty() { 0 } else { page * 50 + 1 },
                            page * 50 + s.turns.len(),
                            data.turn_total
                        )
                        .into(),
                    );
                    app.set_thread_log_metrics(model(metrics(
                        &s,
                        &data.username,
                        data.turn_total,
                        data.item_total,
                    )));
                    let empty = s.turns.is_empty();
                    drop(s);
                    if empty {
                        app.set_thread_log_question("".into());
                        state.borrow_mut().events.clear();
                        app.set_thread_log_loading(false);
                        app.set_thread_log_more(false);
                    } else {
                        load_items(&app, state.clone(), api.clone(), -1);
                    }
                }
                Err(e) => {
                    app.set_thread_log_loading(false);
                    app.set_thread_log_error(format!("无法读取日志：{e}").into());
                }
            }
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        Err(_) => true,
    });
}

fn load_items(app: &MainWindow, state: Rc<RefCell<State>>, api: Arc<NativeDesktop>, after: i64) {
    let (session, turn, generation, round) = {
        let mut s = state.borrow_mut();
        s.generation += 1;
        s.after = after;
        let row = &s.turns[s.selected];
        app.set_thread_log_question(row["summary"].as_str().unwrap_or("").into());
        (
            s.session.clone(),
            row["turn_id"].as_str().unwrap_or("").to_string(),
            s.generation,
            row["user_turn_index"].as_i64().unwrap_or(1) as i32,
        )
    };
    app.set_thread_log_loading(true);
    app.set_thread_log_error("".into());
    app.set_thread_log_events(model(vec![]));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.thread_log_turn(&session, &turn, after).map(|data| {
            let events = data
                .items
                .iter()
                .enumerate()
                .map(|(i, row)| project(row, round, (after + 2 + i as i64) as i32))
                .collect::<Vec<_>>();
            (events, data.next_after, data.has_more)
        });
        let _ = tx.send(result);
    });
    poll(move || match rx.try_recv() {
        Ok(result) => {
            let Some(app) = weak.upgrade() else {
                return true;
            };
            if state.borrow().generation != generation || !app.get_thread_log_open() {
                return true;
            }
            app.set_thread_log_loading(false);
            match result {
                Ok((events, next_after, has_more)) => {
                    let mut types = vec!["全部类型".into()];
                    for row in &events {
                        if !types.contains(&row.event_type) {
                            types.push(row.event_type.clone());
                        }
                    }
                    app.set_thread_log_types(model(types));
                    app.set_thread_log_type(0);
                    app.set_thread_log_more(has_more);
                    let mut s = state.borrow_mut();
                    s.next_after = next_after;
                    s.events = events;
                    filter(&app, &s);
                }
                Err(e) => app.set_thread_log_error(format!("无法读取事件：{e}").into()),
            }
            true
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => false,
        Err(_) => true,
    });
}

// Single-shot continuations own the response without a timer/callback cycle.
fn poll(mut receive: impl FnMut() -> bool + 'static) {
    slint::Timer::single_shot(std::time::Duration::from_millis(33), move || {
        if !receive() {
            poll(receive);
        }
    });
}
fn filter(app: &MainWindow, state: &State) {
    let query = app.get_thread_log_query().trim().to_lowercase();
    let kind = app
        .get_thread_log_types()
        .row_data(app.get_thread_log_type().max(0) as usize)
        .unwrap_or_default();
    app.set_thread_log_events(model(
        state
            .events
            .iter()
            .filter(|e| {
                (app.get_thread_log_type() <= 0 || e.event_type == kind)
                    && (query.is_empty()
                        || e.event_type.to_lowercase().contains(&query)
                        || e.summary.to_lowercase().contains(&query)
                        || e.raw.to_lowercase().contains(&query))
            })
            .cloned()
            .collect(),
    ));
}
fn project(row: &Value, round: i32, order: i32) -> ThreadLogEvent {
    let data = &row["payload"];
    let timestamp = row
        .get("updated_time")
        .or_else(|| row.get("created_time"))
        .unwrap_or(&Value::Null);
    let time = timestamp
        .as_f64()
        .map(|n| format!("{n:.3}"))
        .unwrap_or_else(|| timestamp.as_str().unwrap_or("—").into());
    let summary = data["summary"]
        .as_str()
        .or_else(|| data["message"].as_str())
        .or_else(|| data["status"].as_str())
        .or_else(|| row["status"].as_str())
        .unwrap_or("");
    ThreadLogEvent {
        event_type: row["kind"].as_str().unwrap_or("event").into(),
        time: time.into(),
        summary: summary.chars().take(640).collect::<String>().into(),
        raw: serde_json::to_string_pretty(data)
            .unwrap_or_default()
            .into(),
        order,
        round,
    }
}
fn count(value: &Value) -> String {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| {
            if n >= 1000.0 {
                format!("{:.1}k", n / 1000.0)
            } else {
                format!("{n:.0}")
            }
        })
        .unwrap_or("—".into())
}
fn duration(value: &Value) -> String {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| format!("{n:.1}s"))
        .unwrap_or("—".into())
}
fn metrics(s: &State, username: &str, rounds: i64, events: i64) -> Vec<LogMetric> {
    let v = &s.overview;
    let status = match v["status"].as_str().unwrap_or("") {
        "finished" | "completed" => "已完成",
        "running" => "运行中",
        "queued" | "waiting" => "排队中",
        "error" | "failed" => "失败",
        "cancelled" => "已取消",
        _ => "—",
    };
    let rate = |key: &str| {
        if v[key].is_number() {
            format!(
                "{} token/秒",
                v[key]
                    .as_f64()
                    .map(|n| if n >= 1000.0 {
                        format!("{:.1}k", n / 1000.0)
                    } else {
                        format!("{n:.1}")
                    })
                    .unwrap_or_default()
            )
        } else {
            "—".into()
        }
    };
    let values = [
        ("用户名称", username.to_string(), "log-user"),
        ("智能体", s.agent.clone(), "robot"),
        ("状态", status.into(), "log-info"),
        ("耗时", duration(&v["elapsed_s"]), "status-clock"),
        ("用户轮次", rounds.to_string(), "rotate-right"),
        ("工具调用", count(&v["tool_calls"]), "screwdriver-wrench"),
        ("额度消耗", count(&v["model_request_count"]), "coins"),
        ("Token 消耗", count(&v["consumed_tokens"]), "bolt"),
        (
            "首字延迟",
            duration(
                &v["ttft_ms"]
                    .as_f64()
                    .map(|n| Value::from(n / 1000.0))
                    .unwrap_or(Value::Null),
            ),
            "bolt",
        ),
        ("预填充速度", rate("prefill_speed_tps"), "arrow-up"),
        ("生成速度", rate("decode_speed_tps"), "log-down"),
        ("事件", events.to_string(), "list-check"),
        ("线程 ID", s.session.clone(), "log-fingerprint"),
    ];
    values
        .into_iter()
        .map(|(label, value, icon)| LogMetric {
            label: label.into(),
            value: value.into(),
            icon: slint::Image::load_from_svg_data(icon_bytes(icon)).unwrap_or_default(),
        })
        .collect()
}
fn icon_bytes(name: &str) -> &'static [u8] {
    match name {
        "log-user" => include_bytes!("../assets/icons/log-user.svg"),
        "robot" => include_bytes!("../assets/icons/robot.svg"),
        "log-info" => include_bytes!("../assets/icons/log-info.svg"),
        "status-clock" => include_bytes!("../assets/icons/status-clock.svg"),
        "rotate-right" => include_bytes!("../assets/icons/rotate-right.svg"),
        "screwdriver-wrench" => include_bytes!("../assets/icons/screwdriver-wrench.svg"),
        "coins" => include_bytes!("../assets/icons/coins.svg"),
        "arrow-up" => include_bytes!("../assets/icons/arrow-up.svg"),
        "log-down" => include_bytes!("../assets/icons/log-down.svg"),
        "list-check" => include_bytes!("../assets/icons/list-check.svg"),
        "log-fingerprint" => include_bytes!("../assets/icons/log-fingerprint.svg"),
        _ => include_bytes!("../assets/icons/bolt.svg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn durable_log_pages_and_export_preserve_public_history() -> anyhow::Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "native-log-fixture-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let cleanup = Cleanup(directory.clone());
        let config = directory.join("runtime/config");
        std::fs::create_dir_all(&config)?;
        std::fs::write(config.join("wunder.yaml"), "{}\n")?;
        std::fs::write(
            config.join("desktop.settings.json"),
            r#"{"lan_mesh":{"enabled":false},"llm":{"default":"","models":{}}}"#,
        )?;
        let mut args = wunder_desktop::args::DesktopArgs::native_defaults();
        args.temp_root = Some(directory.join("runtime").canonicalize()?);
        args.workspace = Some(directory.join("workspace"));
        let api = NativeDesktop::start_with_args(args)?;
        let session = api.create_session(None)?;
        let mut latest = String::new();
        for n in 0..52 {
            let row = api.state().storage.accept_thread_turn(
                api.user_id(),
                &session.id,
                &json!({"content":format!("fixture input {n}")}),
            )?;
            latest = row["turn_id"].as_str().unwrap().into();
        }
        for n in 0..102 {
            api.state().storage.append_thread_item(api.user_id(), &json!({"session_id":session.id,"turn_id":latest,"item_id":format!("fixture-item-{n}"),"kind":"progress","visibility":"user","summary":format!("fixture event {n}")}))?;
        }
        api.state().storage.append_thread_item(api.user_id(), &json!({"session_id":session.id,"turn_id":latest,"item_id":"fixture-private","kind":"context","visibility":"model_internal","content":"fixture hidden"}))?;
        let first = api.thread_log_page(&session.id, None)?;
        assert_eq!(first.turn_total, 52);
        assert_eq!(first.turns.len(), 50);
        assert!(first.has_more);
        let second = api.thread_log_page(&session.id, first.next_before)?;
        assert_eq!(second.turns.len(), 2);
        assert!(!second.has_more);
        let items = api.thread_log_turn(&session.id, &latest, -1)?;
        assert_eq!(items.items.len(), 100);
        assert!(items.has_more);
        let tail = api.thread_log_turn(&session.id, &latest, items.next_after)?;
        assert_eq!(tail.items.len(), 3);
        assert!(!tail.has_more);
        let projected = project(
            &json!({"kind":"progress","updated_time":1234.5,"payload":{"summary":"fixture summary"}}),
            2,
            3,
        );
        assert_eq!(projected.time, "1234.500");
        assert_eq!(projected.round, 2);
        assert_eq!(projected.summary, "fixture summary");
        let target = directory.join("export.json");
        api.export_thread_log(&session.id, &target)?;
        let exported: Value = serde_json::from_slice(&std::fs::read(&target)?)?;
        assert_eq!(exported["turns"].as_array().unwrap().len(), 52);
        assert_eq!(exported["turns"][0]["items"].as_array().unwrap().len(), 103);
        assert!(!exported.to_string().contains("fixture hidden"));
        drop(api);
        drop(cleanup);
        Ok(())
    }
}
