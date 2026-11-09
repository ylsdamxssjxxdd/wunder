use std::collections::HashMap;
use std::sync::Arc;

use image::{DynamicImage, ImageBuffer, Rgba};
use serde_json::{json, Value};
use tempfile::TempDir;
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
            workspace: Arc::clone(&self.workspace),
            lsp_manager: Arc::clone(&self.lsp_manager),
            config: &self.config,
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
        .is_some_and(|content| content.contains("\nneedle beta\n")));
    assert!(!read["data"]["content"]
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
-needle beta
+needle beta revised
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
        "alpha\nneedle beta revised\ngamma\nneedle delta\nomega\n"
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
async fn read_file_defaults_pattern_and_outline_share_one_bounded_workflow() {
    let fixture = ToolFixture::new();
    let mut content = (1..=600)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    content.push_str("needle target\n");
    let write = invoke(
        &fixture,
        "write_file",
        json!({"path": "large.txt", "content": content}),
    )
    .await;
    assert_eq!(write["ok"], true);

    let read = invoke(&fixture, "read_file", json!({"path": "large.txt"})).await;
    assert_eq!(read["ok"], true);
    assert_eq!(read["data"]["read_lines"], 500);
    assert_eq!(read["data"]["total_lines"], 601);
    assert_eq!(read["data"]["files"][0]["read_lines"], 500);
    assert_eq!(read["data"]["files"][0]["total_lines"], 601);
    assert_eq!(read["data"]["budget"]["output_budget_bytes"], 16384);
    // A default window reaching its line limit is informational, not a demand
    // to keep reading; exactly one reason is reported.
    assert_eq!(read["data"]["truncation_reason"], "line_window");
    assert_eq!(read["data"]["has_more"], true);
    assert!(read["data"].get("continuation_required").is_none());
    let text = read["data"]["content"].as_str().expect("raw read content");
    assert!(text.ends_with("line 500"));
    assert!(!text.contains("500: line 500"));

    let long_lines = (1..=100)
        .map(|line| format!("{line}: {}\n", "x".repeat(400)))
        .collect::<String>();
    let write = invoke(
        &fixture,
        "write_file",
        json!({"path": "long-lines.txt", "content": long_lines}),
    )
    .await;
    assert_eq!(write["ok"], true);
    let bounded = invoke(&fixture, "read_file", json!({"path": "long-lines.txt"})).await;
    assert_eq!(bounded["ok"], true);
    assert_eq!(bounded["data"]["output_budget_hit"], true);
    assert_eq!(bounded["data"]["truncation_reason"], "output_budget");
    assert_eq!(bounded["data"]["continuation_required"], true);
    assert_eq!(bounded["data"]["total_lines"], 100);
    let read_lines = bounded["data"]["read_lines"].as_u64().expect("read lines");
    assert!(read_lines > 0 && read_lines < 100);
    let text = bounded["data"]["content"]
        .as_str()
        .expect("bounded content");
    assert_eq!(text.lines().count().saturating_sub(1) as u64, read_lines);
    assert!(text.len() <= 16384);

    let matched = invoke(
        &fixture,
        "read_file",
        json!({"path": "large.txt", "pattern": "needle", "query_mode": "literal", "context": 2}),
    )
    .await;
    assert_eq!(matched["ok"], true);
    assert_eq!(matched["data"]["files"][0]["match_count"], 1);
    assert_eq!(matched["data"]["files"][0]["read_lines"], 3);
    assert_eq!(
        matched["data"]["files"][0]["effective_ranges"],
        json!([[599, 601]])
    );
    let text = matched["data"]["content"]
        .as_str()
        .expect("pattern content");
    assert!(text.contains("needle target"));
    assert!(!text.contains("601: needle target"));

    let write = invoke(
        &fixture,
        "write_file",
        json!({"path": "outline.md", "content": "# Guide\nfn main() {}\ntext\nclass Example:\n"}),
    )
    .await;
    assert_eq!(write["ok"], true);
    let outline = invoke(
        &fixture,
        "read_file",
        json!({"path": "outline.md", "mode": "outline"}),
    )
    .await;
    assert_eq!(outline["ok"], true);
    assert_eq!(outline["data"]["files"][0]["outline_entries"], 3);
    assert_eq!(outline["data"]["files"][0]["read_lines"], 3);
    assert_eq!(outline["data"]["files"][0]["total_lines"], 4);
    let text = outline["data"]["content"]
        .as_str()
        .expect("outline content");
    assert!(text.contains("1: # Guide"));
    assert!(text.contains("2: fn main() {}"));
    assert!(text.contains("4: class Example:"));
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

#[tokio::test]
async fn read_file_limit_and_list_files_top_level_work_in_one_call() {
    let fixture = ToolFixture::new();
    let root = fixture.workspace_path("survey");
    std::fs::create_dir_all(root.join("alpha/nested")).expect("create nested dir");
    std::fs::create_dir_all(root.join("beta")).expect("create second dir");
    for index in 0..40 {
        std::fs::write(root.join(format!("file-{index:02}.txt")), "x")
            .expect("write survey file");
    }
    std::fs::write(root.join("alpha/inside.txt"), "x").expect("write nested file");
    let lines = (1..=80)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    std::fs::write(root.join("many.txt"), lines).expect("write long file");

    // `limit` alone must be an explicit 20-line window, not a silent 500-line
    // default that trips the byte budget and emits a false paging warning.
    let read = invoke(
        &fixture,
        "read_file",
        json!({ "path": "survey/many.txt", "limit": 20 }),
    )
    .await;
    assert_eq!(read["ok"], true);
    assert_eq!(read["data"]["read_lines"], 20);
    assert_eq!(read["data"]["total_lines"], 80);
    assert_eq!(read["data"]["truncation_reason"], Value::Null);
    assert_eq!(read["data"]["has_more"], true);
    assert!(read["data"].get("continuation_required").is_none());

    let list = invoke(&fixture, "list_files", json!({ "path": "survey" })).await;
    assert_eq!(list["ok"], true);
    assert_eq!(list["data"]["folders_first"], true);
    let items = list["data"]["items"].as_array().expect("list items");
    assert!(items.len() > 32);
    assert!(items[0].as_str().is_some_and(|item| item.ends_with('/')));
    assert!(items[1].as_str().is_some_and(|item| item.ends_with('/')));

    let top = invoke(
        &fixture,
        "list_files",
        json!({ "path": "survey", "top_level_only": true }),
    )
    .await;
    assert_eq!(top["ok"], true);
    let top_items = top["data"]["items"].as_array().expect("top-level items");
    assert!(top_items.iter().any(|item| item.as_str() == Some("alpha/")));
    assert!(top_items.iter().any(|item| item.as_str() == Some("beta/")));
    for item in top_items {
        let text = item.as_str().expect("string entry");
        assert!(
            text.ends_with('/') || !text.contains('/'),
            "unexpected nested entry: {text}"
        );
    }
}

#[test]
fn localized_schedule_text_and_browser_surface_match_model_feedback() {
    use wunder_server::cron_schedule::{parse_schedule_text, ParsedScheduleText};

    assert_eq!(
        parse_schedule_text("每天 09:00").unwrap(),
        ParsedScheduleText::Cron("0 9 * * *".to_string())
    );
    assert_eq!(
        parse_schedule_text("每天下午3点半").unwrap(),
        ParsedScheduleText::Cron("30 15 * * *".to_string())
    );
    assert_eq!(
        parse_schedule_text("每周一 10:30").unwrap(),
        ParsedScheduleText::Cron("30 10 * * 1".to_string())
    );
    assert_eq!(
        parse_schedule_text("每 5 分钟").unwrap(),
        ParsedScheduleText::EveryMs(300_000)
    );
    assert_eq!(
        parse_schedule_text("daily at 9am").unwrap(),
        ParsedScheduleText::Cron("0 9 * * *".to_string())
    );
    assert!(parse_schedule_text("随便什么时候").is_err());

    // A flat action schema must not advertise a wait-only field to navigate.
    let browser = wunder_server::tools::builtin_tool_specs()
        .into_iter()
        .find(|spec| spec.name == "browser")
        .expect("browser spec");
    let projected = wunder_server::tools::prompt_tool_spec(&browser, "en-US");
    let properties = projected.input_schema["properties"]
        .as_object()
        .expect("browser properties");
    assert!(!properties.contains_key("load_state"));
    assert!(properties.contains_key("wait_ms"));
    assert!(projected
        .description
        .contains("wait_ms only applies to wait"));
}

#[cfg(windows)]
#[test]
fn command_output_decoder_recovers_gbk_chinese() {
    let (encoded, _, _) = encoding_rs::GBK.encode("中文输出正常");
    assert_eq!(
        wunder_server::tools::decode_command_output(&encoded),
        "中文输出正常"
    );
    assert_eq!(
        wunder_server::tools::decode_command_output("中文输出正常".as_bytes()),
        "中文输出正常"
    );
}

/// Legacy zh-CN text files are GBK/GB18030, and a minified bundle is one huge
/// line. Both used to make `read_file` return either "binary" or no text at all.
#[tokio::test]
async fn read_file_handles_gbk_long_line_and_query_alias() {
    let fixture = ToolFixture::new();

    let gbk_path = fixture.workspace_path("legacy/说明.txt");
    std::fs::create_dir_all(gbk_path.parent().expect("parent dir")).expect("create legacy dir");
    let (encoded, _, had_errors) = encoding_rs::GBK.encode("第一行中文\n第二行中文\n");
    assert!(!had_errors);
    std::fs::write(&gbk_path, &encoded).expect("write gbk file");

    let gbk_payload = invoke(&fixture, "read_file", json!({"path": "legacy/说明.txt"})).await;
    assert_eq!(gbk_payload["ok"], true, "gbk read failed: {gbk_payload}");
    assert!(
        gbk_payload["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("第一行中文"),
        "gbk content: {gbk_payload}"
    );

    let long_line = "X".repeat(300_000);
    std::fs::write(
        fixture.workspace_path("bundle.min.js"),
        long_line.clone() + "\n",
    )
    .expect("write long line");
    let long_read = invoke(&fixture, "read_file", json!({"path": "bundle.min.js"})).await;
    assert_eq!(long_read["ok"], true, "long line read failed: {long_read}");
    let long_content = long_read["data"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        long_content.contains("truncated read output"),
        "long line content len: {}",
        long_content.len()
    );

    invoke(
        &fixture,
        "write_file",
        json!({"path": "notes.txt", "content": "alpha\nneedle beta\ngamma\n"}),
    )
    .await;
    let query_read = invoke(
        &fixture,
        "read_file",
        json!({"path": "notes.txt", "query": "needle"}),
    )
    .await;
    assert_eq!(
        query_read["ok"], true,
        "query alias on a regular file failed: {query_read}"
    );
    assert!(
        query_read["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains("needle beta"),
        "query content: {query_read}"
    );
}

/// A phrase that only matches term-by-term must say so, otherwise the caller
/// cannot tell a real phrase hit from a fallback match.
#[tokio::test]
async fn search_content_reports_phrase_fallback_strategy() {
    let fixture = ToolFixture::new();
    invoke(
        &fixture,
        "write_file",
        json!({"path": "search/target.txt", "content": "alpha\nbeta\ngamma\n"}),
    )
    .await;

    let result = invoke(
        &fixture,
        "search_content",
        json!({"query": "alpha beta", "path": "search", "query_mode": "literal"}),
    )
    .await;
    assert_eq!(result["ok"], true, "search failed: {result}");
    let data = &result["data"];
    assert!(
        data["strategy"].is_string(),
        "missing strategy field: {result}"
    );
    assert!(
        data["fallback_applied"].is_boolean(),
        "missing fallback_applied field: {result}"
    );
    assert_ne!(
        data["strategy"].as_str(),
        Some("literal_exact"),
        "phrase hits must report an exact strategy: {result}"
    );
}
