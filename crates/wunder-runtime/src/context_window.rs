//! What one outgoing request's prompt holds, part by part.
//!
//! The window card needs more than one number: a user has to see that most of
//! the window is tool definitions, or that a thread reached 80% because of a
//! file it read. So the request gate records the composition of the prompt it
//! is about to send — the agent's own prompt, every tool definition, tool
//! results, images and the conversation, each item with its own estimate.
//!
//! Estimates are local (UTF-8 bytes / 4, the same rule local admission uses)
//! and are labelled as estimates. Once the provider reports how many input
//! tokens it really read, [`ContextWindowSnapshot::rescale`] stretches the
//! recorded parts to that counted total, so the split stays truthful and the
//! card can say "measured" instead of "estimated".
use crate::llm::ChatMessage;
use crate::token_utils::{
    approx_token_count, count_image_blocks, estimate_message_tokens, IMAGE_TOKEN_ESTIMATE,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use wunder_core::thread_view::message_content_text;

/// How much item text the process keeps for all threads together. One
/// snapshot per thread, oldest evicted first; the newest request is always
/// kept whole even when it alone is larger than this budget.
pub const MAX_TEXT_CACHE_BYTES: usize = 16 * 1024 * 1024;
/// Threads whose latest request text stays readable at once. Past this the
/// oldest request's text is dropped, not shortened.
pub const MAX_TEXT_CACHE_ENTRIES: usize = 8;

/// The parts a prompt is made of, in the order the card lists them.
/// The names are stable keys: the shells own the wording of each part.
pub const PART_KINDS: [&str; 5] = ["system", "tools", "files", "results", "chat"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextWindowSnapshot {
    /// Model the recorded request went to.
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// The model's context window; 0 when it isn't known.
    #[serde(default)]
    pub window: i64,
    /// What the prompt takes of it.
    #[serde(default)]
    pub tokens: i64,
    /// True once `tokens` is the count the vendor reported for this request.
    #[serde(default)]
    pub measured: bool,
    #[serde(default)]
    pub parts: Vec<ContextPart>,
    /// User turns the prompt carries, for the card's footer note.
    #[serde(default)]
    pub turns: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextPart {
    /// One of [`PART_KINDS`].
    pub kind: String,
    #[serde(default)]
    pub tokens: i64,
    #[serde(default)]
    pub items: Vec<ContextItem>,
}

/// One row of a part: a tool definition, a turn of the conversation, a tool's
/// gathered results or a set of images.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContextItem {
    /// Stable kind for the shell's wording: `prompt`, `reminder`, `tool`,
    /// `turn`, `reply`, `result`, `image`. A tool's own name rides in `name`.
    pub tag: String,
    /// Raw name the card shows as it is (a tool, a path); empty for tagged
    /// items the shell words itself.
    #[serde(default)]
    pub name: String,
    /// How many of them this row stands for (results of one tool, images).
    #[serde(default)]
    pub count: i64,
    #[serde(default)]
    pub tokens: i64,
    /// The item's own text for the detail view, kept whole. Memory only: the
    /// persisted snapshot deliberately skips it, so the card still describes
    /// the request after a restart while the text can be released with the
    /// process.
    #[serde(skip)]
    pub text: String,
}

impl ContextWindowSnapshot {
    /// Split a request that is about to be sent into its parts.
    ///
    /// `messages` are the sanitized chat messages and `tools` the exact native
    /// tool definitions handed to the provider, so the card describes the
    /// request rather than a re-derivation of the thread.
    pub fn build(
        provider: &str,
        model: &str,
        window: i64,
        messages: &[ChatMessage],
        tools: Option<&[Value]>,
    ) -> Self {
        let mut system = ContextPart {
            kind: "system".into(),
            ..Default::default()
        };
        let mut tools_part = ContextPart {
            kind: "tools".into(),
            ..Default::default()
        };
        let mut files = ContextPart {
            kind: "files".into(),
            ..Default::default()
        };
        let mut results = ContextPart {
            kind: "results".into(),
            ..Default::default()
        };
        let mut chat = ContextPart {
            kind: "chat".into(),
            ..Default::default()
        };
        let mut images: i64 = 0;

        if let Some(tools) = tools {
            for tool in tools {
                tools_part.items.push(ContextItem {
                    tag: "tool".into(),
                    name: tool_name(tool),
                    count: 1,
                    tokens: json_tokens(tool),
                    text: serde_json::to_string_pretty(tool).unwrap_or_default(),
                });
            }
        }

        // Tool results are named by the call that produced them, the way the
        // card reads "web_run ×20" rather than twenty anonymous observations.
        let call_names = tool_call_names(messages);
        let mut turn = 0_i64;
        for message in messages {
            let role = message.role.as_str();
            // `ChatMessage` is what leaves the room; estimating its serialized
            // shape keeps the card in step with the wire format.
            let mut tokens = estimate_message_tokens(&message_value(message));
            let carried = count_image_blocks(&message.content);
            if carried > 0 {
                images += carried;
                tokens = (tokens - carried * IMAGE_TOKEN_ESTIMATE).max(0);
            }
            let body = message_content_text(&message.content).unwrap_or_default();
            match role {
                "system" => {
                    // A single leading system message is the agent's own prompt;
                    // any later one is a reminder folded into the request.
                    let tag = if system.items.is_empty() {
                        "prompt"
                    } else {
                        "reminder"
                    };
                    system.items.push(ContextItem {
                        tag: tag.into(),
                        name: String::new(),
                        count: 1,
                        tokens,
                        text: message_content_text(&message.content).unwrap_or_default(),
                    });
                }
                "tool" => {
                    let name = message
                        .tool_call_id
                        .as_deref()
                        .and_then(|id| call_names.get(id).cloned())
                        .unwrap_or_default();
                    merge_item(&mut results.items, "result", &name, tokens, &body);
                }
                "user" => {
                    turn += 1;
                    merge_item_counted(&mut chat.items, "turn", "", turn, tokens, &body);
                }
                "assistant" => {
                    // A turn's request and its answer are one row: the card
                    // reads "Turn 3", not a second anonymous row per reply.
                    merge_item_counted(&mut chat.items, "turn", "", turn.max(1), tokens, &body);
                }
                _ => {
                    // An observation encoded as a user message and any unknown
                    // role still belongs to the conversation it was sent in.
                    merge_item_counted(&mut chat.items, "turn", "", turn.max(1), tokens, &body);
                }
            }
        }

        if images > 0 {
            files.items.push(ContextItem {
                tag: "image".into(),
                name: String::new(),
                count: images,
                tokens: images * IMAGE_TOKEN_ESTIMATE,
                text: String::new(),
            });
        }

        let mut parts = Vec::with_capacity(PART_KINDS.len());
        let turns = chat.items.iter().filter(|item| item.tag == "turn").count() as i64;
        for part in [system, tools_part, files, results, chat].into_iter() {
            if part.items.is_empty() {
                continue;
            }
            let tokens: i64 = part.items.iter().map(|item| item.tokens).sum();
            parts.push(ContextPart { tokens, ..part });
        }
        let total: i64 = parts.iter().map(|part| part.tokens).sum();
        Self {
            model: model.to_string(),
            provider: provider.to_string(),
            window: window.max(0),
            tokens: total,
            measured: false,
            parts,
            turns,
        }
    }

    /// Stretch the recorded parts to the count the vendor reported for this
    /// request: the split was measured from the request, its total is the
    /// vendor's. Largest-remainder keeps every level adding up exactly.
    pub fn rescale(&mut self, total: i64) {
        let total = total.max(0);
        let current: i64 = self.parts.iter().map(|part| part.tokens).sum();
        if current <= 0 {
            self.tokens = total;
            self.measured = true;
            return;
        }
        let part_targets = largest_remainder(
            &self
                .parts
                .iter()
                .map(|part| part.tokens)
                .collect::<Vec<_>>(),
            total,
        );
        for (part, target) in self.parts.iter_mut().zip(part_targets) {
            let item_targets = largest_remainder(
                &part.items.iter().map(|i| i.tokens).collect::<Vec<_>>(),
                target,
            );
            for (item, value) in part.items.iter_mut().zip(item_targets) {
                item.tokens = value;
            }
            part.tokens = target;
        }
        self.tokens = total;
        self.measured = true;
    }

    pub fn part(&self, kind: &str) -> Option<&ContextPart> {
        self.parts.iter().find(|part| part.kind == kind)
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    /// The memory-only text behind one card row. `slot` is the shell's 1-based
    /// palette index; tag, name and count identify the row exactly as the
    /// snapshot recorded it. Counts matter for turns: every turn carries an
    /// empty name, so the ordinal is what keeps "turn 1" and "turn 2" apart.
    pub fn item_text(&self, slot: i32, tag: &str, name: &str, count: i64) -> Option<String> {
        let kind = PART_KINDS.get(slot.checked_sub(1)? as usize)?;
        let part = self.part(kind)?;
        part.items
            .iter()
            .find(|item| item.tag == tag && item.name == name && item.count == count)
            .map(|item| item.text.clone())
            .filter(|text| !text.is_empty())
    }

    /// Bytes this snapshot's text occupies in the bounded cache.
    pub fn text_bytes(&self) -> usize {
        self.parts
            .iter()
            .flat_map(|part| part.items.iter())
            .map(|item| item.text.len())
            .sum()
    }
}

/// One snapshot's text, kept only while the process lives. The card can reopen
/// the thread's last request without ever writing prompt text to storage.
#[derive(Default)]
pub struct ContextTextCache {
    inner: Mutex<TextCacheState>,
}

#[derive(Default)]
struct TextCacheState {
    tick: u64,
    bytes: usize,
    entries: HashMap<String, TextCacheEntry>,
}

struct TextCacheEntry {
    tick: u64,
    bytes: usize,
    snapshot: ContextWindowSnapshot,
}

impl ContextTextCache {
    pub fn put(&self, user_id: &str, session_id: &str, snapshot: ContextWindowSnapshot) {
        let key = text_cache_key(user_id, session_id);
        let mut state = self.inner.lock();
        if let Some(old) = state.entries.remove(&key) {
            state.bytes = state.bytes.saturating_sub(old.bytes);
        }
        let bytes = snapshot.text_bytes();
        if bytes == 0 {
            return;
        }
        state.tick = state.tick.wrapping_add(1);
        let tick = state.tick;
        state.bytes += bytes;
        state.entries.insert(
            key,
            TextCacheEntry {
                tick,
                bytes,
                snapshot,
            },
        );
        // Drop whole requests, never text inside one: the newest snapshot stays
        // readable even when it alone is larger than the budget, because the
        // card's text is for reading the request, not for sampling it.
        while state.entries.len() > 1
            && (state.bytes > MAX_TEXT_CACHE_BYTES
                || state.entries.len() > MAX_TEXT_CACHE_ENTRIES)
        {
            let Some(oldest) = state
                .entries
                .values()
                .min_by_key(|entry| entry.tick)
                .map(|entry| entry.tick)
            else {
                break;
            };
            let Some((key, entry)) = state
                .entries
                .iter()
                .find(|(_, entry)| entry.tick == oldest)
                .map(|(key, entry)| (key.clone(), entry.bytes))
            else {
                break;
            };
            state.entries.remove(&key);
            state.bytes = state.bytes.saturating_sub(entry);
        }
    }

    pub fn text(
        &self,
        user_id: &str,
        session_id: &str,
        slot: i32,
        tag: &str,
        name: &str,
        count: i64,
    ) -> Option<String> {
        let key = text_cache_key(user_id, session_id);
        let mut state = self.inner.lock();
        state.tick = state.tick.wrapping_add(1);
        let tick = state.tick;
        let entry = state.entries.get_mut(&key)?;
        entry.tick = tick;
        entry.snapshot.item_text(slot, tag, name, count)
    }

    pub fn forget_session(&self, user_id: &str, session_id: &str) {
        let key = text_cache_key(user_id, session_id);
        let mut state = self.inner.lock();
        if let Some(old) = state.entries.remove(&key) {
            state.bytes = state.bytes.saturating_sub(old.bytes);
        }
    }
}

fn text_cache_key(user_id: &str, session_id: &str) -> String {
    format!("{user_id}\u{1f}{session_id}")
}

/// The name a tool definition carries. Native function tools nest it under
/// `function`; freeform and older shapes keep it at the top.
fn tool_name(tool: &Value) -> String {
    tool.pointer("/function/name")
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("tool")
        .chars()
        .take(96)
        .collect()
}

/// call id → tool name, from the assistant messages that asked for them.
fn tool_call_names(messages: &[ChatMessage]) -> std::collections::HashMap<String, String> {
    let mut names = std::collections::HashMap::new();
    for message in messages {
        let Some(calls) = message.tool_calls.as_ref().and_then(Value::as_array) else {
            continue;
        };
        for call in calls {
            let id = call
                .get("id")
                .or_else(|| call.get("tool_call_id"))
                .and_then(Value::as_str);
            let name = call
                .pointer("/function/name")
                .or_else(|| call.get("name"))
                .and_then(Value::as_str);
            if let (Some(id), Some(name)) = (id, name) {
                names.insert(id.to_string(), name.chars().take(96).collect());
            }
        }
    }
    names
}

fn message_value(message: &ChatMessage) -> Value {
    serde_json::to_value(message).unwrap_or(Value::Null)
}

/// Gather one result into a row per tool name.
fn merge_item(items: &mut Vec<ContextItem>, tag: &str, name: &str, tokens: i64, text: &str) {
    if let Some(item) = items
        .iter_mut()
        .find(|item| item.tag == tag && item.name == name)
    {
        item.count += 1;
        item.tokens += tokens;
        append_text(item, text);
        return;
    }
    items.push(ContextItem {
        tag: tag.into(),
        name: name.into(),
        count: 1,
        tokens,
        text: text.to_string(),
    });
}

/// Gather one message into the turn row it belongs to.
fn merge_item_counted(
    items: &mut Vec<ContextItem>,
    tag: &str,
    name: &str,
    count: i64,
    tokens: i64,
    text: &str,
) {
    if let Some(item) = items
        .iter_mut()
        .find(|item| item.tag == tag && item.count == count && item.name == name)
    {
        item.tokens += tokens;
        append_text(item, text);
        return;
    }
    items.push(ContextItem {
        tag: tag.into(),
        name: name.into(),
        count,
        tokens,
        text: text.to_string(),
    });
}

fn append_text(item: &mut ContextItem, text: &str) {
    if text.is_empty() {
        return;
    }
    if !item.text.is_empty() {
        item.text.push_str("\n\n");
    }
    item.text.push_str(text);
}

fn json_tokens(value: &Value) -> i64 {
    approx_token_count(&value.to_string())
}

/// Split `target` across `values` in proportion, each level summing exactly to
/// it. Values are floored first, the remainder goes to the largest fractions.
fn largest_remainder(values: &[i64], target: i64) -> Vec<i64> {
    let sum: i64 = values.iter().map(|value| (*value).max(0)).sum();
    if values.is_empty() {
        return Vec::new();
    }
    if sum <= 0 || target <= 0 {
        return vec![0; values.len()];
    }
    let mut out = Vec::with_capacity(values.len());
    let mut fractions = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let value = (*value).max(0);
        let exact = (value as i128) * (target as i128);
        let floor = (exact / (sum as i128)) as i64;
        out.push(floor);
        fractions.push((exact % (sum as i128), index, value));
    }
    let mut left = target - out.iter().sum::<i64>();
    fractions.sort_by(|a, b| b.0.cmp(&a.0).then(b.2.cmp(&a.2)).then(a.1.cmp(&b.1)));
    for (_, index, _) in fractions.iter().take(left.max(0) as usize) {
        out[*index] += 1;
        left -= 1;
        if left <= 0 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: Value::String(content.into()),
            reasoning_content: None,
            anthropic_thinking_blocks: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn request() -> Vec<ChatMessage> {
        let mut call = text("assistant", "");
        call.content = Value::Null;
        call.tool_calls = Some(json!([
            {"id":"c1","type":"function","function":{"name":"read_file","arguments":"{}"}}
        ]));
        let mut result = text("tool", "file body");
        result.tool_call_id = Some("c1".into());
        vec![
            text("system", "agent instructions"),
            text("user", "first question"),
            text("assistant", "first answer"),
            text("user", "second question"),
            call,
            result,
            text("assistant", "done"),
        ]
    }

    #[test]
    fn parts_cover_the_whole_prompt_and_name_tool_results() {
        let tools = vec![
            json!({"type":"function","function":{"name":"read_file","description":"d"}}),
            json!({"type":"function","function":{"name":"goal","description":"g"}}),
        ];
        let snapshot =
            ContextWindowSnapshot::build("openai", "gpt-5.5", 128_000, &request(), Some(&tools));
        assert_eq!(snapshot.model, "gpt-5.5");
        assert_eq!(snapshot.window, 128_000);
        assert!(!snapshot.measured);
        let total: i64 = snapshot.parts.iter().map(|p| p.tokens).sum();
        assert_eq!(snapshot.tokens, total);
        let tools_part = snapshot.part("tools").expect("tools part");
        let names: Vec<_> = tools_part.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, ["read_file", "goal"]);
        let results = snapshot.part("results").expect("results part");
        assert_eq!(results.items.len(), 1);
        assert_eq!(results.items[0].name, "read_file");
        assert_eq!(results.items[0].count, 1);
        let chat = snapshot.part("chat").expect("chat part");
        assert!(chat.items.iter().any(|i| i.tag == "turn" && i.count == 2));
        assert_eq!(snapshot.turns, 2);
    }

    #[test]
    fn images_are_carried_by_the_files_part_not_the_message() {
        let mut messages = vec![text("user", "")];
        messages[0].content = json!([
            {"type":"text","text":"look"},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,AA=="}}
        ]);
        let snapshot = ContextWindowSnapshot::build("p", "m", 0, &messages, None);
        let files = snapshot.part("files").expect("files part");
        assert_eq!(files.items[0].count, 1);
        assert_eq!(files.items[0].tokens, IMAGE_TOKEN_ESTIMATE);
        let chat = snapshot.part("chat").expect("chat part");
        // The turn keeps its text and envelope, never the image again.
        assert!(chat.items[0].tokens < IMAGE_TOKEN_ESTIMATE);
    }

    #[test]
    fn rescale_keeps_every_level_summing_to_the_measured_total() {
        let mut snapshot = ContextWindowSnapshot::build(
            "p",
            "m",
            8_000,
            &request(),
            Some(&[json!({"type":"function","function":{"name":"t","description":"d"}})]),
        );
        snapshot.rescale(9_000);
        assert!(snapshot.measured);
        assert_eq!(snapshot.tokens, 9_000);
        let parts: i64 = snapshot.parts.iter().map(|p| p.tokens).sum();
        assert_eq!(parts, 9_000);
        for part in &snapshot.parts {
            assert_eq!(
                part.items.iter().map(|i| i.tokens).sum::<i64>(),
                part.tokens
            );
        }
    }

    #[test]
    fn empty_requests_record_no_parts() {
        let snapshot = ContextWindowSnapshot::build("p", "m", 4_000, &[], None);
        assert!(snapshot.is_empty());
        assert_eq!(snapshot.tokens, 0);
    }

    #[test]
    fn item_text_stays_out_of_the_persisted_snapshot() {
        let snapshot = ContextWindowSnapshot::build(
            "p",
            "m",
            8_000,
            &request(),
            Some(&[json!({"type":"function","function":{"name":"read_file","description":"d"}})]),
        );
        assert_eq!(
            snapshot.item_text(1, "prompt", "", 1),
            Some("agent instructions".to_string())
        );
        assert_eq!(
            snapshot.item_text(4, "result", "read_file", 1),
            Some("file body".to_string())
        );
        let stored = serde_json::to_string(&snapshot).expect("snapshot serializes");
        assert!(!stored.contains("agent instructions"));
        assert!(!stored.contains("file body"));
        let reloaded: ContextWindowSnapshot =
            serde_json::from_str(&stored).expect("snapshot parses");
        assert_eq!(reloaded.item_text(1, "prompt", "", 1), None);
    }

    #[test]
    fn turn_text_uses_the_turn_ordinal() {
        let snapshot = ContextWindowSnapshot::build("p", "m", 8_000, &request(), None);
        let first = snapshot
            .item_text(5, "turn", "", 1)
            .expect("first turn text");
        let second = snapshot
            .item_text(5, "turn", "", 2)
            .expect("second turn text");
        assert!(first.starts_with("first question"), "{first}");
        assert!(second.starts_with("second question"), "{second}");
        assert_ne!(first, second);
    }

    #[test]
    fn long_item_text_is_kept_whole_in_memory() {
        let long = "x".repeat(256 * 1024);
        let messages = vec![text("system", &long)];
        let snapshot = ContextWindowSnapshot::build("p", "m", 8_000, &messages, None);
        let system = snapshot.part("system").expect("system part");
        assert_eq!(system.items[0].text.len(), long.len());
        assert_eq!(
            snapshot.item_text(1, "prompt", "", 1),
            Some(system.items[0].text.clone())
        );
    }

    #[test]
    fn text_cache_answers_by_row_and_forgets_the_session() {
        let snapshot = ContextWindowSnapshot::build("p", "m", 8_000, &request(), None);
        let cache = ContextTextCache::default();
        cache.put("u", "s", snapshot);
        let text = cache.text("u", "s", 5, "turn", "", 1).expect("cached turn text");
        assert!(text.starts_with("first question"));
        cache.forget_session("u", "s");
        assert_eq!(cache.text("u", "s", 5, "turn", "", 1), None);
    }
}
