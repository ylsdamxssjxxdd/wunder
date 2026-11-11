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
use crate::services::chat_payload_sanitizer::image_path_marker_in_text;

pub const DEFAULT_MAX_CONTEXT_IMAGES: usize = 16;
/// Total inline image data-URL bytes kept in one request. Base64 inflates raw
/// image bytes by ~1/3, so this leaves room for text and tool schemas under a
/// 32 MiB provider body ceiling.
pub const DEFAULT_MAX_CONTEXT_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const DEFAULT_MAX_VERSIONS_PER_PATH: usize = 1;
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
    let emergency = enforce_image_budget_with_limits(messages, policy, 0, 0, EMERGENCY_KEEP_IMAGES);
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

    // Total budget: always keep images from the newest user message and the
    // newest few overall; evict older tool images first.
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
    while image_count > max_images || image_bytes > max_bytes {
        let candidate = remaining.iter().enumerate().find_map(|(position, &index)| {
            let part = &parts[index];
            (!part.protected && !newest_keep.contains(&index)).then_some((position, index))
        });
        let Some((position, index)) = candidate else {
            break;
        };
        remaining.remove(position);
        image_count = image_count.saturating_sub(1);
        image_bytes = image_bytes.saturating_sub(parts[index].url_len);
        reasons.insert(index, DemoteReason::Budget);
        report.budget_images_removed = report.budget_images_removed.saturating_add(1);
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
        let note = image_omitted_note(reason, part.path.as_deref());
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

fn collect_image_parts(messages: &[Value]) -> Vec<ImagePartRef> {
    let latest_user_message = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
    let mut parts = Vec::new();
    for (message_index, message) in messages.iter().enumerate() {
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
    // hint that lets the image be re-read after compaction.
    content.iter().any(|part| {
        part.get("text")
            .and_then(Value::as_str)
            .and_then(image_path_marker_in_text)
            .is_some()
    })
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

fn image_omitted_note(reason: DemoteReason, path: Option<&str>) -> String {
    let key = match reason {
        DemoteReason::Duplicate => "image.context.omitted_duplicate",
        DemoteReason::Superseded => "image.context.omitted_superseded",
        DemoteReason::Budget => "image.context.omitted_budget",
    };
    let mut note = crate::i18n::t(key);
    if let Some(path) = path.map(str::trim).filter(|path| !path.is_empty()) {
        note.push_str(&crate::i18n::t_with_params(
            "image.context.path_suffix",
            &HashMap::from([("path".to_string(), path.to_string())]),
        ));
    }
    note
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

    fn policy(max_images: usize, max_bytes: usize, max_versions: usize) -> ImageContextPolicy {
        ImageContextPolicy {
            max_images,
            max_total_bytes: max_bytes,
            max_versions_per_path: max_versions,
            max_request_bytes: 8 * 1024 * 1024,
        }
    }

    #[test]
    fn hundred_hd_screenshots_stay_within_default_budget() {
        // 100 张降规格后的 1080p 入模视图（1024×576 JPEG，base64 约 200KB）。
        // 历史会一直带着这些图，预算必须在字节维度封顶，而不是等 token 兜底。
        let mut messages: Vec<Value> = (0..100)
            .map(|index| {
                let url = format!(
                    "data:image/jpeg;base64,{}{}",
                    "A".repeat(200 * 1024),
                    index
                );
                serde_json::json!({ "role": "user", "content": [ image(&url) ] })
            })
            .collect();
        let defaults = ImageContextPolicy {
            max_images: DEFAULT_MAX_CONTEXT_IMAGES,
            max_total_bytes: DEFAULT_MAX_CONTEXT_IMAGE_BYTES,
            max_versions_per_path: DEFAULT_MAX_VERSIONS_PER_PATH,
            max_request_bytes: DEFAULT_MAX_REQUEST_BYTES,
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
    fn byte_budget_evicts_oldest_images_but_keeps_newest_four() {
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
