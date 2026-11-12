//! Bounded inline-image context.
//!
//! Images are useful across turns, but every inline image is re-sent on every
//! model request. A long visual iteration (render → read → edit → read) can
//! therefore accumulate dozens of multi-megabyte data URLs until the provider
//! closes the connection before returning an HTTP status. The runtime keeps the
//! newest images, collapses identical repeated reads and superseded versions of
//! the same path, and replaces evicted images with text placeholders that keep
//! the original path for an on-demand re-read.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{self, Write};

use crate::config::LlmModelConfig;
use crate::services::chat_payload_sanitizer::{
    image_path_marker_in_text, is_omitted_image_placeholder_text, omitted_image_placeholder_with_path,
};

pub const DEFAULT_MAX_CONTEXT_IMAGES: usize = 16;
/// Total inline image data-URL bytes kept in one request. Base64 inflates raw
/// image bytes by ~1/3, so this leaves room for text and tool schemas under a
/// 32 MiB provider body ceiling.
pub const DEFAULT_MAX_CONTEXT_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_VERSIONS_PER_PATH: usize = 1;
/// 单个用户轮次内最多内联的图片数（含工具回读）。必须小于总预算，否则
/// 本轮读图仍会把历史图逐张挤出去。
pub const DEFAULT_MAX_CURRENT_TURN_IMAGES: usize = 8;
pub const DEFAULT_MAX_REQUEST_BYTES: usize = 32 * 1024 * 1024;

/// The newest images are always kept even when an old history is over budget;
/// they are the ones the current turn is actually looking at.
const MIN_KEEP_IMAGES: usize = 4;
/// When the request still cannot fit, keep only the single newest image so a
/// handful of large frames cannot force the whole turn to fail.
const EMERGENCY_KEEP_IMAGES: usize = 1;
pub const REQUEST_BODY_OVERHEAD_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageContextPolicy {
    pub max_images: usize,
    pub max_total_bytes: usize,
    pub max_versions_per_path: usize,
    pub max_request_bytes: usize,
    /// 当前用户轮次（含工具回读）最多内联的图片数。超出后从这一轮最旧的
    /// 图片开始降级：新图入场不再挤掉历史，前缀断点因此只出现在本轮尾部。
    pub max_current_turn_images: usize,
}

impl ImageContextPolicy {
    pub fn from_config(config: &LlmModelConfig) -> Self {
        Self {
            max_images: DEFAULT_MAX_CONTEXT_IMAGES,
            max_total_bytes: DEFAULT_MAX_CONTEXT_IMAGE_BYTES,
            max_versions_per_path: DEFAULT_MAX_VERSIONS_PER_PATH,
            max_request_bytes: config
                .max_request_bytes
                .filter(|value| *value > 0)
                .unwrap_or(DEFAULT_MAX_REQUEST_BYTES),
            max_current_turn_images: DEFAULT_MAX_CURRENT_TURN_IMAGES,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ImageBudgetReport {
    pub images_before: usize,
    pub bytes_before: usize,
    pub images_after: usize,
    pub bytes_after: usize,
    pub duplicate_images_removed: usize,
    pub superseded_images_removed: usize,
    pub budget_images_removed: usize,
}

impl ImageBudgetReport {
    pub fn changed(&self) -> bool {
        self.duplicate_images_removed > 0
            || self.superseded_images_removed > 0
            || self.budget_images_removed > 0
    }

    pub fn to_payload(&self) -> Value {
        json!({
            "images_before": self.images_before,
            "image_bytes_before": self.bytes_before,
            "images_after": self.images_after,
            "image_bytes_after": self.bytes_after,
            "duplicate_images_removed": self.duplicate_images_removed,
            "superseded_images_removed": self.superseded_images_removed,
            "budget_images_removed": self.budget_images_removed,
        })
    }
}

#[derive(Debug, Default, Clone)]
pub struct ImageRestoreReport {
    pub placeholders: usize,
    pub restored: usize,
    pub restored_bytes: usize,
    pub skipped_by_path_limit: usize,
    pub skipped_by_budget: usize,
    pub skipped_missing: usize,
}

impl ImageRestoreReport {
    pub fn to_payload(&self) -> Value {
        json!({
            "placeholders": self.placeholders,
            "restored": self.restored,
            "restored_bytes": self.restored_bytes,
            "skipped_by_path_limit": self.skipped_by_path_limit,
            "skipped_by_budget": self.skipped_by_budget,
            "skipped_missing": self.skipped_missing,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RequestTooLarge {
    pub request_bytes: usize,
    pub limit_bytes: usize,
    pub image_count: usize,
    pub image_bytes: usize,
}

impl RequestTooLarge {
    pub fn message(&self) -> String {
        let request_mb = format!("{:.1}", self.request_bytes as f64 / (1024.0 * 1024.0));
        let limit_mb = format!("{:.1}", self.limit_bytes as f64 / (1024.0 * 1024.0));
        crate::i18n::t_with_params(
            "error.llm_request_too_large",
            &HashMap::from([
                ("request_mb".to_string(), request_mb),
                ("limit_mb".to_string(), limit_mb),
            ]),
        )
    }

    pub fn detail(&self) -> Value {
        json!({
            "request_bytes": self.request_bytes,
            "limit_bytes": self.limit_bytes,
            "image_count": self.image_count,
            "image_bytes": self.image_bytes,
            "source": "local_request_size_guard",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DemoteReason {
    Duplicate,
    Superseded,
    Budget,
}

struct ImagePartRef {
    message_index: usize,
    part_index: usize,
    url_len: usize,
    digest: u64,
    path: Option<String>,
    internal_followup: bool,
    protected: bool,
    /// 1-based user turn index; eviction works on whole turns.
    turn: usize,
    /// True for images that belong to the user turn being executed right now
    /// (the newest user message plus its tool follow-ups). History images are
    /// false, so a long visual iteration cannot push them out.
    in_current_turn: bool,
}

/// Keeps the newest inline images within the context budget. Older images are
/// replaced by text placeholders that retain the source path, so the model can
/// re-read one deliberately instead of paying every image on every request.
pub fn enforce_image_budget(
    messages: &mut [Value],
    policy: &ImageContextPolicy,
) -> ImageBudgetReport {
    enforce_image_budget_with_limits(
        messages,
        policy,
        policy.max_images,
        policy.max_total_bytes,
        policy.max_current_turn_images,
        MIN_KEEP_IMAGES,
    )
}

/// Applies the context image budget and then the provider request-body ceiling.
/// If the non-image payload alone exceeds the ceiling, returns a readable error
/// instead of sending an oversized body that most gateways drop without status.
pub fn enforce_request_limits(
    messages: &mut [Value],
    tools: Option<&[Value]>,
    policy: &ImageContextPolicy,
) -> Result<ImageBudgetReport, RequestTooLarge> {
    let mut report = enforce_image_budget(messages, policy);
    let mut request_bytes = serialized_request_bytes(messages, tools);
    if request_bytes <= policy.max_request_bytes {
        return Ok(report);
    }

    // The normal image budget leaves a text/tool-schema reserve. If that was
    // still not enough, this emergency pass keeps only the protected images and
    // the newest one, then re-measures before giving up.
    let emergency = enforce_image_budget_with_limits(
        messages,
        policy,
        0,
        0,
        policy.max_current_turn_images,
        EMERGENCY_KEEP_IMAGES,
    );
    report.images_after = emergency.images_after;
    report.bytes_after = emergency.bytes_after;
    report.budget_images_removed = report
        .budget_images_removed
        .saturating_add(emergency.budget_images_removed);
    request_bytes = serialized_request_bytes(messages, tools);
    if request_bytes <= policy.max_request_bytes {
        return Ok(report);
    }

    let (image_count, image_bytes) = inline_image_stats(messages);
    Err(RequestTooLarge {
        request_bytes,
        limit_bytes: policy.max_request_bytes,
        image_count,
        image_bytes,
    })
}

pub fn inline_image_stats(messages: &[Value]) -> (usize, usize) {
    collect_image_parts(messages)
        .iter()
        .fold((0, 0), |(count, bytes), part| {
            (count + 1, bytes.saturating_add(part.url_len))
        })
}

/// Serialized bytes of the exact message/tool payload the provider receives,
/// plus a small envelope reserve. Images dominate this number; keeping the
/// accounting in one place lets the image budget and the size guard agree.
pub fn serialized_request_bytes(messages: &[Value], tools: Option<&[Value]>) -> usize {
    let mut counter = ByteCounter::default();
    if serde_json::to_writer(&mut counter, messages).is_err() {
        return usize::MAX;
    }
    if let Some(tools) = tools {
        if serde_json::to_writer(&mut counter, tools).is_err() {
            return usize::MAX;
        }
    }
    counter.0.saturating_add(REQUEST_BODY_OVERHEAD_BYTES)
}

pub fn serialized_bytes<T: Serialize + ?Sized>(value: &T) -> usize {
    let mut counter = ByteCounter::default();
    if serde_json::to_writer(&mut counter, value).is_err() {
        return usize::MAX;
    }
    counter.0
}

fn enforce_image_budget_with_limits(
    messages: &mut [Value],
    policy: &ImageContextPolicy,
    max_images: usize,
    max_bytes: usize,
    max_current_turn_images: usize,
    min_keep_images: usize,
) -> ImageBudgetReport {
    let parts = collect_image_parts(messages);
    let mut report = ImageBudgetReport {
        images_before: parts.len(),
        bytes_before: parts.iter().map(|part| part.url_len).sum(),
        ..Default::default()
    };
    if parts.is_empty() {
        return report;
    }

    let mut reasons: HashMap<usize, DemoteReason> = HashMap::new();

    // Keep the newest copy of an exact repeated read. Visual iteration often
    // reads the same unchanged frame several times; only the pixels matter.
    let mut by_digest: HashMap<(u64, usize), Vec<usize>> = HashMap::new();
    for (index, part) in parts.iter().enumerate() {
        by_digest
            .entry((part.digest, part.url_len))
            .or_default()
            .push(index);
    }
    for indexes in by_digest.values() {
        if indexes.len() <= 1 {
            continue;
        }
        for &index in &indexes[..indexes.len() - 1] {
            reasons.entry(index).or_insert(DemoteReason::Duplicate);
        }
    }

    // A rendered file is usually overwritten before it is read again. Keep the
    // newest versions of one path and demote older ones as superseded.
    let mut by_path: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, part) in parts.iter().enumerate() {
        if reasons.contains_key(&index) || !part.internal_followup {
            continue;
        }
        if let Some(path) = part.path.as_deref() {
            by_path.entry(path).or_default().push(index);
        }
    }
    let keep_versions = policy.max_versions_per_path.max(1);
    for indexes in by_path.values() {
        if indexes.len() <= keep_versions {
            continue;
        }
        for &index in &indexes[..indexes.len() - keep_versions] {
            reasons.entry(index).or_insert(DemoteReason::Superseded);
        }
    }

    // Total budget. Eviction is **batch and monotone**: the unit is the whole
    // user turn, never a single image. A one-image window slides on every new
    // read, which rewrites one earlier message per round and invalidates the
    // provider prompt cache from that point on (measured: 47/50 -> 4/50 shared
    // prefix after the 17th image). Dropping the oldest turns in batches keeps
    // the inline/placeholder state stable across turns instead.
    let mut remaining: Vec<usize> = (0..parts.len())
        .filter(|index| !reasons.contains_key(index))
        .collect();
    let newest_keep: HashSet<usize> = remaining
        .iter()
        .rev()
        .take(min_keep_images)
        .copied()
        .collect();
    let mut image_count = remaining.len();
    let mut image_bytes: usize = remaining.iter().map(|&index| parts[index].url_len).sum();

    // 当前用户轮次封顶：本轮（最新用户消息 + 它的工具回读）读的图只占
    // `max_current_turn_images` 个名额，超出就从本轮最旧的开始降级。这样新图
    // 入场不会挤掉历史轮次的图，前缀断点只出现在本轮尾部，跨轮前缀保持稳定。
    if max_current_turn_images > 0 {
        // 名额按本轮所有图计算，但用户亲手附的图受保护，只有工具回读产生的
        // 跟进图会被淘汰。
        let current_total = remaining
            .iter()
            .filter(|index| parts[**index].in_current_turn)
            .count();
        let turn_indexes: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|index| parts[*index].in_current_turn && !parts[*index].protected)
            .collect();
        if current_total > max_current_turn_images {
            let excess = current_total - max_current_turn_images;
            for index in turn_indexes.into_iter().take(excess) {
                let Some(position) = remaining.iter().position(|entry| *entry == index) else {
                    continue;
                };
                remaining.remove(position);
                image_count = image_count.saturating_sub(1);
                image_bytes = image_bytes.saturating_sub(parts[index].url_len);
                reasons.insert(index, DemoteReason::Budget);
                report.budget_images_removed = report.budget_images_removed.saturating_add(1);
            }
        }
    }

    while image_count > max_images || image_bytes > max_bytes {
        let Some(turn) = oldest_evictable_turn(&remaining, &parts, &newest_keep) else {
            break;
        };
        // Prefer evicting the whole turn; if that would drop more than needed
        // and the turn is large, drop its oldest images one by one so the
        // request still fits the ceiling.
        let turn_indexes: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|index| parts[*index].turn == turn)
            .collect();
        let turn_bytes: usize = turn_indexes.iter().map(|index| parts[*index].url_len).sum();
        let can_drop_turn = image_count - turn_indexes.len() >= min_keep_images
            && image_bytes.saturating_sub(turn_bytes) <= max_bytes;
        let drop_indexes: Vec<usize> = if can_drop_turn {
            turn_indexes
        } else {
            turn_indexes.into_iter().take(1).collect()
        };
        for index in drop_indexes {
            let Some(position) = remaining.iter().position(|entry| *entry == index) else {
                continue;
            };
            remaining.remove(position);
            image_count = image_count.saturating_sub(1);
            image_bytes = image_bytes.saturating_sub(parts[index].url_len);
            reasons.insert(index, DemoteReason::Budget);
            report.budget_images_removed = report.budget_images_removed.saturating_add(1);
        }
    }

    for (index, part) in parts.iter().enumerate() {
        let Some(reason) = reasons.get(&index).copied() else {
            continue;
        };
        match reason {
            DemoteReason::Duplicate => {
                report.duplicate_images_removed = report.duplicate_images_removed.saturating_add(1);
            }
            DemoteReason::Superseded => {
                report.superseded_images_removed =
                    report.superseded_images_removed.saturating_add(1);
            }
            DemoteReason::Budget => {}
        }
        let note = image_omitted_note(part.path.as_deref());
        if let Some(slot) = messages
            .get_mut(part.message_index)
            .and_then(|message| message.get_mut("content"))
            .and_then(Value::as_array_mut)
            .and_then(|content| content.get_mut(part.part_index))
        {
            *slot = json!({ "type": "text", "text": note });
        }
    }

    report.images_after = parts.len().saturating_sub(reasons.len());
    report.bytes_after = image_bytes;
    report
}

/// Oldest user turn that may still lose images: never the newest user message
/// and never one of the images we always keep.
fn oldest_evictable_turn(
    remaining: &[usize],
    parts: &[ImagePartRef],
    newest_keep: &HashSet<usize>,
) -> Option<usize> {
    remaining
        .iter()
        .filter(|index| !parts[**index].protected && !newest_keep.contains(index))
        .map(|index| parts[*index].turn)
        .min()
}

fn collect_image_parts(messages: &[Value]) -> Vec<ImagePartRef> {
    let latest_user_message = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
    // 当前用户轮次的起点：最后一条“不是工具回读”的用户消息。它之后的内部
    // 跟进都属于同一轮，数量再多也不该被当成很多历史轮次。
    let current_round_start = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.get("role").and_then(Value::as_str) == Some("user"))
        .filter(|(_, message)| match message.get("content") {
            Some(Value::Array(parts)) => !is_internal_followup(message, parts),
            _ => true,
        })
        .map(|(index, _)| index)
        .next_back()
        .unwrap_or(0);
    let mut parts = Vec::new();
    let mut turn = 0_usize;
    for (message_index, message) in messages.iter().enumerate() {
        let is_user = message.get("role").and_then(Value::as_str) == Some("user");
        if is_user {
            turn = turn.saturating_add(1);
        }
        let Some(content) = message.get("content").and_then(Value::as_array) else {
            continue;
        };
        let internal_followup = is_internal_followup(message, content);
        let protected = Some(message_index) == latest_user_message;
        for (part_index, part) in content.iter().enumerate() {
            let Some(url) = inline_image_url(part) else {
                continue;
            };
            parts.push(ImagePartRef {
                message_index,
                part_index,
                url_len: url.len(),
                digest: digest_url(url),
                path: image_path_before(content, part_index),
                internal_followup,
                protected,
                turn,
                in_current_turn: message_index >= current_round_start,
            });
        }
    }
    parts
}

fn is_internal_followup(message: &Value, content: &[Value]) -> bool {
    if message
        .pointer("/meta/type")
        .and_then(Value::as_str)
        .is_some_and(|value| value == "model_context_internal")
    {
        return true;
    }
    // Rebuilt history drops message meta, but tool follow-ups keep the path
    // hint that lets the image be re-read after compaction. A real user turn
    // also carries a path hint for the image it attached, so the hint alone
    // only counts when the message carries no user-authored text part.
    if content.iter().any(is_user_authored_text) {
        return false;
    }
    content.iter().any(|part| {
        part.get("text")
            .and_then(Value::as_str)
            .and_then(image_path_marker_in_text)
            .is_some()
    })
}

/// 用户自己写的那段文字（不是路径提示、不是占位）。带它的消息是一个真实
/// 用户轮次，不是工具回读产生的内部跟进。
fn is_user_authored_text(part: &Value) -> bool {
    let Some(text) = part.get("text").and_then(Value::as_str) else {
        return false;
    };
    let text = text.trim();
    !text.is_empty()
        && image_path_marker_in_text(text).is_none()
        && !is_omitted_image_placeholder_text(text)
}

fn inline_image_url(part: &Value) -> Option<&str> {
    let image_url = part.get("image_url")?;
    let url = match image_url {
        Value::String(value) => value.as_str(),
        Value::Object(map) => map.get("url")?.as_str()?,
        _ => return None,
    };
    let trimmed = url.trim();
    let prefix = "data:image/";
    if trimmed
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    {
        Some(trimmed)
    } else {
        None
    }
}

fn image_path_before(content: &[Value], part_index: usize) -> Option<String> {
    content.iter().take(part_index).rev().find_map(|part| {
        part.get("text")
            .and_then(Value::as_str)
            .and_then(image_path_marker_in_text)
    })
}

fn digest_url(url: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

fn image_omitted_note(path: Option<&str>) -> String {
    omitted_image_placeholder_with_path(path)
}

#[derive(Default)]
struct ByteCounter(usize);

impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::chat_payload_sanitizer::{
        omitted_image_path_in_text, INLINE_IMAGE_OMITTED_TEXT,
    };

    fn policy(max_images: usize, max_bytes: usize, max_versions: usize) -> ImageContextPolicy {
        ImageContextPolicy {
            max_images,
            max_total_bytes: max_bytes,
            max_versions_per_path: max_versions,
            max_request_bytes: 8 * 1024 * 1024,
            max_current_turn_images: max_images,
        }
    }

    #[test]
    fn hundred_hd_screenshots_stay_within_default_budget() {
        // 100 张降规格后的 1080p 入模视图（1024×576 JPEG，base64 约 200KB）。
        // 历史会一直带着这些图，预算必须在字节维度封顶，而不是等 token 兜底。
        let mut messages: Vec<Value> = (0..100)
            .map(|index| {
                let url = format!("data:image/jpeg;base64,{}{}", "A".repeat(200 * 1024), index);
                serde_json::json!({ "role": "user", "content": [ image(&url) ] })
            })
            .collect();
        let defaults = ImageContextPolicy {
            max_images: DEFAULT_MAX_CONTEXT_IMAGES,
            max_total_bytes: DEFAULT_MAX_CONTEXT_IMAGE_BYTES,
            max_versions_per_path: DEFAULT_MAX_VERSIONS_PER_PATH,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
            max_current_turn_images: DEFAULT_MAX_CURRENT_TURN_IMAGES,
        };
        let report = enforce_image_budget(&mut messages, &defaults);
        let (count, bytes) = inline_image_stats(&messages);
        assert!(
            count <= DEFAULT_MAX_CONTEXT_IMAGES,
            "inline images {count} 超出默认上限"
        );
        assert!(
            bytes <= DEFAULT_MAX_CONTEXT_IMAGE_BYTES,
            "inline bytes {bytes} 超出默认预算"
        );
        assert!(report.budget_images_removed > 0, "超预算时必须裁掉旧图");
    }

    fn image(url: &str) -> Value {
        serde_json::json!({
            "type": "image_url",
            "image_url": { "url": url }
        })
    }

    fn followup(path: &str, url: &str) -> Value {
        serde_json::json!({
            "role": "user",
            "meta": { "type": "model_context_internal", "source": "read_image_followup" },
            "content": [
                { "type": "text", "text": format!("[image_path:{path}]") },
                image(url)
            ]
        })
    }

    fn message(content: Vec<Value>) -> Value {
        serde_json::json!({ "role": "user", "content": content })
    }

    #[test]
    fn identical_images_keep_only_latest_copy() {
        let url = "data:image/png;base64,AAAA";
        let mut messages = vec![
            followup("/tmp/a.png", url),
            followup("/tmp/a.png", url),
            followup("/tmp/a.png", url),
        ];

        let report = enforce_image_budget(&mut messages, &policy(24, 24 * 1024 * 1024, 2));

        assert_eq!(report.duplicate_images_removed, 2);
        assert_eq!(report.images_after, 1);
        assert_eq!(inline_image_stats(&messages).0, 1);
        let last = &messages[2]["content"][1];
        assert_eq!(last["type"], "image_url");
        assert_eq!(messages[0]["content"][1]["type"], "text");
        assert_eq!(messages[1]["content"][1]["type"], "text");
    }

    #[test]
    fn repeated_path_keeps_latest_versions_only() {
        let mut messages = vec![
            followup("/tmp/frame.png", "data:image/png;base64,AAAA"),
            followup("/tmp/frame.png", "data:image/png;base64,BBBB"),
            followup("/tmp/frame.png", "data:image/png;base64,CCCC"),
        ];

        let report = enforce_image_budget(&mut messages, &policy(24, 24 * 1024 * 1024, 1));

        assert_eq!(report.superseded_images_removed, 2);
        assert_eq!(inline_image_stats(&messages).0, 1);
        assert_eq!(messages[2]["content"][1]["type"], "image_url");
        assert_eq!(messages[0]["content"][1]["type"], "text");
        assert_eq!(messages[1]["content"][1]["type"], "text");
    }

    #[test]
    fn byte_budget_drops_whole_oldest_turn() {
        // 每张图各占一个用户轮次；预算只够两轮时整轮丢弃，而不是逐张滑动。
        let mut messages: Vec<Value> = (0..6)
            .map(|index| {
                message(vec![image(&format!(
                    "data:image/png;base64,{}",
                    "A".repeat(1000 + index)
                ))])
            })
            .collect();

        let report = enforce_image_budget(&mut messages, &policy(24, 4110, 2));

        assert_eq!(report.budget_images_removed, 2);
        assert_eq!(inline_image_stats(&messages).0, 4);
        for entry in &messages[..2] {
            assert_eq!(entry["content"][0]["type"], "text");
        }
        for entry in &messages[2..] {
            assert_eq!(entry["content"][0]["type"], "image_url");
        }
    }

    #[test]
    fn eviction_is_batch_monotone_across_turns() {
        // 回归：单张窗口滑动会让每轮改写一条更早的消息，前缀缓存全断。
        // 批量淘汰后，第 N 轮淘汰掉的图在第 N+1 轮仍然是占位，内联集合只缩不涨。
        let mut messages: Vec<Value> = (0..10)
            .map(|index| {
                followup(
                    &format!("/tmp/frame_{index}.png"),
                    &format!("data:image/png;base64,{}{index}", "B".repeat(1000)),
                )
            })
            .collect();
        let limits = policy(4, 24 * 1024 * 1024, 1);

        enforce_image_budget(&mut messages, &limits);
        let first: Vec<bool> = messages
            .iter()
            .map(|entry| entry["content"][1]["type"] == "image_url")
            .collect();
        assert_eq!(first.iter().filter(|inline| **inline).count(), 4);

        // 追加两张新图后再次执行：先前被淘汰的仍是占位，只有新的可能被淘汰。
        for index in 10..12 {
            messages.push(followup(
                &format!("/tmp/frame_{index}.png"),
                &format!("data:image/png;base64,{}{index}", "B".repeat(1000)),
            ));
        }
        enforce_image_budget(&mut messages, &limits);
        for (index, inline) in first.iter().enumerate() {
            if !*inline {
                assert_eq!(
                    messages[index]["content"][1]["type"], "text",
                    "第 {index} 条曾被淘汰的图又被内联回来了，前缀会再次抖动"
                );
            }
        }
        // 淘汰单位是轮次：要么整轮留下，要么整轮变占位，不会出现半轮状态。
        let inline_count = inline_image_stats(&messages).0;
        assert!(inline_count <= 4, "内联图 {inline_count} 超出上限");
        assert_eq!(
            inline_count % 1,
            0,
            "每个轮次只有一张图时，内联数必然是整轮数"
        );
        let tail: Vec<bool> = messages[10..]
            .iter()
            .map(|entry| entry["content"][1]["type"] == "image_url")
            .collect();
        assert_eq!(tail, vec![true, true], "最新两张必须保留");
    }

    #[test]
    fn evicted_image_uses_the_persisted_placeholder_text() {
        // 请求内淘汰与持久化省略必须是同一段文本，否则下一轮占位文案变化，
        // 同一条消息被重新投影，前缀从这里断开。
        let mut messages: Vec<Value> = (0..3)
            .map(|index| {
                followup(
                    &format!("/tmp/frame_{index}.png"),
                    &format!("data:image/png;base64,{}", "D".repeat(1000)),
                )
            })
            .collect();

        enforce_image_budget(&mut messages, &policy(2, 24 * 1024 * 1024, 1));

        let placeholder = messages[0]["content"][1]["text"]
            .as_str()
            .expect("placeholder text");
        assert!(
            placeholder.starts_with(INLINE_IMAGE_OMITTED_TEXT),
            "占位必须以统一文案开头，实际为 {placeholder}"
        );
        assert_eq!(
            omitted_image_path_in_text(placeholder).as_deref(),
            Some("/tmp/frame_0.png")
        );
    }

    #[test]
    fn current_turn_cap_keeps_history_inline() {
        // 回归：一个轮次里连续读很多图时，如果只按总预算淘汰，新图会把历史
        // 轮次的图逐张挤出去，跨轮前缀每轮都断。当前轮次封顶后，历史保持
        // 内联，只有本轮最旧的图降级。
        let mut messages: Vec<Value> = (0..4)
            .map(|index| {
                message(vec![
                    serde_json::json!({ "type": "text", "text": format!("看第 {index} 张") }),
                    serde_json::json!({ "type": "text", "text": format!("[image_path:/tmp/history_{index}.png]") }),
                    image(&format!("data:image/png;base64,{}{index}", "H".repeat(1000))),
                ])
            })
            .collect();
        // 本轮用户消息：自己不打图，只是让模型去连续看图。
        messages.push(message(vec![serde_json::json!({
            "type": "text",
            "text": "连续看图"
        })]));
        let mut limits = policy(16, 24 * 1024 * 1024, 1);
        limits.max_current_turn_images = 4;

        for index in 0..10 {
            let mut entry = followup(
                &format!("/tmp/round_{index}.png"),
                &format!("data:image/png;base64,{}{index}", "R".repeat(1000)),
            );
            entry["meta"] = serde_json::json!({
                "type": "model_context_internal",
                "source": "read_image_followup",
                "round": index
            });
            messages.push(entry);
        }

        let report = enforce_image_budget(&mut messages, &limits);

        assert_eq!(report.budget_images_removed, 6);
        for entry in &messages[..4] {
            assert_eq!(
                entry["content"][2]["type"], "image_url",
                "历史轮次的图被本轮新图挤掉了，跨轮前缀会断"
            );
        }
        let round_inline = messages[5..]
            .iter()
            .filter(|entry| entry["content"][1]["type"] == "image_url")
            .count();
        assert_eq!(round_inline, 4, "本轮只保留最新 4 张");
        assert_eq!(inline_image_stats(&messages).0, 8);
    }


    #[test]
    fn newest_user_message_images_are_protected() {
        let mut messages = vec![message(
            (0..6)
                .map(|index| {
                    image(&format!(
                        "data:image/png;base64,{}",
                        "B".repeat(1000 + index)
                    ))
                })
                .collect(),
        )];

        let report = enforce_image_budget(&mut messages, &policy(2, 100, 1));

        assert_eq!(report.budget_images_removed, 0);
        assert_eq!(inline_image_stats(&messages).0, 6);
    }

    #[test]
    fn request_guard_rejects_payload_when_images_cannot_fix_it() {
        let mut messages = vec![
            message(vec![serde_json::json!({
                "type": "text",
                "text": "x".repeat(4096)
            })]),
            message(vec![image("data:image/png;base64,AAAA")]),
        ];
        let mut small = policy(24, 24 * 1024 * 1024, 2);
        small.max_request_bytes = 512;

        let error = enforce_request_limits(&mut messages, None, &small)
            .expect_err("text-only payload should not fit a 512-byte limit");

        assert!(error.request_bytes > error.limit_bytes);
        assert_eq!(error.image_count, 1);
    }
    #[test]
    fn emergency_pass_shrinks_below_newest_four_to_fit_request_limit() {
        let mut messages: Vec<Value> = (0..4)
            .map(|index| {
                message(vec![image(&format!(
                    "data:image/png;base64,{}",
                    "D".repeat(1000 + index)
                ))])
            })
            .collect();
        let mut limited = policy(24, 24 * 1024 * 1024, 2);
        limited.max_request_bytes = 4000;

        let report = enforce_request_limits(&mut messages, None, &limited)
            .expect("the newest image alone should fit");

        assert_eq!(inline_image_stats(&messages).0, 1);
        assert_eq!(report.budget_images_removed, 3);
        assert_eq!(messages[3]["content"][0]["type"], "image_url");
    }

    #[test]
    fn request_bytes_include_inline_image_payloads() {
        let without = vec![message(vec![serde_json::json!({
            "type": "text",
            "text": "hello"
        })])];
        let url = format!("data:image/png;base64,{}", "C".repeat(2048));
        let with = vec![message(vec![image(&url)])];

        let baseline = serialized_request_bytes(&without, None);
        let bounded = serialized_request_bytes(&with, None);

        assert!(bounded > baseline + url.len());
    }
}
