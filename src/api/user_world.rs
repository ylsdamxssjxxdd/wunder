use crate::api::errors::error_response;
use crate::api::user_context::resolve_user;
use crate::i18n;
use crate::services::desktop_lan::{self, DesktopLanEnvelope};
use crate::services::user_world::{UserWorldContact, UserWorldConversationView};
use crate::state::AppState;
use crate::storage::normalize_workspace_container_id;
use axum::body::Body;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::Response;
use axum::{routing::get, routing::post, Json, Router};
use bytes::Bytes;
use futures::Stream;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::Sha256;
use std::collections::HashSet;
use std::convert::Infallible;
use std::io;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;
use tracing::warn;
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::FileOptions;

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 500;
type HmacSha256 = Hmac<Sha256>;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/wunder/user_world/contacts", get(list_contacts))
        .route(
            "/wunder/user_world/groups",
            get(list_groups).post(create_group),
        )
        .route(
            "/wunder/user_world/groups/{group_id}",
            get(get_group_detail),
        )
        .route(
            "/wunder/user_world/groups/{group_id}/announcement",
            post(update_group_announcement),
        )
        .route(
            "/wunder/user_world/conversations",
            get(list_conversations).post(create_or_get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}",
            get(get_conversation),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/messages",
            get(list_messages).post(send_message),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/read",
            post(mark_read),
        )
        .route(
            "/wunder/user_world/files/download",
            get(download_user_world_file),
        )
        .route(
            "/wunder/user_world/conversations/{conversation_id}/events",
            get(stream_events),
        )
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    keyword: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConversationCreateRequest {
    peer_user_id: String,
}

#[derive(Debug, Deserialize)]
struct GroupCreateRequest {
    group_name: String,
    #[serde(default)]
    member_user_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GroupAnnouncementUpdateRequest {
    #[serde(default)]
    announcement: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListMessageQuery {
    #[serde(default)]
    before_message_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    client_msg_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MarkReadRequest {
    #[serde(default)]
    last_read_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EventQuery {
    #[serde(default)]
    after_event_id: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserWorldFileDownloadQuery {
    conversation_id: String,
    #[serde(default)]
    owner_user_id: Option<String>,
    #[serde(default)]
    container_id: Option<i32>,
    #[serde(default)]
    path: String,
    #[serde(default)]
    check: Option<bool>,
}

async fn list_contacts(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let keyword = query.keyword.as_deref();
    let (mut items, total) = state
        .user_world
        .list_contacts(&resolved.user.user_id, keyword, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let snapshots = state
        .user_presence
        .snapshot_many(items.iter().map(|item| item.user_id.as_str()), now_ts());
    for item in &mut items {
        if let Some(snapshot) = snapshots.get(item.user_id.as_str()) {
            item.online = snapshot.online;
            item.last_seen_at = Some(snapshot.last_seen_at);
        } else {
            item.online = false;
            item.last_seen_at = None;
        }
    }
    append_lan_contacts(state.as_ref(), &resolved.user.user_id, &mut items)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err))?;
    let total = (total as usize).max(items.len()) as i64;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn create_or_get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ConversationCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .resolve_or_create_direct_conversation(
            &resolved.user.user_id,
            payload.peer_user_id.trim(),
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": item })))
}

async fn list_groups(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let (items, total) = state
        .user_world
        .list_groups(&resolved.user.user_id, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn create_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<GroupCreateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .create_group(
            &resolved.user.user_id,
            payload.group_name.trim(),
            &payload.member_user_ids,
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Some(group_id) = item
        .group_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        desktop_lan::manager()
            .register_group_link(group_id, &item.conversation_id)
            .await;
    }
    if let Err(err) =
        relay_group_upsert_to_lan(state.as_ref(), &resolved.user.user_id, &item, &payload).await
    {
        warn!("relay group upsert to lan failed: {err}");
    }
    Ok(Json(json!({ "data": item })))
}

async fn get_group_detail(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(group_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .get_group_detail(&resolved.user.user_id, group_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(item) = item else {
        return Err(error_response(StatusCode::NOT_FOUND, "group not found"));
    };
    Ok(Json(json!({ "data": item })))
}

async fn update_group_announcement(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(group_id): AxumPath<String>,
    Json(payload): Json<GroupAnnouncementUpdateRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .update_group_announcement(
            &resolved.user.user_id,
            group_id.trim(),
            payload.announcement.as_deref(),
            now_ts(),
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(item) = item else {
        return Err(error_response(StatusCode::NOT_FOUND, "group not found"));
    };
    desktop_lan::manager()
        .register_group_link(&item.group_id, &item.conversation_id)
        .await;
    if let Err(err) = relay_group_announcement_to_lan(
        state.as_ref(),
        &resolved.user.user_id,
        &item.group_id,
        payload.announcement.as_deref(),
    )
    .await
    {
        warn!("relay group announcement to lan failed: {err}");
    }
    Ok(Json(json!({ "data": item })))
}

async fn list_conversations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let (offset, limit) = normalize_pagination(query.offset, query.limit);
    let (items, total) = state
        .user_world
        .list_conversations(&resolved.user.user_id, offset, limit)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "total": total,
            "offset": offset,
            "limit": limit
        }
    })))
}

async fn get_conversation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let item = state
        .user_world
        .get_conversation(&resolved.user.user_id, conversation_id.trim())
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let Some(item) = item else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "conversation not found",
        ));
    };
    Ok(Json(json!({ "data": item })))
}

async fn list_messages(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<ListMessageQuery>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let items = state
        .user_world
        .list_messages(
            &resolved.user.user_id,
            conversation_id.trim(),
            query.before_message_id,
            limit,
        )
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({
        "data": {
            "items": items,
            "limit": limit
        }
    })))
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let content = payload.content.trim();
    if content.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "content is required",
        ));
    }
    let result = state
        .user_world
        .send_message(
            &resolved.user.user_id,
            conversation_id.trim(),
            content,
            payload.content_type.as_deref().unwrap_or("text"),
            payload.client_msg_id.as_deref(),
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if let Err(err) = relay_message_to_lan(
        state.as_ref(),
        &resolved.user.user_id,
        conversation_id.trim(),
        &result.message.content,
        &result.message.content_type,
        result.message.client_msg_id.as_deref(),
    )
    .await
    {
        warn!("relay user_world message to lan failed: {err}");
    }
    Ok(Json(json!({ "data": result })))
}

async fn mark_read(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Json(payload): Json<MarkReadRequest>,
) -> Result<Json<Value>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let result = state
        .user_world
        .mark_read(
            &resolved.user.user_id,
            conversation_id.trim(),
            payload.last_read_message_id,
            now_ts(),
        )
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    Ok(Json(json!({ "data": result })))
}

async fn stream_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(conversation_id): AxumPath<String>,
    Query(query): Query<EventQuery>,
) -> Result<Sse<ReceiverStream<Result<Event, Infallible>>>, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let conversation_id = conversation_id.trim().to_string();
    if conversation_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "conversation_id is required",
        ));
    }
    let after_event_id = query.after_event_id.unwrap_or(0).max(0);
    let fetch_limit = normalize_limit(query.limit.unwrap_or(DEFAULT_LIMIT));
    let user_id = resolved.user.user_id.clone();
    let service = state.user_world.clone();
    service
        .list_events(&user_id, &conversation_id, after_event_id, 1)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        let mut current_event_id = after_event_id;
        loop {
            let events = service
                .list_events(&user_id, &conversation_id, current_event_id, fetch_limit)
                .unwrap_or_default();
            for item in events {
                current_event_id = current_event_id.max(item.event_id);
                let payload =
                    serde_json::to_string(&item.payload).unwrap_or_else(|_| "null".to_string());
                let event = Event::default()
                    .event(item.event_type)
                    .id(item.event_id.to_string())
                    .data(payload);
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(700)).await;
        }
    });

    Ok(Sse::new(ReceiverStream::new(rx)))
}

async fn download_user_world_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(params): Query<UserWorldFileDownloadQuery>,
) -> Result<Response, Response> {
    let resolved = resolve_user(&state, &headers, None).await?;
    let requester_id = resolved.user.user_id;
    let conversation_id = params.conversation_id.trim();
    if conversation_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "conversation_id is required".to_string(),
        ));
    }
    let owner_user_id = params
        .owner_user_id
        .as_deref()
        .unwrap_or(&requester_id)
        .trim()
        .to_string();
    if owner_user_id.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "owner_user_id is required".to_string(),
        ));
    }
    let normalized = normalize_relative_path(&params.path);
    if normalized.is_empty() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            i18n::t("workspace.error.path_required"),
        ));
    }
    let check_only = params.check.unwrap_or(false);
    let conversation = state
        .storage
        .get_user_world_conversation(conversation_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if conversation.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "conversation not found".to_string(),
        ));
    }
    let member = state
        .storage
        .get_user_world_member(conversation_id, &requester_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if member.is_none() {
        return Err(error_response(
            StatusCode::FORBIDDEN,
            "forbidden".to_string(),
        ));
    }
    let owner_member = state
        .storage
        .get_user_world_member(conversation_id, &owner_user_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if owner_member.is_none() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "owner not in conversation".to_string(),
        ));
    }

    let workspace_id = if let Some(container_id) = params.container_id {
        state.workspace.scoped_user_id_by_container(
            &owner_user_id,
            normalize_workspace_container_id(container_id),
        )
    } else {
        state.workspace.scoped_user_id(&owner_user_id, None)
    };
    let root = state
        .workspace
        .ensure_user_root(&workspace_id)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !root.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("workspace.error.workspace_not_found"),
        ));
    }
    let target = state
        .workspace
        .resolve_path(&workspace_id, &normalized)
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    if !target.exists() {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            i18n::t("error.file_not_found"),
        ));
    }
    if target.is_dir() {
        let filename_prefix = target
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("archive")
            .to_string();
        let filename = if filename_prefix.to_lowercase().ends_with(".zip") {
            filename_prefix
        } else {
            format!("{filename_prefix}.zip")
        };
        if check_only {
            return Ok(empty_response(&filename, "application/zip"));
        }
        let base_root = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let archive_path = create_temp_archive_file()
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let archive_path_clone = archive_path.clone();
        let target_clone = target.clone();
        let base_clone = base_root.clone();
        tokio::task::spawn_blocking(move || {
            build_archive(&archive_path_clone, &target_clone, &base_clone)
        })
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?
        .map_err(|err| {
            let _ = std::fs::remove_file(&archive_path);
            error_response(StatusCode::BAD_REQUEST, err.to_string())
        })?;
        let file = tokio::fs::File::open(&archive_path)
            .await
            .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
        let stream = TempFileStream::new(archive_path.clone(), ReaderStream::new(file));
        return Ok(stream_response(stream, &filename, "application/zip"));
    }
    let filename = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    if check_only {
        return Ok(empty_response(filename, "application/octet-stream"));
    }
    let file = tokio::fs::File::open(&target)
        .await
        .map_err(|err| error_response(StatusCode::BAD_REQUEST, err.to_string()))?;
    let stream = ReaderStream::new(file);
    Ok(stream_response(
        stream,
        filename,
        "application/octet-stream",
    ))
}

async fn append_lan_contacts(
    state: &AppState,
    current_user_id: &str,
    items: &mut Vec<UserWorldContact>,
) -> Result<(), String> {
    let settings = desktop_lan::manager().settings().await;
    if !settings.enabled {
        return Ok(());
    }
    let peers = desktop_lan::manager().list_peers().await;
    if peers.is_empty() {
        return Ok(());
    }

    let mut existing = items
        .iter()
        .map(|item| item.user_id.clone())
        .collect::<HashSet<_>>();
    let now = now_ts();
    let ttl_seconds = settings.peer_ttl_ms as f64 / 1_000.0;
    let self_peer_id = settings.peer_id.trim();

    for peer in peers {
        let peer_id = peer.peer_id.trim();
        if peer_id.is_empty() || peer_id == self_peer_id {
            continue;
        }
        let user_id = lan_peer_user_id(peer_id, Some(peer.lan_ip.as_str()));
        if !existing.insert(user_id.clone()) {
            continue;
        }
        let username = choose_non_empty(&[peer.display_name.trim(), peer.user_id.trim(), peer_id]);
        let username = normalize_lan_ip(peer.lan_ip.as_str())
            .map_or(username.clone(), |ip| format!("{username} · {ip}"));
        let conversation = state
            .user_world
            .resolve_or_create_direct_conversation(current_user_id, &user_id, now)
            .map_err(|err| err.to_string())?;
        items.push(UserWorldContact {
            user_id,
            username,
            status: "active".to_string(),
            online: now - peer.seen_at <= ttl_seconds.max(3.0),
            last_seen_at: Some(peer.seen_at),
            unit_id: None,
            conversation_id: Some(conversation.conversation_id.clone()),
            last_message_preview: conversation.last_message_preview.clone(),
            last_message_at: (conversation.last_message_at > 0.0)
                .then_some(conversation.last_message_at),
            unread_count: conversation.unread_count_cache,
        });
    }

    items.sort_by(|left, right| {
        let left_ts = left.last_message_at.unwrap_or(0.0);
        let right_ts = right.last_message_at.unwrap_or(0.0);
        right_ts
            .total_cmp(&left_ts)
            .then_with(|| left.username.cmp(&right.username))
    });
    Ok(())
}

async fn relay_message_to_lan(
    state: &AppState,
    sender_user_id: &str,
    conversation_id: &str,
    content: &str,
    content_type: &str,
    client_msg_id: Option<&str>,
) -> Result<(), String> {
    let settings = desktop_lan::manager().settings().await;
    if !settings.enabled {
        return Ok(());
    }
    let Some(conversation) = state
        .storage
        .get_user_world_conversation(conversation_id)
        .map_err(|err| err.to_string())?
    else {
        return Ok(());
    };

    let conversation_type = conversation.conversation_type.trim().to_ascii_lowercase();
    if conversation_type == "direct" {
        let Some(member) = state
            .storage
            .get_user_world_member(conversation_id, sender_user_id)
            .map_err(|err| err.to_string())?
        else {
            return Ok(());
        };
        let peer_user_id = member.peer_user_id.trim();
        let Some(target_peer) = parse_lan_peer_identity(peer_user_id) else {
            return Ok(());
        };
        let payload = json!({
            "content": content,
            "content_type": content_type,
            "client_msg_id": client_msg_id.map(str::to_string),
        });
        relay_envelope_to_lan(
            &settings,
            sender_user_id,
            "uw_direct_message",
            Some(target_peer.peer_id.as_str()),
            target_peer.lan_ip.as_deref(),
            Some(conversation_id),
            payload,
        )
        .await?;
        return Ok(());
    }

    if conversation_type == "group" {
        let global_group_id = desktop_lan::manager()
            .global_group_id_by_conversation(conversation_id, conversation.group_id.as_deref())
            .await
            .or_else(|| conversation.group_id.clone())
            .unwrap_or_else(|| conversation_id.to_string());
        desktop_lan::manager()
            .register_group_link(&global_group_id, conversation_id)
            .await;
        let payload = json!({
            "global_group_id": global_group_id,
            "group_name": conversation
                .group_name
                .clone()
                .unwrap_or_else(|| "LAN Group".to_string()),
            "content": content,
            "content_type": content_type,
            "client_msg_id": client_msg_id.map(str::to_string),
        });
        relay_envelope_to_lan(
            &settings,
            sender_user_id,
            "uw_group_message",
            None,
            None,
            Some(conversation_id),
            payload,
        )
        .await?;
    }
    Ok(())
}

async fn relay_group_upsert_to_lan(
    _state: &AppState,
    owner_user_id: &str,
    group: &UserWorldConversationView,
    payload: &GroupCreateRequest,
) -> Result<(), String> {
    let settings = desktop_lan::manager().settings().await;
    if !settings.enabled {
        return Ok(());
    }
    let Some(group_id) = group
        .group_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    desktop_lan::manager()
        .register_group_link(group_id, &group.conversation_id)
        .await;
    let mut members = payload
        .member_user_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<HashSet<_>>();
    members.insert(owner_user_id.to_string());
    let mut member_user_ids = members.into_iter().collect::<Vec<_>>();
    member_user_ids.sort();
    let payload = json!({
        "global_group_id": group_id,
        "group_name": payload.group_name.trim(),
        "owner_user_id": owner_user_id,
        "member_user_ids": member_user_ids,
    });
    relay_envelope_to_lan(
        &settings,
        owner_user_id,
        "uw_group_upsert",
        None,
        None,
        Some(&group.conversation_id),
        payload,
    )
    .await
}

async fn relay_group_announcement_to_lan(
    state: &AppState,
    sender_user_id: &str,
    group_id: &str,
    announcement: Option<&str>,
) -> Result<(), String> {
    let settings = desktop_lan::manager().settings().await;
    if !settings.enabled {
        return Ok(());
    }
    let Some(group) = state
        .storage
        .get_user_world_group_by_id(group_id)
        .map_err(|err| err.to_string())?
    else {
        return Ok(());
    };
    let global_group_id = desktop_lan::manager()
        .global_group_id_by_conversation(&group.conversation_id, Some(group_id))
        .await
        .unwrap_or_else(|| group_id.to_string());
    desktop_lan::manager()
        .register_group_link(&global_group_id, &group.conversation_id)
        .await;
    let payload = json!({
        "global_group_id": global_group_id,
        "announcement": announcement.map(str::to_string),
    });
    relay_envelope_to_lan(
        &settings,
        sender_user_id,
        "uw_group_announcement",
        None,
        None,
        Some(&group.conversation_id),
        payload,
    )
    .await
}

async fn relay_envelope_to_lan(
    settings: &desktop_lan::DesktopLanMeshSettings,
    source_user_id: &str,
    envelope_type: &str,
    target_peer_id: Option<&str>,
    target_lan_ip: Option<&str>,
    conversation_id: Option<&str>,
    payload: Value,
) -> Result<(), String> {
    if settings.peer_id.trim().is_empty() {
        return Ok(());
    }
    let peers = desktop_lan::manager().list_peers().await;
    if peers.is_empty() {
        return Ok(());
    }

    let target_peer = target_peer_id.map(|value| value.trim().to_string());
    let target_lan_ip = target_lan_ip.and_then(normalize_lan_ip);
    let candidates = peers
        .into_iter()
        .filter(|peer| {
            let peer_id = peer.peer_id.trim();
            if peer_id.is_empty() || peer_id == settings.peer_id.trim() {
                return false;
            }
            if let Some(target) = target_peer.as_deref() {
                if peer_id != target {
                    return false;
                }
                if let Some(target_ip) = target_lan_ip.as_deref() {
                    return normalize_lan_ip(peer.lan_ip.as_str()).as_deref() == Some(target_ip);
                }
            }
            true
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(());
    }

    let bound_port = desktop_lan::manager()
        .bound_port()
        .await
        .unwrap_or(settings.listen_port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|err| format!("build lan relay client failed: {err}"))?;

    for peer in candidates {
        let peer_ip = peer.lan_ip.trim();
        if peer_ip.is_empty() || peer.listen_port == 0 {
            continue;
        }
        let local_lan_ip = resolve_local_ip_for_peer(peer_ip)
            .map(|value| value.to_string())
            .unwrap_or_else(|| settings.listen_host.clone());
        let mut envelope = DesktopLanEnvelope {
            envelope_id: format!("lan_{}", Uuid::new_v4().simple()),
            envelope_type: envelope_type.to_string(),
            source_peer_id: settings.peer_id.trim().to_string(),
            source_user_id: source_user_id.trim().to_string(),
            target_peer_id: Some(peer.peer_id.clone()),
            target_user_id: None,
            conversation_id: conversation_id.map(|value| value.trim().to_string()),
            sent_at: now_ts(),
            payload: payload.clone(),
            signature: None,
        };
        if !settings.shared_secret.trim().is_empty() {
            envelope.signature = Some(sign_lan_envelope(&settings.shared_secret, &envelope));
        }
        let local_peer = desktop_lan::DesktopLanPeerSnapshot {
            peer_id: settings.peer_id.trim().to_string(),
            user_id: source_user_id.trim().to_string(),
            display_name: choose_non_empty(&[settings.display_name.trim(), source_user_id.trim()]),
            lan_ip: local_lan_ip,
            listen_port: bound_port,
            seen_at: now_ts(),
            capabilities: vec!["user_world".to_string()],
        };
        let endpoint = format!(
            "http://{}:{}{}",
            peer_ip, peer.listen_port, settings.peer_http_path
        );
        let body = json!({
            "peer": local_peer,
            "envelope": envelope,
        });
        let response = client.post(endpoint).json(&body).send().await;
        if let Err(err) = response {
            warn!("relay lan envelope failed: {err}");
        }
    }
    Ok(())
}

fn resolve_local_ip_for_peer(peer_ip: &str) -> Option<IpAddr> {
    let target = format!("{peer_ip}:9");
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(target).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn sign_lan_envelope(shared_secret: &str, envelope: &DesktopLanEnvelope) -> String {
    let content = format!(
        "{}|{}|{}|{}|{}|{}",
        envelope.envelope_id.trim(),
        envelope.envelope_type.trim(),
        envelope.source_peer_id.trim(),
        envelope.source_user_id.trim(),
        envelope.sent_at,
        serde_json::to_string(&envelope.payload).unwrap_or_else(|_| "null".to_string())
    );
    let mut mac = match HmacSha256::new_from_slice(shared_secret.trim().as_bytes()) {
        Ok(value) => value,
        Err(_) => return String::new(),
    };
    mac.update(content.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[derive(Debug, Clone)]
struct LanPeerIdentity {
    peer_id: String,
    lan_ip: Option<String>,
}

fn lan_peer_user_id(peer_id: &str, lan_ip: Option<&str>) -> String {
    let peer_id = peer_id.trim();
    let lan_ip = lan_ip.and_then(normalize_lan_ip);
    if let Some(lan_ip) = lan_ip {
        return format!("lan:{peer_id}@{lan_ip}");
    }
    format!("lan:{peer_id}")
}

fn parse_lan_peer_identity(user_id: &str) -> Option<LanPeerIdentity> {
    let value = user_id
        .trim()
        .strip_prefix("lan:")
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    if let Some((peer_id, lan_ip_raw)) = value.split_once('@') {
        let cleaned_peer_id = peer_id.trim();
        if cleaned_peer_id.is_empty() {
            return None;
        }
        return Some(LanPeerIdentity {
            peer_id: cleaned_peer_id.to_string(),
            lan_ip: normalize_lan_ip(lan_ip_raw),
        });
    }
    Some(LanPeerIdentity {
        peer_id: value.to_string(),
        lan_ip: None,
    })
}

fn normalize_lan_ip(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(|ch| ch == '[' || ch == ']');
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<IpAddr>().ok().map(|ip| ip.to_string())
}

fn choose_non_empty(candidates: &[&str]) -> String {
    candidates
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_default()
}

fn normalize_pagination(offset: Option<i64>, limit: Option<i64>) -> (i64, i64) {
    (
        offset.unwrap_or(0).max(0),
        normalize_limit(limit.unwrap_or(DEFAULT_LIMIT)),
    )
}

fn normalize_limit(limit: i64) -> i64 {
    if limit <= 0 {
        DEFAULT_LIMIT
    } else {
        limit.min(MAX_LIMIT)
    }
}

fn now_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

struct TempFileStream {
    path: PathBuf,
    inner: Option<ReaderStream<tokio::fs::File>>,
}

impl TempFileStream {
    fn new(path: PathBuf, inner: ReaderStream<tokio::fs::File>) -> Self {
        Self {
            path,
            inner: Some(inner),
        }
    }
}

impl Stream for TempFileStream {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.as_mut() {
            Some(inner) => Pin::new(inner).poll_next(cx),
            None => Poll::Ready(None),
        }
    }
}

impl Drop for TempFileStream {
    fn drop(&mut self) {
        self.inner.take();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn stream_response<S>(stream: S, filename: &str, content_type: &'static str) -> Response
where
    S: Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
{
    let disposition = build_content_disposition(filename);
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn empty_response(filename: &str, content_type: &'static str) -> Response {
    let disposition = build_content_disposition(filename);
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

fn build_content_disposition(filename: &str) -> String {
    let ascii_name = sanitize_filename(filename);
    if ascii_name == filename {
        return format!("attachment; filename=\"{ascii_name}\"");
    }
    let encoded = percent_encode(filename);
    format!("attachment; filename=\"{ascii_name}\"; filename*=UTF-8''{encoded}")
}

fn sanitize_filename(value: &str) -> String {
    let mut output = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }
    if output.trim().is_empty() {
        "download".to_string()
    } else {
        output
    }
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.as_bytes() {
        let ch = *byte as char;
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' || ch == '~' {
            output.push(ch);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

fn normalize_relative_path(value: &str) -> String {
    let trimmed = value.replace('\\', "/");
    let trimmed = trimmed.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return String::new();
    }
    trimmed.trim_start_matches('/').to_string()
}

fn create_temp_archive_file() -> Result<PathBuf, io::Error> {
    let mut root = std::env::temp_dir();
    root.push("wunder_user_world");
    std::fs::create_dir_all(&root)?;
    let filename = format!("wunder_user_world_{}.zip", uuid::Uuid::new_v4().simple());
    Ok(root.join(filename))
}

fn build_archive(archive_path: &Path, target: &Path, base_root: &Path) -> Result<(), io::Error> {
    let file = std::fs::File::create(archive_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    write_archive_entries(&mut zip, target, base_root, options)?;
    zip.finish()
        .map(|_| ())
        .map_err(|err| io::Error::other(err.to_string()))
}

fn write_archive_entries(
    zip: &mut zip::ZipWriter<std::fs::File>,
    target: &Path,
    base_root: &Path,
    options: FileOptions,
) -> Result<(), io::Error> {
    if target.is_file() {
        let rel = relative_zip_path(target, base_root);
        zip.start_file(rel, options)?;
        let mut file = std::fs::File::open(target)?;
        std::io::copy(&mut file, zip)?;
        return Ok(());
    }
    let mut file_count = 0;
    for entry in WalkDir::new(target).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = relative_zip_path(path, base_root);
        zip.start_file(rel, options)?;
        let mut file = std::fs::File::open(path)?;
        std::io::copy(&mut file, zip)?;
        file_count += 1;
    }
    if file_count == 0 {
        let mut dir_rel = relative_zip_path(target, base_root);
        if !dir_rel.ends_with('/') {
            dir_rel.push('/');
        }
        if !dir_rel.is_empty() && dir_rel != "./" {
            zip.add_directory(dir_rel, options)?;
        }
    }
    Ok(())
}

fn relative_zip_path(path: &Path, base_root: &Path) -> String {
    path.strip_prefix(base_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
