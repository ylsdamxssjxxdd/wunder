use crate::config::{Config, UserAgentPresetConfig};
use crate::core::atomic_write::atomic_write_text;
use crate::services::default_agent_sync::{DEFAULT_AGENT_ID_ALIAS, DEFAULT_AGENT_NAME};
use crate::services::inner_visible::{
    build_worker_card, parse_worker_card, WorkerCardDocument, WorkerCardPreset,
    WorkerCardRecordUpdate,
};
use crate::services::user_agent_presets::resolve_preset_id;
use crate::services::worker_card_files::{worker_card_file_name, WORKER_CARD_FILE_SUFFIX};
use crate::services::worker_card_settings::{
    canonicalize_preset_config, normalize_agent_approval_mode, normalize_agent_status,
    normalize_optional_model_name, normalize_preset_questions, normalize_tool_list,
    preset_config_from_update, preset_update_from_config,
};
use crate::storage::{normalize_hive_id, normalize_sandbox_container_id, UserAgentRecord};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

const PRESET_WORKER_CARD_USER_ID: &str = "__preset_worker_cards__";

#[derive(Debug, Clone)]
pub struct PresetWorkerCardAsset {
    pub preset: UserAgentPresetConfig,
    pub path: PathBuf,
}

pub fn configured_worker_cards_root(config: &Config) -> Option<PathBuf> {
    let cleaned = config.user_agents.worker_cards_root.trim();
    (!cleaned.is_empty()).then(|| PathBuf::from(cleaned))
}

pub fn load_effective_preset_configs(
    config: &Config,
    skill_name_keys: &HashSet<String>,
) -> Result<Vec<UserAgentPresetConfig>> {
    if let Some(root) = configured_worker_cards_root(config) {
        let assets = load_preset_worker_card_assets(&root, skill_name_keys)?;
        if !assets.is_empty() {
            return Ok(assets.into_iter().map(|asset| asset.preset).collect());
        }
    }

    let mut seen_ids = HashSet::new();
    let mut presets = Vec::new();
    for item in &config.user_agents.presets {
        let preset_id = resolve_preset_id(&item.preset_id, &item.name);
        let Some(normalized) = canonicalize_preset_config(item, &preset_id, skill_name_keys) else {
            continue;
        };
        if seen_ids.insert(preset_id) {
            presets.push(normalized);
        }
    }
    Ok(presets)
}

pub fn persist_preset_configs(
    config: &Config,
    items: &[UserAgentPresetConfig],
    skill_name_keys: &HashSet<String>,
) -> Result<bool> {
    let Some(root) = configured_worker_cards_root(config) else {
        return Ok(false);
    };

    fs::create_dir_all(&root)
        .with_context(|| format!("create preset worker-card dir failed: {}", root.display()))?;

    let existing_assets = load_preset_worker_card_assets(&root, skill_name_keys)?;
    let existing_paths = existing_assets
        .iter()
        .map(|asset| (asset.preset.preset_id.clone(), asset.path.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut retained_paths = HashSet::new();

    for item in items {
        let preset_id = resolve_preset_id(&item.preset_id, &item.name);
        let Some(normalized) = canonicalize_preset_config(item, &preset_id, skill_name_keys) else {
            continue;
        };
        let Some(document) = worker_card_document_from_preset_config(&normalized, skill_name_keys)
        else {
            continue;
        };
        let target_path = root.join(export_file_name_for_preset(&normalized));
        atomic_write_text(
            &target_path,
            &serde_json::to_string_pretty(&document)
                .context("serialize preset worker card failed")?,
        )?;
        retained_paths.insert(target_path.clone());

        if let Some(previous_path) = existing_paths.get(&normalized.preset_id) {
            if previous_path != &target_path {
                let _ = fs::remove_file(previous_path);
            }
        }
    }

    for asset in existing_assets {
        if !retained_paths.contains(&asset.path) {
            let _ = fs::remove_file(&asset.path);
        }
    }

    Ok(true)
}

pub fn load_preset_worker_card_assets(
    root: &Path,
    skill_name_keys: &HashSet<String>,
) -> Result<Vec<PresetWorkerCardAsset>> {
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut files = fs::read_dir(root)
        .with_context(|| format!("read preset worker-card dir failed: {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry.file_type().ok().and_then(|file_type| {
                if file_type.is_file() {
                    Some(entry.path())
                } else {
                    None
                }
            })
        })
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.trim().ends_with(WORKER_CARD_FILE_SUFFIX))
        })
        .collect::<Vec<_>>();
    files.sort();

    let mut assets_by_id = BTreeMap::new();
    for path in files {
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                warn!(
                    "failed to read preset worker card {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        let document = match serde_json::from_str::<WorkerCardDocument>(&content) {
            Ok(document) => document,
            Err(err) => {
                warn!(
                    "failed to parse preset worker card {}: {err}",
                    path.display()
                );
                continue;
            }
        };
        let Some(preset) = preset_config_from_worker_card_document(&document, skill_name_keys)
        else {
            warn!(
                "skip preset worker card without valid preset identity: {}",
                path.display()
            );
            continue;
        };
        assets_by_id.insert(
            preset.preset_id.clone(),
            PresetWorkerCardAsset { preset, path },
        );
    }

    Ok(assets_by_id.into_values().collect())
}

pub fn worker_card_document_from_preset_config(
    config: &UserAgentPresetConfig,
    skill_name_keys: &HashSet<String>,
) -> Option<WorkerCardDocument> {
    let preset_id = resolve_preset_id(&config.preset_id, &config.name);
    let normalized = canonicalize_preset_config(config, &preset_id, skill_name_keys)?;
    let update = preset_update_from_config(&normalized, skill_name_keys)?;
    let record = record_from_preset_update(&normalized.preset_id, &update);
    let mut document = build_worker_card(&record, None, None, skill_name_keys);
    document.metadata.agent_id = normalized.preset_id.clone();
    document.preset = Some(WorkerCardPreset {
        revision: normalized.revision.max(1),
        status: normalize_agent_status(Some(&normalized.status)),
    });
    Some(document)
}

fn preset_config_from_worker_card_document(
    document: &WorkerCardDocument,
    skill_name_keys: &HashSet<String>,
) -> Option<UserAgentPresetConfig> {
    let parsed = parse_worker_card(document.clone(), None);
    let preset_id = normalize_preset_id(Some(&document.metadata.agent_id))
        .filter(|value| !value.trim().is_empty())?;
    let (revision, status) = preset_document_meta(document);
    let update = canonicalize_worker_card_update(parsed, skill_name_keys);
    let config = preset_config_from_update(&preset_id, revision, &status, &update);
    canonicalize_preset_config(&config, &preset_id, skill_name_keys)
}

fn canonicalize_worker_card_update(
    update: WorkerCardRecordUpdate,
    skill_name_keys: &HashSet<String>,
) -> WorkerCardRecordUpdate {
    crate::services::worker_card_settings::canonicalize_worker_card_update(update, skill_name_keys)
}

fn record_from_preset_update(preset_id: &str, update: &WorkerCardRecordUpdate) -> UserAgentRecord {
    UserAgentRecord {
        agent_id: preset_id.trim().to_string(),
        user_id: PRESET_WORKER_CARD_USER_ID.to_string(),
        hive_id: normalize_hive_id(&update.hive_id),
        name: update.name.trim().to_string(),
        description: update.description.trim().to_string(),
        system_prompt: update.system_prompt.trim().to_string(),
        model_name: normalize_optional_model_name(update.model_name.as_deref()),
        ability_items: update.ability_items.clone(),
        tool_names: normalize_tool_list(update.tool_names.clone()),
        declared_tool_names: normalize_tool_list(update.declared_tool_names.clone()),
        declared_skill_names: normalize_tool_list(update.declared_skill_names.clone()),
        preset_questions: normalize_preset_questions(update.preset_questions.clone()),
        access_level: "A".to_string(),
        approval_mode: normalize_agent_approval_mode(Some(&update.approval_mode)),
        is_shared: false,
        status: normalize_agent_status(Some("active")),
        icon: update.icon.clone(),
        sandbox_container_id: normalize_sandbox_container_id(update.sandbox_container_id),
        created_at: 0.0,
        updated_at: 0.0,
        preset_binding: None,
        silent: update.silent,
        prefer_mother: update.prefer_mother,
    }
}

fn normalize_preset_id(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn preset_document_meta(document: &WorkerCardDocument) -> (u64, String) {
    let revision = document
        .preset
        .as_ref()
        .map(|item| item.revision.max(1))
        .unwrap_or(1);
    let status = document
        .preset
        .as_ref()
        .map(|item| normalize_agent_status(Some(&item.status)))
        .unwrap_or_else(|| normalize_agent_status(Some("active")));
    (revision, status)
}

pub fn export_file_name_for_preset(config: &UserAgentPresetConfig) -> String {
    worker_card_file_name(Some(&config.name), None)
}

pub fn export_file_name_for_default_agent(record: &UserAgentRecord) -> String {
    let display_name = record.name.trim();
    let canonical_name = if record.agent_id.trim() == DEFAULT_AGENT_ID_ALIAS
        && (display_name.is_empty() || display_name == DEFAULT_AGENT_ID_ALIAS)
    {
        DEFAULT_AGENT_NAME
    } else if display_name.is_empty() {
        record.agent_id.as_str()
    } else {
        display_name
    };
    worker_card_file_name(Some(canonical_name), None)
}

#[cfg(test)]
mod tests {
    use super::{
        configured_worker_cards_root, export_file_name_for_preset, load_effective_preset_configs,
        load_preset_worker_card_assets, persist_preset_configs,
        worker_card_document_from_preset_config,
    };
    use crate::config::{Config, UserAgentPresetConfig};
    use crate::services::inner_visible::WorkerCardPreset;
    use std::collections::HashSet;

    fn sample_preset() -> UserAgentPresetConfig {
        UserAgentPresetConfig {
            preset_id: "preset_demo".to_string(),
            revision: 3,
            name: "Demo Preset".to_string(),
            description: "desc".to_string(),
            system_prompt: "prompt".to_string(),
            model_name: Some("gpt-5".to_string()),
            icon_name: "spark".to_string(),
            icon_color: "#ABC".to_string(),
            sandbox_container_id: 4,
            tool_names: vec!["read_file".to_string()],
            declared_tool_names: Vec::new(),
            declared_skill_names: Vec::new(),
            preset_questions: vec!["Q1".to_string()],
            approval_mode: "suggest".to_string(),
            status: "active".to_string(),
        }
    }

    #[test]
    fn worker_card_round_trip_preserves_preset_metadata() {
        let skill_keys = HashSet::new();
        let preset = sample_preset();
        let document =
            worker_card_document_from_preset_config(&preset, &skill_keys).expect("build document");
        let root = std::env::temp_dir().join(format!(
            "wunder-preset-worker-card-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        let path = root.join(export_file_name_for_preset(&preset));
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&document).expect("serialize document"),
        )
        .expect("write document");

        let loaded = load_preset_worker_card_assets(&root, &skill_keys).expect("load assets");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].preset.preset_id, "preset_demo");
        assert_eq!(loaded[0].preset.revision, 3);
        assert_eq!(loaded[0].preset.name, "Demo Preset");
        assert_eq!(loaded[0].preset.tool_names, vec!["read_file".to_string()]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn worker_card_document_uses_metadata_agent_id_as_preset_identity() {
        let skill_keys = HashSet::new();
        let preset = sample_preset();
        let document =
            worker_card_document_from_preset_config(&preset, &skill_keys).expect("build document");
        assert_eq!(document.metadata.agent_id, "preset_demo");
        assert_eq!(
            document.preset,
            Some(WorkerCardPreset {
                revision: 3,
                status: "active".to_string(),
            })
        );
        assert!(
            document
                .extensions
                .as_object()
                .is_some_and(|item| item.is_empty()),
            "preset worker cards should no longer emit extension noise"
        );
        assert_eq!(document.abilities.tool_names, vec!["read_file".to_string()]);
    }

    #[test]
    fn preset_export_file_name_hides_internal_stable_id() {
        let preset = sample_preset();
        assert_eq!(
            export_file_name_for_preset(&preset),
            "Demo Preset.worker-card.json"
        );
    }

    #[test]
    fn persist_preset_configs_renames_files_to_canonical_name() {
        let root = std::env::temp_dir().join(format!(
            "wunder-preset-worker-card-persist-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        let mut config = Config::default();
        config.user_agents.worker_cards_root = root.to_string_lossy().to_string();
        let skill_keys = HashSet::new();

        let mut preset = sample_preset();
        persist_preset_configs(&config, &[preset.clone()], &skill_keys).expect("persist first");
        let first_name = export_file_name_for_preset(&preset);
        assert!(root.join(&first_name).exists());

        preset.name = "Renamed Preset".to_string();
        persist_preset_configs(&config, &[preset.clone()], &skill_keys).expect("persist second");
        let second_name = export_file_name_for_preset(&preset);
        assert!(root.join(&second_name).exists());
        assert!(!root.join(&first_name).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn effective_preset_configs_fall_back_to_legacy_when_asset_dir_is_empty() {
        let root = std::env::temp_dir().join(format!(
            "wunder-preset-worker-card-fallback-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).expect("create temp root");
        let mut config = Config::default();
        config.user_agents.worker_cards_root = root.to_string_lossy().to_string();
        config.user_agents.presets = vec![sample_preset()];

        let presets =
            load_effective_preset_configs(&config, &HashSet::new()).expect("load effective");
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].preset_id, "preset_demo");
        assert_eq!(
            configured_worker_cards_root(&config).expect("configured root"),
            root
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shipped_preset_worker_cards_declare_default_tools_and_skills_explicitly() {
        let shipped_root = std::path::PathBuf::from("config").join("preset_worker_cards");
        let skill_keys = HashSet::from(["技能创建器".to_string()]);
        let assets = load_preset_worker_card_assets(&shipped_root, &skill_keys)
            .expect("load shipped assets");
        assert_eq!(assets.len(), 5);
        for asset in assets {
            assert!(
                !asset.preset.declared_tool_names.is_empty(),
                "shipped preset worker card {} should declare tools explicitly",
                asset.path.display()
            );
            assert_eq!(
                asset.preset.declared_skill_names,
                vec!["技能创建器".to_string()],
                "shipped preset worker card {} should declare the default skill explicitly",
                asset.path.display()
            );
        }
    }
}
