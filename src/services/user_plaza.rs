use crate::attachment::sanitize_filename_stem;
use crate::config::Config;
use crate::services::agent_abilities::resolve_agent_ability_selection;
use crate::services::hive_pack::{
    resolve_export_artifact_path, run_export_job, run_import_job, HivePackExportOptions,
    HivePackImportOptions,
};
use crate::services::inner_visible::{build_worker_card, parse_worker_card, WorkerCardDocument};
use crate::services::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::services::user_agent_presets::filter_allowed_tools;
use crate::services::user_store::build_default_agent_record_from_storage;
use crate::services::user_tools::{UserToolAlias, UserToolBindings, UserToolKind};
use crate::services::worker_card_settings::collect_context_skill_names;
use crate::skills::{load_skills, SkillSpec};
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, HiveRecord, UserAccountRecord,
    UserAgentRecord, DEFAULT_HIVE_ID,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const USER_PLAZA_META_PREFIX: &str = "user_plaza:item:";
const USER_PLAZA_SHARED_DIR: &str = "_shared";
const USER_PLAZA_ROOT_DIR: &str = "hive_plaza";
const USER_PLAZA_ITEM_DIR: &str = "items";
const USER_PLAZA_TEMP_DIR: &str = "tmp";
const DEFAULT_AGENT_ACCESS_LEVEL: &str = "A";
const DEFAULT_AGENT_STATUS: &str = "active";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPlazaItemRecord {
    pub item_id: String,
    pub owner_user_id: String,
    pub owner_username: String,
    pub kind: String,
    pub source_key: String,
    pub title: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    pub artifact_filename: String,
    pub artifact_path: String,
    pub artifact_size_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_updated_at: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_signature: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublishUserPlazaItemRequest {
    pub kind: String,
    pub source_key: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListUserPlazaItemsQuery {
    #[serde(default)]
    pub mine_only: bool,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserPlazaImportResult {
    pub kind: String,
    pub item_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_hive_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_job: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_import: Option<Value>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UserPlazaOwnerPurgeResult {
    pub deleted_items: usize,
    pub deleted_meta_records: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlazaFreshnessStatus {
    Current,
    Outdated,
    SourceMissing,
}

impl PlazaFreshnessStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Outdated => "outdated",
            Self::SourceMissing => "source_missing",
        }
    }
}

pub async fn list_items(
    state: &AppState,
    current_user_id: &str,
    query: &ListUserPlazaItemsQuery,
) -> Result<Vec<Value>> {
    let current_user_id = current_user_id.trim();
    let filtered_kind = normalize_item_kind(query.kind.as_deref());
    let config = state.config_store.get().await;
    let mut items = Vec::new();
    for (_, raw) in state.storage.list_meta_prefix(USER_PLAZA_META_PREFIX)? {
        let record: UserPlazaItemRecord = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if query.mine_only && record.owner_user_id != current_user_id {
            continue;
        }
        if let Some(expected_kind) = filtered_kind.as_deref() {
            if record.kind != expected_kind {
                continue;
            }
        }
        if !Path::new(&record.artifact_path).is_file() {
            continue;
        }
        let freshness = resolve_plaza_item_freshness(state, &config, &record).await?;
        items.push(item_payload(&record, current_user_id, freshness));
    }
    Ok(items)
}

pub async fn get_item(state: &AppState, item_id: &str) -> Result<Option<UserPlazaItemRecord>> {
    let cleaned = item_id.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let Some(raw) = state.user_store.get_meta(&meta_key(cleaned))? else {
        return Ok(None);
    };
    let record: UserPlazaItemRecord = serde_json::from_str(&raw)
        .with_context(|| format!("parse plaza item failed: {cleaned}"))?;
    if !Path::new(&record.artifact_path).is_file() {
        return Ok(None);
    }
    Ok(Some(record))
}

pub fn purge_owner_items(
    state: &AppState,
    owner_user_id: &str,
) -> Result<UserPlazaOwnerPurgeResult> {
    let cleaned_owner_user_id = owner_user_id.trim();
    if cleaned_owner_user_id.is_empty() {
        return Ok(UserPlazaOwnerPurgeResult::default());
    }
    let mut result = UserPlazaOwnerPurgeResult::default();
    let mut removed_item_ids = HashSet::new();
    for (meta_key_value, raw) in state.storage.list_meta_prefix(USER_PLAZA_META_PREFIX)? {
        let record: UserPlazaItemRecord = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if record.owner_user_id != cleaned_owner_user_id {
            continue;
        }
        if let Some(item_id) = purge_item_id(&meta_key_value, &record) {
            if removed_item_ids.insert(item_id.clone()) {
                remove_path_if_exists(item_dir(state.workspace.root(), &item_id))?;
            }
        }
        result.deleted_items += 1;
        result.deleted_meta_records += state
            .storage
            .delete_meta_prefix(&meta_key_value)
            .context("delete owned plaza item meta failed")?;
    }
    Ok(result)
}

pub async fn publish_item(
    state: &AppState,
    user: &UserAccountRecord,
    request: PublishUserPlazaItemRequest,
) -> Result<Value> {
    state.inner_visible.sync_user_state(&user.user_id).await?;
    let kind = normalize_item_kind(Some(&request.kind))
        .ok_or_else(|| anyhow!("unsupported plaza item kind"))?;
    let source_key = request.source_key.trim();
    if source_key.is_empty() {
        return Err(anyhow!("source_key is required"));
    }
    let existing = find_existing_owned_item(state, &user.user_id, &kind, source_key)?;
    let published = match kind.as_str() {
        "hive_pack" => {
            publish_hive_pack(state, user, source_key, &request, existing.as_ref()).await?
        }
        "worker_card" => {
            publish_worker_card(state, user, source_key, &request, existing.as_ref()).await?
        }
        "skill_pack" => {
            publish_skill_pack(state, user, source_key, &request, existing.as_ref()).await?
        }
        _ => return Err(anyhow!("unsupported plaza item kind")),
    };
    let payload = item_payload(&published, &user.user_id, PlazaFreshnessStatus::Current);
    state
        .user_store
        .set_meta(
            &meta_key(&published.item_id),
            &serde_json::to_string(&published).context("serialize plaza item failed")?,
        )
        .context("persist plaza item failed")?;
    Ok(payload)
}

pub fn unpublish_item(state: &AppState, user_id: &str, item_id: &str) -> Result<bool> {
    let cleaned_item_id = item_id.trim();
    if cleaned_item_id.is_empty() {
        return Ok(false);
    }
    let Some(record) = state
        .user_store
        .get_meta(&meta_key(cleaned_item_id))?
        .and_then(|raw| serde_json::from_str::<UserPlazaItemRecord>(&raw).ok())
    else {
        return Ok(false);
    };
    if record.owner_user_id != user_id.trim() {
        return Ok(false);
    }
    let _ = remove_path_if_exists(item_dir(state.workspace.root(), cleaned_item_id));
    let affected = state
        .storage
        .delete_meta_prefix(&meta_key(cleaned_item_id))
        .context("delete plaza item meta failed")?;
    Ok(affected > 0)
}

pub async fn import_item(
    state: &AppState,
    user: &UserAccountRecord,
    item_id: &str,
) -> Result<UserPlazaImportResult> {
    state.inner_visible.sync_user_state(&user.user_id).await?;
    let record = get_item(state, item_id)
        .await?
        .ok_or_else(|| anyhow!("plaza item not found"))?;
    let artifact = PathBuf::from(&record.artifact_path);
    if !artifact.is_file() {
        return Err(anyhow!("plaza item artifact is missing"));
    }
    let result = match record.kind.as_str() {
        "hive_pack" => import_hive_pack_item(state, user, &record, &artifact).await?,
        "worker_card" => import_worker_card_item(state, user, &record, &artifact).await?,
        "skill_pack" => import_skill_pack_item(state, user, &record, &artifact).await?,
        _ => return Err(anyhow!("unsupported plaza item kind")),
    };
    state.inner_visible.sync_user_state(&user.user_id).await?;
    Ok(result)
}

fn find_existing_owned_item(
    state: &AppState,
    owner_user_id: &str,
    kind: &str,
    source_key: &str,
) -> Result<Option<UserPlazaItemRecord>> {
    for (_, raw) in state.storage.list_meta_prefix(USER_PLAZA_META_PREFIX)? {
        let record: UserPlazaItemRecord = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if record.owner_user_id == owner_user_id.trim()
            && record.kind == kind
            && record.source_key == source_key.trim()
        {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

async fn publish_hive_pack(
    state: &AppState,
    user: &UserAccountRecord,
    group_id: &str,
    request: &PublishUserPlazaItemRequest,
    existing: Option<&UserPlazaItemRecord>,
) -> Result<UserPlazaItemRecord> {
    let normalized_group_id = normalize_hive_id(group_id);
    let hive = if normalized_group_id == DEFAULT_HIVE_ID {
        state.user_store.ensure_default_hive(&user.user_id)?
    } else {
        state
            .user_store
            .get_hive(&user.user_id, &normalized_group_id)?
            .ok_or_else(|| anyhow!("hive not found"))?
    };
    let agents = state
        .user_store
        .list_user_agents_by_hive_with_default(&user.user_id, &normalized_group_id)?;
    let export_job = run_export_job(
        state,
        user,
        HivePackExportOptions {
            group_id: normalized_group_id.clone(),
            mode: Some("full".to_string()),
        },
    )
    .await?;
    let artifact_path = resolve_export_artifact_path(&export_job)
        .ok_or_else(|| anyhow!("hive pack export missing artifact"))?;
    let artifact_filename = export_job
        .artifact
        .as_ref()
        .map(|item| item.filename.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "{}.hivepack",
                sanitize_filename_stem(&request_title_or(&request.title, &hive.name))
            )
        });
    let title = request_title_or(&request.title, &hive.name);
    let summary = request_summary_or(&request.summary, &hive.description);
    let config = state.config_store.get().await;
    let source_signature = compute_hive_pack_source_signature(
        state,
        &config,
        user,
        &hive,
        &agents,
    )
    .await?;
    let metadata = json!({
        "group_id": hive.hive_id,
        "group_name": hive.name,
        "agent_total": agents.len(),
        "mother_agent_name": agents.iter().find(|item| item.prefer_mother).map(|item| item.name.clone())
    });
    let tags = vec!["swarm".to_string(), "hivepack".to_string()];
    build_record_from_artifact(
        state,
        existing,
        user,
        "hive_pack",
        &normalized_group_id,
        &title,
        &summary,
        None,
        tags,
        metadata,
        hive.updated_time,
        Some(source_signature),
        &artifact_path,
        &artifact_filename,
    )
}

async fn publish_worker_card(
    state: &AppState,
    user: &UserAccountRecord,
    agent_id: &str,
    request: &PublishUserPlazaItemRequest,
    existing: Option<&UserPlazaItemRecord>,
) -> Result<UserPlazaItemRecord> {
    let record = resolve_agent_for_publish(state, &user.user_id, agent_id)?;
    let hive = state.user_store.get_hive(&user.user_id, &record.hive_id)?;
    let tool_context = build_user_tool_context(state, &user.user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let document = build_worker_card(
        &record,
        hive.as_ref().map(|item| item.name.as_str()),
        hive.as_ref().map(|item| item.description.as_str()),
        &skill_name_keys,
    );
    let temp_filename = format!(
        "{}.worker-card.json",
        sanitize_filename_stem(&request_title_or(&request.title, &record.name))
    );
    let temp_path = temp_artifact_path(state.workspace.root(), &temp_filename);
    fs::create_dir_all(
        temp_path
            .parent()
            .ok_or_else(|| anyhow!("worker card temp dir missing"))?,
    )?;
    fs::write(
        &temp_path,
        serde_json::to_vec_pretty(&document).context("serialize worker card failed")?,
    )?;
    let title = request_title_or(&request.title, &record.name);
    let summary = request_summary_or(&request.summary, &record.description);
    let source_signature = compute_worker_card_source_signature(
        state,
        user,
        &record,
        hive.as_ref(),
    )
    .await?;
    let metadata = json!({
        "agent_id": record.agent_id,
        "agent_name": record.name,
        "hive_id": record.hive_id,
        "hive_name": hive.as_ref().map(|item| item.name.clone()),
        "hive_description": hive.as_ref().map(|item| item.description.clone()),
        "declared_tools": record.declared_tool_names,
        "declared_skills": record.declared_skill_names
    });
    let tags = vec!["agent".to_string(), "worker-card".to_string()];
    build_record_from_artifact(
        state,
        existing,
        user,
        "worker_card",
        agent_id,
        &title,
        &summary,
        record.icon.clone(),
        tags,
        metadata,
        record.updated_at,
        Some(source_signature),
        &temp_path,
        &temp_filename,
    )
}

async fn publish_skill_pack(
    state: &AppState,
    user: &UserAccountRecord,
    skill_name: &str,
    request: &PublishUserPlazaItemRequest,
    existing: Option<&UserPlazaItemRecord>,
) -> Result<UserPlazaItemRecord> {
    let config = state.config_store.get().await;
    let spec = resolve_custom_user_skill_spec(state, &config, &user.user_id, skill_name)?;
    let skill_dir_name = spec
        .root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("skill root is invalid"))?
        .to_string();
    let title = request_title_or(&request.title, &spec.name);
    let summary = request_summary_or(&request.summary, &spec.description);
    let temp_filename = format!("{}.skill", sanitize_filename_stem(&title));
    let temp_path = temp_artifact_path(state.workspace.root(), &temp_filename);
    create_skill_archive(&spec.root, &skill_dir_name, &temp_path)?;
    let source_signature = compute_skill_pack_source_signature(&spec)?;
    let metadata = json!({
        "skill_name": spec.name,
        "skill_dir": skill_dir_name,
        "skill_path": spec.path
    });
    let tags = vec!["skill".to_string(), "skill-pack".to_string()];
    build_record_from_artifact(
        state,
        existing,
        user,
        "skill_pack",
        skill_name,
        &title,
        &summary,
        None,
        tags,
        metadata,
        Some(file_modified_ts(&spec.root)),
        Some(source_signature),
        &temp_path,
        &temp_filename,
    )
}

fn build_record_from_artifact(
    state: &AppState,
    existing: Option<&UserPlazaItemRecord>,
    user: &UserAccountRecord,
    kind: &str,
    source_key: &str,
    title: &str,
    summary: &str,
    icon: Option<String>,
    tags: Vec<String>,
    metadata: Value,
    source_updated_at: impl Into<Option<f64>>,
    source_signature: Option<String>,
    artifact_source: &Path,
    artifact_filename: &str,
) -> Result<UserPlazaItemRecord> {
    let now = now_ts();
    let item_id = existing
        .map(|item| item.item_id.clone())
        .unwrap_or_else(|| format!("plaza_{}", Uuid::new_v4().simple()));
    let target_dir = item_dir(state.workspace.root(), &item_id);
    if target_dir.exists() {
        remove_path_if_exists(&target_dir)?;
    }
    fs::create_dir_all(&target_dir)?;
    let target_path = target_dir.join(artifact_filename);
    fs::copy(artifact_source, &target_path)
        .with_context(|| format!("copy plaza artifact failed: {}", artifact_source.display()))?;
    let artifact_size_bytes = fs::metadata(&target_path)?.len();
    let record = UserPlazaItemRecord {
        item_id,
        owner_user_id: user.user_id.clone(),
        owner_username: user.username.clone(),
        kind: kind.to_string(),
        source_key: source_key.trim().to_string(),
        title: title.trim().to_string(),
        summary: summary.trim().to_string(),
        icon,
        artifact_filename: artifact_filename.to_string(),
        artifact_path: target_path.to_string_lossy().to_string(),
        artifact_size_bytes,
        source_updated_at: source_updated_at.into(),
        source_signature,
        tags,
        metadata,
        created_at: existing.map(|item| item.created_at).unwrap_or(now),
        updated_at: now,
    };
    let _ = remove_path_if_exists(artifact_source);
    Ok(record)
}

async fn import_hive_pack_item(
    state: &AppState,
    user: &UserAccountRecord,
    record: &UserPlazaItemRecord,
    artifact: &Path,
) -> Result<UserPlazaImportResult> {
    let data = fs::read(artifact)
        .with_context(|| format!("read hive plaza artifact failed: {}", artifact.display()))?;
    let filename = artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("shared.hivepack");
    let job = run_import_job(
        state,
        user,
        filename,
        data,
        HivePackImportOptions {
            group_id: None,
            create_hive_if_missing: Some(true),
            conflict_mode: None,
        },
    )
    .await?;
    if !job.status.eq_ignore_ascii_case("completed") {
        let detail = job
            .detail
            .as_ref()
            .and_then(|value| value.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("hive plaza import failed");
        return Err(anyhow!(detail.to_string()));
    }
    Ok(UserPlazaImportResult {
        kind: record.kind.clone(),
        item_id: record.item_id.clone(),
        title: record.title.clone(),
        imported_agent_id: None,
        imported_hive_id: job
            .report
            .as_ref()
            // Keep plaza import compatible with both the old and current hivepack report keys.
            .and_then(|value| value.get("target_hive_id").or_else(|| value.get("hive_id")))
            .and_then(Value::as_str)
            .map(str::to_string),
        imported_job: Some(json!({
            "job_id": job.job_id,
            "status": job.status,
            "summary": job.summary,
            "report": job.report,
            "detail": job.detail
        })),
        skill_import: None,
        message: format!("imported swarm plaza item: {}", record.title),
    })
}

async fn import_worker_card_item(
    state: &AppState,
    user: &UserAccountRecord,
    record: &UserPlazaItemRecord,
    artifact: &Path,
) -> Result<UserPlazaImportResult> {
    let document = fs::read_to_string(artifact).with_context(|| {
        format!(
            "read worker card plaza artifact failed: {}",
            artifact.display()
        )
    })?;
    let worker_card: WorkerCardDocument =
        serde_json::from_str(&document).context("parse worker card plaza artifact failed")?;
    let parsed = parse_worker_card(worker_card, None);
    let tool_context = build_user_tool_context(state, &user.user_id).await;
    let allowed_tool_names = compute_allowed_tool_names(user, &tool_context);
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let selection = resolve_agent_ability_selection(
        &parsed.tool_names,
        Some(parsed.ability_items.clone()),
        Some(parsed.declared_tool_names.clone()),
        Some(parsed.declared_skill_names.clone()),
        &skill_name_keys,
    );
    let target_hive = ensure_import_hive(
        state,
        &user.user_id,
        &parsed.hive_id,
        record.metadata.get("hive_name").and_then(Value::as_str),
        record
            .metadata
            .get("hive_description")
            .and_then(Value::as_str),
    )?;
    let now = now_ts();
    let agent = UserAgentRecord {
        agent_id: format!("agent_{}", Uuid::new_v4().simple()),
        user_id: user.user_id.clone(),
        hive_id: target_hive.hive_id.clone(),
        name: parsed.name.clone(),
        description: parsed.description.clone(),
        system_prompt: parsed.system_prompt.clone(),
        model_name: parsed.model_name.clone(),
        ability_items: selection.ability_items,
        tool_names: filter_allowed_tools(&selection.tool_names, &allowed_tool_names),
        declared_tool_names: selection.declared_tool_names,
        declared_skill_names: selection.declared_skill_names,
        preset_questions: parsed.preset_questions.clone(),
        access_level: DEFAULT_AGENT_ACCESS_LEVEL.to_string(),
        approval_mode: parsed.approval_mode.clone(),
        is_shared: false,
        status: DEFAULT_AGENT_STATUS.to_string(),
        icon: parsed.icon.clone(),
        sandbox_container_id: normalize_sandbox_container_id(parsed.sandbox_container_id),
        created_at: now,
        updated_at: now,
        preset_binding: None,
        silent: parsed.silent,
        prefer_mother: parsed.prefer_mother,
    };
    state.user_store.upsert_user_agent(&agent)?;
    Ok(UserPlazaImportResult {
        kind: record.kind.clone(),
        item_id: record.item_id.clone(),
        title: record.title.clone(),
        imported_agent_id: Some(agent.agent_id.clone()),
        imported_hive_id: Some(target_hive.hive_id),
        imported_job: None,
        skill_import: None,
        message: format!("imported worker card plaza item: {}", record.title),
    })
}

async fn import_skill_pack_item(
    state: &AppState,
    user: &UserAccountRecord,
    record: &UserPlazaItemRecord,
    artifact: &Path,
) -> Result<UserPlazaImportResult> {
    let filename = artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("shared.skill");
    let data = fs::read(artifact)
        .with_context(|| format!("read skill plaza artifact failed: {}", artifact.display()))?;
    let config = state.config_store.get().await;
    let summary = import_skill_archive_for_user(state, &config, &user.user_id, filename, &data)?;
    Ok(UserPlazaImportResult {
        kind: record.kind.clone(),
        item_id: record.item_id.clone(),
        title: record.title.clone(),
        imported_agent_id: None,
        imported_hive_id: None,
        imported_job: None,
        skill_import: Some(json!({
            "extracted": summary.extracted,
            "top_level_dirs": summary.top_level_dirs,
        })),
        message: format!("imported skill plaza item: {}", record.title),
    })
}

fn resolve_agent_for_publish(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
) -> Result<UserAgentRecord> {
    let cleaned = agent_id.trim();
    if cleaned.is_empty() || cleaned.eq_ignore_ascii_case("__default__") {
        return build_default_agent_record_from_storage(
            state.user_store.storage_backend().as_ref(),
            user_id,
        );
    }
    state
        .user_store
        .get_user_agent(user_id, cleaned)?
        .ok_or_else(|| anyhow!("agent not found"))
}

fn resolve_custom_user_skill_spec(
    state: &AppState,
    config: &Config,
    user_id: &str,
    skill_name: &str,
) -> Result<SkillSpec> {
    let cleaned = skill_name.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("skill name is required"));
    }
    let skill_root = state.user_tool_store.get_skill_root(user_id);
    let mut scan_config = config.clone();
    scan_config.skills.paths = vec![skill_root.to_string_lossy().to_string()];
    scan_config.skills.enabled = Vec::new();
    let registry = load_skills(&scan_config, false, false, false);
    let spec = registry
        .get(cleaned)
        .ok_or_else(|| anyhow!("skill not found"))?;
    if !spec.root.starts_with(&skill_root) {
        return Err(anyhow!("only custom user skills can be published"));
    }
    Ok(spec)
}

fn create_skill_archive(skill_root: &Path, top_dir: &str, target_zip: &Path) -> Result<()> {
    if let Some(parent) = target_zip.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = fs::File::create(target_zip)?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut entries = WalkDir::new(skill_root)
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path().to_string_lossy().to_string());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(skill_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let target_relative = if relative.is_empty() {
            top_dir.to_string()
        } else {
            format!("{top_dir}/{relative}")
        };
        if entry.file_type().is_dir() {
            writer.add_directory(format!("{target_relative}/"), options)?;
            continue;
        }
        writer.start_file(target_relative, options)?;
        let bytes = fs::read(path)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}

#[derive(Debug)]
struct SkillArchiveImportSummary {
    extracted: usize,
    top_level_dirs: Vec<String>,
}

fn import_skill_archive_for_user(
    state: &AppState,
    config: &Config,
    user_id: &str,
    filename: &str,
    data: &[u8],
) -> Result<SkillArchiveImportSummary> {
    let lower_name = filename.trim().to_ascii_lowercase();
    if !(lower_name.ends_with(".zip") || lower_name.ends_with(".skill")) {
        return Err(anyhow!("skill archive must be .zip or .skill"));
    }
    let skill_root = state.user_tool_store.get_skill_root(user_id);
    fs::create_dir_all(&skill_root)?;
    let reserved_top_dirs = build_reserved_skill_dir_names(config, &skill_root);
    let cursor = Cursor::new(data);
    let mut archive = ZipArchive::new(cursor).context("invalid skill archive")?;
    let mut extracted = 0;
    let mut top_level_dirs = BTreeSet::new();
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .context("invalid skill archive entry")?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        if name.starts_with('/') || name.starts_with('\\') {
            return Err(anyhow!("skill archive contains absolute paths"));
        }
        let path = Path::new(&name);
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(anyhow!("skill archive contains illegal paths"));
        }
        let top_dir = uploaded_skill_archive_top_dir(path)?;
        if reserved_top_dirs.contains(&top_dir) {
            return Err(anyhow!(
                "skill archive conflicts with builtin skill directory"
            ));
        }
        top_level_dirs.insert(top_dir);
        let dest = skill_root.join(path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        fs::write(&dest, buffer)?;
        extracted += 1;
    }
    if extracted > 0 {
        let payload = state.user_tool_store.load_user_tools(user_id);
        let _ = state.user_tool_store.update_skills(
            user_id,
            payload.skills.enabled.clone(),
            payload.skills.shared.clone(),
        );
        state.user_tool_manager.clear_skill_cache(Some(user_id));
    }
    Ok(SkillArchiveImportSummary {
        extracted,
        top_level_dirs: top_level_dirs.into_iter().collect(),
    })
}

fn build_reserved_skill_dir_names(config: &Config, skill_root: &Path) -> HashSet<String> {
    load_skills(config, false, false, true)
        .list_specs()
        .into_iter()
        .filter(|spec| !spec.root.starts_with(skill_root))
        .filter_map(|spec| {
            spec.root
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn uploaded_skill_archive_top_dir(path: &Path) -> Result<String> {
    let mut components = path.components();
    let top = components
        .next()
        .ok_or_else(|| anyhow!("skill archive entry is empty"))?;
    if components.next().is_none() {
        return Err(anyhow!(
            "skill archive must contain a dedicated top-level directory"
        ));
    }
    match top {
        std::path::Component::Normal(value) => {
            let text = value.to_string_lossy().trim().to_string();
            if text.is_empty() {
                Err(anyhow!("skill archive top-level directory is empty"))
            } else {
                Ok(text)
            }
        }
        _ => Err(anyhow!("skill archive top-level path is invalid")),
    }
}

fn ensure_import_hive(
    state: &AppState,
    user_id: &str,
    hive_id: &str,
    hive_name: Option<&str>,
    hive_description: Option<&str>,
) -> Result<HiveRecord> {
    let normalized_hive_id = normalize_hive_id(hive_id);
    if normalized_hive_id == DEFAULT_HIVE_ID {
        return state.user_store.ensure_default_hive(user_id);
    }
    if let Some(existing) = state.user_store.get_hive(user_id, &normalized_hive_id)? {
        return Ok(existing);
    }
    let now = now_ts();
    let record = HiveRecord {
        hive_id: normalized_hive_id.clone(),
        user_id: user_id.trim().to_string(),
        name: hive_name
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(&normalized_hive_id)
            .to_string(),
        description: hive_description
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string(),
        is_default: false,
        status: "active".to_string(),
        created_time: now,
        updated_time: now,
    };
    state.user_store.upsert_hive(&record)?;
    Ok(record)
}

fn item_payload(
    record: &UserPlazaItemRecord,
    current_user_id: &str,
    freshness_status: PlazaFreshnessStatus,
) -> Value {
    json!({
        "item_id": record.item_id,
        "kind": record.kind,
        "title": record.title,
        "summary": record.summary,
        "icon": record.icon,
        "owner_user_id": record.owner_user_id,
        "owner_username": record.owner_username,
        "source_key": record.source_key,
        "artifact_filename": record.artifact_filename,
        "artifact_size_bytes": record.artifact_size_bytes,
        "source_updated_at": record.source_updated_at,
        "freshness_status": freshness_status.as_str(),
        "source_signature": record.source_signature,
        "tags": record.tags,
        "metadata": record.metadata,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "mine": record.owner_user_id == current_user_id.trim(),
    })
}

fn normalize_item_kind(value: Option<&str>) -> Option<String> {
    match value
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "hive_pack" | "hivepack" | "swarm" | "beeroom" => Some("hive_pack".to_string()),
        "worker_card" | "worker-card" | "agent" => Some("worker_card".to_string()),
        "skill_pack" | "skill-pack" | "skill" => Some("skill_pack".to_string()),
        _ => None,
    }
}

fn request_title_or(requested: &Option<String>, fallback: &str) -> String {
    requested
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback.trim())
        .to_string()
}

fn request_summary_or(requested: &Option<String>, fallback: &str) -> String {
    requested
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback.trim())
        .to_string()
}

fn meta_key(item_id: &str) -> String {
    format!("{USER_PLAZA_META_PREFIX}{}", item_id.trim())
}

fn purge_item_id(meta_key_value: &str, record: &UserPlazaItemRecord) -> Option<String> {
    let from_record = record.item_id.trim();
    if !from_record.is_empty() {
        return Some(from_record.to_string());
    }
    meta_key_value
        .trim()
        .strip_prefix(USER_PLAZA_META_PREFIX)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn plaza_root(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(USER_PLAZA_SHARED_DIR)
        .join(USER_PLAZA_ROOT_DIR)
}

fn item_dir(workspace_root: &Path, item_id: &str) -> PathBuf {
    plaza_root(workspace_root)
        .join(USER_PLAZA_ITEM_DIR)
        .join(item_id.trim())
}

fn temp_artifact_path(workspace_root: &Path, filename: &str) -> PathBuf {
    plaza_root(workspace_root)
        .join(USER_PLAZA_TEMP_DIR)
        .join(format!("{}_{}", Uuid::new_v4().simple(), filename))
}

fn remove_path_if_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn file_modified_ts(path: &Path) -> f64 {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return now_ts(),
    };
    let modified = match metadata.modified() {
        Ok(value) => value,
        Err(_) => return now_ts(),
    };
    match modified.duration_since(std::time::UNIX_EPOCH) {
        Ok(value) => value.as_secs_f64(),
        Err(_) => now_ts(),
    }
}

async fn resolve_plaza_item_freshness(
    state: &AppState,
    config: &Config,
    record: &UserPlazaItemRecord,
) -> Result<PlazaFreshnessStatus> {
    let Some(published_signature) = record
        .source_signature
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(PlazaFreshnessStatus::Current);
    };
    let current_signature = match compute_current_source_signature(state, config, record).await? {
        Some(value) => value,
        None => return Ok(PlazaFreshnessStatus::SourceMissing),
    };
    if current_signature == published_signature {
        Ok(PlazaFreshnessStatus::Current)
    } else {
        Ok(PlazaFreshnessStatus::Outdated)
    }
}

async fn compute_current_source_signature(
    state: &AppState,
    config: &Config,
    record: &UserPlazaItemRecord,
) -> Result<Option<String>> {
    match record.kind.as_str() {
        "worker_card" => {
            let agent =
                match resolve_agent_for_publish(state, &record.owner_user_id, &record.source_key) {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                };
            let hive = state.user_store.get_hive(&record.owner_user_id, &agent.hive_id)?;
            let Some(user) = state.user_store.get_user_by_id(&record.owner_user_id)? else {
                return Ok(None);
            };
            Ok(Some(
                compute_worker_card_source_signature(
                    state,
                    &user,
                    &agent,
                    hive.as_ref(),
                )
                .await?,
            ))
        }
        "skill_pack" => {
            let spec = match resolve_custom_user_skill_spec(state, config, &record.owner_user_id, &record.source_key) {
                Ok(value) => value,
                Err(_) => return Ok(None),
            };
            Ok(Some(compute_skill_pack_source_signature(&spec)?))
        }
        "hive_pack" => {
            let hive_id = normalize_hive_id(&record.source_key);
            let hive = if hive_id == DEFAULT_HIVE_ID {
                match state.user_store.ensure_default_hive(&record.owner_user_id) {
                    Ok(value) => value,
                    Err(_) => return Ok(None),
                }
            } else {
                match state.user_store.get_hive(&record.owner_user_id, &hive_id)? {
                    Some(value) => value,
                    None => return Ok(None),
                }
            };
            let agents = state
                .user_store
                .list_user_agents_by_hive_with_default(&record.owner_user_id, &hive_id)?;
            if agents.is_empty() {
                return Ok(None);
            }
            let user = state
                .user_store
                .get_user_by_id(&record.owner_user_id)?
                .ok_or_else(|| anyhow!("plaza owner not found"))?;
            Ok(Some(
                compute_hive_pack_source_signature(state, config, &user, &hive, &agents).await?,
            ))
        }
        _ => Ok(None),
    }
}

async fn compute_worker_card_source_signature(
    state: &AppState,
    user: &UserAccountRecord,
    agent: &UserAgentRecord,
    hive: Option<&HiveRecord>,
) -> Result<String> {
    let tool_context = build_user_tool_context(state, &user.user_id).await;
    let skill_name_keys = collect_context_skill_names(&tool_context);
    let mut document = build_worker_card(
        agent,
        hive.map(|item| item.name.as_str()),
        hive.map(|item| item.description.as_str()),
        &skill_name_keys,
    );
    document.metadata.exported_at.clear();
    stable_json_signature(&document)
}

fn compute_skill_pack_source_signature(spec: &SkillSpec) -> Result<String> {
    directory_tree_signature(&spec.root)
}

async fn compute_hive_pack_source_signature(
    state: &AppState,
    config: &Config,
    user: &UserAccountRecord,
    hive: &HiveRecord,
    agents: &[UserAgentRecord],
) -> Result<String> {
    let skills = state.skills.read().await.clone();
    let global_skill_specs = skills.list_specs();
    let bindings = state
        .user_tool_manager
        .build_bindings(config, &skills, &user.user_id);
    let skill_root = state.user_tool_store.get_skill_root(&user.user_id);

    let mut worker_entries = Vec::new();
    let mut occupied_worker_id_keys = HashSet::new();
    let mut exported_skill_names: BTreeSet<String> = BTreeSet::new();
    let mut included_skill_signatures = Vec::new();

    for (index, agent) in agents.iter().enumerate() {
        let preferred_worker_id = export_worker_id_local(agent, index);
        let worker_id = unique_label_with_reserved_local(
            &preferred_worker_id,
            &occupied_worker_id_keys,
            &format!("worker-{}", index + 1),
        );
        occupied_worker_id_keys.insert(normalize_conflict_key_local(&worker_id));

        let mut worker_skill_sources =
            collect_agent_skills_for_export(agent, &bindings, &skill_root, &global_skill_specs);
        worker_skill_sources.sort_by(|left, right| left.name.cmp(&right.name));
        worker_skill_sources.dedup_by(|left, right| left.name == right.name);

        let mut attached_skill_names = Vec::new();
        for skill in &worker_skill_sources {
            let skill_name = skill.name.trim();
            if skill_name.is_empty() {
                continue;
            }
            attached_skill_names.push(skill_name.to_string());
            if !skill.include_in_package || exported_skill_names.contains(skill_name) {
                continue;
            }
            if !skill.source_dir.exists()
                || !skill.source_dir.is_dir()
                || !skill.source_dir.join("SKILL.md").is_file()
            {
                continue;
            }
            included_skill_signatures.push(json!({
                "skill_name": skill_name,
                "signature": directory_tree_signature(&skill.source_dir)?,
            }));
            exported_skill_names.insert(skill_name.to_string());
        }
        attached_skill_names.sort();
        attached_skill_names.dedup();
        let declared_tool_names =
            collect_agent_declared_tools_for_export(agent, &bindings.alias_map);
        let declared_skill_names = if agent.declared_skill_names.is_empty() {
            attached_skill_names.clone()
        } else {
            normalize_string_items_local(&agent.declared_skill_names)
        };
        worker_entries.push(json!({
            "worker_id": worker_id,
            "agent_id": agent.agent_id,
            "name": agent.name,
            "description": agent.description,
            "system_prompt": agent.system_prompt,
            "model_name": agent.model_name,
            "declared_tool_names": declared_tool_names,
            "declared_skill_names": declared_skill_names,
            "preset_questions": normalize_string_items_local(&agent.preset_questions),
            "approval_mode": normalize_approval_mode_local(Some(&agent.approval_mode)),
            "icon": agent.icon,
            "sandbox_container_id": normalize_sandbox_container_id(agent.sandbox_container_id),
            "silent": agent.silent,
            "prefer_mother": agent.prefer_mother,
            "attached_skill_names": attached_skill_names,
        }));
    }

    stable_json_signature(&json!({
        "kind": "hive_pack",
        "hive_id": hive.hive_id,
        "hive_name": hive.name,
        "hive_description": hive.description,
        "workers": worker_entries,
        "included_skills": included_skill_signatures,
    }))
}

fn directory_tree_signature(root: &Path) -> Result<String> {
    if !root.exists() || !root.is_dir() {
        return Err(anyhow!("source directory missing"));
    }
    let mut hasher = Sha256::new();
    let mut entries = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path().to_string_lossy().to_string());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type().is_dir() {
            hasher.update(b"D:");
            hasher.update(relative.as_bytes());
            hasher.update(b"\n");
            continue;
        }
        hasher.update(b"F:");
        hasher.update(relative.as_bytes());
        hasher.update(b"\n");
        hasher.update(fs::read(path)?);
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

fn stable_json_signature<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value)?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Ok(hex::encode(hasher.finalize()))
}

fn normalize_conflict_key_local(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn export_worker_id_local(agent: &UserAgentRecord, index: usize) -> String {
    let indexed_fallback = format!("worker-{}", index + 1);
    let id_fallback = if agent.agent_id.trim().is_empty() {
        indexed_fallback.clone()
    } else {
        normalize_name_like(&agent.agent_id, &indexed_fallback)
    };
    normalize_export_filename_stem_local(&agent.name, &id_fallback)
}

fn unique_label_with_reserved_local(
    preferred: &str,
    reserved: &HashSet<String>,
    fallback: &str,
) -> String {
    let base = normalize_name_like(preferred, fallback);
    if !reserved.contains(&normalize_conflict_key_local(&base)) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let candidate = format!("{base}-{index}");
        if !reserved.contains(&normalize_conflict_key_local(&candidate)) {
            return candidate;
        }
        index += 1;
    }
}

fn normalize_name_like(raw: &str, fallback: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return fallback.to_string();
    }
    let mut output = String::with_capacity(cleaned.len());
    for ch in cleaned.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch.to_ascii_lowercase());
        } else if ch == '_' || ch == '-' {
            output.push(ch);
        } else if ch.is_whitespace() {
            output.push('-');
        }
    }
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    let mut output = output.trim_matches('-').to_string();
    if output.is_empty() {
        output = fallback.to_string();
    }
    output
}

fn normalize_export_filename_stem_local(hive_name: &str, hive_id: &str) -> String {
    let base = if hive_name.trim().is_empty() {
        hive_id.trim()
    } else {
        hive_name.trim()
    };
    let mut output = String::with_capacity(base.len());
    for ch in base.chars() {
        if ch.is_control() {
            continue;
        }
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            output.push('-');
            continue;
        }
        if ch.is_whitespace() {
            output.push('-');
        } else {
            output.push(ch);
        }
    }
    while output.contains("--") {
        output = output.replace("--", "-");
    }
    let cleaned = output.trim_matches(['-', '.', ' ']).to_string();
    if cleaned.is_empty() {
        normalize_name_like(hive_id, "hivepack")
    } else {
        cleaned
    }
}

fn normalize_string_items_local(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        let owned = cleaned.to_string();
        if seen.insert(owned.clone()) {
            items.push(owned);
        }
    }
    items
}

fn normalize_approval_mode_local(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("suggest").trim().to_ascii_lowercase();
    if matches!(cleaned.as_str(), "suggest" | "auto_edit" | "full_auto") {
        cleaned
    } else {
        "suggest".to_string()
    }
}

fn collect_agent_declared_tools_for_export(
    agent: &UserAgentRecord,
    alias_map: &std::collections::HashMap<String, UserToolAlias>,
) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let source_names = if agent.declared_tool_names.is_empty() {
        &agent.tool_names
    } else {
        &agent.declared_tool_names
    };
    for name in source_names {
        let cleaned = name.trim();
        if cleaned.is_empty() {
            continue;
        }
        if alias_map
            .get(cleaned)
            .is_some_and(|alias| matches!(alias.kind, UserToolKind::Skill))
        {
            continue;
        }
        let owned = cleaned.to_string();
        if seen.insert(owned.clone()) {
            names.push(owned);
        }
    }
    names
}

#[derive(Debug, Clone)]
struct ExportSkillSourceLocal {
    name: String,
    include_in_package: bool,
    source_dir: PathBuf,
}

fn collect_agent_skills_for_export(
    agent: &UserAgentRecord,
    bindings: &UserToolBindings,
    skill_root: &Path,
    global_skill_specs: &[SkillSpec],
) -> Vec<ExportSkillSourceLocal> {
    let mut alias_to_skill_name = std::collections::HashMap::new();
    let mut skill_name_to_source = std::collections::HashMap::new();
    for spec in &bindings.skill_specs {
        let alias_name = spec.name.trim();
        if alias_name.is_empty() {
            continue;
        }
        let Some(alias) = bindings.alias_map.get(alias_name) else {
            continue;
        };
        if !matches!(alias.kind, UserToolKind::Skill) {
            continue;
        }
        let skill_name = alias.target.trim();
        if skill_name.is_empty() {
            continue;
        }
        alias_to_skill_name.insert(alias_name.to_string(), skill_name.to_string());
        skill_name_to_source
            .entry(skill_name.to_string())
            .or_insert_with(|| spec.root.clone());
    }
    for spec in global_skill_specs {
        let skill_name = spec.name.trim();
        if skill_name.is_empty() {
            continue;
        }
        skill_name_to_source
            .entry(skill_name.to_string())
            .or_insert_with(|| spec.root.clone());
    }

    let mut output = Vec::new();
    let mut seen = HashSet::new();
    for tool_name in &agent.tool_names {
        let normalized_tool_name = tool_name.trim();
        if normalized_tool_name.is_empty() {
            continue;
        }

        let mut skill_name = String::new();
        if let Some(alias) = bindings.alias_map.get(normalized_tool_name) {
            if matches!(alias.kind, UserToolKind::Skill) && !alias.target.trim().is_empty() {
                skill_name = alias.target.trim().to_string();
            }
        }
        if skill_name.is_empty() {
            if let Some(from_alias) = alias_to_skill_name.get(normalized_tool_name) {
                skill_name = from_alias.clone();
            } else if skill_name_to_source.contains_key(normalized_tool_name) {
                skill_name = normalized_tool_name.to_string();
            } else if let Some((owner_id, maybe_skill_name)) = normalized_tool_name.split_once('@')
            {
                if !owner_id.trim().is_empty()
                    && skill_name_to_source.contains_key(maybe_skill_name)
                {
                    skill_name = maybe_skill_name.to_string();
                }
            }
        }
        if skill_name.is_empty() {
            continue;
        }

        let source_dir = skill_name_to_source
            .get(&skill_name)
            .cloned()
            .or_else(|| {
                let candidate = skill_root.join(&skill_name);
                if candidate.is_dir() && candidate.join("SKILL.md").is_file() {
                    Some(candidate)
                } else {
                    None
                }
            })
            .or_else(|| {
                let candidate = skill_root.join(normalized_tool_name);
                if candidate.is_dir() && candidate.join("SKILL.md").is_file() {
                    Some(candidate)
                } else {
                    None
                }
            });
        let Some(source_dir) = source_dir else {
            continue;
        };
        if !seen.insert(normalize_conflict_key_local(&skill_name)) {
            continue;
        }
        output.push(ExportSkillSourceLocal {
            name: skill_name,
            include_in_package: source_dir.starts_with(skill_root),
            source_dir,
        });
    }

    output
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
