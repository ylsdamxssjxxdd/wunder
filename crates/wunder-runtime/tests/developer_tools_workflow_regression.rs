use std::collections::HashMap;
use std::sync::Arc;

use image::{DynamicImage, ImageBuffer, Rgba};
use serde_json::{json, Value};
use tempfile::TempDir;
use wunder_server::a2a_store::A2aStore;
use wunder_server::config::Config;
use wunder_server::skills::SkillRegistry;
use wunder_server::storage::{SqliteStorage, StorageBackend};
use wunder_server::tools::{build_read_image_followup_user_message, execute_tool, ToolContext};
use wunder_server::workspace::WorkspaceManager;
use wunder_server::LspManager;

const USER_ID: &str = "developer-tools";
const SESSION_ID: &str = "developer-tools-regression";

struct ToolFixture {
    _temp: TempDir,
    storage: Arc<dyn StorageBackend>,
    workspace: Arc<WorkspaceManager>,
    lsp_manager: Arc<LspManager>,
    config: Config,
    a2a_store: A2aStore,
    skills: SkillRegistry,
    http: reqwest::Client,
}

impl ToolFixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("create temporary developer tools workspace");
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            temp.path()
                .join("state.sqlite3")
                .to_string_lossy()
                .to_string(),
        ));
        storage
            .ensure_initialized()
            .expect("initialize temporary storage");
        let workspace = Arc::new(WorkspaceManager::new(
            temp.path().join("workspace").to_string_lossy().as_ref(),
            Arc::clone(&storage),
            0,
            &HashMap::new(),
        ));
        workspace
            .ensure_user_root(USER_ID)
            .expect("initialize tool workspace");
        let lsp_manager = LspManager::new(Arc::clone(&workspace));
        let mut config = Config::default();
        config.server.mode = "desktop".to_string();
        config.lsp.enabled = false;
        config.security.allow_commands = vec!["*".to_string()];
        Self {
            _temp: temp,
            storage,
            workspace,
            lsp_manager,
            config,
            a2a_store: A2aStore::default(),
            skills: SkillRegistry::default(),
            http: reqwest::Client::new(),
        }
    }

    fn context(&self) -> ToolContext<'_> {
        ToolContext {
            user_id: USER_ID,
            session_id: SESSION_ID,
            workspace_id: USER_ID,
            agent_id: None,
            user_round: Some(1),
            model_round: Some(1),
            is_admin: false,
            storage: Arc::clone(&self.storage),
            orchestrator: None,
            monitor: None,
            beeroom_realtime: None,
            workspace: Arc::clone(&self.workspace),
            lsp_manager: Arc::clone(&self.lsp_manager),
            config: &self.config,
            a2a_store: &self.a2a_store,
            skills: &self.skills,
            gateway: None,
            user_world: None,
            cron_wake_signal: None,
            user_tool_manager: None,
            user_tool_bindings: None,
            user_tool_store: None,
            request_config_overrides: None,
            allow_roots: None,
            read_roots: None,
            command_sessions: None,
            event_emitter: None,
            http: &self.http,
        }
    }

    fn workspace_path(&self, path: &str) -> std::path::PathBuf {
        self.workspace.workspace_root(USER_ID).join(path)
    }
}

async fn invoke(fixture: &ToolFixture, name: &str, args: Value) -> Value {
    let context = fixture.context();
    execute_tool(&context, name, &args)
        .await
        .unwrap_or_else(|error| panic!("{name} execution failed unexpectedly: {error}"))
}

#[tokio::test]
async fn common_developer_tools_complete_a_real_workspace_workflow() {
    let fixture = ToolFixture::new();

    let write = invoke(
        &fixture,
        "write_file",
        json!({
            "path": "docs/notes.txt",
            "content": "alpha\nneedle beta\ngamma\nneedle delta\nomega\n"
        }),
    )
    .await;
    assert_eq!(write["ok"], true);
    assert_eq!(write["data"]["bytes"], 43);

    let read = invoke(
        &fixture,
        "read_file",
        json!({ "path": "docs/notes.txt", "start_line": 1, "end_line": 5 }),
    )
    .await;
    assert_eq!(read["ok"], true);
    assert!(read["data"]["content"]
        .as_str()
        .is_some_and(|content| content.contains("2: needle beta")));

    let search = invoke(
        &fixture,
        "search_content",
        json!({
            "query": "needle",
            "path": "docs",
            "query_mode": "literal",
            "context_before": 1,
            "context_after": 1,
            "max_matches": 10,
            "engine": "rust"
        }),
    )
    .await;
    assert_eq!(search["ok"], true);
    let hits = search["data"]["hits"]
        .as_array()
        .expect("structured search hits");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0]["before"][0]["content"], "alpha");
    assert_eq!(hits[0]["after"][0]["content"], "gamma");

    let edit = invoke(
        &fixture,
        "edit_file2",
        json!({
            "path": "docs/notes.txt",
            "old_text": "needle beta",
            "new_text": "needle BETA"
        }),
    )
    .await;
    assert_eq!(edit["ok"], true);
    assert_eq!(edit["data"]["edit_count"], 1);

    let archive = invoke(
        &fixture,
        "write_file",
        json!({ "path": "docs/archive.txt", "content": "retire\n" }),
    )
    .await;
    assert_eq!(archive["ok"], true);

    let patch = r#"*** Begin Patch
*** Update File: docs/notes.txt
*** Move to: docs/final-notes.txt
@@
 alpha
-needle BETA
+needle BETA revised
 gamma
 needle delta
 omega
*** Add File: docs/manifest.txt
+final-notes.txt
*** Delete File: docs/archive.txt
*** End Patch"#;
    let preview = invoke(
        &fixture,
        "apply_patch",
        json!({ "input": patch, "dry_run": true }),
    )
    .await;
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["data"]["dry_run"], true);
    assert_eq!(preview["data"]["added"], 1);
    assert_eq!(preview["data"]["updated"], 1);
    assert_eq!(preview["data"]["deleted"], 1);
    assert_eq!(preview["data"]["moved"], 1);
    assert!(fixture.workspace_path("docs/notes.txt").is_file());
    assert!(!fixture.workspace_path("docs/final-notes.txt").exists());

    let applied = invoke(&fixture, "apply_patch", json!({ "input": patch })).await;
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["data"]["changed_files"], 4);
    assert!(fixture.workspace_path("docs/final-notes.txt").is_file());
    assert!(!fixture.workspace_path("docs/notes.txt").exists());
    assert!(!fixture.workspace_path("docs/archive.txt").exists());
    assert_eq!(
        std::fs::read_to_string(fixture.workspace_path("docs/final-notes.txt"))
            .expect("read patched file"),
        "alpha\nneedle BETA revised\ngamma\nneedle delta\nomega\n"
    );

    let atomic_failure = r#"*** Begin Patch
*** Add File: docs/should-not-exist.txt
+not committed
*** Update File: docs/missing.txt
@@
-old
+new
*** End Patch"#;
    let failed_patch = invoke(&fixture, "apply_patch", json!({ "input": atomic_failure })).await;
    assert_eq!(failed_patch["ok"], false);
    assert_eq!(
        failed_patch["error_meta"]["code"],
        "PATCH_CONFLICT_FILE_NOT_FOUND"
    );
    assert!(!fixture.workspace_path("docs/should-not-exist.txt").exists());

    let success_command = if cfg!(windows) {
        "cmd /c echo workflow-ok"
    } else {
        "printf workflow-ok"
    };
    let command = invoke(
        &fixture,
        "execute_command",
        json!({ "content": success_command }),
    )
    .await;
    assert_eq!(command["ok"], true);
    assert!(command["data"]["results"][0]["stdout"]
        .as_str()
        .is_some_and(|output| output.contains("workflow-ok")));

    let failing_command = if cfg!(windows) {
        "cmd /c exit 7"
    } else {
        "sh -c 'exit 7'"
    };
    let failed_command = invoke(
        &fixture,
        "execute_command",
        json!({ "content": failing_command }),
    )
    .await;
    assert_eq!(failed_command["ok"], false);
    assert_eq!(
        failed_command["error_meta"]["code"],
        "TOOL_EXEC_NON_ZERO_EXIT"
    );
    assert_eq!(failed_command["data"]["returncode"], 7);
}

#[tokio::test]
async fn patch_dry_run_rejects_mismatched_context_without_mutating_the_workspace() {
    let fixture = ToolFixture::new();
    std::fs::write(fixture.workspace_path("file.txt"), "current\n").unwrap();
    let version = fixture.workspace.get_tree_version(USER_ID);
    let result = invoke(&fixture, "apply_patch", json!({
        "input": "*** Begin Patch\n*** Add File: staged.txt\n+new\n*** Update File: file.txt\n@@\n-missing\n+replacement\n*** End Patch",
        "dry_run": true
    })).await;
    assert_eq!(result["ok"], false);
    assert_eq!(result["error_meta"]["code"], "PATCH_CONTEXT_NOT_FOUND");
    assert_eq!(fixture.workspace.get_tree_version(USER_ID), version);
    assert_eq!(
        std::fs::read_to_string(fixture.workspace_path("file.txt")).unwrap(),
        "current\n"
    );
    assert!(!fixture.workspace_path("staged.txt").exists());
}

#[tokio::test]
async fn large_delete_patch_has_a_shared_preview_budget_and_exact_counts() {
    let fixture = ToolFixture::new();
    let content = "line\r\n".repeat(100_000);
    let mut patch = String::from("*** Begin Patch\n");
    for index in 0..6 {
        let path = format!("file-{index}.txt");
        std::fs::write(fixture.workspace_path(&path), &content).expect("write deletion fixture");
        patch.push_str(&format!("*** Delete File: {path}\n"));
    }
    patch.push_str("*** End Patch");
    let preview = invoke(
        &fixture,
        "apply_patch",
        json!({"input": patch, "dry_run": true}),
    )
    .await;
    assert_eq!(preview["ok"], true);
    assert_eq!(preview["data"]["deleted_lines"], 600_000);
    assert_eq!(preview["data"]["diff_lines_omitted"], 599_680);
    assert!(preview.to_string().len() < 48 * 1024);
    for index in 0..6 {
        assert_eq!(
            std::fs::read_to_string(fixture.workspace_path(&format!("file-{index}.txt"))).unwrap(),
            content
        );
    }
    let applied = invoke(&fixture, "apply_patch", json!({"input": patch})).await;
    assert_eq!(applied["ok"], true);
    assert_eq!(applied["data"]["files"], preview["data"]["files"]);
    assert_eq!(applied["data"]["deleted_lines"], 600_000);
    for index in 0..6 {
        assert!(!fixture
            .workspace_path(&format!("file-{index}.txt"))
            .exists());
    }
}

#[tokio::test]
async fn patch_preview_skips_oversized_unicode_lines_without_clipping_text() {
    let fixture = ToolFixture::new();
    let content = format!("{}\nsmall\n", "文".repeat(20_000));
    std::fs::write(fixture.workspace_path("long.txt"), content).unwrap();
    let result = invoke(
        &fixture,
        "apply_patch",
        json!({
            "input": "*** Begin Patch\n*** Delete File: long.txt\n*** End Patch",
            "dry_run": true
        }),
    )
    .await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["deleted_lines"], 2);
    assert_eq!(result["data"]["diff_lines_omitted"], 1);
    assert_eq!(
        result["data"]["files"][0]["diff_blocks"][0]["lines"],
        json!([
            {"kind": "delete", "old_line": 2, "new_line": null, "text": "small"}
        ])
    );
    assert!(result.to_string().len() < 2048);
    assert!(fixture.workspace_path("long.txt").exists());
}

#[tokio::test]
async fn read_image_generates_the_followup_image_block_for_the_model() {
    let fixture = ToolFixture::new();
    let image = ImageBuffer::<Rgba<u8>, _>::from_pixel(2, 2, Rgba([20, 40, 60, 255]));
    let mut bytes = Vec::new();
    DynamicImage::ImageRgba8(image)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .expect("encode png test fixture");
    std::fs::write(fixture.workspace_path("preview.png"), bytes).expect("write png fixture");

    let result = invoke(&fixture, "read_image", json!({ "path": "preview.png" })).await;
    assert_eq!(result["ok"], true);
    assert_eq!(result["data"]["media_kind"], "image");

    let context = fixture.context();
    let followup = build_read_image_followup_user_message(&context, &result["data"])
        .await
        .expect("build image followup")
        .expect("image result creates a followup message");
    let content = followup["content"]
        .as_array()
        .expect("followup content array");
    assert_eq!(content[0]["type"], "text");
    let image_block = content
        .iter()
        .find(|block| block["type"] == "image_url")
        .expect("image_url block");
    assert!(image_block["image_url"]["url"]
        .as_str()
        .is_some_and(|url| url.starts_with("data:image/png;base64,")));
}
