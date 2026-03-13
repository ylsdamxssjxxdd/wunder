use crate::services::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::services::user_tools::{UserToolBindings, UserToolKind};
use crate::skills::SkillSpec;
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, normalize_sandbox_container_id, HiveRecord, UserAccountRecord,
    UserAgentRecord, DEFAULT_HIVE_ID, DEFAULT_SANDBOX_CONTAINER_ID,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const HIVE_PACK_META_PREFIX: &str = "beeroom_pack_job:";
const HIVE_PACK_TEMP_ENV: &str = "WUNDER_TEMP_DIR_ROOT";
const HIVE_PACK_TEMP_DIR: &str = "wunder_hivepack";
const HIVE_PACK_CHECKSUM_FILE: &str = "checksums.sha256";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivePackJobRecord {
    pub job_id: String,
    pub job_type: String,
    pub user_id: String,
    pub status: String,
    pub phase: String,
    pub progress: i64,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<HivePackArtifact>,
    pub created_at: f64,
    pub updated_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HivePackArtifact {
    pub filename: String,
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HivePackImportOptions {
    #[serde(default, alias = "groupId", alias = "hive_id", alias = "hiveId")]
    pub group_id: Option<String>,
    #[serde(default)]
    pub create_hive_if_missing: Option<bool>,
    #[serde(
        default,
        alias = "conflictMode",
        alias = "importMode",
        alias = "import_mode"
    )]
    pub conflict_mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HivePackExportOptions {
    #[serde(alias = "groupId", alias = "hive_id", alias = "hiveId")]
    pub group_id: String,
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HiveManifest {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    pack: HivePackMeta,
    #[serde(default)]
    workers: Vec<HiveWorkerRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HivePackMeta {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct HiveWorkerRef {
    #[serde(default)]
    worker_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    duty: Option<String>,
    #[serde(default)]
    approval_mode: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    path: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardManifest {
    #[serde(default)]
    schema_version: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    metadata: WorkerCardMetadata,
    #[serde(default)]
    prompt: WorkerCardPrompt,
    #[serde(default)]
    abilities: WorkerCardAbilities,
    #[serde(default)]
    interaction: WorkerCardInteraction,
    #[serde(default)]
    runtime: WorkerCardRuntime,
    #[serde(default)]
    hive: WorkerCardHive,
    #[serde(default)]
    extensions: Option<Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardMetadata {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    exported_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardPrompt {
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    extra_prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardAbilities {
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardInteraction {
    #[serde(default)]
    preset_questions: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardRuntime {
    #[serde(default)]
    approval_mode: Option<String>,
    #[serde(default)]
    sandbox_container_id: Option<i32>,
    #[serde(default)]
    is_shared: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerCardHive {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HiveExportManifest {
    protocol: String,
    kind: String,
    pack: HiveExportPackMeta,
    compatibility: HiveExportCompatibility,
    mount_policy: HiveExportMountPolicy,
    workers: Vec<HiveExportWorker>,
}

#[derive(Debug, Clone, Serialize)]
struct HiveExportPackMeta {
    id: String,
    name: String,
    version: String,
    description: String,
    author: String,
    created_at: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HiveExportCompatibility {
    wunder_version: String,
    runtime_modes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct HiveExportMountPolicy {
    builtin_tools: String,
    mcp_tools: String,
    knowledge_tools: String,
    imported_skills: String,
}

#[derive(Debug, Clone, Serialize)]
struct HiveExportWorker {
    worker_id: String,
    path: String,
    role: String,
    display_name: String,
    description: String,
    duty: String,
    approval_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillExportMeta {
    protocol: String,
    kind: String,
    skill: SkillExportSkill,
}

#[derive(Debug, Clone, Serialize)]
struct SkillExportSkill {
    id: String,
    name: String,
    version: String,
    entry: String,
    language: String,
    tags: Vec<String>,
}

#[derive(Default)]
struct ImportRuntime {
    captured_previous_skills: bool,
    previous_enabled: Vec<String>,
    previous_shared: Vec<String>,
    created_hive: Option<HiveRecord>,
    created_hive_new: bool,
    created_agents: Vec<String>,
    replaced_agents: Vec<UserAgentRecord>,
    replaced_agent_deleted_ids: Vec<String>,
    installed_skill_dirs: Vec<PathBuf>,
    installed_skill_names: Vec<String>,
    replaced_skill_backups: Vec<ReplacedSkillBackup>,
    import_skill_name_keys: HashSet<String>,
}

#[derive(Debug)]
struct WorkerImportSnapshot {
    worker_id: String,
    display_name: String,
    description: String,
    duty: String,
    approval_mode: String,
    icon: Option<String>,
    role_prompt: String,
    declared_tool_names: Vec<String>,
    declared_skill_names: Vec<String>,
    preset_questions: Vec<String>,
    sandbox_container_id: i32,
    installed_skills: Vec<String>,
    skill_installs: Vec<SkillInstallSnapshot>,
}

#[derive(Debug, Clone)]
struct ImportWorkerRef {
    worker_id: String,
    path: PathBuf,
    display_name: Option<String>,
    description: Option<String>,
    duty: Option<String>,
    approval_mode: Option<String>,
    icon: Option<String>,
}

#[derive(Debug, Clone)]
struct WorkerSkillSource {
    source_skill_id: String,
    preferred_name: String,
    source_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct ExportSkillSource {
    name: String,
    source_dir: PathBuf,
}

#[derive(Debug)]
struct SkillInstallSnapshot {
    source_skill_id: String,
    preferred_name: String,
    final_name: String,
}

#[derive(Debug)]
struct ReplacedSkillBackup {
    skill_name: String,
    target_dir: PathBuf,
    backup_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct TargetHiveSelection {
    hive: HiveRecord,
    preferred_hive_id: String,
    preferred_hive_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportConflictMode {
    AutoRenameOnly,
    UpdateReplace,
}

impl ImportConflictMode {
    fn as_policy(self) -> &'static str {
        match self {
            Self::AutoRenameOnly => "auto_rename_only",
            Self::UpdateReplace => "update_replace",
        }
    }

    fn allows_direct_replace(self) -> bool {
        matches!(self, Self::UpdateReplace)
    }
}

pub fn job_payload(job: &HivePackJobRecord) -> Value {
    json!({
        "job_id": job.job_id,
        "job_type": job.job_type,
        "status": job.status,
        "phase": job.phase,
        "progress": job.progress,
        "summary": job.summary,
        "detail": job.detail,
        "report": job.report,
        "artifact": job.artifact,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
    })
}

pub fn get_job_for_user(
    state: &AppState,
    user_id: &str,
    job_id: &str,
) -> Result<Option<HivePackJobRecord>> {
    let cleaned_job = job_id.trim();
    if cleaned_job.is_empty() {
        return Ok(None);
    }
    let key = meta_key(cleaned_job);
    let Some(raw) = state.user_store.get_meta(&key)? else {
        return Ok(None);
    };
    let record: HivePackJobRecord = serde_json::from_str(&raw)
        .with_context(|| format!("parse hive pack job failed: {cleaned_job}"))?;
    if record.user_id != user_id.trim() {
        return Ok(None);
    }
    Ok(Some(record))
}

pub fn resolve_export_artifact_path(job: &HivePackJobRecord) -> Option<PathBuf> {
    job.artifact.as_ref().map(|item| PathBuf::from(&item.path))
}

pub async fn run_import_job(
    state: &AppState,
    user: &UserAccountRecord,
    filename: &str,
    data: Vec<u8>,
    options: HivePackImportOptions,
) -> Result<HivePackJobRecord> {
    let mut job = new_job("import", &user.user_id);
    let mut runtime = ImportRuntime::default();
    persist_job(state, &job)?;
    let result = run_import_job_inner(
        state,
        user,
        filename,
        &data,
        options,
        &mut job,
        &mut runtime,
    )
    .await;
    if let Err(err) = result {
        let rollback = rollback_import(state, &user.user_id, &runtime);
        job.status = "failed".to_string();
        job.phase = "failed".to_string();
        job.progress = 100;
        job.summary = "hivepack import failed".to_string();
        job.detail = Some(json!({
            "error": err.to_string(),
            "rollback": rollback,
        }));
        job.updated_at = now_ts();
        persist_job(state, &job)?;
    }
    Ok(job)
}

pub async fn run_export_job(
    state: &AppState,
    user: &UserAccountRecord,
    options: HivePackExportOptions,
) -> Result<HivePackJobRecord> {
    let mut job = new_job("export", &user.user_id);
    persist_job(state, &job)?;
    let result = run_export_job_inner(state, user, &options, &mut job).await;
    if let Err(err) = result {
        job.status = "failed".to_string();
        job.phase = "failed".to_string();
        job.progress = 100;
        job.summary = "hivepack export failed".to_string();
        job.detail = Some(json!({ "error": err.to_string() }));
        job.updated_at = now_ts();
        persist_job(state, &job)?;
    }
    Ok(job)
}

async fn run_import_job_inner(
    state: &AppState,
    user: &UserAccountRecord,
    filename: &str,
    data: &[u8],
    options: HivePackImportOptions,
    job: &mut HivePackJobRecord,
    runtime: &mut ImportRuntime,
) -> Result<()> {
    if filename.trim().is_empty() {
        return Err(anyhow!("filename is required"));
    }
    if !filename.to_ascii_lowercase().ends_with(".hivepack")
        && !filename.to_ascii_lowercase().ends_with(".zip")
    {
        return Err(anyhow!("hivepack file must end with .hivepack or .zip"));
    }
    if data.is_empty() {
        return Err(anyhow!("hivepack file is empty"));
    }

    update_job(job, "validating", 10, "validating hivepack structure");
    persist_job(state, job)?;

    let import_root = hivepack_temp_root().join("imports").join(&job.job_id);
    if import_root.exists() {
        std::fs::remove_dir_all(&import_root).ok();
    }
    std::fs::create_dir_all(&import_root)?;
    let extract_root = import_root.join("extract");
    std::fs::create_dir_all(&extract_root)?;
    extract_zip(data, &extract_root)?;
    let package_root = resolve_package_root(&extract_root)?;

    let hive_manifest_path = package_root.join("hive.yaml");
    let hive_manifest_text = std::fs::read_to_string(&hive_manifest_path)
        .with_context(|| format!("read {} failed", hive_manifest_path.display()))?;
    let hive_manifest: HiveManifest = serde_yaml::from_str(&hive_manifest_text)
        .with_context(|| format!("parse {} failed", hive_manifest_path.display()))?;
    validate_hive_manifest(&hive_manifest)?;
    let worker_refs = resolve_import_workers(&hive_manifest, &package_root)?;

    update_job(job, "planning", 20, "planning hive import tasks");
    persist_job(state, job)?;

    let current_tools = state.user_tool_store.load_user_tools(&user.user_id);
    runtime.captured_previous_skills = true;
    runtime.previous_enabled = current_tools.skills.enabled.clone();
    runtime.previous_shared = current_tools.skills.shared.clone();
    let import_conflict_mode = normalize_import_conflict_mode(options.conflict_mode.as_deref());

    let target_hive = resolve_or_create_target_hive(
        state,
        &user.user_id,
        &hive_manifest,
        &options,
        import_conflict_mode,
        runtime,
    )?;

    update_job(job, "installing", 35, "installing worker skill packs");
    persist_job(state, job)?;

    let mut worker_snapshots = Vec::new();
    let skill_root = state.user_tool_store.get_skill_root(&user.user_id);
    std::fs::create_dir_all(&skill_root)?;
    let replace_backup_root = import_root.join("replace_backup").join("skills");
    if import_conflict_mode.allows_direct_replace() {
        std::fs::create_dir_all(&replace_backup_root)?;
    }
    for worker_ref in &worker_refs {
        let worker_snapshot = install_worker_snapshot(
            worker_ref,
            &package_root,
            &skill_root,
            import_conflict_mode,
            &replace_backup_root,
            runtime,
        )?;
        worker_snapshots.push(worker_snapshot);
    }

    let mut enabled_set = runtime
        .previous_enabled
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in &runtime.installed_skill_names {
        enabled_set.insert(name.clone());
    }
    state.user_tool_store.update_skills(
        &user.user_id,
        enabled_set.into_iter().collect(),
        runtime.previous_shared.clone(),
    )?;
    state
        .user_tool_manager
        .clear_skill_cache(Some(&user.user_id));

    update_job(job, "creating_agents", 70, "creating worker agents");
    persist_job(state, job)?;

    let context = build_user_tool_context(state, &user.user_id).await;
    let allowed = compute_allowed_tool_names(user, &context);
    let base_tool_names = collect_non_skill_tools(&allowed, &context.bindings.alias_map);
    let existing_agents = state
        .user_store
        .list_user_agents_by_hive(&user.user_id, &target_hive.hive.hive_id)?;
    runtime.replaced_agents = if import_conflict_mode.allows_direct_replace() {
        existing_agents.clone()
    } else {
        Vec::new()
    };
    let mut occupied_agent_name_keys = if import_conflict_mode.allows_direct_replace() {
        HashSet::new()
    } else {
        existing_agents
            .iter()
            .map(|item| normalize_conflict_key(&item.name))
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>()
    };

    // Collect deterministic rename entries so UI can explain conflict outcomes.
    let skill_renames = worker_snapshots
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .skill_installs
                .iter()
                .filter_map(|skill_install| {
                    if skill_install.preferred_name == skill_install.final_name {
                        return None;
                    }
                    Some(json!({
                        "worker_id": snapshot.worker_id,
                        "source_skill_id": skill_install.source_skill_id,
                        "from": skill_install.preferred_name,
                        "to": skill_install.final_name,
                    }))
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut created_agents = Vec::new();
    let mut agent_renames = Vec::new();
    let now = now_ts();
    for snapshot in &worker_snapshots {
        let declared_tool_name_keys = snapshot
            .declared_tool_names
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>();
        let mut tool_names = if declared_tool_name_keys.is_empty() {
            base_tool_names.clone()
        } else {
            base_tool_names
                .iter()
                .filter(|name| declared_tool_name_keys.contains(name.trim()))
                .cloned()
                .collect::<Vec<_>>()
        };
        for skill_name in &snapshot.installed_skills {
            let alias = state
                .user_tool_store
                .build_alias_name(&user.user_id, skill_name);
            if allowed.contains(&alias) {
                tool_names.push(alias);
            }
        }
        tool_names.sort();
        tool_names.dedup();
        let preferred_agent_name = if snapshot.display_name.trim().is_empty() {
            "Imported Worker".to_string()
        } else {
            snapshot.display_name.trim().to_string()
        };
        let final_agent_name = unique_label_with_reserved(
            &snapshot.display_name,
            &occupied_agent_name_keys,
            "Imported Worker",
        );
        if final_agent_name != preferred_agent_name {
            agent_renames.push(json!({
                "worker_id": snapshot.worker_id,
                "from": preferred_agent_name,
                "to": final_agent_name,
            }));
        }
        occupied_agent_name_keys.insert(normalize_conflict_key(&final_agent_name));

        let record = UserAgentRecord {
            agent_id: format!("agent_{}", Uuid::new_v4().simple()),
            user_id: user.user_id.clone(),
            hive_id: target_hive.hive.hive_id.clone(),
            name: final_agent_name.clone(),
            description: snapshot.description.clone(),
            system_prompt: snapshot.role_prompt.clone(),
            tool_names,
            declared_tool_names: snapshot.declared_tool_names.clone(),
            declared_skill_names: snapshot.declared_skill_names.clone(),
            preset_questions: snapshot.preset_questions.clone(),
            access_level: "A".to_string(),
            approval_mode: snapshot.approval_mode.clone(),
            is_shared: false,
            status: "active".to_string(),
            icon: snapshot.icon.clone(),
            sandbox_container_id: snapshot.sandbox_container_id,
            created_at: now,
            updated_at: now,
        };
        state.user_store.upsert_user_agent(&record)?;
        runtime.created_agents.push(record.agent_id.clone());
        created_agents.push(json!({
            "agent_id": record.agent_id,
            "name": record.name,
            "worker_id": snapshot.worker_id,
            "duty": snapshot.duty,
            "skill_total": snapshot.installed_skills.len(),
            "preset_question_total": snapshot.preset_questions.len(),
        }));
    }

    let mut replaced_agent_total = 0usize;
    if import_conflict_mode.allows_direct_replace() && !runtime.replaced_agents.is_empty() {
        let replaced_agents = runtime.replaced_agents.clone();
        replaced_agent_total =
            replace_existing_hive_agents(state, &user.user_id, &replaced_agents, runtime)?;
    }

    update_job(job, "activating", 90, "activating hivepack");
    persist_job(state, job)?;

    // Keep an aggregate conflict summary for fast front-end feedback.
    let hive_renamed = target_hive.preferred_hive_id != target_hive.hive.hive_id
        || normalize_conflict_key(&target_hive.preferred_hive_name)
            != normalize_conflict_key(&target_hive.hive.name);
    let mut renamed_total = skill_renames.len() + agent_renames.len();
    if hive_renamed {
        renamed_total += 1;
    }

    job.status = "completed".to_string();
    job.phase = "completed".to_string();
    job.progress = 100;
    job.summary = "hivepack import completed".to_string();
    job.report = Some(json!({
        "hive_id": target_hive.hive.hive_id,
        "hive_name": target_hive.hive.name,
        "created_hive": runtime.created_hive_new,
        "worker_total": worker_snapshots.len(),
        "agents": created_agents,
        "skills_installed": runtime.installed_skill_names,
        "conflicts": {
            "policy": import_conflict_mode.as_policy(),
            "renamed_total": renamed_total,
            "hive": {
                "renamed": hive_renamed,
                "from": {
                    "hive_id": target_hive.preferred_hive_id,
                    "name": target_hive.preferred_hive_name,
                },
                "to": {
                    "hive_id": target_hive.hive.hive_id,
                    "name": target_hive.hive.name,
                }
            },
            "agents": {
                "renamed_total": agent_renames.len(),
                "renames": agent_renames,
            },
            "skills": {
                "renamed_total": skill_renames.len(),
                "renames": skill_renames,
            },
        },
        "replace": {
            "enabled": import_conflict_mode.allows_direct_replace(),
            "replaced_agent_total": replaced_agent_total,
        },
        "package": {
            "id": hive_manifest.pack.id,
            "name": hive_manifest.pack.name,
            "version": hive_manifest.pack.version,
        }
    }));
    job.updated_at = now_ts();
    persist_job(state, job)?;

    std::fs::remove_dir_all(&import_root).ok();
    Ok(())
}

async fn run_export_job_inner(
    state: &AppState,
    user: &UserAccountRecord,
    options: &HivePackExportOptions,
    job: &mut HivePackJobRecord,
) -> Result<()> {
    let hive_id = normalize_hive_id(&options.group_id);
    let Some(hive) = state.user_store.get_hive(&user.user_id, &hive_id)? else {
        return Err(anyhow!("hive {hive_id} not found"));
    };

    update_job(job, "planning", 15, "collecting hive members");
    persist_job(state, job)?;

    let agents = state
        .user_store
        .list_user_agents_by_hive(&user.user_id, &hive.hive_id)?;
    if agents.is_empty() {
        return Err(anyhow!("hive {} has no agents to export", hive.hive_id));
    }

    let export_root = hivepack_temp_root().join("exports").join(&job.job_id);
    if export_root.exists() {
        std::fs::remove_dir_all(&export_root).ok();
    }
    std::fs::create_dir_all(&export_root)?;
    let package_root = export_root.join("package");
    std::fs::create_dir_all(&package_root)?;
    std::fs::create_dir_all(package_root.join("workers"))?;
    std::fs::create_dir_all(package_root.join("skills"))?;

    update_job(job, "installing", 35, "building hivepack structure");
    persist_job(state, job)?;

    let config = state.config_store.get().await;
    let skills = state.skills.read().await.clone();
    let global_skill_specs = skills.list_specs();
    let bindings = state
        .user_tool_manager
        .build_bindings(&config, &skills, &user.user_id);
    let skill_root = state.user_tool_store.get_skill_root(&user.user_id);
    let export_mode = normalize_export_mode(options.mode.as_deref());

    let mut workers = Vec::new();
    let mut worker_reports = Vec::new();
    let mut total_skill_links = 0usize;
    let mut exported_skill_names: BTreeSet<String> = BTreeSet::new();
    let mut occupied_worker_id_keys = HashSet::new();
    for (index, agent) in agents.iter().enumerate() {
        let preferred_worker_id = export_worker_id(agent, index);
        let worker_id = unique_label_with_reserved(
            &preferred_worker_id,
            &occupied_worker_id_keys,
            &format!("worker-{}", index + 1),
        );
        occupied_worker_id_keys.insert(normalize_conflict_key(&worker_id));
        let worker_dir = package_root.join("workers").join(&worker_id);
        std::fs::create_dir_all(&worker_dir)?;
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
            let source = &skill.source_dir;
            if !source.exists() || !source.is_dir() || !source.join("SKILL.md").is_file() {
                continue;
            }
            attached_skill_names.push(skill_name.to_string());
            if exported_skill_names.contains(skill_name) {
                continue;
            }
            let relative = package_root.join("skills").join(skill_name);
            if export_mode == "full" {
                copy_dir_recursive(source, &relative)?;
            } else {
                std::fs::create_dir_all(&relative)?;
                std::fs::write(
                    relative.join("SKILL.md"),
                    b"# Placeholder

reference_only mode does not include full skill files.
",
                )?;
            }
            write_skill_meta(&relative, skill_name)?;
            exported_skill_names.insert(skill_name.to_string());
        }
        attached_skill_names.sort();
        attached_skill_names.dedup();
        let declared_tool_names = collect_agent_declared_tools_for_export(agent, &bindings.alias_map);
        let declared_skill_names = if agent.declared_skill_names.is_empty() {
            attached_skill_names.clone()
        } else {
            normalize_string_items(&agent.declared_skill_names)
        };
        let worker_card = build_worker_card_manifest(
            &worker_id,
            agent,
            &hive,
            &declared_tool_names,
            &declared_skill_names,
        );
        write_worker_card_manifest(&worker_dir, &worker_card)?;
        total_skill_links += attached_skill_names.len();

        workers.push(HiveExportWorker {
            worker_id: worker_id.clone(),
            path: format!("workers/{worker_id}"),
            role: "specialist".to_string(),
            display_name: agent.name.clone(),
            description: agent.description.clone(),
            duty: "specialist".to_string(),
            approval_mode: normalize_approval_mode(Some(&agent.approval_mode)),
            icon: agent.icon.clone(),
        });
        worker_reports.push(json!({
            "worker_id": worker_id,
            "agent_id": agent.agent_id,
            "agent_name": agent.name,
            "skills": attached_skill_names,
        }));
    }

    let hive_manifest = HiveExportManifest {
        protocol: "hpp/1.0".to_string(),
        kind: "hive_pack".to_string(),
        pack: HiveExportPackMeta {
            id: format!("hivepack_{}", hive.hive_id),
            name: hive.name.clone(),
            version: "1.0.0".to_string(),
            description: hive.description.clone(),
            author: user.user_id.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            tags: vec!["beeroom".to_string(), "hivepack".to_string()],
        },
        compatibility: HiveExportCompatibility {
            wunder_version: ">=0.1.0".to_string(),
            runtime_modes: vec![
                "server".to_string(),
                "desktop".to_string(),
                "cli".to_string(),
            ],
        },
        mount_policy: HiveExportMountPolicy {
            builtin_tools: "system_all".to_string(),
            mcp_tools: "system_all".to_string(),
            knowledge_tools: "system_all".to_string(),
            imported_skills: "package_only".to_string(),
        },
        workers,
    };
    std::fs::write(
        package_root.join("hive.yaml"),
        serde_yaml::to_string(&hive_manifest)?,
    )?;
    std::fs::write(
        package_root.join("HIVE_ROLE.md"),
        hive.description.as_bytes(),
    )?;
    write_checksums(&package_root)?;

    update_job(job, "activating", 80, "assembling hivepack archive");
    persist_job(state, job)?;

    let package_filename = format!(
        "{}-{}.zip",
        normalize_export_filename_stem(&hive.name, &hive.hive_id),
        chrono::Local::now().format("%Y%m%d%H%M%S")
    );
    let package_path = export_root.join(&package_filename);
    zip_directory(&package_root, &package_path)?;
    let size_bytes = std::fs::metadata(&package_path)
        .map(|item| item.len())
        .unwrap_or(0);

    job.status = "completed".to_string();
    job.phase = "completed".to_string();
    job.progress = 100;
    job.summary = "hivepack export completed".to_string();
    job.report = Some(json!({
        "hive_id": hive.hive_id,
        "hive_name": hive.name,
        "worker_total": agents.len(),
        "skill_total": total_skill_links,
        "unique_skill_total": exported_skill_names.len(),
        "mode": export_mode,
        "workers": worker_reports,
    }));
    job.artifact = Some(HivePackArtifact {
        filename: package_filename,
        path: package_path.to_string_lossy().to_string(),
        size_bytes,
    });
    job.updated_at = now_ts();
    persist_job(state, job)?;
    Ok(())
}

fn install_worker_snapshot(
    worker_ref: &ImportWorkerRef,
    package_root: &Path,
    skill_root: &Path,
    conflict_mode: ImportConflictMode,
    replace_backup_root: &Path,
    runtime: &mut ImportRuntime,
) -> Result<WorkerImportSnapshot> {
    let worker_root = package_root.join(&worker_ref.path);
    if !worker_root.exists() || !worker_root.is_dir() {
        return Err(anyhow!(
            "worker path missing: {}",
            worker_root.to_string_lossy()
        ));
    }
    let worker_card = load_worker_card_manifest(&worker_root)?.ok_or_else(|| {
        anyhow!(
            "worker card missing: provide workers/<id>/worker-card.json"
        )
    })?;

    let role_prompt = resolve_worker_role_prompt(&worker_card)?;
    let display_name = worker_card
        .metadata
        .name
        .clone()
        .or_else(|| worker_ref.display_name.clone())
        .unwrap_or_else(|| worker_ref.worker_id.clone());
    let description = worker_card
        .metadata
        .description
        .clone()
        .or_else(|| worker_ref.description.clone())
        .unwrap_or_default();
    let duty = worker_ref.duty.clone().unwrap_or_default();
    let approval_mode = normalize_approval_mode(
        worker_card
            .runtime
            .approval_mode
            .as_deref()
            .or(worker_ref.approval_mode.as_deref()),
    );
    let icon = worker_card.metadata.icon.clone().or_else(|| worker_ref.icon.clone());
    let declared_tool_names = normalize_string_items(&worker_card.abilities.tool_names);
    let declared_skill_names = normalize_string_items(&worker_card.abilities.skills);
    let preset_questions = normalize_string_items(&worker_card.interaction.preset_questions);
    let sandbox_container_id = normalize_sandbox_container_id(
        worker_card
            .runtime
            .sandbox_container_id
            .unwrap_or(DEFAULT_SANDBOX_CONTAINER_ID),
    );

    let skill_sources = resolve_worker_skill_sources(package_root, &worker_root, &worker_card)?;
    let mut installed_skills = Vec::new();
    let mut skill_installs = Vec::new();
    for skill_source in &skill_sources {
        let preferred = normalize_name(&skill_source.preferred_name, "skill");
        let source_skill_id = if skill_source.source_skill_id.trim().is_empty() {
            preferred.clone()
        } else {
            skill_source.source_skill_id.trim().to_string()
        };
        let final_name = resolve_import_skill_name(
            skill_root,
            &preferred,
            conflict_mode,
            &runtime.import_skill_name_keys,
        );
        let skill_target = skill_root.join(&final_name);
        if conflict_mode.allows_direct_replace() && skill_target.exists() {
            let backup_dir =
                replace_backup_root.join(format!("{}-{}", final_name, Uuid::new_v4().simple()));
            copy_dir_recursive(&skill_target, &backup_dir)?;
            runtime.replaced_skill_backups.push(ReplacedSkillBackup {
                skill_name: final_name.clone(),
                target_dir: skill_target.clone(),
                backup_dir,
            });
            std::fs::remove_dir_all(&skill_target)?;
        }
        copy_dir_recursive(&skill_source.source_dir, &skill_target)?;
        runtime.installed_skill_dirs.push(skill_target);
        runtime.installed_skill_names.push(final_name.clone());
        runtime.import_skill_name_keys.insert(final_name.clone());
        skill_installs.push(SkillInstallSnapshot {
            source_skill_id,
            preferred_name: preferred,
            final_name: final_name.clone(),
        });
        installed_skills.push(final_name);
    }

    Ok(WorkerImportSnapshot {
        worker_id: worker_ref.worker_id.clone(),
        display_name,
        description,
        duty,
        approval_mode,
        icon,
        role_prompt,
        declared_tool_names,
        declared_skill_names,
        preset_questions,
        sandbox_container_id,
        installed_skills,
        skill_installs,
    })
}


fn resolve_import_workers(
    hive_manifest: &HiveManifest,
    package_root: &Path,
) -> Result<Vec<ImportWorkerRef>> {
    let mut workers = Vec::new();
    if !hive_manifest.workers.is_empty() {
        for (index, worker_ref) in hive_manifest.workers.iter().enumerate() {
            let worker_path = validate_relative_path(&worker_ref.path)?;
            workers.push(ImportWorkerRef {
                worker_id: resolve_worker_id(
                    worker_ref.worker_id.as_deref(),
                    worker_path.file_name().and_then(|name| name.to_str()),
                    index + 1,
                ),
                path: worker_path,
                display_name: worker_ref.display_name.clone(),
                description: worker_ref.description.clone(),
                duty: worker_ref.duty.clone(),
                approval_mode: worker_ref.approval_mode.clone(),
                icon: worker_ref.icon.clone(),
            });
        }
    } else {
        let workers_root = package_root.join("workers");
        if workers_root.is_dir() {
            let mut entries = std::fs::read_dir(&workers_root)?
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_string());
            for (index, entry) in entries.into_iter().enumerate() {
                let worker_dir_name = entry.file_name().to_string_lossy().to_string();
                let worker_path = validate_relative_path(&format!("workers/{worker_dir_name}"))?;
                workers.push(ImportWorkerRef {
                    worker_id: resolve_worker_id(
                        Some(&worker_dir_name),
                        Some(&worker_dir_name),
                        index + 1,
                    ),
                    path: worker_path,
                    display_name: None,
                    description: None,
                    duty: None,
                    approval_mode: None,
                    icon: None,
                });
            }
        }
    }
    if workers.is_empty() {
        return Err(anyhow!(
            "no workers found: provide hive.yaml workers[] or create workers/<id>/ directories"
        ));
    }

    // Ensure worker IDs are deterministic and unique for report and conflict handling.
    let mut occupied = HashSet::new();
    for worker in &mut workers {
        worker.worker_id = unique_slug_with_reserved(&worker.worker_id, &occupied, "worker");
        occupied.insert(worker.worker_id.clone());
    }
    Ok(workers)
}

fn normalize_string_items(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for value in values {
        let cleaned = value.trim();
        if cleaned.is_empty() {
            continue;
        }
        let owned = cleaned.to_string();
        if !seen.insert(owned.clone()) {
            continue;
        }
        items.push(owned);
    }
    items
}

fn load_worker_card_manifest(worker_root: &Path) -> Result<Option<WorkerCardManifest>> {
    for candidate in ["worker-card.json", "worker-card.yaml", "worker-card.yml"] {
        let path = worker_root.join(candidate);
        if !path.is_file() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {} failed", path.display()))?;
        let raw = raw.strip_prefix('\u{feff}').unwrap_or(&raw);
        let manifest = if candidate.ends_with(".json") {
            serde_json::from_str::<WorkerCardManifest>(raw)
                .with_context(|| format!("parse {} failed", path.display()))?
        } else {
            serde_yaml::from_str::<WorkerCardManifest>(raw)
                .with_context(|| format!("parse {} failed", path.display()))?
        };
        validate_worker_card_manifest(&manifest)?;
        return Ok(Some(manifest));
    }
    Ok(None)
}

fn validate_worker_card_manifest(manifest: &WorkerCardManifest) -> Result<()> {
    if manifest
        .schema_version
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && !value.starts_with("wunder/worker-card@"))
    {
        return Err(anyhow!("unsupported worker card schema version"));
    }
    if manifest.kind.as_deref().is_some_and(|kind| {
        let normalized = kind.trim();
        !normalized.is_empty()
            && normalized != "WorkerCard"
            && normalized != "worker-card"
            && normalized != "worker_card"
    }) {
        return Err(anyhow!("invalid worker card kind"));
    }
    Ok(())
}

fn worker_card_prompt_text(worker_card: &WorkerCardManifest) -> String {
    [
        worker_card.prompt.system_prompt.as_deref(),
        worker_card.prompt.extra_prompt.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn resolve_worker_role_prompt(worker_card: &WorkerCardManifest) -> Result<String> {
    let prompt = worker_card_prompt_text(worker_card);
    if !prompt.is_empty() {
        return Ok(prompt);
    }
    Ok(String::new())
}

fn build_worker_card_manifest(
    worker_id: &str,
    agent: &UserAgentRecord,
    hive: &HiveRecord,
    declared_tool_names: &[String],
    attached_skill_names: &[String],
) -> WorkerCardManifest {
    WorkerCardManifest {
        schema_version: Some("wunder/worker-card@1".to_string()),
        kind: Some("WorkerCard".to_string()),
        metadata: WorkerCardMetadata {
            id: Some(worker_id.to_string()),
            name: Some(agent.name.clone()),
            description: Some(agent.description.clone()),
            icon: agent.icon.clone(),
            exported_at: Some(chrono::Utc::now().to_rfc3339()),
        },
        prompt: WorkerCardPrompt {
            system_prompt: Some(agent.system_prompt.clone()),
            extra_prompt: None,
        },
        abilities: WorkerCardAbilities {
            tool_names: normalize_string_items(declared_tool_names),
            skills: normalize_string_items(attached_skill_names),
        },
        interaction: WorkerCardInteraction {
            preset_questions: normalize_string_items(&agent.preset_questions),
        },
        runtime: WorkerCardRuntime {
            approval_mode: Some(normalize_approval_mode(Some(&agent.approval_mode))),
            sandbox_container_id: Some(normalize_sandbox_container_id(agent.sandbox_container_id)),
            is_shared: Some(agent.is_shared),
        },
        hive: WorkerCardHive {
            id: Some(hive.hive_id.clone()),
            name: Some(hive.name.clone()),
            description: Some(hive.description.clone()),
        },
        extensions: None,
    }
}

fn write_worker_card_manifest(worker_root: &Path, worker_card: &WorkerCardManifest) -> Result<()> {
    std::fs::write(
        worker_root.join("worker-card.json"),
        serde_json::to_vec_pretty(worker_card)?,
    )?;
    Ok(())
}

fn resolve_worker_skill_sources(
    package_root: &Path,
    worker_root: &Path,
    worker_card: &WorkerCardManifest,
) -> Result<Vec<WorkerSkillSource>> {
    let skill_names = normalize_string_items(&worker_card.abilities.skills);
    let mut sources = Vec::new();
    for name in skill_names {
        let Some(source_dir) = resolve_declared_skill_source_dir(package_root, worker_root, &name)?
        else {
            continue;
        };
        sources.push(WorkerSkillSource {
            source_skill_id: name.clone(),
            preferred_name: name,
            source_dir,
        });
    }
    Ok(sources)
}

fn resolve_declared_skill_source_dir(
    package_root: &Path,
    worker_root: &Path,
    skill_name: &str,
) -> Result<Option<PathBuf>> {
    let sanitized = skill_name.trim();
    if sanitized.is_empty() {
        return Err(anyhow!("declared skill name is empty"));
    }
    let normalized = normalize_name(sanitized, "skill");
    let candidate_dirs = [
        package_root.join("skills").join(sanitized),
        package_root.join("skills").join(&normalized),
        worker_root.join("skills").join(sanitized),
        worker_root.join("skills").join(&normalized),
    ];
    Ok(candidate_dirs
        .into_iter()
        .find(|path| path.is_dir() && path.join("SKILL.md").is_file()))
}

fn resolve_worker_id(preferred: Option<&str>, from_path: Option<&str>, index: usize) -> String {
    let base = preferred
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| from_path.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or("worker");
    let fallback = format!("worker-{index}");
    normalize_name(base, &fallback)
}

fn collect_non_skill_tools(
    allowed: &HashSet<String>,
    alias_map: &HashMap<String, crate::services::user_tools::UserToolAlias>,
) -> Vec<String> {
    let mut names = allowed
        .iter()
        .filter_map(|name| {
            if let Some(alias) = alias_map.get(name) {
                if matches!(alias.kind, UserToolKind::Skill) {
                    return None;
                }
            }
            Some(name.clone())
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn collect_agent_declared_tools_for_export(
    agent: &UserAgentRecord,
    alias_map: &HashMap<String, crate::services::user_tools::UserToolAlias>,
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

fn collect_agent_skills_for_export(
    agent: &UserAgentRecord,
    bindings: &UserToolBindings,
    skill_root: &Path,
    global_skill_specs: &[SkillSpec],
) -> Vec<ExportSkillSource> {
    let mut alias_to_skill_name = HashMap::new();
    let mut skill_name_to_source = HashMap::new();
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
        if !seen.insert(normalize_conflict_key(&skill_name)) {
            continue;
        }
        output.push(ExportSkillSource {
            name: skill_name,
            source_dir,
        });
    }

    output
}

fn resolve_or_create_target_hive(
    state: &AppState,
    user_id: &str,
    hive_manifest: &HiveManifest,
    options: &HivePackImportOptions,
    conflict_mode: ImportConflictMode,
    runtime: &mut ImportRuntime,
) -> Result<TargetHiveSelection> {
    state.user_store.ensure_default_hive(user_id)?;
    let requested_hive = options
        .group_id
        .as_deref()
        .map(normalize_hive_id)
        .filter(|value| !value.is_empty());
    let create_hive_if_missing = options.create_hive_if_missing.unwrap_or(true);

    let preferred_hive_name = hive_manifest
        .pack
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Imported Hive");
    let fallback_description = hive_manifest.pack.description.clone().unwrap_or_default();

    let existing_hives = state.user_store.list_hives(user_id, true)?;
    let occupied_hive_ids = existing_hives
        .iter()
        .map(|item| normalize_hive_id(&item.hive_id))
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();
    let occupied_hive_name_keys = existing_hives
        .iter()
        .map(|item| normalize_conflict_key(&item.name))
        .filter(|value| !value.is_empty())
        .collect::<HashSet<_>>();

    let preferred_hive_id = requested_hive.clone().unwrap_or_else(|| {
        hive_manifest
            .pack
            .id
            .as_deref()
            .map(normalize_hive_id)
            .filter(|value| !value.is_empty() && value != DEFAULT_HIVE_ID)
            .unwrap_or_else(|| normalize_hive_id(preferred_hive_name))
    });
    let preferred_hive_id = if preferred_hive_id == DEFAULT_HIVE_ID {
        format!("hivepack-{}", Uuid::new_v4().simple())
    } else {
        preferred_hive_id
    };
    let preferred_hive_name_string = preferred_hive_name.to_string();

    if conflict_mode.allows_direct_replace() {
        if let Some(hive_id) = requested_hive.as_deref() {
            if let Some(existing) = state.user_store.get_hive(user_id, hive_id)? {
                let updated = update_hive_metadata(
                    state,
                    &existing,
                    Some(preferred_hive_name),
                    Some(&fallback_description),
                )?;
                runtime.created_hive = Some(updated.clone());
                runtime.created_hive_new = false;
                return Ok(TargetHiveSelection {
                    hive: updated,
                    preferred_hive_id,
                    preferred_hive_name: preferred_hive_name_string,
                });
            }
            if !create_hive_if_missing {
                return Err(anyhow!("target hive {hive_id} not found"));
            }
            let created = create_hive(
                state,
                user_id,
                hive_id,
                preferred_hive_name,
                &fallback_description,
            )?;
            runtime.created_hive = Some(created.clone());
            runtime.created_hive_new = true;
            return Ok(TargetHiveSelection {
                hive: created,
                preferred_hive_id,
                preferred_hive_name: preferred_hive_name_string,
            });
        }

        if let Some(existing) = state.user_store.get_hive(user_id, &preferred_hive_id)? {
            let updated = update_hive_metadata(
                state,
                &existing,
                Some(preferred_hive_name),
                Some(&fallback_description),
            )?;
            runtime.created_hive = Some(updated.clone());
            runtime.created_hive_new = false;
            return Ok(TargetHiveSelection {
                hive: updated,
                preferred_hive_id,
                preferred_hive_name: preferred_hive_name_string,
            });
        }

        if let Some(existing) = existing_hives
            .iter()
            .find(|item| {
                normalize_conflict_key(&item.name) == normalize_conflict_key(preferred_hive_name)
            })
            .cloned()
        {
            let updated = update_hive_metadata(
                state,
                &existing,
                Some(preferred_hive_name),
                Some(&fallback_description),
            )?;
            runtime.created_hive = Some(updated.clone());
            runtime.created_hive_new = false;
            return Ok(TargetHiveSelection {
                hive: updated,
                preferred_hive_id,
                preferred_hive_name: preferred_hive_name_string,
            });
        }

        let final_hive_id = if occupied_hive_ids.contains(&preferred_hive_id) {
            unique_slug_with_reserved(&preferred_hive_id, &occupied_hive_ids, "hivepack")
        } else {
            preferred_hive_id.clone()
        };
        let final_hive_name =
            if occupied_hive_name_keys.contains(&normalize_conflict_key(preferred_hive_name)) {
                unique_label_with_reserved(
                    preferred_hive_name,
                    &occupied_hive_name_keys,
                    "Imported Hive",
                )
            } else {
                preferred_hive_name.to_string()
            };
        let created = create_hive(
            state,
            user_id,
            &final_hive_id,
            &final_hive_name,
            &fallback_description,
        )?;
        runtime.created_hive = Some(created.clone());
        runtime.created_hive_new = true;
        return Ok(TargetHiveSelection {
            hive: created,
            preferred_hive_id,
            preferred_hive_name: preferred_hive_name_string,
        });
    }

    if let Some(hive_id) = requested_hive.as_deref() {
        if state.user_store.get_hive(user_id, hive_id)?.is_none() && !create_hive_if_missing {
            return Err(anyhow!("target hive {hive_id} not found"));
        }
    }

    let final_hive_id =
        unique_slug_with_reserved(&preferred_hive_id, &occupied_hive_ids, "hivepack");
    let final_hive_name = unique_label_with_reserved(
        preferred_hive_name,
        &occupied_hive_name_keys,
        "Imported Hive",
    );

    let created = create_hive(
        state,
        user_id,
        &final_hive_id,
        &final_hive_name,
        &fallback_description,
    )?;
    runtime.created_hive = Some(created.clone());
    runtime.created_hive_new = true;
    Ok(TargetHiveSelection {
        hive: created,
        preferred_hive_id,
        preferred_hive_name: preferred_hive_name_string,
    })
}

fn create_hive(
    state: &AppState,
    user_id: &str,
    hive_id: &str,
    name: &str,
    description: &str,
) -> Result<HiveRecord> {
    let now = now_ts();
    let record = HiveRecord {
        hive_id: normalize_hive_id(hive_id),
        user_id: user_id.to_string(),
        name: name.trim().to_string(),
        description: description.to_string(),
        is_default: false,
        status: "active".to_string(),
        created_time: now,
        updated_time: now,
    };
    state.user_store.upsert_hive(&record)?;
    Ok(record)
}

fn update_hive_metadata(
    state: &AppState,
    existing: &HiveRecord,
    next_name: Option<&str>,
    next_description: Option<&str>,
) -> Result<HiveRecord> {
    let mut updated = existing.clone();
    if let Some(name) = next_name {
        let cleaned = name.trim();
        if !cleaned.is_empty() {
            updated.name = cleaned.to_string();
        }
    }
    if let Some(description) = next_description {
        updated.description = description.to_string();
    }
    updated.updated_time = now_ts();
    state.user_store.upsert_hive(&updated)?;
    Ok(updated)
}

fn replace_existing_hive_agents(
    state: &AppState,
    user_id: &str,
    replaced_agents: &[UserAgentRecord],
    runtime: &mut ImportRuntime,
) -> Result<usize> {
    if replaced_agents.is_empty() {
        return Ok(0);
    }
    let mut deleted_ids = Vec::new();
    for agent in replaced_agents {
        if let Err(err) = state.user_store.delete_user_agent(user_id, &agent.agent_id) {
            for restored in replaced_agents {
                if !deleted_ids.contains(&restored.agent_id) {
                    continue;
                }
                state.user_store.upsert_user_agent(restored).ok();
            }
            return Err(anyhow!(
                "replace existing hive agents failed on {}: {err}",
                agent.agent_id
            ));
        }
        deleted_ids.push(agent.agent_id.clone());
    }
    runtime.replaced_agent_deleted_ids = deleted_ids;
    Ok(replaced_agents.len())
}

fn rollback_import(state: &AppState, user_id: &str, runtime: &ImportRuntime) -> Value {
    // Best-effort rollback: keep the system in a consistent state even if partial failures happen.
    let mut errors = Vec::new();
    for agent_id in &runtime.created_agents {
        if let Err(err) = state.user_store.delete_user_agent(user_id, agent_id) {
            errors.push(format!("delete agent {agent_id} failed: {err}"));
        }
    }
    for skill_dir in &runtime.installed_skill_dirs {
        if let Err(err) = std::fs::remove_dir_all(skill_dir) {
            errors.push(format!(
                "remove skill dir {} failed: {err}",
                skill_dir.display()
            ));
        }
    }
    for backup in &runtime.replaced_skill_backups {
        if backup.target_dir.exists() {
            if let Err(err) = std::fs::remove_dir_all(&backup.target_dir) {
                errors.push(format!(
                    "remove replaced skill dir {} failed: {err}",
                    backup.target_dir.display()
                ));
                continue;
            }
        }
        if let Err(err) = copy_dir_recursive(&backup.backup_dir, &backup.target_dir) {
            errors.push(format!(
                "restore replaced skill {} from {} failed: {err}",
                backup.skill_name,
                backup.backup_dir.display()
            ));
        }
    }
    let replaced_deleted_ids = runtime
        .replaced_agent_deleted_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    for agent in &runtime.replaced_agents {
        if !replaced_deleted_ids.contains(&agent.agent_id) {
            continue;
        }
        if let Err(err) = state.user_store.upsert_user_agent(agent) {
            errors.push(format!(
                "restore replaced agent {} failed: {err}",
                agent.agent_id
            ));
        }
    }
    if runtime.captured_previous_skills {
        if let Err(err) = state.user_tool_store.update_skills(
            user_id,
            runtime.previous_enabled.clone(),
            runtime.previous_shared.clone(),
        ) {
            errors.push(format!("restore skills config failed: {err}"));
        }
        state.user_tool_manager.clear_skill_cache(Some(user_id));
    }
    if runtime.created_hive_new {
        if let Some(mut hive) = runtime.created_hive.clone() {
            hive.status = "archived".to_string();
            hive.updated_time = now_ts();
            if let Err(err) = state.user_store.upsert_hive(&hive) {
                errors.push(format!("archive created hive failed: {err}"));
            }
        }
    }
    json!({
        "errors": errors,
        "created_agents": runtime.created_agents,
        "installed_skills": runtime.installed_skill_names,
        "replaced_agents_restored": runtime.replaced_agent_deleted_ids,
        "replaced_skills_restored": runtime
            .replaced_skill_backups
            .iter()
            .map(|item| item.skill_name.clone())
            .collect::<Vec<_>>(),
    })
}

fn normalize_export_mode(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("full").trim().to_ascii_lowercase();
    if cleaned == "reference_only" {
        "reference_only".to_string()
    } else {
        "full".to_string()
    }
}

fn normalize_import_conflict_mode(raw: Option<&str>) -> ImportConflictMode {
    let cleaned = raw
        .unwrap_or("auto_rename_only")
        .trim()
        .to_ascii_lowercase();
    if matches!(
        cleaned.as_str(),
        "update_replace" | "replace" | "update-replace"
    ) {
        ImportConflictMode::UpdateReplace
    } else {
        ImportConflictMode::AutoRenameOnly
    }
}

fn export_worker_id(agent: &UserAgentRecord, index: usize) -> String {
    let indexed_fallback = format!("worker-{}", index + 1);
    let id_fallback = if agent.agent_id.trim().is_empty() {
        indexed_fallback.clone()
    } else {
        normalize_name(&agent.agent_id, &indexed_fallback)
    };
    normalize_export_filename_stem(&agent.name, &id_fallback)
}

fn write_skill_meta(skill_root: &Path, skill_name: &str) -> Result<()> {
    let payload = SkillExportMeta {
        protocol: "hpp/1.0".to_string(),
        kind: "skill_pack".to_string(),
        skill: SkillExportSkill {
            id: skill_name.to_string(),
            name: skill_name.to_string(),
            version: "1.0.0".to_string(),
            entry: "SKILL.md".to_string(),
            language: "zh-CN".to_string(),
            tags: vec!["skill".to_string()],
        },
    };
    std::fs::write(
        skill_root.join("skill.yaml"),
        serde_yaml::to_string(&payload)?,
    )?;
    Ok(())
}

fn write_checksums(package_root: &Path) -> Result<()> {
    let mut lines = Vec::new();
    for entry in WalkDir::new(package_root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
    {
        let path = entry.path();
        let relative = path
            .strip_prefix(package_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative == HIVE_PACK_CHECKSUM_FILE {
            continue;
        }
        let bytes = std::fs::read(path)?;
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hex::encode(hasher.finalize());
        lines.push(format!("{digest}  {relative}"));
    }
    lines.sort();
    let text = if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    };
    std::fs::write(package_root.join(HIVE_PACK_CHECKSUM_FILE), text.as_bytes())?;
    Ok(())
}

fn zip_directory(source_root: &Path, target_zip: &Path) -> Result<()> {
    if let Some(parent) = target_zip.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(target_zip)?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut entries = WalkDir::new(source_root)
        .into_iter()
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.path().to_string_lossy().to_string());
    for entry in entries {
        let path = entry.path();
        let relative = path
            .strip_prefix(source_root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if relative.is_empty() {
            continue;
        }
        if entry.file_type().is_dir() {
            writer.add_directory(format!("{relative}/"), options)?;
            continue;
        }
        writer.start_file(relative, options)?;
        let bytes = std::fs::read(path)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    for entry in WalkDir::new(source).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        let relative = path.strip_prefix(source).unwrap_or(path);
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&destination)?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(path, &destination).with_context(|| {
            format!(
                "copy {} to {} failed",
                path.display(),
                destination.display()
            )
        })?;
    }
    Ok(())
}

fn extract_zip(data: &[u8], output_root: &Path) -> Result<()> {
    // Security note: every entry path is validated before writing to disk.
    let cursor = Cursor::new(data.to_vec());
    let mut archive = ZipArchive::new(cursor).context("invalid zip archive")?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).context("invalid zip entry")?;
        if file.is_dir() {
            continue;
        }
        let relative = validate_archive_entry_path(file.name())?;
        let destination = output_root.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        std::fs::write(destination, bytes)?;
    }
    Ok(())
}

fn resolve_package_root(extract_root: &Path) -> Result<PathBuf> {
    let direct = extract_root.join("hive.yaml");
    if direct.is_file() {
        return Ok(extract_root.to_path_buf());
    }
    let candidates = std::fs::read_dir(extract_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join("hive.yaml").is_file())
        .collect::<Vec<_>>();
    if candidates.len() == 1 {
        return Ok(candidates[0].clone());
    }
    Err(anyhow!("hive.yaml not found in package root"))
}

fn validate_archive_entry_path(raw: &str) -> Result<PathBuf> {
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') || normalized.starts_with('\\') {
        return Err(anyhow!("zip entry path is absolute: {normalized}"));
    }
    let path = Path::new(&normalized);
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(anyhow!("zip entry path is unsafe: {normalized}"));
        }
    }
    Ok(path.to_path_buf())
}

fn validate_relative_path(raw: &str) -> Result<PathBuf> {
    let cleaned = raw.trim().replace('\\', "/");
    if cleaned.is_empty() {
        return Err(anyhow!("relative path is empty"));
    }
    if cleaned.starts_with('/') {
        return Err(anyhow!("path must be relative: {cleaned}"));
    }
    let path = Path::new(&cleaned);
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            return Err(anyhow!("path is unsafe: {cleaned}"));
        }
    }
    Ok(path.to_path_buf())
}

fn validate_hive_manifest(manifest: &HiveManifest) -> Result<()> {
    if manifest
        .protocol
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && !value.starts_with("hpp/"))
    {
        return Err(anyhow!("unsupported hive manifest protocol"));
    }
    if manifest
        .kind
        .as_deref()
        .is_some_and(|kind| kind.trim() != "hive_pack")
    {
        return Err(anyhow!("invalid hive manifest kind"));
    }
    Ok(())
}

fn unique_skill_name(skill_root: &Path, preferred: &str) -> String {
    let normalized = normalize_name(preferred, "skill");
    let candidate = if normalized.is_empty() {
        "skill".to_string()
    } else {
        normalized
    };
    let mut final_name = candidate.clone();
    let mut index = 2usize;
    while skill_root.join(&final_name).exists() {
        final_name = format!("{candidate}-{index}");
        index += 1;
    }
    final_name
}

fn unique_skill_name_for_replace(
    skill_root: &Path,
    preferred: &str,
    reserved: &HashSet<String>,
) -> String {
    let base = normalize_name(preferred, "skill");
    if !reserved.contains(&base) {
        return base;
    }
    let mut index = 2usize;
    loop {
        let next = format!("{base}-{index}");
        if !reserved.contains(&next) && !skill_root.join(&next).exists() {
            return next;
        }
        index += 1;
    }
}

fn resolve_import_skill_name(
    skill_root: &Path,
    preferred: &str,
    conflict_mode: ImportConflictMode,
    reserved: &HashSet<String>,
) -> String {
    if conflict_mode.allows_direct_replace() {
        unique_skill_name_for_replace(skill_root, preferred, reserved)
    } else {
        unique_skill_name(skill_root, preferred)
    }
}

fn unique_slug_with_reserved(
    preferred: &str,
    reserved: &HashSet<String>,
    fallback: &str,
) -> String {
    let candidate = normalize_name(preferred, fallback);
    if !reserved.contains(&candidate) {
        return candidate;
    }
    let mut index = 2usize;
    loop {
        let next = format!("{candidate}-{index}");
        if !reserved.contains(&next) {
            return next;
        }
        index += 1;
    }
}

fn normalize_conflict_key(raw: &str) -> String {
    raw.trim().to_lowercase()
}

fn unique_label_with_reserved(
    preferred: &str,
    reserved: &HashSet<String>,
    fallback: &str,
) -> String {
    let base = preferred.trim();
    let candidate = if base.is_empty() {
        fallback.trim()
    } else {
        base
    };
    let base_key = normalize_conflict_key(candidate);
    if base_key.is_empty() || !reserved.contains(&base_key) {
        return candidate.to_string();
    }
    let mut index = 2usize;
    loop {
        let next = format!("{candidate}-{index}");
        if !reserved.contains(&normalize_conflict_key(&next)) {
            return next;
        }
        index += 1;
    }
}

fn normalize_name(raw: &str, fallback: &str) -> String {
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
    let output = output.trim_matches('-').to_string();
    if output.is_empty() {
        fallback.to_string()
    } else {
        output
    }
}

fn normalize_export_filename_stem(hive_name: &str, hive_id: &str) -> String {
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
        normalize_name(hive_id, "hivepack")
    } else {
        cleaned
    }
}

fn normalize_approval_mode(raw: Option<&str>) -> String {
    let cleaned = raw.unwrap_or("suggest").trim().to_ascii_lowercase();
    if matches!(cleaned.as_str(), "suggest" | "auto_edit" | "full_auto") {
        cleaned
    } else {
        "suggest".to_string()
    }
}

fn new_job(job_type: &str, user_id: &str) -> HivePackJobRecord {
    let now = now_ts();
    let prefix = if job_type == "export" { "exp" } else { "imp" };
    HivePackJobRecord {
        job_id: format!("hpack_{prefix}_{}", Uuid::new_v4().simple()),
        job_type: job_type.to_string(),
        user_id: user_id.to_string(),
        status: "running".to_string(),
        phase: "uploaded".to_string(),
        progress: 0,
        summary: "job uploaded".to_string(),
        detail: None,
        report: None,
        artifact: None,
        created_at: now,
        updated_at: now,
    }
}

fn update_job(job: &mut HivePackJobRecord, phase: &str, progress: i64, summary: &str) {
    job.phase = phase.to_string();
    job.progress = progress.clamp(0, 100);
    job.summary = summary.to_string();
    job.updated_at = now_ts();
}

fn persist_job(state: &AppState, job: &HivePackJobRecord) -> Result<()> {
    let key = meta_key(&job.job_id);
    let payload = serde_json::to_string(job)?;
    state.user_store.set_meta(&key, &payload)?;
    Ok(())
}

fn meta_key(job_id: &str) -> String {
    format!("{HIVE_PACK_META_PREFIX}{}", job_id.trim())
}

fn hivepack_temp_root() -> PathBuf {
    if let Ok(raw) = std::env::var(HIVE_PACK_TEMP_ENV) {
        let cleaned = raw.trim();
        if !cleaned.is_empty() {
            return PathBuf::from(cleaned).join(HIVE_PACK_TEMP_DIR);
        }
    }
    std::env::temp_dir().join(HIVE_PACK_TEMP_DIR)
}

fn now_ts() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::{
        collect_agent_skills_for_export, export_worker_id, normalize_approval_mode,
        normalize_conflict_key, normalize_export_filename_stem, normalize_import_conflict_mode,
        normalize_name, resolve_import_skill_name, resolve_import_workers,
        resolve_worker_skill_sources, load_worker_card_manifest, unique_label_with_reserved,
        unique_slug_with_reserved, validate_archive_entry_path, validate_hive_manifest,
        validate_relative_path, HiveManifest, HivePackMeta, ImportConflictMode,
    };
    use crate::services::user_tools::{UserToolAlias, UserToolBindings, UserToolKind};
    use crate::skills::SkillSpec;
    use crate::storage::{UserAgentRecord, DEFAULT_SANDBOX_CONTAINER_ID};
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use tempfile::tempdir;

    #[test]
    fn normalize_name_keeps_safe_ascii_chars() {
        assert_eq!(normalize_name("HR Recruiter", "fallback"), "hr-recruiter");
        assert_eq!(normalize_name("A_B-C", "fallback"), "a_b-c");
    }

    #[test]
    fn normalize_approval_mode_falls_back_to_suggest() {
        assert_eq!(normalize_approval_mode(Some("full_auto")), "full_auto");
        assert_eq!(normalize_approval_mode(Some("unknown")), "suggest");
    }

    #[test]
    fn validate_archive_entry_rejects_parent_dir() {
        assert!(validate_archive_entry_path("../evil.txt").is_err());
        assert!(validate_archive_entry_path("a/../../evil.txt").is_err());
    }

    #[test]
    fn validate_relative_path_rejects_absolute_path() {
        assert!(validate_relative_path("/tmp/file").is_err());
        assert!(validate_relative_path("../tmp/file").is_err());
    }

    #[test]
    fn unique_slug_with_reserved_appends_numeric_suffix() {
        let reserved = ["hr-hive".to_string(), "hr-hive-2".to_string()]
            .into_iter()
            .collect::<HashSet<_>>();
        assert_eq!(
            unique_slug_with_reserved("hr hive", &reserved, "hive"),
            "hr-hive-3"
        );
    }

    #[test]
    fn unique_label_with_reserved_appends_numeric_suffix() {
        let reserved = ["鎷涜仒涓撳憳".to_string(), "鎷涜仒涓撳憳-2".to_string()]
            .into_iter()
            .map(|item| normalize_conflict_key(&item))
            .collect::<HashSet<_>>();
        assert_eq!(
            unique_label_with_reserved("鎷涜仒涓撳憳", &reserved, "Imported Worker"),
            "鎷涜仒涓撳憳-3"
        );
    }

    #[test]
    fn normalize_import_conflict_mode_supports_update_replace_alias() {
        assert_eq!(
            normalize_import_conflict_mode(Some("update_replace")),
            ImportConflictMode::UpdateReplace
        );
        assert_eq!(
            normalize_import_conflict_mode(Some("replace")),
            ImportConflictMode::UpdateReplace
        );
        assert_eq!(
            normalize_import_conflict_mode(Some("auto_rename_only")),
            ImportConflictMode::AutoRenameOnly
        );
    }

    #[test]
    fn resolve_import_skill_name_update_replace_prefers_base_name() {
        let root = tempdir().expect("tempdir");
        let skill_root = root.path();
        std::fs::create_dir_all(skill_root.join("planner-skill")).expect("seed base skill");
        let reserved = HashSet::new();
        assert_eq!(
            resolve_import_skill_name(
                skill_root,
                "planner skill",
                ImportConflictMode::UpdateReplace,
                &reserved
            ),
            "planner-skill"
        );
    }

    #[test]
    fn resolve_import_skill_name_update_replace_avoids_reserved_and_existing_suffix() {
        let root = tempdir().expect("tempdir");
        let skill_root = root.path();
        std::fs::create_dir_all(skill_root.join("planner-skill-2")).expect("seed suffix skill");
        let reserved = ["planner-skill".to_string()]
            .into_iter()
            .collect::<HashSet<_>>();
        assert_eq!(
            resolve_import_skill_name(
                skill_root,
                "planner skill",
                ImportConflictMode::UpdateReplace,
                &reserved
            ),
            "planner-skill-3"
        );
    }

    #[test]
    fn validate_hive_manifest_allows_empty_workers_for_auto_discovery() {
        let manifest = HiveManifest {
            protocol: Some("hpp/1.0".to_string()),
            kind: Some("hive_pack".to_string()),
            pack: HivePackMeta {
                id: Some("demo_hive".to_string()),
                name: Some("Demo".to_string()),
                version: Some("1.0.0".to_string()),
                description: None,
            },
            workers: Vec::new(),
        };
        assert!(validate_hive_manifest(&manifest).is_ok());
    }

    #[test]
    fn resolve_import_workers_discovers_workers_when_manifest_empty() {
        let root = tempdir().expect("tempdir");
        let workers_root = root.path().join("workers");
        std::fs::create_dir_all(workers_root.join("planner")).expect("planner dir");
        std::fs::create_dir_all(workers_root.join("executor")).expect("executor dir");
        let manifest = HiveManifest {
            protocol: Some("hpp/1.0".to_string()),
            kind: Some("hive_pack".to_string()),
            pack: HivePackMeta {
                id: Some("demo_hive".to_string()),
                name: Some("Demo".to_string()),
                version: Some("1.0.0".to_string()),
                description: None,
            },
            workers: Vec::new(),
        };
        let workers = resolve_import_workers(&manifest, root.path()).expect("resolve workers");
        let ids = workers
            .iter()
            .map(|item| item.worker_id.clone())
            .collect::<HashSet<_>>();
        let paths = workers
            .iter()
            .map(|item| item.path.to_string_lossy().to_string())
            .collect::<HashSet<_>>();
        assert!(ids.contains("planner"));
        assert!(ids.contains("executor"));
        assert!(paths.contains("workers/planner"));
        assert!(paths.contains("workers/executor"));
    }

    #[test]
    fn resolve_worker_skill_sources_supports_worker_card_declared_skills() {
        let root = tempdir().expect("tempdir");
        let worker_root = root.path().join("workers").join("planner");
        std::fs::create_dir_all(&worker_root).expect("worker dir");
        std::fs::write(
            worker_root.join("worker-card.json"),
            r#"{
  "schema_version": "wunder/worker-card@1",
  "kind": "WorkerCard",
  "metadata": { "name": "Planner" },
  "prompt": { "system_prompt": "plan first" },
  "abilities": { "skills": ["requirement_analyzer"] }
}"#,
        )
        .expect("worker card");
        let skill_root = root.path().join("skills").join("requirement_analyzer");
        std::fs::create_dir_all(&skill_root).expect("skill dir");
        std::fs::write(skill_root.join("SKILL.md"), "# demo").expect("skill file");
        let worker_card = load_worker_card_manifest(worker_root.as_path())
            .expect("load worker card")
            .expect("worker card exists");
        let skill_refs =
            resolve_worker_skill_sources(root.path(), worker_root.as_path(), &worker_card)
                .expect("resolve worker skills");
        assert_eq!(skill_refs.len(), 1);
        assert_eq!(skill_refs[0].preferred_name, "requirement_analyzer");
        assert_eq!(skill_refs[0].source_dir, skill_root);
    }

    #[test]
    fn resolve_worker_skill_sources_skips_missing_worker_card_skills() {
        let root = tempdir().expect("tempdir");
        let worker_root = root.path().join("workers").join("planner");
        std::fs::create_dir_all(&worker_root).expect("worker dir");
        std::fs::write(
            worker_root.join("worker-card.json"),
            r#"{
  "schema_version": "wunder/worker-card@1",
  "kind": "WorkerCard",
  "metadata": { "name": "Planner" },
  "prompt": { "system_prompt": "plan first" },
  "abilities": { "skills": ["requirement_analyzer", "missing_skill"] }
}"#,
        )
        .expect("worker card");
        let skill_root = root.path().join("skills").join("requirement_analyzer");
        std::fs::create_dir_all(&skill_root).expect("skill dir");
        std::fs::write(skill_root.join("SKILL.md"), "# demo").expect("skill file");

        let worker_card = load_worker_card_manifest(worker_root.as_path())
            .expect("load worker card")
            .expect("worker card exists");
        let skill_refs =
            resolve_worker_skill_sources(root.path(), worker_root.as_path(), &worker_card)
                .expect("resolve worker skills");
        assert_eq!(skill_refs.len(), 1);
        assert_eq!(skill_refs[0].preferred_name, "requirement_analyzer");
    }

    #[test]
    fn normalize_export_filename_stem_keeps_chinese_hive_name() {
        assert_eq!(
            normalize_export_filename_stem("浜哄姏璧勬簮铚傜兢", "hive_123"),
            "浜哄姏璧勬簮铚傜兢"
        );
        assert_eq!(normalize_export_filename_stem("  ", "hive_123"), "hive_123");
    }

    #[test]
    fn collect_agent_skills_for_export_supports_plain_skill_name_entries() {
        let root = tempdir().expect("tempdir");
        let skill_root = root.path().join("skills");
        let skill_dir = skill_root.join("recruit-specialist");
        std::fs::create_dir_all(&skill_dir).expect("skill dir");
        std::fs::write(skill_dir.join("SKILL.md"), "# demo").expect("skill file");

        let owner_id = "u_1".to_string();
        let alias_name = format!("{}@{}", owner_id, "鎷涜仒鍔╂墜");
        let mut alias_map = HashMap::new();
        alias_map.insert(
            alias_name.clone(),
            UserToolAlias {
                kind: UserToolKind::Skill,
                owner_id: owner_id.clone(),
                target: "鎷涜仒鍔╂墜".to_string(),
            },
        );

        let bindings = UserToolBindings {
            alias_specs: HashMap::new(),
            alias_map,
            skill_specs: vec![SkillSpec {
                name: alias_name,
                description: String::new(),
                path: skill_dir.join("SKILL.md").to_string_lossy().to_string(),
                input_schema: json!({}),
                frontmatter: String::new(),
                root: skill_dir.clone(),
                entrypoint: None,
            }],
            skill_sources: HashMap::new(),
            mcp_servers: HashMap::new(),
            shared_tools_enabled: HashSet::new(),
            user_version: 0.0,
            shared_version: 0.0,
        };

        let agent = UserAgentRecord {
            agent_id: "worker_100".to_string(),
            user_id: owner_id.clone(),
            hive_id: "hive_1".to_string(),
            name: "Recruit Specialist".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            tool_names: vec!["鎷涜仒鍔╂墜".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "private".to_string(),
            approval_mode: "suggest".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: 0.0,
            updated_at: 0.0,
        };

        let skill_refs = collect_agent_skills_for_export(&agent, &bindings, &skill_root, &[]);
        assert_eq!(skill_refs.len(), 1);
        assert_eq!(skill_refs[0].name, "鎷涜仒鍔╂墜");
        assert_eq!(skill_refs[0].source_dir, skill_dir);
    }

    #[test]
    fn collect_agent_skills_for_export_supports_global_skill_specs() {
        let root = tempdir().expect("tempdir");
        let user_skill_root = root.path().join("user_skills");
        let global_skill_dir = root.path().join("global_skills").join("report_writer");
        std::fs::create_dir_all(&global_skill_dir).expect("global skill dir");
        std::fs::write(global_skill_dir.join("SKILL.md"), "# demo").expect("global skill file");

        let agent = UserAgentRecord {
            agent_id: "worker_200".to_string(),
            user_id: "u_2".to_string(),
            hive_id: "hive_1".to_string(),
            name: "Report Worker".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            tool_names: vec!["report_writer".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "private".to_string(),
            approval_mode: "suggest".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: 0.0,
            updated_at: 0.0,
        };
        let global_specs = vec![SkillSpec {
            name: "report_writer".to_string(),
            description: String::new(),
            path: global_skill_dir
                .join("SKILL.md")
                .to_string_lossy()
                .to_string(),
            input_schema: json!({}),
            frontmatter: String::new(),
            root: global_skill_dir.clone(),
            entrypoint: None,
        }];

        let skill_refs = collect_agent_skills_for_export(
            &agent,
            &UserToolBindings::default(),
            &user_skill_root,
            &global_specs,
        );
        assert_eq!(skill_refs.len(), 1);
        assert_eq!(skill_refs[0].name, "report_writer");
        assert_eq!(skill_refs[0].source_dir, global_skill_dir);
    }

    #[test]
    fn export_worker_id_prefers_agent_name_over_numeric_id() {
        let agent = UserAgentRecord {
            agent_id: "worker_1234567890".to_string(),
            user_id: "u_1".to_string(),
            hive_id: "hive_1".to_string(),
            name: "Recruit Specialist".to_string(),
            description: String::new(),
            system_prompt: String::new(),
            tool_names: Vec::new(),
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: Vec::new(),
            access_level: "private".to_string(),
            approval_mode: "suggest".to_string(),
            is_shared: false,
            status: "active".to_string(),
            icon: None,
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
            created_at: 0.0,
            updated_at: 0.0,
        };
        assert_eq!(export_worker_id(&agent, 0), "Recruit-Specialist");
    }
}

