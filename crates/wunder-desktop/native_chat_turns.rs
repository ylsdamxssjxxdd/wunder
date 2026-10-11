//! Durable user-turn projection. Execution turns never allocate page rows.
use super::{
    message_from_value, NativeDesktop, NativeMessage, NativeSubagentCard, NativeWorkflowEntry,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use wunder_server::storage::StorageBackend;

/// Tool and compaction entries one turn projects. The timeline lays out every
/// visible row, so this is a frame-cost bound as well as a display bound.
const MAX_TURN_ENTRIES: usize = 24;

/// Child-run cards one turn projects. A turn spawning more children than the
/// cap keeps the newest cards; the runtime list view remains the full index.
const MAX_TURN_SUBAGENTS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct NativeChatTurn {
    pub root_id: String,
    pub user: NativeMessage,
    pub assistant: NativeMessage,
    /// The turn's model rounds in durable order. One round is the unit the
    /// runtime registers as a single `assistant_message` item, so it carries
    /// that round's thinking, its answer block and the tools it invoked.
    pub rounds: Vec<NativeChatRound>,
    /// Child runs spawned by this turn, in durable order. Cards render at the
    /// turn's tail, matching the web messenger's subagent panel.
    pub subagents: Vec<NativeSubagentCard>,
}

/// One model round of an assistant turn.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeChatRound {
    pub round: i64,
    pub reasoning: String,
    pub text: String,
    pub items: Vec<NativeWorkflowEntry>,
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
    // A stop can land before an assistant item is committed, leaving that round
    // with an empty payload while its streamed text is already durable as
    // blocks. Recover every round's own fields: the timeline is per round, so
    // reviving only the newest answer silently drops the earlier ones.
    for item in items.iter_mut().filter(|item| stable_answer(item)) {
        let item_id = item["item_id"].as_str().unwrap_or_default().to_string();
        if item_id.is_empty() {
            continue;
        }
        for field in ["content", "reasoning"] {
            if !item["payload"][field]
                .as_str()
                .unwrap_or_default()
                .is_empty()
            {
                continue;
            }
            let text = recover_text(&**storage, user, session, &item_id, field)?;
            if !text.is_empty() {
                item["payload"][field] = json!(text);
            }
        }
    }
    Ok(project_turn(root, &items))
}

/// Concatenate one item's durable text blocks of one field. Bounded by the
/// storage page size and an 8 MiB ceiling, the same limits the single-answer
/// recovery carried before it became per-round.
fn recover_text(
    storage: &dyn StorageBackend,
    user: &str,
    session: &str,
    item_id: &str,
    field: &str,
) -> Result<String> {
    let mut from = 0;
    let mut text = String::new();
    for page in 0..64 {
        let (blocks, next, more) =
            storage.list_thread_item_blocks_page(user, session, item_id, Some(field), from, 100, false)?;
        for block in blocks {
            let data = block.get("data").unwrap_or(&block);
            if data["field"].as_str().unwrap_or(field) != field {
                continue;
            }
            if let Some(part) = data["content"].as_str() {
                if text.len() + part.len() > 8 * 1024 * 1024 {
                    return Err(anyhow!("turn text limit exceeded"));
                }
                text.push_str(part);
            }
        }
        if !more {
            break;
        }
        if page == 63 {
            return Err(anyhow!("turn block limit exceeded"));
        }
        from = next.ok_or_else(|| anyhow!("missing block cursor"))? + 1;
    }
    Ok(text)
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
    // Rounds in durable order: every `assistant_message` item is one model
    // round, and every tool or compaction entry belongs to the round that
    // admitted it. Keeping that grouping is what lets a reloaded turn rebuild
    // the same batches and answer blocks the live stream produced.
    let mut rounds: Vec<NativeChatRound> = Vec::new();
    let mut subagents: Vec<NativeSubagentCard> = Vec::new();
    let mut entries = 0usize;
    let mut last_round = 1;
    for item in items {
        let kind = item["kind"].as_str().unwrap_or_default();
        let data = &item["payload"];
        let round = item_round(item).unwrap_or(last_round);
        if kind == "assistant_message" {
            if !stable_answer(item) {
                continue;
            }
            last_round = round;
            let reasoning = data["reasoning"].as_str().unwrap_or_default().to_string();
            let text = data["content"].as_str().unwrap_or_default().to_string();
            if reasoning.is_empty() && text.is_empty() {
                // A round admitted by `llm_request` and never answered carries
                // no timeline entry of its own.
                continue;
            }
            match rounds.iter_mut().rev().find(|slot| slot.round == round) {
                Some(slot) => {
                    slot.text = text;
                    slot.reasoning = reasoning;
                }
                None => rounds.push(NativeChatRound {
                    round,
                    reasoning,
                    text,
                    items: Vec::new(),
                }),
            }
            continue;
        }
        if kind == "subagent_run" {
            // Child runs render as cards at the turn's tail; each progress
            // revision rewrites one stable item, so a single pass yields one
            // card per child.
            let mut data = data.clone();
            if let Some(id) = item["item_id"].as_str() {
                data["item_id"] = json!(id);
            }
            if let Some(card) = NativeSubagentCard::from_payload(&data) {
                if subagents.len() >= MAX_TURN_SUBAGENTS {
                    subagents.remove(0);
                }
                subagents.push(card);
            }
            continue;
        }
        if !matches!(kind, "tool_call" | "compaction") || entries >= MAX_TURN_ENTRIES {
            continue;
        }
        entries += 1;
        let entry = if kind == "compaction" {
            let detail = data["summary_text"].as_str().unwrap_or_default().to_string();
            if latest.is_none() {
                assistant.text = detail.clone();
            }
            NativeWorkflowEntry::from_payload(
                item["item_id"].as_str().unwrap_or("compaction"),
                "上下文压缩",
                detail,
                item["status"].as_str().unwrap_or_default(),
                data,
            )
        } else {
            let tool = data["tool"]
                .as_str()
                .or_else(|| data["tool_name"].as_str())
                .or_else(|| data["name"].as_str())
                .unwrap_or("工具");
            let detail = wunder_server::tool_result_display::tool_result_display(
                tool,
                data,
                matches!(item["status"].as_str(), Some("running" | "queued")),
            );
            NativeWorkflowEntry::from_payload(
                item["item_id"].as_str().unwrap_or(tool),
                tool,
                detail,
                item["status"].as_str().unwrap_or_default(),
                data,
            )
        };
        match rounds.iter_mut().rev().find(|slot| slot.round == round) {
            Some(slot) => slot.items.push(entry),
            None => rounds.push(NativeChatRound {
                round,
                reasoning: String::new(),
                text: String::new(),
                items: vec![entry],
            }),
        }
    }
    Some(NativeChatTurn {
        root_id: id.into(),
        user,
        assistant,
        rounds,
        subagents,
    })
}

/// The model round a durable item was recorded against. Envelope fields are
/// kept inside `payload` by thread-log storage; the stable answer identity
/// repeats the round in its item id, which older records lean on.
fn item_round(item: &Value) -> Option<i64> {
    if let Some(round) = item["payload"]["model_round"].as_i64() {
        return Some(round);
    }
    item["item_id"]
        .as_str()
        .and_then(|id| id.rsplit(":text-").next())
        .and_then(|tail| tail.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_command_replay_keeps_failure_details_without_summary() {
        let turn = project_turn(&json!({"turn_id":"fixture-root", "status":"failed"}), &[
            json!({"kind":"user_message", "turn_id":"fixture-root", "payload":{"content":"/compact"}}),
            json!({"kind":"compaction", "item_id":"fixture-compact", "turn_id":"fixture-root", "status":"failed", "payload":{"status":"failed", "reason":"manual", "error_message":"fixture failure"}})
        ]).unwrap();
        assert_eq!(turn.user.text, "/compact");
        assert_eq!(turn.rounds[0].items.len(), 1);
        assert_eq!(turn.rounds[0].items[0].state, "failed");
        assert!(turn.rounds[0].items[0]
            .sections
            .iter()
            .any(|section| section.kind == "error"));
    }
    #[test]
    fn replay_keeps_tool_request_usage_from_durable_result() {
        let root = json!({"turn_id":"fixture-root", "status":"completed"});
        let items = vec![
            json!({"kind":"user_message","turn_id":"fixture-root","payload":{"content":"Fixture input"}}),
            json!({"kind":"tool_call","item_id":"fixture-tool","turn_id":"fixture-root","status":"completed","payload":{
                "tool":"read_file","request_context_tokens":1800,"request_usage":{"total":2400},
                "meta":{"duration_ms":1200},"data":{"consumed_tokens":99999,"content":"Fixture output"}
            }}),
        ];
        let turn = project_turn(&root, &items).unwrap();
        assert_eq!(turn.rounds[0].items[0].tokens, "2.4k token");
        assert_eq!(turn.rounds[0].items[0].duration, "1.2s");
    }

    /// The timeline is rebuilt per model round: dropping everything but the
    /// newest answer is what made a finished reply lose its thinking and its
    /// earlier blocks.
    #[test]
    fn replay_keeps_every_rounds_thinking_answer_and_tools() {
        let root = json!({"turn_id":"fixture-root", "status":"completed"});
        let items = vec![
            json!({"kind":"user_message","turn_id":"fixture-root","payload":{"content":"Fixture input"}}),
            json!({"kind":"assistant_message","item_id":"fixture-root:text-1","turn_id":"fixture-root",
                   "status":"completed","payload":{"content":"先做第一步", "reasoning":"想想第一步", "model_round":1}}),
            json!({"kind":"tool_call","item_id":"fixture-tool","turn_id":"fixture-root","status":"completed",
                   "payload":{"tool":"read_file","model_round":1,"data":{"content":"Fixture output"}}}),
            json!({"kind":"assistant_message","item_id":"fixture-root:text-2","turn_id":"fixture-root",
                   "status":"completed","payload":{"content":"结论", "reasoning":"想想第二步", "model_round":2}}),
        ];
        let turn = project_turn(&root, &items).unwrap();
        assert_eq!(turn.rounds.len(), 2);
        assert_eq!(turn.rounds[0].reasoning, "想想第一步");
        assert_eq!(turn.rounds[0].text, "先做第一步");
        assert_eq!(turn.rounds[0].items.len(), 1);
        assert_eq!(turn.rounds[1].reasoning, "想想第二步");
        assert_eq!(turn.rounds[1].text, "结论");
        assert!(turn.rounds[1].items.is_empty());
        assert_eq!(turn.assistant.text, "结论");
    }
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
        let root = json!({"turn_id": "fixture-root", "status": "completed"});
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

    #[test]
    fn subagent_run_items_project_into_child_cards() {
        let root = json!({"turn_id": "fixture-root", "status": "running"});
        let items = vec![
            json!({"kind":"user_message","turn_id":"fixture-root","payload":{"content":"Fixture input"}}),
            json!({"kind":"subagent_run","item_id":"fixture-root:sub-fixture-run","turn_id":"fixture-root",
                   "status":"completed","visibility":"user",
                   "payload":{"session_id":"fixture-parent","turn_id":"fixture-root","user_round":1,
                              "item_id":"fixture-root:sub-fixture-run","kind":"subagent_run",
                              "status":"completed","visibility":"user","runtime":{
                                  "session_id":"fixture-child","run_id":"fixture-run","title":"资料整理",
                                  "status":"running","terminal":false,"failed":false,
                                  "latest_message":"正在读取资料","can_terminate":true,
                                  "metrics":{"tool_calls":2,"model_request_count":1,
                                             "account_credits_consumed":0,"context_tokens":1024}}}}),
        ];
        let turn = project_turn(&root, &items).unwrap();
        assert_eq!(turn.subagents.len(), 1);
        let card = &turn.subagents[0];
        assert_eq!(card.session_id, "fixture-child");
        assert_eq!(card.item_id, "fixture-root:sub-fixture-run");
        assert_eq!(card.title, "资料整理");
        assert!(card.is_running());
        assert!(card.can_terminate);
        assert_eq!(card.tool_calls, 2);
        assert_eq!(card.context_tokens, 1024);
        // A card without a child identity cannot render anything useful.
        let broken = vec![
            items[0].clone(),
            json!({"kind":"subagent_run","item_id":"fixture-root:sub-broken","turn_id":"fixture-root",
                   "status":"completed","payload":{"runtime":{"title":"无身份"}}}),
        ];
        assert!(project_turn(&root, &broken).unwrap().subagents.is_empty());
    }
}
