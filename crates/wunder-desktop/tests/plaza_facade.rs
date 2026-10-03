//! Plaza façade contract tests. The desktop plaza only carries normal user
//! assets (worker cards and skill packs); the fixtures plant hive-pack items
//! into storage and require the façade to hide and refuse them.

use std::path::Path;
use wunder_desktop::{args::DesktopArgs, NativeDesktop};

fn start_isolated() -> Result<NativeDesktop, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("runtime/config");
    std::fs::create_dir_all(&config)?;
    std::fs::write(config.join("wunder.yaml"), "{}\n")?;
    let settings = serde_json::json!({
        "workspace_root": "", "desktop_token": "", "updated_at": 0,
        "lan_mesh": {"enabled": false},
        "llm": {"default": "test-model", "models": {
            "test-model": {"provider": "openai", "model": "test-model", "model_type": "llm",
                           "base_url": "http://127.0.0.1:9/v1", "timeout_s": 5,
                           "support_vision": false}}}
    });
    std::fs::write(
        config.join("desktop.settings.json"),
        serde_json::to_vec(&settings)?,
    )?;
    let mut args = DesktopArgs::native_defaults();
    args.temp_root = Some(directory.path().join("runtime").canonicalize()?);
    args.workspace = Some(directory.path().join("workspace"));
    std::mem::forget(directory);
    Ok(NativeDesktop::start_with_args(args)?)
}

const WORKER_CARD_ARTIFACT: &str = r#"{
    "schema_version": "wunder/worker-card@2",
    "kind": "WorkerCard",
    "metadata": { "agent_id": "", "name": "广场测试卡", "description": "来自广场的契约测试卡" },
    "abilities": { "tool_names": [], "skills": [] },
    "interaction": { "preset_questions": [] },
    "runtime": { "model_name": "", "approval_mode": "suggest" }
}"#;

fn plant_item(
    runtime: &NativeDesktop,
    root: &Path,
    item_id: &str,
    kind: &str,
    artifact: &str,
    updated_at: f64,
) {
    let artifact_path = root.join(format!("{item_id}.artifact"));
    std::fs::write(&artifact_path, artifact).expect("artifact file");
    let record = serde_json::json!({
        "item_id": item_id,
        "owner_user_id": "u_plaza_owner",
        "owner_username": "plaza-owner",
        "kind": kind,
        "source_key": format!("{kind}:{item_id}"),
        "title": format!("契约资产 {item_id}"),
        "summary": "契约测试广场条目",
        "icon": null,
        "artifact_filename": artifact_path.file_name().unwrap().to_str().unwrap(),
        "artifact_path": artifact_path.to_str().unwrap(),
        "artifact_size_bytes": artifact.len() as u64,
        "source_updated_at": null,
        "source_signature": null,
        "tags": ["契约"],
        "metadata": {},
        "created_at": 0.0,
        "updated_at": updated_at,
    });
    runtime
        .state()
        .storage
        .set_meta(&format!("user_plaza:item:{item_id}"), &record.to_string())
        .expect("plant plaza meta");
}

/// Plaza fixtures live in their own OS temp dir; the leaked runtime tempdir
/// keeps the process alive for the whole test either way.
fn fixture_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("wunder-plaza-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture dir");
    dir
}

#[test]
fn plaza_lists_normal_assets_and_hides_swarm_packs() {
    let runtime = start_isolated().expect("isolated runtime");
    let fixtures = fixture_dir();

    plant_item(
        &runtime,
        &fixtures,
        "card-1",
        "worker_card",
        WORKER_CARD_ARTIFACT,
        100.0,
    );
    plant_item(
        &runtime,
        &fixtures,
        "pack-1",
        "skill_pack",
        "skill-archive-placeholder",
        200.0,
    );
    plant_item(
        &runtime,
        &fixtures,
        "hive-1",
        "hive_pack",
        "swarm-pack-placeholder",
        300.0,
    );

    let items = runtime.list_plaza_items(None).expect("list");
    let ids: Vec<String> = items.iter().map(|item| item.item_id.clone()).collect();
    assert!(ids.contains(&"card-1".to_string()), "worker card missing");
    assert!(ids.contains(&"pack-1".to_string()), "skill pack missing");
    assert!(
        !ids.contains(&"hive-1".to_string()),
        "hive pack leaked into desktop plaza"
    );
    // Newest first ordering.
    assert_eq!(ids.first().map(String::as_str), Some("pack-1"));

    // Kind filters stay inside the desktop kinds.
    let cards = runtime
        .list_plaza_items(Some("worker_card"))
        .expect("cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].item_id, "card-1");
    assert!(runtime.list_plaza_items(Some("hive_pack")).is_err());

    let card = &items.iter().find(|item| item.item_id == "card-1").unwrap();
    assert_eq!(card.kind, "worker_card");
    assert!(!card.mine);
    assert_eq!(card.owner_username, "plaza-owner");
    assert!(card.tags.contains("契约"));
}

#[test]
fn plaza_import_refuses_unknown_and_swarm_items() {
    let runtime = start_isolated().expect("isolated runtime");
    let fixtures = fixture_dir();
    plant_item(
        &runtime,
        &fixtures,
        "hive-2",
        "hive_pack",
        "swarm-pack",
        10.0,
    );

    assert!(runtime.import_plaza_item("missing-item").is_err());
    let error = runtime
        .import_plaza_item("hive-2")
        .expect_err("hive pack must be refused");
    assert!(error.to_string().contains("桌面广场不提供该类型的资产"));
}

#[test]
fn plaza_import_creates_agent_from_worker_card() {
    let runtime = start_isolated().expect("isolated runtime");
    let fixtures = fixture_dir();
    plant_item(
        &runtime,
        &fixtures,
        "card-2",
        "worker_card",
        WORKER_CARD_ARTIFACT,
        5.0,
    );

    let outcome = runtime.import_plaza_item("card-2").expect("import");
    assert_eq!(outcome.kind, "worker_card");
    assert!(!outcome.imported_agent_id.is_empty(), "agent id missing");
    assert!(outcome.message.contains("广场测试卡") || !outcome.message.is_empty());

    let agents = runtime.list_agents().expect("agents");
    assert!(agents
        .iter()
        .any(|agent| agent.id == outcome.imported_agent_id));
    runtime
        .delete_agent(&outcome.imported_agent_id)
        .expect("cleanup");
    runtime
        .state()
        .storage
        .set_meta("user_plaza:item:card-2", "")
        .expect("drop fixture");
}
