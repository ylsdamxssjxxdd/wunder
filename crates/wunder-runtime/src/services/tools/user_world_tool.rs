use super::{build_model_tool_success, context::ToolContext};
use crate::i18n;
use crate::path_utils::is_within_root;
use crate::user_store::UserStore;
use anyhow::{anyhow, Result};
use chrono::Utc;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path};
use std::sync::OnceLock;
use uuid::Uuid;
use walkdir::WalkDir;

const MAX_USER_WORLD_LIST_LIMIT: i64 = 500;
const USER_WORLD_FILE_STAGING_DIR: &str = "user_world_uploads";

#[derive(Debug, Deserialize)]
struct UserWorldToolArgs {
    action: String,
    #[serde(default)]
    keyword: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    user_ids: Option<Vec<String>>,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

pub(crate) async fn execute_user_world_tool(
    context: &ToolContext<'_>,
    args: &Value,
) -> Result<Value> {
    let payload: UserWorldToolArgs =
        serde_json::from_value(args.clone()).map_err(|err| anyhow!(err.to_string()))?;
    let action = payload.action.trim().to_lowercase();
    match action.as_str() {
        "list_users" | "list" | "users" => user_world_list_users(context, &payload).await,
        "send_message" | "send" | "message" => user_world_send_message(context, &payload).await,
        _ => Err(anyhow!("未知用户世界工具 action: {action}")),
    }
}

async fn user_world_list_users(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let keyword = payload
        .keyword
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let offset = payload.offset.unwrap_or(0).max(0);
    let limit = payload.limit.unwrap_or(0);
    let limit = if limit <= 0 {
        0
    } else {
        limit.clamp(1, MAX_USER_WORLD_LIST_LIMIT)
    };
    let user_store = UserStore::new(context.storage.clone());
    let (users, total) = user_store.list_users(keyword, None, offset, limit)?;
    let items = users
        .into_iter()
        .map(|user| {
            json!({
                "user_id": user.user_id,
                "username": user.username,
                "status": user.status,
                "unit_id": user.unit_id
            })
        })
        .collect::<Vec<_>>();
    Ok(build_model_tool_success(
        "list_users",
        "completed",
        format!("Listed {total} users from user world."),
        json!({
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }),
    ))
}

#[derive(Debug, Clone)]
struct UserWorldFileRefMatch {
    token_start: usize,
    token_end: usize,
    normalized_path: String,
    suffix: String,
}

#[derive(Debug, Clone)]
struct UserWorldCopiedFile {
    source_path: String,
    staged_path: String,
    entry_type: &'static str,
}

fn user_world_file_ref_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?m)(^|[\s\n])@("[^"]+"|'[^']+'|\S+)"#)
            .expect("user world file ref regex must be valid")
    })
}

fn is_user_world_file_ref_suffix(ch: char) -> bool {
    matches!(
        ch,
        ')' | ']'
            | '}'
            | '>'
            | ','
            | '.'
            | ';'
            | ':'
            | '!'
            | '?'
            | '，'
            | '。'
            | '；'
            | '：'
            | '！'
            | '？'
            | '）'
            | '】'
            | '》'
            | '、'
    )
}

fn split_user_world_file_ref_suffix(value: &str) -> (&str, &str) {
    let mut split_at = value.len();
    for (index, ch) in value.char_indices().rev() {
        if is_user_world_file_ref_suffix(ch) {
            split_at = index;
        } else {
            break;
        }
    }
    if split_at == value.len() {
        (value, "")
    } else {
        (&value[..split_at], &value[split_at..])
    }
}

fn looks_like_user_world_file_ref(raw: &str, normalized: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('@') {
        return false;
    }
    if raw.starts_with('/')
        || raw.starts_with('\\')
        || raw.starts_with("./")
        || raw.starts_with("../")
        || raw.starts_with("workspaces/")
        || raw.starts_with("/workspaces/")
        || raw.starts_with("workspace/")
        || raw.starts_with("/workspace/")
    {
        return true;
    }
    normalized.contains('/') || normalized.contains('.')
}

fn normalize_user_world_file_ref_path(raw: &str, source_workspace_id: &str) -> Option<String> {
    let mut value = raw.trim().replace('\\', "/");
    if value.is_empty() {
        return None;
    }
    if let Some(stripped) = value.strip_prefix("/workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("workspaces/") {
        let stripped = stripped.trim_matches('/');
        let mut segments = stripped.splitn(2, '/');
        let owner = segments.next().unwrap_or("").trim();
        let rest = segments.next().unwrap_or("").trim();
        if owner.is_empty() || rest.is_empty() {
            return None;
        }
        if owner != source_workspace_id {
            return None;
        }
        value = rest.to_string();
    } else if let Some(stripped) = value.strip_prefix("/workspace/") {
        value = stripped.trim_matches('/').to_string();
    } else if let Some(stripped) = value.strip_prefix("workspace/") {
        value = stripped.trim_matches('/').to_string();
    }
    while let Some(stripped) = value.strip_prefix("./") {
        value = stripped.to_string();
    }
    value = value.trim_start_matches('/').trim().to_string();
    if value.is_empty() {
        return None;
    }
    if !looks_like_user_world_file_ref(raw, &value) {
        return None;
    }
    let candidate = Path::new(&value);
    for component in candidate.components() {
        match component {
            Component::ParentDir | Component::Prefix(_) | Component::RootDir => return None,
            Component::CurDir | Component::Normal(_) => {}
        }
    }
    Some(value)
}

fn extract_user_world_file_refs(
    content: &str,
    source_workspace_id: &str,
) -> Vec<UserWorldFileRefMatch> {
    let mut items = Vec::new();
    for captures in user_world_file_ref_regex().captures_iter(content) {
        let Some(token_match) = captures.get(2) else {
            continue;
        };
        let token = token_match.as_str();
        if token.trim().is_empty() {
            continue;
        }
        let wrapped_in_quotes = ((token.starts_with('"') && token.ends_with('"'))
            || (token.starts_with('\'') && token.ends_with('\'')))
            && token.len() >= 2;
        let (raw_path, suffix) = if wrapped_in_quotes {
            (&token[1..token.len().saturating_sub(1)], "")
        } else {
            split_user_world_file_ref_suffix(token)
        };
        let Some(normalized_path) =
            normalize_user_world_file_ref_path(raw_path, source_workspace_id)
        else {
            continue;
        };
        items.push(UserWorldFileRefMatch {
            token_start: token_match.start(),
            token_end: token_match.end(),
            normalized_path,
            suffix: suffix.to_string(),
        });
    }
    items
}

fn copy_user_world_staged_path(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in WalkDir::new(source).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_symlink() {
                return Err(anyhow!(
                    "symbolic links are not supported in user_world file refs"
                ));
            }
            let relative = entry.path().strip_prefix(source).unwrap_or(entry.path());
            let target = destination.join(relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target)?;
            } else if entry.file_type().is_file() {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(entry.path(), &target)?;
            }
        }
        return Ok(());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, destination)?;
    Ok(())
}

fn stage_user_world_file_refs(
    context: &ToolContext<'_>,
    content: &str,
) -> Result<(String, Vec<UserWorldCopiedFile>)> {
    let matches = extract_user_world_file_refs(content, context.workspace_id);
    if matches.is_empty() {
        return Ok((content.to_string(), Vec::new()));
    }
    let source_workspace_id = context.workspace_id;
    let sender_workspace_id = context.workspace.scoped_user_id(context.user_id, None);
    let source_root = context.workspace.ensure_user_root(source_workspace_id)?;
    let _ = context.workspace.ensure_user_root(&sender_workspace_id)?;
    let transfer_id = format!(
        "{}_{}",
        Utc::now().format("%Y%m%d%H%M%S"),
        Uuid::new_v4().simple()
    );
    let mut staged_path_map = HashMap::<String, UserWorldCopiedFile>::new();
    for item in &matches {
        if staged_path_map.contains_key(&item.normalized_path) {
            continue;
        }
        let source_target = context
            .workspace
            .resolve_path(source_workspace_id, &item.normalized_path)?;
        if !source_target.exists() {
            return Err(anyhow!(
                "user_world file ref not found in workspace: {}",
                item.normalized_path
            ));
        }
        if !is_within_root(&source_root, &source_target) {
            return Err(anyhow!(
                "user_world file ref is outside workspace: {}",
                item.normalized_path
            ));
        }
        let source_meta = fs::symlink_metadata(&source_target)?;
        if source_meta.file_type().is_symlink() {
            return Err(anyhow!(
                "symbolic links are not supported in user_world file refs: {}",
                item.normalized_path
            ));
        }
        let staged_path = format!(
            "{}/{}/{}",
            USER_WORLD_FILE_STAGING_DIR, transfer_id, item.normalized_path
        );
        let destination = context
            .workspace
            .resolve_path(&sender_workspace_id, &staged_path)?;
        copy_user_world_staged_path(&source_target, &destination)?;
        staged_path_map.insert(
            item.normalized_path.clone(),
            UserWorldCopiedFile {
                source_path: item.normalized_path.clone(),
                staged_path,
                entry_type: if source_target.is_dir() {
                    "dir"
                } else {
                    "file"
                },
            },
        );
    }
    let mut rewritten = String::with_capacity(content.len() + matches.len() * 32);
    let mut cursor = 0usize;
    for item in &matches {
        let Some(staged) = staged_path_map.get(&item.normalized_path) else {
            continue;
        };
        rewritten.push_str(&content[cursor..item.token_start]);
        let quoted_path = staged.staged_path.replace('"', "%22");
        rewritten.push('"');
        rewritten.push_str(&quoted_path);
        rewritten.push('"');
        rewritten.push_str(&item.suffix);
        cursor = item.token_end;
    }
    rewritten.push_str(&content[cursor..]);
    let mut copied = staged_path_map.into_values().collect::<Vec<_>>();
    copied.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok((rewritten, copied))
}

async fn user_world_send_message(
    context: &ToolContext<'_>,
    payload: &UserWorldToolArgs,
) -> Result<Value> {
    let user_world = context
        .user_world
        .as_ref()
        .ok_or_else(|| anyhow!(i18n::t("error.internal_error")))?;
    let sender = context.user_id.trim();
    if sender.is_empty() {
        return Err(anyhow!(i18n::t("error.user_id_required")));
    }
    let content = payload.content.as_deref().unwrap_or("").trim();
    if content.is_empty() {
        return Err(anyhow!("content is required"));
    }
    let content_type = payload
        .content_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("text");
    let mut raw_targets = Vec::new();
    if let Some(user_id) = payload.user_id.as_deref() {
        raw_targets.push(user_id.to_string());
    }
    if let Some(user_ids) = payload.user_ids.as_ref() {
        raw_targets.extend(user_ids.iter().map(|value| value.to_string()));
    }
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for raw in raw_targets {
        let cleaned = raw.trim();
        if cleaned.is_empty() {
            continue;
        }
        if seen.insert(cleaned.to_string()) {
            targets.push(cleaned.to_string());
        }
    }
    if targets.is_empty() {
        return Err(anyhow!("user_id or user_ids required"));
    }
    let user_store = UserStore::new(context.storage.clone());
    let mut target_exists = HashMap::new();
    let mut has_valid_target = false;
    for target in &targets {
        if target == sender {
            continue;
        }
        let exists = user_store.get_user_by_id(target)?.is_some();
        if exists {
            has_valid_target = true;
        }
        target_exists.insert(target.to_string(), exists);
    }
    let (content, copied_files) = if has_valid_target {
        stage_user_world_file_refs(context, content)?
    } else {
        (content.to_string(), Vec::new())
    };
    let mut results = Vec::new();
    for target in targets {
        if target == sender {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "cannot send to self"
            }));
            continue;
        }
        if !target_exists.get(&target).copied().unwrap_or(false) {
            results.push(json!({
                "user_id": target,
                "ok": false,
                "error": "user not found"
            }));
            continue;
        }
        let now = now_ts();
        let conversation =
            match user_world.resolve_or_create_direct_conversation(sender, &target, now) {
                Ok(value) => value,
                Err(err) => {
                    results.push(json!({
                        "user_id": target,
                        "ok": false,
                        "error": err.to_string()
                    }));
                    continue;
                }
            };
        let send_result = match user_world
            .send_message(
                sender,
                &conversation.conversation_id,
                &content,
                content_type,
                payload.client_msg_id.as_deref(),
                now,
            )
            .await
        {
            Ok(value) => value,
            Err(err) => {
                results.push(json!({
                    "user_id": target,
                    "ok": false,
                    "error": err.to_string()
                }));
                continue;
            }
        };
        results.push(json!({
            "user_id": target,
            "ok": true,
            "conversation_id": conversation.conversation_id,
            "message_id": send_result.message.message_id,
            "inserted": send_result.inserted
        }));
    }
    Ok(build_model_tool_success(
        "send_message",
        "completed",
        format!("Processed {} user world message deliveries.", results.len()),
        json!({
            "results": results,
            "staged_files": copied_files.iter().map(|item| {
                json!({
                    "source_path": item.source_path,
                    "staged_path": item.staged_path,
                    "entry_type": item.entry_type
                })
            }).collect::<Vec<_>>()
        }),
    ))
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::extract_user_world_file_refs;

    #[test]
    fn extract_user_world_file_refs_handles_quotes_suffix_and_email_mentions() {
        let content =
            "查看 @./docs/report.md, 以及 @\"assets/my file.txt\"，并抄送 @alice@example.com";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].normalized_path, "docs/report.md");
        assert_eq!(refs[0].suffix, ",");
        assert_eq!(refs[1].normalized_path, "assets/my file.txt");
        assert_eq!(refs[1].suffix, "");
    }

    #[test]
    fn extract_user_world_file_refs_accepts_workspace_prefixed_token() {
        let content = "@/workspaces/owner__c__2/projects/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].normalized_path, "projects/demo.md");
    }

    #[test]
    fn extract_user_world_file_refs_ignores_mismatched_workspace_owner() {
        let content = "@/workspaces/another_owner/demo.md";
        let refs = extract_user_world_file_refs(content, "owner__c__2");
        assert!(refs.is_empty());
    }
}
