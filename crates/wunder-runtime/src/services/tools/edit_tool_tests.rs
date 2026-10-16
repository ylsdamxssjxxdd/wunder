use super::*;
use crate::{
    config::Config,
    lsp::LspManager,
    skills::SkillRegistry,
    storage::{SqliteStorage, StorageBackend},
    workspace::WorkspaceManager,
};
use std::{collections::HashMap, sync::Arc};

struct Fixture {
    _dir: tempfile::TempDir,
    workspace: Arc<WorkspaceManager>,
    storage: Arc<dyn StorageBackend>,
    config: Config,
    skills: SkillRegistry,
    http: reqwest::Client,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let storage: Arc<dyn StorageBackend> = Arc::new(SqliteStorage::new(
            dir.path().join("state.db").to_string_lossy().into_owned(),
        ));
        let workspace = Arc::new(WorkspaceManager::new(
            dir.path().join("workspaces").to_string_lossy().as_ref(),
            storage.clone(),
            0,
            &HashMap::new(),
        ));
        workspace.ensure_user_root("workspace").unwrap();
        let mut config = Config::default();
        config.server.mode = "desktop".into();
        Self {
            _dir: dir,
            workspace,
            storage,
            config,
            skills: SkillRegistry::default(),
            http: reqwest::Client::new(),
        }
    }

    async fn call(&self, args: Value) -> Value {
        let context = ToolContext {
            user_id: "user",
            session_id: "thread",
            workspace_id: "workspace",
            agent_id: None,
            user_round: None,
            model_round: None,
            is_admin: false,
            storage: self.storage.clone(),
            orchestrator: None,
            monitor: None,
            workspace: self.workspace.clone(),
            lsp_manager: LspManager::new_disabled(self.workspace.clone()),
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
        };
        crate::services::tools::dispatch::execute_builtin_tool(&context, "文本编辑", &args)
            .await
            .unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.workspace.resolve_path("workspace", name).unwrap()
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name)).unwrap()
    }
    fn write(&self, name: &str, text: &str) {
        self.workspace
            .write_file("workspace", name, text, true)
            .unwrap();
    }
}

fn assert_ok(result: &Value) {
    assert_eq!(result["ok"], true, "{result}");
}

#[tokio::test]
async fn replacement_forms_share_canonical_alias_and_replace_all_semantics() {
    let fixture = Fixture::new();
    assert_ok(&fixture.call(json!({"command":"create", "file_path":"sample.txt", "file_text":"alpha\nbeta\nalpha\n"})).await);
    // Canonical fields with the legacy command must work on the first call.
    assert_ok(&fixture.call(json!({"command":"str_replace", "file_path":"sample.txt", "old_string":"beta", "new_string":"gamma"})).await);
    assert_ok(&fixture.call(json!({"command":"str_replace", "path":"sample.txt", "old_str":"gamma", "new_str":"delta"})).await);
    let all = fixture.call(json!({"command":"str_replace", "file_path":"sample.txt", "old_string":"alpha", "new_string":"ALPHA", "replace_all":true})).await;
    assert_ok(&all);
    assert_eq!(all["data"]["replacements"], 2);
    assert_eq!(fixture.read("sample.txt"), "ALPHA\ndelta\nALPHA\n");
    let default = fixture.call(json!({"file_path":"sample.txt", "old_str":"ALPHA", "new_str":"alpha", "replace_all":true})).await;
    assert_ok(&default);
    assert_eq!(default["data"]["replacements"], 2);
    assert_eq!(fixture.read("sample.txt"), "alpha\ndelta\nalpha\n");
}

#[tokio::test]
async fn invalid_or_ambiguous_replacement_never_writes_and_explains_recovery() {
    let fixture = Fixture::new();
    let before = "token\n".repeat(400);
    fixture.write("sample.txt", &before);
    let version = fixture.workspace.get_tree_version("workspace");
    let ambiguous = fixture.call(json!({"command":"str_replace", "file_path":"sample.txt", "old_string":"token", "new_string":"x"})).await;
    assert_eq!(ambiguous["error_meta"]["code"], "TOOL_EDIT_NOT_UNIQUE");
    assert_eq!(ambiguous["error_meta"]["retryable"], false);
    let error = ambiguous["error"].as_str().unwrap();
    assert!(error.contains("400") && error.contains("[1, 2, 3, 4, 5, 6, 7, 8]"));
    assert!(ambiguous.to_string().len() < 2200);
    for args in [
        json!({"old_string":"token", "new_string":"x", "old_str":"different"}),
        json!({"old_string":"token", "new_string":"x", "new_str":"different"}),
        json!({"old_string":"token", "new_str":42}),
        json!({"old_string":"token", "new_string":null}),
        json!({"old_string":"token", "new_string":"x", "replace_all":"true"}),
        json!({"old_string":"token", "new_string":"x", "dry_run":"true"}),
        json!({"old_string":"token", "new_string":"x", "input":"*** Begin Patch\n*** End Patch"}),
    ] {
        let mut args = args;
        args["file_path"] = json!("sample.txt");
        args["command"] = json!("str_replace");
        let result = fixture.call(args).await;
        assert_eq!(
            result["error_meta"]["code"], "TOOL_EDIT_INVALID_ARGS",
            "{result}"
        );
    }
    let missing = fixture
        .call(json!({"file_path":"sample.txt", "old_string":"absent", "new_string":"x"}))
        .await;
    assert_eq!(missing["error_meta"]["code"], "TOOL_EDIT_NO_MATCH");
    assert_eq!(missing["error_meta"]["retryable"], false);
    let omitted = fixture.call(json!({"command":"str_replace", "file_path":"sample.txt", "old_str":"token", "replace_all":true})).await;
    assert_eq!(
        omitted["error_meta"]["code"],
        "TOOL_EDIT_NEW_STRING_REQUIRED"
    );
    assert_eq!(fixture.read("sample.txt"), before);
    assert_eq!(fixture.workspace.get_tree_version("workspace"), version);
}

#[tokio::test]
async fn every_write_mode_previews_without_creating_files_or_changing_versions() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\nbeta\n");
    let version = fixture.workspace.get_tree_version("workspace");
    for args in [
        json!({"command":"create", "file_path":"nested/new.txt", "file_text":"new\n"}),
        json!({"command":"insert", "file_path":"sample.txt", "insert_line":1, "new_string":"middle\n"}),
        json!({"command":"str_replace", "file_path":"sample.txt", "old_str":"beta", "new_str":"BETA"}),
        json!({"file_path":"sample.txt", "old_string":"alpha", "new_string":"ALPHA"}),
        json!({"input":"*** Begin Patch\n*** Add File: nested/patch.txt\n+new\n*** End Patch"}),
    ] {
        let mut preview = args.clone();
        preview["dry_run"] = json!(true);
        let result = fixture.call(preview).await;
        assert_ok(&result);
        assert_eq!(result["state"], "dry_run", "{result}");
        assert!(result["data"]["files"]
            .as_array()
            .is_some_and(|files| !files.is_empty()));
        assert_eq!(fixture.read("sample.txt"), "alpha\nbeta\n");
        assert!(!fixture.path("nested").exists());
        assert_eq!(fixture.workspace.get_tree_version("workspace"), version);
    }
    let patch = fixture
        .call(json!({"input":"*** Begin Patch\n*** Add File: added.txt\n+created\n*** End Patch"}))
        .await;
    assert_ok(&patch);
    assert_eq!(fixture.read("added.txt"), "created\n");
}

#[tokio::test]
async fn replacements_preserve_crlf_and_wrapped_json_routes_before_dispatch() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\r\nbeta\r\ngamma\r\n");
    let result = fixture.call(json!({"command":"str_replace", "file_path":"sample.txt", "old_string":"alpha\nbeta", "new_string":"first\nsecond"})).await;
    assert_ok(&result);
    assert_eq!(fixture.read("sample.txt"), "first\r\nsecond\r\ngamma\r\n");
    let wrapped = fixture.call(json!({"raw": json!({"command":"insert", "file_path":"sample.txt", "insert_line":2, "new_string":"middle\n"}).to_string()})).await;
    assert_ok(&wrapped);
    assert_eq!(
        fixture.read("sample.txt"),
        "first\r\nsecond\r\nmiddle\r\ngamma\r\n"
    );
}

#[tokio::test]
async fn insertion_uses_logical_lines_and_does_not_duplicate_boundary_newlines() {
    let fixture = Fixture::new();
    for (before, line, inserted, expected) in [
        ("", 0, "new", "new"),
        ("one\ntwo\n", 0, "new\n", "new\none\ntwo\n"),
        ("one\ntwo\n", 1, "new\n", "one\nnew\ntwo\n"),
        ("one\ntwo\n", 2, "new", "one\ntwo\nnew\n"),
        ("one\ntwo", 2, "new", "one\ntwo\nnew"),
        ("one\r\ntwo\r\n", 1, "new\n", "one\r\nnew\r\ntwo\r\n"),
    ] {
        fixture.write("sample.txt", before);
        let result = fixture.call(json!({"command":"insert", "file_path":"sample.txt", "insert_line":line, "new_string":inserted})).await;
        assert_ok(&result);
        assert_eq!(fixture.read("sample.txt"), expected);
    }
    fixture.write("sample.txt", "one\ntwo\n");
    let invalid = fixture.call(json!({"command":"insert", "file_path":"sample.txt", "insert_line":3, "new_string":"bad"})).await;
    assert_eq!(invalid["ok"], false);
    assert_eq!(invalid["data"]["total_lines"], 2);
    let view = fixture
        .call(json!({"command":"view", "file_path":"sample.txt"}))
        .await;
    assert_ok(&view);
    assert!(view["data"]["content"]
        .as_str()
        .unwrap()
        .contains("total of 2 lines"));
    assert_eq!(fixture.read("sample.txt"), "one\ntwo\n");
}

#[tokio::test]
async fn no_op_edits_leave_workspace_version_unchanged() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\n");
    let version = fixture.workspace.get_tree_version("workspace");
    for args in [
        json!({"file_path":"sample.txt", "old_string":"alpha", "new_string":"alpha"}),
        json!({"command":"insert", "file_path":"sample.txt", "insert_line":1, "new_string":""}),
    ] {
        let result = fixture.call(args).await;
        assert_ok(&result);
        assert_eq!(result["state"], "unchanged");
        assert_eq!(result["data"]["changed_files"], 0);
        assert_eq!(fixture.workspace.get_tree_version("workspace"), version);
    }
    assert_eq!(fixture.read("sample.txt"), "alpha\n");
}
