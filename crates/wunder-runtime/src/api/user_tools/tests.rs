use super::{
    build_user_tools_summary, build_visible_user_skills_payload, resolve_visible_user_skill,
    UserSkillSourceKind, BUILTIN_SKILLS_ROOT_ENV,
};
use crate::config::Config;
use crate::core::schemas::ToolSpec;
use crate::services::skill_archive::{import_skill_archive, uploaded_skill_archive_top_dir};
use crate::services::user_access::UserToolContext;
use crate::services::user_tools::{UserToolAlias, UserToolBindings, UserToolKind};
use crate::skills::{load_skills, SkillRegistry, SkillSpec};
use crate::storage::{SqliteStorage, StorageBackend};
use crate::user_tools::UserToolsPayload;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::tempdir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

fn builtin_skills_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn write_test_skill(root: &Path, name: &str, description: &str) {
    let skill_dir = root.join(name);
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n"),
    )
    .expect("write skill file");
}

fn normalize_test_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace("\\\\?\\", "")
        .replace('\\', "/")
}

#[test]
fn uploaded_skill_archive_requires_top_level_directory() {
    let nested = Path::new("demo-skill/SKILL.md");
    assert_eq!(
        uploaded_skill_archive_top_dir(nested).unwrap(),
        "demo-skill"
    );

    let root_skill = Path::new("SKILL.md");
    assert!(uploaded_skill_archive_top_dir(root_skill).is_err());

    let root_script = Path::new("run.py");
    assert!(uploaded_skill_archive_top_dir(root_script).is_err());
}

fn build_skill_archive(entries: &[(&str, &str)]) -> Vec<u8> {
    let cursor = std::io::Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for (path, content) in entries {
        writer
            .start_file(path.replace('\\', "/"), options)
            .expect("start archive file");
        std::io::Write::write_all(&mut writer, content.as_bytes()).expect("write archive file");
    }
    writer.finish().expect("finish archive").into_inner()
}

#[test]
fn import_skill_archive_accepts_single_wrapper_directory() {
    let dir = tempdir().expect("tempdir");
    let archive = build_skill_archive(&[
        (
            "package-root/demo-skill/SKILL.md",
            "---\nname: demo-skill\ndescription: demo\n---\n",
        ),
        ("package-root/demo-skill/run.py", "print('ok')\n"),
    ]);
    let imported = import_skill_archive("demo-skill.zip", &archive, dir.path(), &HashSet::new())
        .expect("import wrapped archive");

    assert_eq!(imported.extracted, 2);
    assert_eq!(imported.top_level_dirs, vec!["demo-skill".to_string()]);
    assert!(dir.path().join("demo-skill").join("SKILL.md").is_file());
    assert!(dir.path().join("demo-skill").join("run.py").is_file());
    assert!(!dir.path().join("package-root").exists());
}

#[test]
fn import_skill_archive_accepts_skill_root_with_nested_files() {
    let dir = tempdir().expect("tempdir");
    let archive = build_skill_archive(&[
        (
            "hello-world-skill/SKILL.md",
            "---\nname: hello-world-skill\ndescription: demo\n---\n",
        ),
        (
            "hello-world-skill/scripts/generate_report.py",
            "print('ok')\n",
        ),
        ("hello-world-skill/assets/team_template.md", "# template\n"),
        (
            "hello-world-skill/references/report_templates.md",
            "# refs\n",
        ),
    ]);
    let imported = import_skill_archive(
        "hello-world-skill.zip",
        &archive,
        dir.path(),
        &HashSet::new(),
    )
    .expect("import direct skill archive");

    assert_eq!(imported.extracted, 4);
    assert_eq!(
        imported.top_level_dirs,
        vec!["hello-world-skill".to_string()]
    );
    assert!(dir
        .path()
        .join("hello-world-skill")
        .join("SKILL.md")
        .is_file());
    assert!(dir
        .path()
        .join("hello-world-skill")
        .join("scripts")
        .join("generate_report.py")
        .is_file());
    assert!(dir
        .path()
        .join("hello-world-skill")
        .join("assets")
        .join("team_template.md")
        .is_file());
    assert!(dir
        .path()
        .join("hello-world-skill")
        .join("references")
        .join("report_templates.md")
        .is_file());
    assert_eq!(imported.final_names, vec!["hello-world-skill".to_string()]);
}

#[test]
fn import_skill_archive_rejects_mixed_wrapped_and_direct_layouts() {
    let dir = tempdir().expect("tempdir");
    let archive = build_skill_archive(&[
        (
            "package-root/demo-skill/SKILL.md",
            "---\nname: demo-skill\ndescription: demo\n---\n",
        ),
        (
            "other-skill/SKILL.md",
            "---\nname: other-skill\ndescription: demo\n---\n",
        ),
    ]);
    let err = import_skill_archive("mixed.zip", &archive, dir.path(), &HashSet::new())
        .expect_err("mixed archive should be rejected");

    assert!(err.to_string().contains("top-level skill directory"));
}

#[test]
fn import_skill_archive_auto_renames_conflicting_skill_dir_and_frontmatter_name() {
    let dir = tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("demo-skill")).expect("create existing skill dir");
    fs::write(
        dir.path().join("demo-skill").join("SKILL.md"),
        "---\nname: demo-skill\ndescription: existing\n---\n",
    )
    .expect("write existing skill");
    let archive = build_skill_archive(&[
        (
            "demo-skill/SKILL.md",
            "---\nname: demo-skill\ndescription: imported\n---\n",
        ),
        ("demo-skill/run.py", "print('ok')\n"),
    ]);

    let imported = import_skill_archive("demo-skill.zip", &archive, dir.path(), &HashSet::new())
        .expect("import renamed skill");

    assert_eq!(imported.top_level_dirs, vec!["demo-skill-2".to_string()]);
    assert_eq!(imported.final_names, vec!["demo-skill-2".to_string()]);
    let skill_md = fs::read_to_string(dir.path().join("demo-skill-2").join("SKILL.md"))
        .expect("read renamed skill");
    assert!(skill_md.contains("name: demo-skill-2"));
    assert!(dir.path().join("demo-skill-2").join("run.py").is_file());
}

#[test]
fn import_skill_archive_inserts_frontmatter_name_when_missing() {
    let dir = tempdir().expect("tempdir");
    let archive = build_skill_archive(&[
        ("demo-skill/SKILL.md", "# Demo Skill\n\ncontent\n"),
        ("demo-skill/run.py", "print('ok')\n"),
    ]);

    let imported = import_skill_archive("demo-skill.zip", &archive, dir.path(), &HashSet::new())
        .expect("import skill without frontmatter");

    assert_eq!(imported.top_level_dirs, vec!["demo-skill".to_string()]);
    let skill_md = fs::read_to_string(dir.path().join("demo-skill").join("SKILL.md"))
        .expect("read imported skill");
    assert!(skill_md.starts_with("---\nname: demo-skill\n---\n"));
}

#[test]
fn load_skills_accepts_skill_markdown_without_frontmatter() {
    let dir = tempdir().expect("tempdir");
    let skill_dir = dir.path().join("plain-skill");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "# 数据库校验技能2\n\n本技能用于对数据库表中的数据进行规则校验。\n",
    )
    .expect("write skill file");

    let mut config = Config::default();
    config.skills.paths = vec![dir.path().to_string_lossy().to_string()];

    let registry = load_skills(&config, false, false, false);
    let specs = registry.list_specs();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "数据库校验技能2");
    assert_eq!(
        specs[0].description,
        "本技能用于对数据库表中的数据进行规则校验。"
    );
    assert!(specs[0].frontmatter.is_empty());
}

#[test]
fn catalog_can_expose_user_custom_skills_even_when_not_allowed() {
    let dir = tempdir().expect("tempdir");
    let db_path = dir.path().join("user-tools-catalog.db");
    let storage: Arc<dyn StorageBackend> =
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    let mut bindings = UserToolBindings::default();
    bindings.alias_specs.insert(
        "custom_skill".to_string(),
        ToolSpec {
            name: "custom_skill".to_string(),
            title: None,
            description: "custom skill".to_string(),
            input_schema: json!({ "type": "object" }),
        },
    );
    bindings.alias_map.insert(
        "custom_skill".to_string(),
        UserToolAlias {
            kind: UserToolKind::Skill,
            owner_id: "alice".to_string(),
            target: "custom_skill".to_string(),
        },
    );
    bindings.alias_specs.insert(
        "alice@mcp_demo@tool".to_string(),
        ToolSpec {
            name: "alice@mcp_demo@tool".to_string(),
            title: None,
            description: "mcp tool".to_string(),
            input_schema: json!({ "type": "object" }),
        },
    );
    bindings.alias_map.insert(
        "alice@mcp_demo@tool".to_string(),
        UserToolAlias {
            kind: UserToolKind::Mcp,
            owner_id: "alice".to_string(),
            target: "mcp_demo@tool".to_string(),
        },
    );
    let context = UserToolContext {
        config: crate::config::Config::default(),
        skills: SkillRegistry::default(),
        bindings,
        tool_access: None,
        org_units: Vec::new(),
    };
    let allowed = HashSet::new();

    let summary_without_catalog =
        build_user_tools_summary("alice", &allowed, &context, false, storage.as_ref());
    assert!(
        summary_without_catalog.user_skills.is_empty(),
        "non-catalog summary should keep allow-list filtering"
    );

    let summary_with_catalog =
        build_user_tools_summary("alice", &allowed, &context, true, storage.as_ref());
    assert_eq!(summary_with_catalog.user_skills.len(), 1);
    assert_eq!(summary_with_catalog.user_skills[0].name, "custom_skill");
    assert!(
        summary_with_catalog
            .user_mcp_tools
            .iter()
            .all(|tool| tool.name != "alice@mcp_demo@tool"),
        "catalog fallback should only relax user skill visibility"
    );
}

#[test]
fn desktop_catalog_includes_all_builtin_skills_for_agent_settings() {
    let _guard = builtin_skills_env_lock()
        .lock()
        .expect("lock builtin skills env");
    let dir = tempdir().expect("tempdir");
    let builtin_root = tempdir().expect("builtin root");
    let db_path = dir.path().join("desktop-catalog-skills.db");
    let storage: Arc<dyn StorageBackend> =
        Arc::new(SqliteStorage::new(db_path.to_string_lossy().to_string()));
    write_test_skill(
        builtin_root.path(),
        "builtin_skill_a",
        "enabled builtin skill",
    );
    write_test_skill(
        builtin_root.path(),
        "builtin_skill_b",
        "available builtin skill",
    );

    let previous = std::env::var(BUILTIN_SKILLS_ROOT_ENV).ok();
    std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, builtin_root.path());

    let mut config = Config::default();
    config.server.mode = "desktop".to_string();
    config.skills.enabled = vec!["builtin_skill_a".to_string()];
    let mut skills = SkillRegistry::default();
    skills.add_spec_for_test(SkillSpec {
        name: "builtin_skill_a".to_string(),
        description: "enabled builtin skill".to_string(),
        path: builtin_root
            .path()
            .join("builtin_skill_a")
            .join("SKILL.md")
            .to_string_lossy()
            .to_string(),
        input_schema: json!({ "type": "object" }),
        frontmatter: String::new(),
        root: builtin_root.path().join("builtin_skill_a"),
        entrypoint: None,
    });
    let context = UserToolContext {
        config,
        skills,
        bindings: UserToolBindings::default(),
        tool_access: None,
        org_units: Vec::new(),
    };
    let allowed = HashSet::from(["builtin_skill_a".to_string()]);

    let summary_with_catalog =
        build_user_tools_summary("alice", &allowed, &context, true, storage.as_ref());
    let names = summary_with_catalog
        .admin_skills
        .iter()
        .map(|skill| skill.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["builtin_skill_a", "builtin_skill_b"]);

    if let Some(value) = previous {
        std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, value);
    } else {
        std::env::remove_var(BUILTIN_SKILLS_ROOT_ENV);
    }
}

#[test]
fn desktop_visible_skill_payload_includes_all_builtin_without_user_copy() {
    let _guard = builtin_skills_env_lock()
        .lock()
        .expect("lock builtin skills env");
    let builtin_root = tempdir().expect("builtin root");
    let user_root = tempdir().expect("user root");
    write_test_skill(builtin_root.path(), "内置技能A", "builtin skill");
    write_test_skill(builtin_root.path(), "内置技能B", "disabled builtin skill");

    let previous = std::env::var(BUILTIN_SKILLS_ROOT_ENV).ok();
    std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, builtin_root.path());

    let mut config = Config::default();
    config.server.mode = "desktop".to_string();
    config.skills.enabled = vec!["内置技能A".to_string()];

    let payload = UserToolsPayload::default();
    let (skills, enabled, shared) =
        build_visible_user_skills_payload(&config, &payload, user_root.path());

    let names = skills
        .iter()
        .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["内置技能A", "内置技能B"]);
    assert_eq!(enabled, vec!["内置技能A"]);
    assert!(shared.is_empty());
    assert_eq!(
        skills[0].get("source").and_then(serde_json::Value::as_str),
        Some("builtin")
    );
    assert_eq!(
        skills[0]
            .get("readonly")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        skills[0]
            .get("enabled")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        skills[1]
            .get("enabled")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );

    if let Some(value) = previous {
        std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, value);
    } else {
        std::env::remove_var(BUILTIN_SKILLS_ROOT_ENV);
    }
}

#[test]
fn resolve_visible_user_skill_can_read_builtin_from_builtin_root() {
    let _guard = builtin_skills_env_lock()
        .lock()
        .expect("lock builtin skills env");
    let builtin_root = tempdir().expect("builtin root");
    let user_root = tempdir().expect("user root");
    write_test_skill(builtin_root.path(), "内置技能A", "builtin skill");

    let previous = std::env::var(BUILTIN_SKILLS_ROOT_ENV).ok();
    std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, builtin_root.path());

    let mut config = Config::default();
    config.server.mode = "desktop".to_string();
    config.skills.enabled = vec!["内置技能A".to_string()];

    let resolved =
        resolve_visible_user_skill(&config, user_root.path(), "内置技能A").expect("resolve skill");
    assert!(matches!(resolved.source, UserSkillSourceKind::Builtin));
    assert_eq!(resolved.spec.name, "内置技能A");
    assert_eq!(
        normalize_test_path(&resolved.root),
        normalize_test_path(&builtin_root.path().join("内置技能A"))
    );

    if let Some(value) = previous {
        std::env::set_var(BUILTIN_SKILLS_ROOT_ENV, value);
    } else {
        std::env::remove_var(BUILTIN_SKILLS_ROOT_ENV);
    }
}

#[test]
fn resolve_visible_user_skill_accepts_user_skill_directory_alias() {
    let user_root = tempdir().expect("user root");
    let skill_dir = user_root.path().join("original_skill");
    fs::create_dir_all(&skill_dir).expect("create skill dir");
    fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: renamed_skill\ndescription: renamed skill\n---\n",
    )
    .expect("write skill file");

    let resolved =
        resolve_visible_user_skill(&Config::default(), user_root.path(), "original_skill")
            .expect("resolve skill by directory alias");

    assert!(matches!(resolved.source, UserSkillSourceKind::Custom));
    assert_eq!(resolved.spec.name, "renamed_skill");
    assert_eq!(
        normalize_test_path(&resolved.root),
        normalize_test_path(&skill_dir)
    );
}

#[test]
fn resolve_visible_user_skill_can_read_enabled_global_skill_from_config_paths() {
    let user_root = tempdir().expect("user root");
    let global_root = tempdir().expect("global root");
    write_test_skill(global_root.path(), "政策知识库检索", "global skill");

    let mut config = Config::default();
    config.skills.paths = vec![global_root.path().to_string_lossy().to_string()];
    config.skills.enabled = vec!["政策知识库检索".to_string()];

    let resolved = resolve_visible_user_skill(&config, user_root.path(), "政策知识库检索")
        .expect("resolve global skill");
    assert!(matches!(resolved.source, UserSkillSourceKind::Global));
    assert_eq!(resolved.spec.name, "政策知识库检索");
    assert_eq!(
        normalize_test_path(&resolved.root),
        normalize_test_path(&global_root.path().join("政策知识库检索"))
    );
}
