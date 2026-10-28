//! Cloud thread controller.
//!
//! Cloud threads are a data source for the ordinary desktop transcript, not a
//! second chat product. This module owns only the bounded catalogue,
//! page/watch lifecycle and command hand-off. `TimelineView` and `Composer`
//! remain the single reader and writer surface for every thread.
use crate::{CloudThreadCard, MainWindow, ModelCard};
use serde_json::Value;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use wunder_desktop::{
    native::{NativeCloudAgent, NativeCloudThread},
    NativeDesktop,
};
use wunder_server::{
    cloud::CloudThreadEvent,
    stable_core::thread_view::{ChatViewRow, ThreadViewPage},
};

thread_local! {
    static CATALOG: RefCell<(Vec<NativeCloudAgent>, Vec<NativeCloudThread>)> = RefCell::new((Vec::new(), Vec::new()));
    static PENDING_SELECTION: RefCell<Option<String>> = const { RefCell::new(None) };
    static WATCH: RefCell<Option<wunder_server::cloud::CloudThreadCancellation>> = const { RefCell::new(None) };
    static MESSAGES: RefCell<Vec<ChatViewRow>> = const { RefCell::new(Vec::new()) };
    static PROCESS_ITEMS: RefCell<Vec<Value>> = const { RefCell::new(Vec::new()) };
    static LOADED_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static DRAFTS: RefCell<std::collections::HashMap<String, String>> = RefCell::new(std::collections::HashMap::new());
    static ACCOUNT_KEY: RefCell<(bool, String, String)> = RefCell::new((false, String::new(), String::new()));
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static ACCOUNT_GENERATION: AtomicU64 = AtomicU64::new(0);
static CATALOG_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(crate) fn account_status(app: &MainWindow, logged_in: bool, server: &str, username: &str) {
    let key = (logged_in, server.to_string(), username.to_string());
    let changed = ACCOUNT_KEY.with(|slot| {
        let changed = *slot.borrow() != key;
        *slot.borrow_mut() = key;
        changed
    });
    if changed {
        reset_account(app);
    }
}

pub(crate) fn reset_account(app: &MainWindow) {
    let was_cloud = app.get_cloud_chat_active();
    leave(app);
    if was_cloud {
        crate::native_chat::prepare_external_timeline(app);
    }
    ACCOUNT_GENERATION.fetch_add(1, Ordering::AcqRel);
    CATALOG_GENERATION.fetch_add(1, Ordering::AcqRel);
    CATALOG.with(|slot| *slot.borrow_mut() = (Vec::new(), Vec::new()));
    MESSAGES.with(|slot| slot.borrow_mut().clear());
    DRAFTS.with(|slot| slot.borrow_mut().clear());
    LOADED_ID.with(|slot| slot.borrow_mut().clear());
    PENDING_SELECTION.with(|slot| *slot.borrow_mut() = None);
    app.set_cloud_active_thread_id("".into());
    app.set_cloud_thread_index(0);
    app.set_cloud_thread_rows(ModelRc::default());
    app.set_cloud_thread_models(ModelRc::default());
    app.set_cloud_greeting("".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(round: i64, revision: i64) -> ChatViewRow {
        ChatViewRow {
            root_turn_id: format!("fixture-{round}"),
            user_round: round,
            revision,
            ..Default::default()
        }
    }

    #[test]
    fn cloud_live_reasoning_rejects_stale_rounds_and_old_durable_snapshots() {
        let mut current = row(1, 1);
        current.status = "running".into();
        current.process_count = Some(1);
        current.execution_turn_id = "fixture-execution".into();
        current.assistant.item_id = "fixture-execution:text-1".into();
        let mut rows = vec![current.clone()];
        assert!(apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-2",
            "field":"reasoning","offset":0,"text":"Fixture current"})
        ));
        assert!(!apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-1",
            "field":"reasoning","offset":15,"text":" stale"})
        ));
        current.revision = 2;
        merge_messages(&mut rows, &[current], false);
        assert_eq!(rows[0].assistant.reasoning, "Fixture current");
        assert_eq!(rows[0].reasoning_item_id, "fixture-execution:text-2");
        assert_eq!(rows[0].process_count, Some(2));
        assert!(apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-2",
            "field":"reasoning","offset":15,"text":" continued"})
        ));
        assert_eq!(rows[0].assistant.reasoning, "Fixture current continued");
    }

    #[test]
    fn cloud_process_overlay_keeps_running_tools_visible_and_terminal_states_monotonic() {
        PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
        MESSAGES.with(|messages| {
            *messages.borrow_mut() = vec![ChatViewRow {
                root_turn_id: "fixture-turn".into(),
                execution_turn_id: "fixture-turn".into(),
                status: "running".into(),
                ..Default::default()
            }]
        });
        let tool = serde_json::json!({"turn_id":"fixture-turn","root_turn_id":"fixture-turn",
            "item_id":"fixture-turn:tool-first","kind":"tool_call","status":"running",
            "payload":{"tool":"read_file","tool_call_id":"fixture-first","process_seq":2}});
        assert!(apply_process_item(tool.clone()));
        let mut row = crate::TimelineRow {
            process_idx: 1,
            process_root: "fixture-turn".into(),
            process_entries: ModelRc::new(VecModel::<crate::ProcessEntry>::default()),
            ..Default::default()
        };
        merge_live_process(&mut row);
        assert_eq!(row.process_entries.row_count(), 1);
        assert_eq!(
            row.process_entries.row_data(0).unwrap().status_kind,
            crate::timeline::STATUS_RUNNING
        );
        let mut completed = tool.clone();
        completed["status"] = serde_json::json!("completed");
        assert!(apply_process_item(completed));
        assert!(!apply_process_item(tool));
        merge_live_process(&mut row);
        assert_eq!(row.process_entries.row_count(), 1);
        assert_eq!(
            row.process_entries.row_data(0).unwrap().status_kind,
            crate::timeline::STATUS_DONE
        );
        PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
        MESSAGES.with(|messages| messages.borrow_mut().clear());
    }

    #[test]
    fn cloud_continuation_process_items_bind_to_the_user_root() {
        PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
        MESSAGES.with(|messages| {
            *messages.borrow_mut() = vec![ChatViewRow {
                root_turn_id: "fixture-root".into(),
                execution_turn_id: "fixture-continuation".into(),
                status: "running".into(),
                ..Default::default()
            }]
        });
        assert!(apply_process_item(
            serde_json::json!({"turn_id":"fixture-continuation",
            "root_turn_id":"fixture-continuation","item_id":"fixture-continuation:tool-first",
            "kind":"tool_call","status":"running","payload":{"tool":"read_file","model_round":1,"process_seq":1}})
        ));
        let mut row = crate::TimelineRow {
            process_idx: 1,
            process_root: "fixture-root".into(),
            process_entries: ModelRc::new(VecModel::<crate::ProcessEntry>::default()),
            ..Default::default()
        };
        merge_live_process(&mut row);
        assert_eq!(row.process_entries.row_count(), 1);
        PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
        MESSAGES.with(|messages| messages.borrow_mut().clear());
    }

    #[test]
    fn cloud_gap_does_not_clear_live_state_or_reopen_tool_phase() {
        let mut current = row(1, 1);
        current.execution_turn_id = "fixture-execution".into();
        current.status = "running".into();
        current.assistant.item_id = "fixture-execution:text-1".into();
        current.assistant.reasoning = "Fixture previous".into();
        current.reasoning_item_id = current.assistant.item_id.clone();
        let mut rows = vec![current];
        assert!(!apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-2",
            "field":"reasoning","offset":5,"text":"gap"})
        ));
        assert_eq!(rows[0].assistant.reasoning, "Fixture previous");
        assert!(apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-2",
            "field":"reasoning","offset":0,"text":"Fixture current"})
        ));
        rows[0].process_activity.kind = "tool".into();
        assert!(apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-2",
            "field":"reasoning","offset":15,"text":" continued"})
        ));
        assert_eq!(rows[0].process_activity.kind, "tool");
        assert!(apply_tail_to(
            &mut rows,
            &serde_json::json!({"item_id":"fixture-execution:text-3",
            "field":"reasoning","offset":0,"text":"Fixture next"})
        ));
        assert_eq!(rows[0].process_activity.kind, "thinking");
    }

    #[test]
    fn cloud_overlay_restores_start_order_and_respects_history_window() {
        PROCESS_ITEMS.with(|items| *items.borrow_mut() = (1..=2).map(|order| serde_json::json!({
            "turn_id":"fixture-turn", "root_turn_id":"fixture-turn",
            "item_id":format!("fixture-turn:tool-{order}"), "kind":"tool_call", "status":"completed",
            "payload":{"tool":"read_file","tool_call_id":format!("fixture-{order}"),"process_seq":order}
        })).collect());
        let mut row = crate::TimelineRow {
            process_idx: 1,
            process_root: "fixture-turn".into(),
            process_entries: ModelRc::new(VecModel::<crate::ProcessEntry>::default()),
            ..Default::default()
        };
        merge_live_process(&mut row);
        let expected = row
            .process_entries
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>();
        let mut reversed = row.process_entries.iter().collect::<Vec<_>>();
        reversed.reverse();
        row.process_entries = ModelRc::new(VecModel::from(reversed));
        row.process_newer = true;
        merge_live_process(&mut row);
        assert_ne!(
            row.process_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            expected
        );
        row.process_newer = false;
        merge_live_process(&mut row);
        assert_eq!(
            row.process_entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            expected
        );
        PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
    }

    #[test]
    fn cloud_replay_keeps_order_rejects_old_revisions_and_bounds_history() {
        let mut rows = (1..=20).map(|round| row(round, 2)).collect::<Vec<_>>();
        merge_messages(&mut rows, &[row(19, 1), row(21, 1), row(20, 3)], false);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows.first().unwrap().user_round, 2);
        assert_eq!(rows.last().unwrap().user_round, 21);
        assert_eq!(
            rows.iter()
                .find(|row| row.user_round == 19)
                .unwrap()
                .revision,
            2
        );
        assert_eq!(
            rows.iter()
                .find(|row| row.user_round == 20)
                .unwrap()
                .revision,
            3
        );
        merge_messages(&mut rows, &[row(40, 1)], true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].user_round, 40);
    }
}

fn invalidate() -> u64 {
    PROCESS_ITEMS.with(|items| items.borrow_mut().clear());
    WATCH.with(|watch| {
        if let Some(cancel) = watch.borrow_mut().take() {
            cancel.cancel();
        }
    });
    GENERATION.fetch_add(1, Ordering::AcqRel) + 1
}

fn active(generation: u64) -> bool {
    GENERATION.load(Ordering::Acquire) == generation
}

pub(crate) fn leave(app: &MainWindow) {
    save_draft(app);
    invalidate();
    if app.get_cloud_chat_active() {
        app.set_busy(false);
        app.set_stopping(false);
        app.set_draft("".into());
    }
    app.set_cloud_chat_active(false);
    app.set_cloud_thread_busy(false);
    LOADED_ID.with(|slot| slot.borrow_mut().clear());
}

fn save_draft(app: &MainWindow) {
    LOADED_ID.with(|id| {
        if id.borrow().is_empty() {
            return;
        }
        DRAFTS.with(|slot| {
            let mut drafts = slot.borrow_mut();
            if drafts.len() >= 100 && !drafts.contains_key(id.borrow().as_str()) {
                drafts.clear();
            }
            drafts.insert(id.borrow().clone(), app.get_draft().to_string());
        });
    });
}

pub(crate) fn is_active(app: &MainWindow) -> bool {
    app.get_cloud_chat_active() && !selected_id(app).is_empty()
}

pub(crate) fn selected_id(app: &MainWindow) -> String {
    app.get_cloud_active_thread_id().to_string()
}

pub(crate) fn final_message_id(root: &str) -> String {
    MESSAGES.with(|slot| {
        slot.borrow()
            .iter()
            .find(|row| row.root_turn_id == root)
            .map(|row| row.assistant.item_id.clone())
            .unwrap_or_default()
    })
}

fn selected_agent() -> String {
    CATALOG.with(|catalog| {
        catalog
            .borrow()
            .0
            .first()
            .map(|agent| agent.id.clone())
            .unwrap_or_default()
    })
}

fn merge_messages(messages: &mut Vec<ChatViewRow>, incoming: &[ChatViewRow], replace: bool) {
    if replace {
        messages.clear();
    }
    for row in incoming {
        if let Some(existing) = messages
            .iter_mut()
            .find(|old| old.root_turn_id == row.root_turn_id)
        {
            if row.revision >= existing.revision {
                let mut next = row.clone();
                let same_execution = next.execution_turn_id == existing.execution_turn_id;
                if same_execution
                    && matches!(next.status.as_str(), "accepted" | "running")
                    && (model_round(&existing.assistant.item_id)
                        > model_round(&next.assistant.item_id)
                        || (existing.assistant.item_id == next.assistant.item_id
                            && (existing.assistant.reasoning_offset
                                > next.assistant.reasoning_offset
                                || existing.assistant.content_offset
                                    > next.assistant.content_offset)))
                {
                    next.assistant.item_id = existing.assistant.item_id.clone();
                    next.assistant.content = existing.assistant.content.clone();
                    next.assistant.reasoning = existing.assistant.reasoning.clone();
                    next.assistant.content_offset = existing.assistant.content_offset;
                    next.assistant.reasoning_offset = existing.assistant.reasoning_offset;
                    next.assistant.tail = existing.assistant.tail.clone();
                    next.assistant.tail_offset = existing.assistant.tail_offset;
                    next.reasoning_item_id = existing.reasoning_item_id.clone();
                    next.process_activity = existing.process_activity.clone();
                    next.reasoning_count = next.reasoning_count.max(existing.reasoning_count);
                    next.process_count = match (next.process_count, existing.process_count) {
                        (Some(incoming), Some(current)) => Some(incoming.max(current)),
                        (incoming, current) => incoming.or(current),
                    };
                }
                *existing = next;
            }
        } else {
            messages.push(row.clone());
        }
    }
    messages.sort_by(|left, right| {
        left.user_round
            .cmp(&right.user_round)
            .then_with(|| left.created_time.total_cmp(&right.created_time))
            .then_with(|| left.root_turn_id.cmp(&right.root_turn_id))
    });
    if messages.len() > 20 {
        let remove = messages.len() - 20;
        messages.drain(..remove);
    }
}

fn model_round(id: &str) -> i64 {
    id.rsplit_once(":text-")
        .and_then(|(_, round)| round.parse().ok())
        .unwrap_or(-1)
}

fn apply_tail_to(messages: &mut [ChatViewRow], tail: &Value) -> bool {
    let id = tail["item_id"].as_str().unwrap_or_default();
    let Some((execution, _)) = id.rsplit_once(":text-") else {
        return false;
    };
    let Some(row) = messages.iter_mut().find(|row| {
        let current = if row.execution_turn_id.is_empty() {
            &row.root_turn_id
        } else {
            &row.execution_turn_id
        };
        current == execution && matches!(row.status.as_str(), "accepted" | "running")
    }) else {
        return false;
    };
    let round = model_round(id);
    let next_round = row.assistant.item_id != id;
    let next_thought = row.reasoning_item_id != id;
    if round < model_round(&row.assistant.item_id) {
        return false;
    }
    let field = tail["field"].as_str().unwrap_or("content");
    let text = tail["text"].as_str().unwrap_or_default();
    let offset = tail["offset"].as_u64().unwrap_or(u64::MAX);
    let reasoning = field == "reasoning";
    if !matches!(field, "content" | "reasoning") || text.is_empty() {
        return false;
    }
    let expected = if row.assistant.item_id != id || (reasoning && row.reasoning_item_id != id) {
        0
    } else if reasoning {
        row.assistant.reasoning_offset
    } else {
        row.assistant.content_offset
    };
    if offset != expected as u64 {
        return false;
    }
    if row.assistant.item_id != id {
        row.assistant = Default::default();
        row.assistant.item_id = id.into();
    }
    if reasoning && row.reasoning_item_id != id {
        row.assistant.reasoning.clear();
        row.assistant.reasoning_offset = 0;
    }
    let (body, cursor) = if reasoning {
        (
            &mut row.assistant.reasoning,
            &mut row.assistant.reasoning_offset,
        )
    } else {
        (
            &mut row.assistant.content,
            &mut row.assistant.content_offset,
        )
    };
    if offset != *cursor as u64 || text.is_empty() {
        return false;
    }
    body.push_str(text);
    *cursor = cursor.saturating_add(text.encode_utf16().count());
    if body.len() > 32768 {
        let mut start = body.len() - 32768;
        while !body.is_char_boundary(start) {
            start += 1;
        }
        body.drain(..start);
        body.insert_str(0, "…\n");
    }
    if next_round
        || !matches!(row.process_activity.kind.as_str(), "tool" | "responding")
        || (!reasoning && row.process_activity.kind != "tool")
    {
        row.process_activity.kind = if reasoning { "thinking" } else { "responding" }.into();
        row.process_activity.item_id = id.into();
    }
    if reasoning {
        row.reasoning_item_id = id.into();
        row.reasoning_count = row.reasoning_count.max(1);
        if next_thought {
            if let Some(count) = row.process_count.as_mut() {
                *count = count.saturating_add(1);
            }
        }
    }
    true
}

fn apply_tail(tail: Value) -> bool {
    MESSAGES.with(|slot| {
        let mut messages = slot.borrow_mut();
        apply_tail_to(&mut messages, &tail)
    })
}

fn apply_process_item(mut item: Value) -> bool {
    if !matches!(
        item["kind"].as_str(),
        Some("assistant_message" | "tool_call" | "compaction")
    ) || item["item_id"].as_str().is_none_or(str::is_empty)
    {
        return false;
    }
    let execution = item["turn_id"].as_str().unwrap_or_default().to_owned();
    let root = MESSAGES.with(|messages| {
        messages
            .borrow()
            .iter()
            .find(|row| {
                let current = if row.execution_turn_id.is_empty() {
                    &row.root_turn_id
                } else {
                    &row.execution_turn_id
                };
                current == &execution && matches!(row.status.as_str(), "accepted" | "running")
            })
            .map(|row| row.root_turn_id.clone())
    });
    let Some(root) = root else {
        return false;
    };
    item["root_turn_id"] = serde_json::json!(root);
    if PROCESS_ITEMS.with(|items| {
        items.borrow().iter().any(|previous| {
            previous["item_id"] == item["item_id"]
                && previous["status"] != "running"
                && item["status"] == "running"
        })
    }) {
        return false;
    }
    if item["kind"] == "tool_call" && item["status"] == "running" {
        MESSAGES.with(|messages| {
            for row in messages.borrow_mut().iter_mut().filter(|row| {
                (if row.execution_turn_id.is_empty() {
                    &row.root_turn_id
                } else {
                    &row.execution_turn_id
                }) == &execution
                    && item["payload"]["model_round"].as_i64().unwrap_or(0)
                        >= model_round(&row.assistant.item_id)
            }) {
                row.process_activity.kind = "tool".into();
                row.process_activity.item_id = item["item_id"].as_str().unwrap_or_default().into();
            }
        });
    }
    PROCESS_ITEMS.with(|slot| {
        let mut items = slot.borrow_mut();
        if let Some(previous) = items
            .iter_mut()
            .find(|previous| previous["item_id"] == item["item_id"])
        {
            if previous["status"] != "running" && item["status"] == "running" {
                return;
            }
            *previous = item;
        } else {
            if items.len() >= 100 {
                items.remove(0);
            }
            items.push(item);
        }
    });
    true
}

pub(crate) fn merge_live_process(row: &mut crate::TimelineRow) {
    if row.process_newer {
        return;
    }
    let items = PROCESS_ITEMS.with(|slot| {
        slot.borrow()
            .iter()
            .filter(|item| {
                item["root_turn_id"]
                    .as_str()
                    .or_else(|| item["turn_id"].as_str())
                    == Some(row.process_root.as_str())
            })
            .cloned()
            .collect::<Vec<_>>()
    });
    if items.is_empty() {
        return;
    }
    let final_id = final_message_id(row.process_root.as_str());
    let page = wunder_desktop::native::project_native_process_page(
        &serde_json::json!({"items":items}),
        &final_id,
    );
    let mut overlay = crate::TimelineRow {
        process_idx: row.process_idx,
        ..Default::default()
    };
    crate::chat_process::apply_page(&mut overlay, page, 2);
    let mut entries = row.process_entries.iter().collect::<Vec<_>>();
    for mut entry in overlay.process_entries.iter() {
        if let Some(previous) = entries.iter_mut().find(|previous| previous.id == entry.id) {
            if previous.status_kind != crate::timeline::STATUS_RUNNING
                && entry.status_kind == crate::timeline::STATUS_RUNNING
            {
                continue;
            }
            entry.open = previous.open;
            entry.payload = previous.payload;
            *previous = entry;
        } else {
            if entries.len() >= 201 {
                entries.remove(0);
            }
            entry.payload = -1 - row.process_idx * 1024 - entries.len() as i32;
            entries.push(entry);
        }
    }
    let ranks = overlay
        .process_entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.to_string(), index))
        .collect::<std::collections::HashMap<_, _>>();
    let mut ordered = entries
        .iter()
        .filter(|entry| ranks.contains_key(entry.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    ordered.sort_by_key(|entry| ranks[entry.id.as_str()]);
    let mut ordered = ordered.into_iter();
    for entry in &mut entries {
        if ranks.contains_key(entry.id.as_str()) {
            *entry = ordered.next().unwrap();
        }
    }
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.payload = -1 - row.process_idx * 1024 - index as i32;
    }
    if let Some(model) = row
        .process_entries
        .as_any()
        .downcast_ref::<VecModel<crate::ProcessEntry>>()
    {
        if model.row_count() == entries.len() {
            for (index, entry) in entries.into_iter().enumerate() {
                if model.row_data(index).as_ref() != Some(&entry) {
                    model.set_row_data(index, entry);
                }
            }
        } else {
            model.set_vec(entries);
        }
    } else {
        row.process_entries = ModelRc::new(VecModel::from(entries));
    }
}

fn apply_live_models(app: &MainWindow) {
    let model = app.get_timeline();
    for (index, mut row) in model.iter().enumerate() {
        if row.kind != crate::timeline::KIND_PROCESS {
            continue;
        }
        merge_live_process(&mut row);
        if row.process_entries.row_count() > 0 {
            row.visible = true;
            let count = MESSAGES.with(|messages| {
                messages
                    .borrow()
                    .iter()
                    .find(|message| message.root_turn_id == row.process_root.as_str())
                    .map(|message| {
                        message
                            .process_count
                            .unwrap_or(message.workflow_count + message.reasoning_count)
                    })
                    .unwrap_or(0)
            });
            row.text = crate::timeline_text::process_label(
                count
                    .max(row.process_entries.row_count() as i64)
                    .min(i32::MAX as i64) as i32,
            )
            .into();
            model.set_row_data(index, row);
        }
    }
}

fn publish_page(app: &MainWindow, page: &ThreadViewPage, replace: bool) {
    // A degraded bootstrap is not an empty thread. Keep the committed view.
    if page.degraded {
        app.set_status("云端线程暂时读取缓慢，正在恢复连接".into());
        return;
    }
    let (rows, changed_roots) = MESSAGES.with(|slot| {
        let mut messages = slot.borrow_mut();
        let changed_roots = page
            .chat
            .iter()
            .filter(|incoming| {
                messages
                    .iter()
                    .find(|previous| previous.root_turn_id == incoming.root_turn_id)
                    .is_none_or(|previous| previous.revision < incoming.revision)
            })
            .map(|incoming| incoming.root_turn_id.clone())
            .collect::<std::collections::HashSet<_>>();
        merge_messages(&mut messages, &page.chat, replace);
        (
            crate::native_chat::project_cloud_history(&messages, app.get_timeline()),
            changed_roots,
        )
    });
    crate::native_chat::replace_external_timeline(app, rows, replace);
    apply_live_models(app);
    for (index, row) in app.get_timeline().iter().enumerate() {
        if row.kind == crate::timeline::KIND_PROCESS
            && row.process_paged
            && !row.process_newer
            && changed_roots.contains(row.process_root.as_str())
        {
            app.invoke_timeline_process_page(index as i32, 2);
        }
    }
    app.set_busy(matches!(
        page.runtime_status.as_str(),
        "running" | "queued" | "pending" | "waiting_approval"
    ));
    app.set_stopping(false);
    app.set_cloud_thread_status(page.runtime_status.clone().into());
}

fn load(app: &MainWindow, api: Arc<NativeDesktop>) {
    let id = selected_id(app);
    let generation = invalidate();
    let changed = LOADED_ID.with(|slot| *slot.borrow() != id);
    if changed {
        save_draft(app);
        crate::native_chat::prepare_external_timeline(app);
        MESSAGES.with(|slot| slot.borrow_mut().clear());
        LOADED_ID.with(|slot| *slot.borrow_mut() = id.clone());
        app.set_draft(
            DRAFTS
                .with(|slot| slot.borrow().get(&id).cloned().unwrap_or_default())
                .into(),
        );
        app.set_selected_model_key("".into());
        app.set_selected_agent_model("".into());
        app.set_reasoning_effort("default".into());
        app.set_context_known(false);
        app.set_cloud_greeting("".into());
    }
    app.set_cloud_thread_busy(!id.is_empty());
    if id.is_empty() {
        crate::native_chat::replace_external_timeline(app, Vec::new(), true);
        return;
    }
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let mut failures = 0u32;
        let result = loop {
            if !active(generation) {
                return;
            }
            let result = api.cloud_thread_bootstrap(&id);
            if result
                .as_ref()
                .err()
                .is_none_or(|error| !wunder_server::cloud::retryable_cloud_read(error))
            {
                break result;
            }
            failures = failures.saturating_add(1);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if active(generation) {
                    app.set_cloud_thread_busy(false);
                    app.set_cloud_thread_status("reconnecting".into());
                    app.set_status("云端暂时不可达，正在自动重连".into());
                }
            });
            // One reader per selection, cancellable bounded backoff. A switch
            // must not retain sleeping workers or accept an old bootstrap.
            for _ in 0..((1u64 << failures.min(5)).min(30) * 10) {
                if !active(generation) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        };
        let (view, mut watch) = match result {
            Ok(view) => {
                let watch = api.cloud_thread_watch(&id, view.page.view_seq);
                (view, watch)
            }
            Err(error) => {
                let _ = weak.upgrade_in_event_loop(move |app| {
                    if active(generation) {
                        app.set_cloud_thread_busy(false);
                        app.set_status(format!("无法读取云端线程：{error}").into());
                    }
                });
                return;
            }
        };
        let token = watch.cancel_token();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                token.cancel();
                return;
            }
            WATCH.with(|watch| *watch.borrow_mut() = Some(token));
            app.set_cloud_greeting(
                CATALOG
                    .with(|catalog| {
                        catalog
                            .borrow()
                            .0
                            .iter()
                            .find(|agent| {
                                agent.id == view.agent_id
                                    || (view.agent_id.is_empty() && agent.id == "__default__")
                            })
                            .map(|agent| agent.description.trim().to_string())
                            .unwrap_or_default()
                    })
                    .into(),
            );
            app.set_selected_model_key(view.model_key.into());
            app.set_reasoning_effort(view.reasoning_effort.into());
            publish_page(&app, &view.page, changed);
            app.set_cloud_thread_busy(false);
        });
        // The command acknowledgement only means the turn was admitted.  A
        // fast cloud run can finish between bootstrap and the websocket watch
        // subscription, leaving the initial `running` projection stranded
        // until the user changes threads.  Keep one bounded, cancellable
        // reconciliation loop alongside the live stream so terminal state is
        // visible without navigation.
        let poll_api = api.clone();
        let poll_weak = weak.clone();
        let poll_id = id.clone();
        std::thread::spawn(move || {
            for _ in 0..400 {
                std::thread::sleep(std::time::Duration::from_millis(750));
                if !active(generation) {
                    return;
                }
                let Ok(view) = poll_api.cloud_thread_bootstrap(&poll_id) else {
                    continue;
                };
                let terminal = matches!(
                    view.page.runtime_status.as_str(),
                    "completed" | "failed" | "cancelled" | "canceled" | "timeout"
                );
                let _ = poll_weak.upgrade_in_event_loop(move |app| {
                    if active(generation) && is_active(&app) {
                        publish_page(&app, &view.page, false);
                        if terminal {
                            app.set_cloud_thread_busy(false);
                            app.set_stopping(false);
                        }
                    }
                });
                if terminal {
                    return;
                }
            }
        });
        while active(generation) {
            let Some(event) = watch.blocking_recv() else {
                break;
            };
            std::thread::sleep(std::time::Duration::from_millis(25));
            let mut events = vec![event];
            while events.len() < 32 {
                match watch.try_recv() {
                    Some(event) => events.push(event),
                    None => break,
                }
            }
            let (applied, ack) = std::sync::mpsc::sync_channel(1);
            let weak_event = weak.clone();
            let event_api = api.clone();
            let _ = weak_event.upgrade_in_event_loop(move |app| {
                if !active(generation) || !is_active(&app) {
                    let _ = applied.send(());
                    return;
                }
                let mut live_changed = false;
                let mut process_changed = false;
                for event in events {
                    match event {
                        CloudThreadEvent::Change(change) => {
                            let page = ThreadViewPage {
                                runtime_status: change.runtime_status,
                                chat: change.rows,
                                ..ThreadViewPage::default()
                            };
                            publish_page(&app, &page, false);
                        }
                        CloudThreadEvent::Connection(status) => {
                            app.set_cloud_thread_status(status.into());
                        }
                        CloudThreadEvent::ProcessItem(item) => {
                            let changed = apply_process_item(item);
                            live_changed |= changed;
                            process_changed |= changed;
                        }
                        CloudThreadEvent::Tail(tail) => live_changed |= apply_tail(tail),
                        CloudThreadEvent::Reload => {
                            load(&app, event_api.clone());
                            live_changed = false;
                            break;
                        }
                    }
                }
                if live_changed {
                    let rows = MESSAGES.with(|messages| {
                        crate::native_chat::project_cloud_history(
                            &messages.borrow(),
                            app.get_timeline(),
                        )
                    });
                    crate::native_chat::replace_external_timeline(&app, rows, false);
                    if process_changed {
                        apply_live_models(&app);
                    }
                }
                let _ = applied.send(());
            });
            while active(generation) {
                match ack.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }
    });
}

fn refresh(app: &MainWindow, api: Arc<NativeDesktop>, preserve: Option<String>) {
    if app.get_cloud_thread_busy() {
        return;
    }
    app.set_cloud_thread_busy(true);
    let request = CATALOG_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let selection_generation = GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api
            .cloud_agents()
            .and_then(|agents| api.cloud_threads(0).map(|threads| (agents, threads)));
        let models = api.cloud_thread_models();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if CATALOG_GENERATION.load(Ordering::Acquire) != request || !app.get_cloud_logged_in() {
                return;
            }
            app.set_cloud_thread_busy(false);
            match result {
                Ok((agents, threads)) => {
                    if let Ok(models) = models {
                        app.set_cloud_thread_models(ModelRc::new(VecModel::from(
                            models
                                .into_iter()
                                .map(|key| ModelCard {
                                    key: key.clone().into(),
                                    model: key.into(),
                                    provider: "cloud".into(),
                                    is_cloud: true,
                                    ..Default::default()
                                })
                                .collect::<Vec<_>>(),
                        )));
                    }
                    let pending = PENDING_SELECTION.with(|slot| slot.borrow_mut().take());
                    let current = selected_id(&app);
                    let preserve = if active(selection_generation) {
                        preserve
                    } else {
                        Some(current)
                    };
                    let wanted = pending
                        .or(preserve)
                        .or_else(|| app.get_cloud_chat_active().then(|| selected_id(&app)));
                    let selected = wanted
                        .as_deref()
                        .and_then(|id| threads.iter().position(|thread| thread.id == id))
                        .map(|index| index as i32 + 1)
                        .unwrap_or(0);
                    app.set_cloud_thread_rows(ModelRc::new(VecModel::from(
                        threads
                            .iter()
                            .map(|thread| CloudThreadCard {
                                id: thread.id.clone().into(),
                                title: thread.title.clone().into(),
                                status: thread.status.clone().into(),
                            })
                            .collect::<Vec<_>>(),
                    )));
                    CATALOG.with(|catalog| *catalog.borrow_mut() = (agents, threads));
                    app.set_cloud_thread_index(selected);
                    // Refreshing another row never changes the active thread.
                    if app.get_cloud_chat_active() {
                        app.set_cloud_active_thread_id(
                            wanted.filter(|_| selected > 0).unwrap_or_default().into(),
                        );
                    }
                    crate::navigation_ui::project_sidebar(&app, None);
                    if app.get_cloud_chat_active() && active(selection_generation) {
                        load(&app, api);
                    }
                }
                Err(error) => app.set_status(format!("无法读取云端线程：{error}").into()),
            }
        });
    });
}

pub(crate) fn send(app: &MainWindow, api: Arc<NativeDesktop>, message: String) {
    let id = selected_id(app);
    if id.is_empty() || message.trim().is_empty() || app.get_cloud_thread_busy() {
        return;
    }
    // The selected thread already owns its agent. Do not substitute the first
    // agent from the account catalog when sending an existing conversation.
    let agent = String::new();
    let generation = GENERATION.load(Ordering::Acquire);
    app.set_cloud_thread_busy(true);
    app.set_busy(true);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_send(&id, &agent, &message);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            app.set_cloud_thread_busy(false);
            match result {
                Ok(_) => {
                    app.set_draft("".into());
                    load(&app, api);
                }
                Err(error) => {
                    app.set_busy(false);
                    app.set_status(format!("无法发送云端消息：{error}").into());
                    load(&app, api);
                }
            }
        });
    });
}

pub(crate) fn cancel(app: &MainWindow, api: Arc<NativeDesktop>) {
    let id = selected_id(app);
    if id.is_empty() || app.get_cloud_thread_busy() {
        return;
    }
    let generation = GENERATION.load(Ordering::Acquire);
    app.set_cloud_thread_busy(true);
    app.set_stopping(true);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_cancel(&id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            app.set_cloud_thread_busy(false);
            app.set_stopping(false);
            match result {
                Ok(()) => load(&app, api),
                Err(error) => {
                    app.set_status(format!("无法停止云端线程：{error}").into());
                    load(&app, api);
                }
            }
        });
    });
}

pub(crate) fn set_model(app: &MainWindow, api: Arc<NativeDesktop>, key: String) {
    if !is_active(app) || app.get_cloud_thread_busy() {
        return;
    }
    let id = selected_id(app);
    let generation = GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_set_model(&id, &key);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            match result {
                Ok(()) => app.set_selected_model_key(key.into()),
                Err(error) => app.set_status(format!("无法切换云端模型：{error}").into()),
            }
        });
    });
}

pub(crate) fn set_effort(app: &MainWindow, api: Arc<NativeDesktop>, effort: String) {
    if !is_active(app) || app.get_cloud_thread_busy() {
        return;
    }
    let id = selected_id(app);
    let generation = GENERATION.load(Ordering::Acquire);
    let previous = app.get_reasoning_effort();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_set_effort(&id, &effort);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !active(generation) || !is_active(&app) {
                return;
            }
            match result {
                Ok(value) => app.set_reasoning_effort(value.into()),
                Err(error) => {
                    app.set_reasoning_effort(previous);
                    app.set_status(format!("无法保存云端思考等级：{error}").into());
                }
            }
        });
    });
}

pub(crate) fn rename(app: &MainWindow, api: Arc<NativeDesktop>, id: String, title: String) {
    if id.is_empty() || title.trim().is_empty() {
        return;
    }
    let weak = app.as_weak();
    let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
    std::thread::spawn(move || {
        let result = api.cloud_thread_rename(&id, &title);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if ACCOUNT_GENERATION.load(Ordering::Acquire) != account {
                return;
            }
            match result {
                Ok(()) => refresh(&app, api, Some(selected_id(&app))),
                Err(error) => app.set_status(format!("无法重命名云端线程：{error}").into()),
            }
        });
    });
}

pub(crate) fn archive(app: &MainWindow, api: Arc<NativeDesktop>, id: String) {
    if id.is_empty() {
        return;
    }
    let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.cloud_thread_archive(&id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            if ACCOUNT_GENERATION.load(Ordering::Acquire) != account {
                return;
            }
            match result {
                Ok(()) => {
                    if is_active(&app) && selected_id(&app) == id {
                        invalidate();
                        app.set_cloud_thread_index(0);
                        app.set_cloud_active_thread_id("".into());
                        app.set_busy(false);
                        crate::native_chat::replace_external_timeline(&app, Vec::new(), true);
                    }
                    refresh(&app, api, None);
                }
                Err(error) => app.set_status(format!("无法归档云端线程：{error}").into()),
            }
        });
    });
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_copy_original(move |id| {
        let Some(app) = weak.upgrade() else { return };
        let root = id.strip_prefix("cloud-answer-").unwrap_or_default();
        let Some(message) = MESSAGES.with(|slot| {
            slot.borrow()
                .iter()
                .find(|row| row.root_turn_id == root)
                .map(|row| row.assistant.clone())
        }) else {
            return;
        };
        let thread = selected_id(&app);
        let generation = GENERATION.load(Ordering::Acquire);
        let weak = app.as_weak();
        let backend = backend.clone();
        std::thread::spawn(move || {
            let result = backend.cloud_message_text(&thread, &message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                if !active(generation) || !is_active(&app) {
                    return;
                }
                match result {
                    Ok(text) => app.invoke_copy_raw(text.into()),
                    Err(error) => app.set_status(format!("无法复制消息原文：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_rename_thread(move |id, title| {
        if let Some(app) = weak.upgrade() {
            rename(&app, backend.clone(), id.to_string(), title.to_string());
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_archive_thread(move |id| {
        if let Some(app) = weak.upgrade() {
            archive(&app, backend.clone(), id.to_string());
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_open_threads(move || {
        if let Some(app) = weak.upgrade() {
            if !app.get_cloud_chat_active() {
                crate::native_chat::prepare_external_timeline(&app);
                LOADED_ID.with(|slot| slot.borrow_mut().clear());
            }
            app.set_cloud_chat_active(true);
            refresh(&app, backend.clone(), Some(selected_id(&app)));
        }
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_new_thread(move || {
        let Some(app) = weak.upgrade() else { return };
        if app.get_cloud_thread_busy() || !app.get_cloud_logged_in() {
            return;
        }
        // The workspace add button owns its target even when the previous
        // chat and directory belonged to a local workspace.
        crate::workspace_files::set_target(
            &app,
            backend.clone(),
            crate::workspace_files::Target::Cloud,
        );
        app.set_selected_workspace(-1);
        let agent = selected_agent();
        app.set_cloud_thread_busy(true);
        let account = ACCOUNT_GENERATION.load(Ordering::Acquire);
        let weak = app.as_weak();
        let api = backend.clone();
        std::thread::spawn(move || {
            let result = api.cloud_thread_create(&agent, "");
            let _ = weak.upgrade_in_event_loop(move |app| {
                if ACCOUNT_GENERATION.load(Ordering::Acquire) != account {
                    return;
                }
                app.set_cloud_thread_busy(false);
                match result {
                    Ok(thread) => {
                        PENDING_SELECTION.with(|slot| *slot.borrow_mut() = Some(thread.id));
                        app.set_cloud_chat_active(true);
                        refresh(&app, api, None);
                    }
                    Err(error) => app.set_status(format!("无法新建云端线程：{error}").into()),
                }
            });
        });
    });
    let weak = app.as_weak();
    let backend = api.clone();
    app.on_cloud_reload_threads(move || {
        if let Some(app) = weak.upgrade() {
            refresh(&app, backend.clone(), Some(selected_id(&app)));
        }
    });
    let weak = app.as_weak();
    app.on_cloud_select_thread(move |index| {
        if let Some(app) = weak.upgrade() {
            let id = CATALOG.with(|catalog| {
                catalog
                    .borrow()
                    .1
                    .get(index.saturating_sub(1) as usize)
                    .filter(|_| index > 0)
                    .map(|thread| thread.id.clone())
                    .unwrap_or_default()
            });
            app.set_cloud_active_thread_id(id.into());
            crate::navigation_ui::project_sidebar(&app, None);
            load(&app, api.clone());
        }
    });
}
