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

pub fn list_items(
    state: &AppState,
    current_user_id: &str,
    query: &ListUserPlazaItemsQuery,
) -> Result<Vec<Value>> {
    let current_user_id = current_user_id.trim();
    let filtered_kind = normalize_item_kind(query.kind.as_deref());
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
        items.push(item_payload(&record, current_user_id));
    }
    Ok(items)
}

pub fn get_item(state: &AppState, item_id: &str) -> Result<Option<UserPlazaItemRecord>> {
    let cleaned = item_id.trim();
    if cleaned.is_empty() {
        return Ok(None);
    }
    let Some(raw) = state.user_store.get_meta(&meta_key(cleaned))? else {
        return Ok(None);
    };
    let record: UserPlazaItemRecord =
        serde_json::from_str(&raw).with_context(|| format!("parse plaza item failed: {cleaned}"))?;
    if !Path::new(&record.artifact_path).is_file() {
        return Ok(None);
    }
    Ok(Some(record))
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
        "hive_pack" => publish_hive_pack(state, user, source_key, &request, existing.as_ref()).await?,
        "worker_card" => publish_worker_card(state, user, source_key, &request, existing.as_ref()).await?,
        "skill_pack" => publish_skill_pack(state, user, source_key, &request, existing.as_ref()).await?,
        _ => return Err(anyhow!("unsupported plaza item kind")),
    };
    let payload = item_payload(&published, &user.user_id);
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
    let Some(record) = get_item(state, cleaned_item_id)? else {
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
    let record = get_item(state, item_id)?.ok_or_else(|| anyhow!("plaza item not found"))?;
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
    let artifact_path =
        resolve_export_artifact_path(&export_job).ok_or_else(|| anyhow!("hive pack export missing artifact"))?;
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
            .and_then(|value| {
                value
                    .get("target_hive_id")
                    .or_else(|| value.get("hive_id"))
            })
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
    let document = fs::read_to_string(artifact)
        .with_context(|| format!("read worker card plaza artifact failed: {}", artifact.display()))?;
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
        return build_default_agent_record_from_storage(state.user_store.storage_backend().as_ref(), user_id);
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
        let mut file = archive.by_index(index).context("invalid skill archive entry")?;
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
            return Err(anyhow!("skill archive conflicts with builtin skill directory"));
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
        return Err(anyhow!("skill archive must contain a dedicated top-level directory"));
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

fn item_payload(record: &UserPlazaItemRecord, current_user_id: &str) -> Value {
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
        "tags": record.tags,
        "metadata": record.metadata,
        "created_at": record.created_at,
        "updated_at": record.updated_at,
        "mine": record.owner_user_id == current_user_id.trim(),
    })
}

fn normalize_item_kind(value: Option<&str>) -> Option<String> {
    match value.unwrap_or_default().trim().to_ascii_lowercase().as_str() {
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

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}
