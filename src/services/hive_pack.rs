use crate::services::user_access::{build_user_tool_context, compute_allowed_tool_names};
use crate::services::user_tools::UserToolKind;
use crate::state::AppState;
use crate::storage::{
    normalize_hive_id, HiveRecord, UserAccountRecord, UserAgentRecord, DEFAULT_HIVE_ID,
    DEFAULT_SANDBOX_CONTAINER_ID,
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
    path: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkerManifest {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    worker: WorkerMeta,
    #[serde(default)]
    agent_profile: Option<WorkerAgentProfile>,
    #[serde(default)]
    skills: Vec<WorkerSkillRef>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkerMeta {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    duty: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WorkerAgentProfile {
    #[serde(default)]
    approval_mode: Option<String>,
    #[serde(default)]
    icon: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkerSkillRef {
    #[serde(default)]
    skill_id: Option<String>,
    path: String,
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
}

#[derive(Debug, Clone, Serialize)]
struct WorkerExportManifest {
    protocol: String,
    kind: String,
    worker: WorkerExportMeta,
    agent_profile: WorkerExportProfile,
    skills: Vec<WorkerExportSkillRef>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerExportMeta {
    id: String,
    display_name: String,
    description: String,
    duty: String,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerExportProfile {
    system_prompt_file: String,
    model_hint: String,
    approval_mode: String,
    icon: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct WorkerExportSkillRef {
    skill_id: String,
    path: String,
    required: bool,
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
    installed_skills: Vec<String>,
    skill_installs: Vec<SkillInstallSnapshot>,
}

#[derive(Debug, Clone)]
struct ImportWorkerRef {
    worker_id: String,
    path: PathBuf,
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
        let mut tool_names = base_tool_names.clone();
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
            access_level: "A".to_string(),
            approval_mode: snapshot.approval_mode.clone(),
            is_shared: false,
            status: "active".to_string(),
            icon: snapshot.icon.clone(),
            sandbox_container_id: DEFAULT_SANDBOX_CONTAINER_ID,
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

    update_job(job, "installing", 35, "building hivepack structure");
    persist_job(state, job)?;

    let config = state.config_store.get().await;
    let skills = state.skills.read().await.clone();
    let bindings = state
        .user_tool_manager
        .build_bindings(&config, &skills, &user.user_id);
    let skill_root = state.user_tool_store.get_skill_root(&user.user_id);
    let export_mode = normalize_export_mode(options.mode.as_deref());

    let mut workers = Vec::new();
    let mut worker_reports = Vec::new();
    let mut total_skills = 0usize;
    for (index, agent) in agents.iter().enumerate() {
        let worker_id = export_worker_id(agent, index);
        let worker_dir = package_root.join("workers").join(&worker_id);
        std::fs::create_dir_all(worker_dir.join("skills"))?;
        std::fs::write(
            worker_dir.join("WORKER_ROLE.md"),
            agent.system_prompt.as_bytes(),
        )?;

        let worker_skill_names =
            collect_agent_skill_names(agent, &bindings.alias_map, &user.user_id);
        let mut skills_refs = Vec::new();
        for skill_name in worker_skill_names {
            let source = skill_root.join(&skill_name);
            if !source.exists() || !source.is_dir() || !source.join("SKILL.md").is_file() {
                continue;
            }
            let relative = format!("skills/{skill_name}");
            if export_mode == "full" {
                copy_dir_recursive(&source, &worker_dir.join(&relative))?;
            } else {
                std::fs::create_dir_all(worker_dir.join(&relative))?;
                std::fs::write(
                    worker_dir.join(&relative).join("SKILL.md"),
                    b"# Placeholder\n\nreference_only mode does not include full skill files.\n",
                )?;
            }
            write_skill_meta(&worker_dir.join(&relative), &skill_name)?;
            total_skills += 1;
            skills_refs.push(WorkerExportSkillRef {
                skill_id: skill_name.clone(),
                path: relative,
                required: true,
            });
        }

        let worker_manifest = WorkerExportManifest {
            protocol: "hpp/1.0".to_string(),
            kind: "worker_pack".to_string(),
            worker: WorkerExportMeta {
                id: worker_id.clone(),
                display_name: agent.name.clone(),
                description: agent.description.clone(),
                duty: "specialist".to_string(),
            },
            agent_profile: WorkerExportProfile {
                system_prompt_file: "WORKER_ROLE.md".to_string(),
                model_hint: "inherit".to_string(),
                approval_mode: normalize_approval_mode(Some(&agent.approval_mode)),
                icon: agent.icon.clone(),
            },
            skills: skills_refs.clone(),
        };
        std::fs::write(
            worker_dir.join("worker.yaml"),
            serde_yaml::to_string(&worker_manifest)?,
        )?;

        workers.push(HiveExportWorker {
            worker_id: worker_id.clone(),
            path: format!("workers/{worker_id}"),
            role: "specialist".to_string(),
        });
        worker_reports.push(json!({
            "worker_id": worker_id,
            "agent_id": agent.agent_id,
            "agent_name": agent.name,
            "skills": skills_refs.iter().map(|item| item.skill_id.clone()).collect::<Vec<_>>(),
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
        "{}-{}.hivepack",
        normalize_file_stem(&hive.name),
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
        "skill_total": total_skills,
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
    let worker_manifest = load_worker_manifest(&worker_root, worker_ref)?;

    let role_prompt_path = worker_root.join("WORKER_ROLE.md");
    let role_prompt = std::fs::read_to_string(&role_prompt_path)
        .with_context(|| format!("read {} failed", role_prompt_path.display()))?;
    let display_name = worker_manifest
        .worker
        .display_name
        .clone()
        .unwrap_or_else(|| {
            worker_manifest
                .worker
                .id
                .clone()
                .unwrap_or_else(|| worker_ref.worker_id.clone())
        });
    let description = worker_manifest
        .worker
        .description
        .clone()
        .unwrap_or_default();
    let duty = worker_manifest.worker.duty.clone().unwrap_or_default();
    let approval_mode = normalize_approval_mode(
        worker_manifest
            .agent_profile
            .as_ref()
            .and_then(|profile| profile.approval_mode.as_deref()),
    );
    let icon = worker_manifest
        .agent_profile
        .as_ref()
        .and_then(|profile| profile.icon.clone());

    // Keep manual packaging ergonomic: when worker.yaml omits skills (or is absent),
    // discover skills from skills/*/SKILL.md automatically.
    let skill_refs = resolve_worker_skill_refs(&worker_root, &worker_manifest)?;
    let mut installed_skills = Vec::new();
    let mut skill_installs = Vec::new();
    for skill_ref in &skill_refs {
        let skill_path = validate_relative_path(&skill_ref.path)?;
        let skill_source = worker_root.join(&skill_path);
        if !skill_source.exists() || !skill_source.is_dir() {
            return Err(anyhow!(
                "skill path missing: {}",
                skill_source.to_string_lossy()
            ));
        }
        if !skill_source.join("SKILL.md").is_file() {
            return Err(anyhow!(
                "skill missing SKILL.md: {}",
                skill_source.to_string_lossy()
            ));
        }
        let preferred = normalize_name(
            skill_ref
                .skill_id
                .as_deref()
                .or_else(|| skill_source.file_name().and_then(|name| name.to_str()))
                .unwrap_or("skill"),
            "skill",
        );
        let source_skill_id = skill_ref
            .skill_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| preferred.clone());
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
        copy_dir_recursive(&skill_source, &skill_target)?;
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

fn load_worker_manifest(
    worker_root: &Path,
    worker_ref: &ImportWorkerRef,
) -> Result<WorkerManifest> {
    let worker_manifest_path = worker_root.join("worker.yaml");
    if worker_manifest_path.is_file() {
        let worker_manifest_text = std::fs::read_to_string(&worker_manifest_path)
            .with_context(|| format!("read {} failed", worker_manifest_path.display()))?;
        let worker_manifest: WorkerManifest = serde_yaml::from_str(&worker_manifest_text)
            .with_context(|| format!("parse {} failed", worker_manifest_path.display()))?;
        validate_worker_manifest(&worker_manifest)?;
        return Ok(worker_manifest);
    }

    Ok(WorkerManifest {
        protocol: None,
        kind: None,
        worker: WorkerMeta {
            id: Some(worker_ref.worker_id.clone()),
            display_name: Some(worker_ref.worker_id.clone()),
            description: None,
            duty: None,
        },
        agent_profile: None,
        skills: Vec::new(),
    })
}

fn resolve_worker_skill_refs(
    worker_root: &Path,
    worker_manifest: &WorkerManifest,
) -> Result<Vec<WorkerSkillRef>> {
    if !worker_manifest.skills.is_empty() {
        return Ok(worker_manifest.skills.clone());
    }

    let skills_root = worker_root.join("skills");
    if !skills_root.is_dir() {
        return Err(anyhow!(
            "worker skills missing: provide worker.yaml skills[] or create skills/<id>/SKILL.md"
        ));
    }

    let mut refs = std::fs::read_dir(&skills_root)?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let skill_dir = entry.path();
            if !skill_dir.join("SKILL.md").is_file() {
                return None;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            Some(WorkerSkillRef {
                skill_id: Some(dir_name.clone()),
                path: format!("skills/{dir_name}"),
            })
        })
        .collect::<Vec<_>>();
    refs.sort_by_key(|item| item.path.clone());
    if refs.is_empty() {
        return Err(anyhow!(
            "worker skills missing: no SKILL.md found under {}",
            skills_root.display()
        ));
    }
    Ok(refs)
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

fn collect_agent_skill_names(
    agent: &UserAgentRecord,
    alias_map: &HashMap<String, crate::services::user_tools::UserToolAlias>,
    owner_user_id: &str,
) -> Vec<String> {
    let mut names = BTreeSet::new();
    for tool_name in &agent.tool_names {
        let Some(alias) = alias_map.get(tool_name) else {
            continue;
        };
        if alias.owner_id != owner_user_id {
            continue;
        }
        if !matches!(alias.kind, UserToolKind::Skill) {
            continue;
        }
        if alias.target.trim().is_empty() {
            continue;
        }
        names.insert(alias.target.clone());
    }
    names.into_iter().collect()
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
    let preferred = if !agent.agent_id.trim().is_empty() {
        normalize_name(&agent.agent_id, "worker")
    } else {
        normalize_name(&agent.name, "worker")
    };
    if preferred.is_empty() {
        format!("worker-{}", index + 1)
    } else {
        preferred
    }
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

fn validate_worker_manifest(manifest: &WorkerManifest) -> Result<()> {
    if manifest
        .protocol
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty() && !value.starts_with("hpp/"))
    {
        return Err(anyhow!("unsupported worker manifest protocol"));
    }
    if manifest
        .kind
        .as_deref()
        .is_some_and(|kind| kind.trim() != "worker_pack")
    {
        return Err(anyhow!("invalid worker manifest kind"));
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

fn normalize_file_stem(raw: &str) -> String {
    normalize_name(raw, "hivepack")
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
        normalize_approval_mode, normalize_conflict_key, normalize_import_conflict_mode,
        normalize_name, resolve_import_skill_name, resolve_import_workers,
        resolve_worker_skill_refs, unique_label_with_reserved, unique_slug_with_reserved,
        validate_archive_entry_path, validate_hive_manifest, validate_relative_path, HiveManifest,
        HivePackMeta, ImportConflictMode, WorkerManifest,
    };
    use std::collections::HashSet;
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
        let reserved = ["招聘专员".to_string(), "招聘专员-2".to_string()]
            .into_iter()
            .map(|item| normalize_conflict_key(&item))
            .collect::<HashSet<_>>();
        assert_eq!(
            unique_label_with_reserved("招聘专员", &reserved, "Imported Worker"),
            "招聘专员-3"
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
    fn resolve_worker_skill_refs_auto_discovers_skill_dirs() {
        let root = tempdir().expect("tempdir");
        let worker_root = root.path().join("workers").join("planner");
        let skill_root = worker_root.join("skills").join("requirement_analyzer");
        std::fs::create_dir_all(&skill_root).expect("skill dir");
        std::fs::write(skill_root.join("SKILL.md"), "# demo").expect("skill file");

        let worker_manifest = WorkerManifest::default();
        let skill_refs = resolve_worker_skill_refs(worker_root.as_path(), &worker_manifest)
            .expect("resolve worker skills");
        assert_eq!(skill_refs.len(), 1);
        assert_eq!(skill_refs[0].path, "skills/requirement_analyzer");
    }
}
