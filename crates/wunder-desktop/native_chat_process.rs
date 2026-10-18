//! Bounded, native work-process reads using the same durable keyset pages as web.
use super::{NativeDesktop, NativeWorkflowEntry};
use anyhow::{ensure, Result};
use serde_json::Value;
use wunder_server::stable_core::thread_view::ChatViewRow;

#[derive(Debug, Clone)]
pub struct NativeProcessItem {
    pub id: String,
    pub state: String,
    pub reasoning: String,
    pub text: String,
    pub tool: Option<NativeWorkflowEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct NativeProcessPage {
    pub items: Vec<NativeProcessItem>,
    pub first: Option<i64>,
    pub last: Option<i64>,
    pub has_older: bool,
    pub has_newer: bool,
}

pub(super) fn chat_row(
    state: &wunder_server::state::AppState,
    user: &str,
    session: &str,
    root: &Value,
) -> Result<Option<ChatViewRow>> {
    let Some(round) = root["user_turn_index"].as_i64() else {
        return Ok(None);
    };
    let page = state.storage.thread_view_window(
        user,
        session,
        "chat",
        Some(round.saturating_add(1)),
        None,
        None,
        1,
    )?;
    Ok(page
        .chat
        .into_iter()
        .find(|row| row.root_turn_id == root["turn_id"].as_str().unwrap_or_default()))
}

impl NativeDesktop {
    /// Cursors are exclusive durable item indexes. Absent cursors start at the
    /// latest 100 items. All authorization remains in native storage reads.
    pub fn chat_process_page(
        &self,
        session: &str,
        root: &str,
        before: Option<i64>,
        after: Option<i64>,
    ) -> Result<NativeProcessPage> {
        ensure!(
            before.is_none() || after.is_none(),
            "ambiguous process cursor"
        );
        read_page(self.state(), self.user_id(), session, root, before, after)
    }
}

fn read_page(
    state: &wunder_server::state::AppState,
    user: &str,
    session: &str,
    root: &str,
    before: Option<i64>,
    after: Option<i64>,
) -> Result<NativeProcessPage> {
    let mut page = state
        .storage
        .get_thread_turn_page(
            user,
            session,
            root,
            before.or_else(|| after.is_none().then_some(i64::MAX)),
            after,
            100,
            false,
        )?
        .ok_or_else(|| anyhow::anyhow!("work process unavailable"))?;
    let final_id = chat_row(state, user, session, &page)?
        .map(|row| row.assistant.item_id)
        .unwrap_or_default();
    if let Some(items) = page["items"].as_array_mut() {
        super::chat_turns::recover_fields(&*state.storage, user, session, items)?;
    }
    Ok(project_page(&page, &final_id))
}

pub(super) fn project_page(page: &Value, final_id: &str) -> NativeProcessPage {
    let rows = page["items"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let final_id = if final_id.is_empty() && page["has_newer"] != true {
        rows.iter()
            .rev()
            .find(|row| {
                row["kind"] == "assistant_message"
                    && !row["payload"]["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty())
            })
            .and_then(|row| row["item_id"].as_str())
            .unwrap_or_default()
    } else {
        final_id
    };
    NativeProcessPage {
        first: page["next_before"].as_i64(),
        last: page["next_after"].as_i64(),
        has_older: page["has_older"] == true,
        has_newer: page["has_newer"] == true,
        items: rows
            .iter()
            .filter_map(|row| {
                let id = row["item_id"].as_str().unwrap_or_default();
                let state = row["status"].as_str().unwrap_or_default();
                let data = &row["payload"];
                let mut item = NativeProcessItem {
                    id: id.into(),
                    state: state.into(),
                    reasoning: String::new(),
                    text: String::new(),
                    tool: None,
                };
                match row["kind"].as_str().unwrap_or_default() {
                    "assistant_message" => {
                        item.reasoning = data["reasoning"].as_str().unwrap_or_default().into();
                        if id != final_id {
                            item.text = data["content"].as_str().unwrap_or_default().into();
                        }
                        if item.reasoning.is_empty() && item.text.is_empty() {
                            return None;
                        }
                    }
                    "tool_call" | "compaction" => {
                        let compact = row["kind"] == "compaction";
                        let tool = if compact {
                            "上下文压缩"
                        } else {
                            data["tool"]
                                .as_str()
                                .or_else(|| data["tool_name"].as_str())
                                .or_else(|| data["name"].as_str())
                                .unwrap_or("工具")
                        };
                        let detail = if compact {
                            data["summary_text"].as_str().unwrap_or_default().into()
                        } else {
                            wunder_server::tool_result_display::tool_result_display(
                                tool,
                                data,
                                matches!(state, "running" | "queued" | "pending"),
                            )
                        };
                        item.tool = Some(NativeWorkflowEntry::from_payload(
                            data["tool_call_id"]
                                .as_str()
                                .filter(|id| !id.is_empty())
                                .unwrap_or(id),
                            tool,
                            detail,
                            state,
                            data,
                        ));
                    }
                    _ => return None,
                }
                Some(item)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn long_durable_turn_keeps_answer_and_every_process_page() {
        use wunder_server::{
            config::Config,
            config_store::ConfigStore,
            state::{AppState, AppStateInitOptions},
        };
        let temp = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.storage.backend = "sqlite".into();
        config.storage.db_path = temp
            .path()
            .join("fixture.db")
            .to_string_lossy()
            .into_owned();
        config.workspace.root = temp.path().join("workspace").to_string_lossy().into_owned();
        config.skills.enabled.clear();
        let store = ConfigStore::new(temp.path().join("fixture.yaml"));
        store.update(|value| *value = config.clone()).await.unwrap();
        let state = AppState::new_with_options(
            store,
            config,
            AppStateInitOptions::cli_default().with_start_thread_runtime(false),
        )
        .unwrap();
        let (user, session) = ("fixture-owner", "fixture-thread");
        let accepted = state
            .storage
            .accept_thread_turn(user, session, &json!({"content":"Fixture input"}))
            .unwrap();
        let root = accepted["turn_id"].as_str().unwrap();
        let answer = format!("{root}:text-1");
        state.storage.append_thread_item(user, &json!({"session_id":session,"turn_id":root,"item_id":answer,"kind":"assistant_message","model_round":1,"content":"","reasoning":"","status":"completed"})).unwrap();
        for (field, text) in [
            ("content", "Fixture answer"),
            ("reasoning", "Fixture thought"),
        ] {
            let mut block = json!({"field":field,"content_offset":0});
            block[field] = json!(text);
            state.storage.upsert_thread_text_block(user, session, &json!({"item_id":answer,"field":field,"block_index":0,"event_id":1,"data":block})).unwrap();
        }
        state.storage.append_thread_item(user, &json!({"session_id":session,"turn_id":root,"item_id":"fixture-child-run","kind":"subagent_run","runtime":{"session_id":"fixture-child","title":"Fixture child","status":"running","can_terminate":true,"metrics":{"tool_calls":2}}})).unwrap();
        // Exceeds the old eight-page replay ceiling, with the answer outside
        // the initial tail. No model/network work is involved.
        for index in 0..1605 {
            state.storage.append_thread_item(user, &json!({"session_id":session,"turn_id":root,"item_id":format!("fixture-tool-{index}"),"kind":"tool_call","tool":"read_file","args":{"path":"sample.txt"},"status":"completed"})).unwrap();
        }
        state
            .storage
            .update_thread_turn(user, session, root, "completed", "", &json!({}))
            .unwrap();
        let turn = super::super::chat_turns::load_turn(&state, user, session, &accepted)
            .unwrap()
            .unwrap();
        assert_eq!(turn.assistant.text, "Fixture answer");
        assert_eq!(turn.subagents.len(), 1);
        assert_eq!(turn.subagents[0].session_id, "fixture-child");
        assert!(turn.subagents[0].can_terminate);
        assert!(
            turn.rounds
                .iter()
                .map(|round| round.items.len())
                .sum::<usize>()
                <= 24
        );
        let mut before = None;
        let mut ids = std::collections::HashSet::new();
        let mut thought = false;
        let first_page = read_page(&state, user, session, root, None, None).unwrap();
        let latest_first = first_page.first;
        loop {
            let page = read_page(&state, user, session, root, before, None).unwrap();
            assert!(page.items.len() <= 100);
            for item in &page.items {
                assert!(
                    item.text.is_empty(),
                    "the final answer stays outside the process"
                );
                thought |= item.reasoning == "Fixture thought";
                if let Some(tool) = &item.tool {
                    assert!(ids.insert(tool.id.clone()));
                }
            }
            if !page.has_older {
                let next = read_page(&state, user, session, root, None, page.last).unwrap();
                assert!(next.first > page.last);
                assert!(next.first < latest_first);
                break;
            }
            assert!(page.first < before || before.is_none());
            before = page.first;
        }
        assert_eq!(ids.len(), 1605);
        assert!(
            thought,
            "block-only reasoning remains readable on older pages"
        );
    }
    #[test]
    fn process_pages_keep_late_tools_and_exclude_only_authoritative_final_body() {
        let mut rows = vec![
            json!({"kind":"assistant_message","item_id":"fixture-print","payload":{"content":"Intermediate"}}),
        ];
        for index in 0..90 {
            rows.push(json!({"kind":"tool_call","item_id":format!("fixture-tool-{index}"),"status":"completed","payload":{"tool":"read_file","args":{"path":"sample.txt"}}}));
        }
        rows.push(json!({"kind":"assistant_message","item_id":"fixture-answer","payload":{"content":"Answer","reasoning":"Thought"}}));
        let page = project_page(
            &json!({"items":rows,"next_before":101,"next_after":192,"has_older":true,"has_newer":false}),
            "fixture-answer",
        );
        assert_eq!(page.items.len(), 92);
        assert_eq!(page.items[0].text, "Intermediate");
        assert_eq!(page.items[90].tool.as_ref().unwrap().id, "fixture-tool-89");
        assert_eq!(page.items[91].reasoning, "Thought");
        assert!(page.items[91].text.is_empty());
        assert!(page.has_older);
        assert!(!page.has_newer);
        assert_eq!(page.first, Some(101));
    }
}
