//! Durable user-turn projection. Execution turns never allocate page rows.
use super::{message_from_value, NativeDesktop, NativeMessage};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct NativeChatTurn {
    pub root_id: String,
    pub user: NativeMessage,
    pub assistant: NativeMessage,
}

impl NativeDesktop {
    pub fn load_chat_turns(&self, session: &str) -> Result<Vec<NativeChatTurn>> {
        load_turns(self.state(), self.user_id(), session)
    }
}

pub(super) fn load_turns(
    state: &wunder_server::state::AppState,
    user: &str,
    session: &str,
) -> Result<Vec<NativeChatTurn>> {
    let mut roots = state.storage.list_thread_turns(user, session, None, 50)?;
    roots.reverse();
    roots
        .iter()
        .filter_map(|root| load_turn(state, user, session, root).transpose())
        .collect()
}

pub(super) fn load_turn(
    state: &wunder_server::state::AppState,
    user: &str,
    session: &str,
    root: &Value,
) -> Result<Option<NativeChatTurn>> {
    let storage = &state.storage;
    let id = root["turn_id"]
        .as_str()
        .ok_or_else(|| anyhow!("missing root identity"))?;
    let mut after = -1;
    let mut items = Vec::new();
    for page in 0..8 {
        let Some(detail) = storage.get_thread_turn(user, session, id, after, 200, false)? else {
            break;
        };
        if let Some(rows) = detail["items"].as_array() {
            items.extend(rows.iter().cloned());
        }
        if detail["has_more"] != true {
            break;
        }
        if page == 7 {
            return Err(anyhow!("turn display limit exceeded; inspect thread log"));
        }
        after = detail["next_after"]
            .as_i64()
            .or_else(|| items.last().and_then(|row| row["item_index"].as_i64()))
            .unwrap_or(after);
    }
    // Recover the latest stable answer from durable blocks when a stop
    // happened before the full assistant item was committed.
    if let Some(item) = items
        .iter_mut()
        .rev()
        .find(|item| stable_answer(item))
        .filter(|item| {
            item["payload"]["content"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        })
    {
        let item_id = item["item_id"].as_str().unwrap_or_default().to_string();
        let mut blocks = Vec::new();
        let mut from = 0;
        for page in 0..64 {
            let (part, next, more) = storage.list_thread_item_blocks_page(
                user,
                session,
                &item_id,
                Some("content"),
                from,
                100,
                false,
            )?;
            blocks.extend(part);
            if !more {
                break;
            }
            if page == 63 {
                return Err(anyhow!("turn block limit exceeded"));
            }
            from = next.ok_or_else(|| anyhow!("missing block cursor"))? + 1;
        }
        let mut content = String::new();
        for block in blocks {
            let data = block.get("data").unwrap_or(&block);
            if data["field"].as_str().unwrap_or("content") != "content" {
                continue;
            }
            if let Some(text) = data["content"].as_str() {
                if content.len() + text.len() > 8 * 1024 * 1024 {
                    return Err(anyhow!("turn text limit exceeded"));
                }
                content.push_str(text);
            }
        }
        if !content.is_empty()
            && item["payload"]["content"]
                .as_str()
                .unwrap_or_default()
                .is_empty()
        {
            item["payload"]["content"] = json!(content);
        }
    }
    Ok(project_turn(root, &items))
}

fn stable_answer(item: &Value) -> bool {
    let turn = item["turn_id"].as_str().unwrap_or_default();
    let round = item["payload"]["model_round"].as_i64().unwrap_or(1);
    item["kind"] == "assistant_message" && item["item_id"] == format!("{turn}:text-{round}")
}

fn project_turn(root: &Value, items: &[Value]) -> Option<NativeChatTurn> {
    let id = root["turn_id"].as_str()?;
    let input = items
        .iter()
        .find(|item| item["kind"] == "user_message" && item["turn_id"] == id)?;
    let mut user_payload = input["payload"].clone();
    user_payload["role"] = json!("user");
    user_payload["turn_id"] = json!(id);
    let user = message_from_value(user_payload)?;
    let latest = items.iter().rev().find(|item| stable_answer(item));
    let mut payload = latest
        .map(|item| item["payload"].clone())
        .unwrap_or_else(|| json!({"content":""}));
    payload["role"] = json!("assistant");
    payload["turn_id"] = json!(id);
    let status =
        if root["status"] == "completed" && latest.is_some_and(|item| item["turn_id"] != id) {
            latest
                .map(|item| &item["status"])
                .unwrap_or(&root["status"])
        } else {
            // A completed model action does not settle its owning user turn.
            &root["status"]
        };
    let state = match status.as_str().unwrap_or_default() {
        "cancelled" | "interrupted" | "stopped" => "已停止",
        "failed" | "rejected" => "执行失败",
        "queued" => "正在排队",
        "completed" => "任务完成",
        _ => "正在生成…",
    };
    payload["status"] = json!(state);
    let mut assistant = message_from_value(payload)?;
    if state == "正在排队" {
        if let Some(ahead) = items
            .iter()
            .rev()
            .find(|item| item["kind"] == "queue")
            .and_then(|item| item["payload"]["queue_ahead"].as_i64())
        {
            assistant.stats_status = format!("正在排队 · 前方 {ahead} 名");
        }
    }
    let mut workflow = Vec::new();
    for item in items
        .iter()
        .filter(|item| matches!(item["kind"].as_str(), Some("tool_call" | "compaction")))
        .take(24)
    {
        let data = &item["payload"];
        if item["kind"] == "compaction" {
            workflow.push(format!(
                "上下文压缩\n{}",
                data["summary_text"].as_str().unwrap_or_default()
            ));
            if latest.is_none() {
                assistant.text = data["summary_text"].as_str().unwrap_or_default().into();
            }
        } else {
            let tool = data["tool"]
                .as_str()
                .or_else(|| data["tool_name"].as_str())
                .or_else(|| data["name"].as_str())
                .unwrap_or("工具");
            workflow.push(wunder_server::tool_result_display::tool_result_display(
                tool,
                data,
                matches!(item["status"].as_str(), Some("running" | "queued")),
            ));
        }
    }
    assistant.workflow_detail = workflow.join("\n\n");
    Some(NativeChatTurn {
        root_id: id.into(),
        user,
        assistant,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finished_model_action_does_not_settle_running_channel_turn() {
        let root = json!({"turn_id":"fixture-root", "status":"running"});
        let items = vec![
            json!({"kind":"user_message","turn_id":"fixture-root","payload":{"content":"Fixture input"}}),
            json!({"kind":"assistant_message","item_id":"fixture-root:text-1","turn_id":"fixture-root","status":"completed","payload":{"content":"Fixture action", "model_round":1}}),
        ];
        assert_eq!(
            project_turn(&root, &items).unwrap().assistant.stats_status,
            "正在生成…"
        );
    }
    #[test]
    fn rejected_scheduled_turn_has_terminal_status_without_model_text() {
        let root = json!({"turn_id":"fixture-rejected", "status":"rejected"});
        let items = vec![json!({"kind":"user_message","turn_id":"fixture-rejected",
            "payload":{"content":"Fixture scheduled input"}})];
        let turn = project_turn(&root, &items).unwrap();
        assert_eq!(turn.assistant.stats_status, "执行失败");
    }

    #[test]
    fn continuation_and_model_actions_fill_one_root_pair() {
        let root = json!({"turn_id":"fixture-root", "status":"completed"});
        let items = vec![
            json!({"kind":"user_message","turn_id":"fixture-root","payload":{"content":"Fixture input"}}),
            json!({"kind":"assistant_message","item_id":"fixture-root:text-1","turn_id":"fixture-root","status":"completed","payload":{"content":"Earlier", "model_round":1}}),
            json!({"kind":"assistant_message","item_id":"fixture-child:text-1","turn_id":"fixture-child","status":"cancelled","payload":{"content":"Retained partial", "model_round":1}}),
        ];
        let turn = project_turn(&root, &items).unwrap();
        assert_eq!(turn.root_id, "fixture-root");
        assert!(turn.user.mine);
        assert!(!turn.assistant.mine);
        assert_eq!(turn.assistant.text, "Retained partial");
        assert_eq!(turn.assistant.stats_status, "已停止");
        assert!(project_turn(&root, &items[1..]).is_none());
    }
}
