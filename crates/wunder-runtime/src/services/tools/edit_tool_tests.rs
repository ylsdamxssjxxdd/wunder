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
        self.call_tool("文本编辑", args).await
    }

    async fn call_tool(&self, tool: &str, args: Value) -> Value {
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
        crate::services::tools::dispatch::execute_builtin_tool(&context, tool, &args)
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

/// `create` 的正文参数是 `file_text`、`insert` 的是 `new_string`，两者语义相同。
/// 模型在两个子命令之间切换时沿用上一个参数名不应被判为缺参。
#[tokio::test]
async fn subcommand_text_param_names_are_interchangeable() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\nbeta\n");
    let inserted = fixture
        .call(json!({"command":"insert", "file_path":"sample.txt", "insert_line":1, "file_text":"middle\n"}))
        .await;
    assert_ok(&inserted);
    assert_eq!(fixture.read("sample.txt"), "alpha\nmiddle\nbeta\n");
    let created = fixture
        .call(json!({"command":"create", "file_path":"new.txt", "new_string":"body\n"}))
        .await;
    assert_ok(&created);
    assert_eq!(fixture.read("new.txt"), "body\n");
}

/// 缺参提示要指出同义别名，否则模型只会重复同一个错误调用。
#[tokio::test]
async fn missing_subcommand_text_param_names_the_accepted_aliases() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\n");
    let missing = fixture
        .call(json!({"command":"insert", "file_path":"sample.txt", "insert_line":1}))
        .await;
    assert_eq!(missing["error_meta"]["code"], "TOOL_SRE_MISSING_PARAM");
    let hint = missing["error_meta"]["hint"].as_str().unwrap_or_default();
    assert!(hint.contains("file_text"), "{hint}");
    let error = missing["error"].as_str().unwrap_or_default();
    assert!(error.contains("command=insert"), "{error}");
}

/// 预览标记必须落在 `data` 里：模型和观测投影都靠它区分「没写盘」与
/// 「写盘但内容没变」，只写在 carrier 的 `state` 上会两边都读到。
#[tokio::test]
async fn preview_flags_dry_run_inside_data_for_every_write_subcommand() {
    let fixture = Fixture::new();
    fixture.write("sample.txt", "alpha\n");
    for args in [
        json!({"command":"create", "file_path":"new.txt", "file_text":"body\n"}),
        json!({"command":"insert", "file_path":"sample.txt", "insert_line":1, "new_string":"middle\n"}),
    ] {
        let mut preview = args;
        preview["dry_run"] = json!(true);
        let result = fixture.call(preview).await;
        assert_ok(&result);
        assert_eq!(result["state"], "dry_run");
        assert_eq!(result["data"]["dry_run"], json!(true), "{result}");
    }
    assert_eq!(fixture.read("sample.txt"), "alpha\n");
    assert!(!fixture.path("new.txt").exists());
}

#[tokio::test]
async fn range_replace_rewrites_lines_and_returns_a_verifiable_receipt() {
    let fixture = Fixture::new();
    let source = "def draw(t):\n    x = 1\n    y = 2\n    return x + y\n\n\ndef render(t):\n    return draw(t)\n";
    fixture.write("sketch.py", source);
    let result = fixture
        .call(json!({
            "file_path": "sketch.py",
            "start_line": 2,
            "end_line": 3,
            "new_text": "    x = 10\n    y = 20\n"
        }))
        .await;
    assert_ok(&result);
    assert_eq!(
        fixture.read("sketch.py"),
        "def draw(t):\n    x = 10\n    y = 20\n    return x + y\n\n\ndef render(t):\n    return draw(t)\n"
    );
    assert_eq!(result["data"]["range"], json!([2, 3]));
    assert_eq!(result["data"]["lines"], json!(8));
    let receipt = result["data"]["receipt"].as_str().expect("receipt");
    assert!(receipt.contains("sha256:"), "{receipt}");
    assert!(receipt.contains("symbols:") && receipt.contains("draw(1-4)"), "{receipt}");
    let syntax = result["data"]["syntax"].as_str().expect("syntax");
    assert!(syntax.contains("py_compile") || syntax.contains("skipped"), "{syntax}");
    let unified = result["data"]["unified_diff"].as_str().expect("unified diff");
    assert!(unified.contains("-    x = 1"), "{unified}");
    assert!(unified.contains("+    x = 10"), "{unified}");
    // 覆盖前的内容留在 temp/.history，可直接 diff 回滚。
    let snapshot = result["data"]["history"].as_str().expect("history").to_string();
    assert!(fixture.path(&snapshot).exists(), "{snapshot}");
    assert_eq!(std::fs::read_to_string(fixture.path(&snapshot)).unwrap(), source);
}

#[tokio::test]
async fn range_replace_beyond_eof_reports_the_real_line_count() {
    let fixture = Fixture::new();
    fixture.write("short.txt", "a\nb\n");
    let result = fixture
        .call(json!({
            "file_path": "short.txt",
            "start_line": 5,
            "end_line": 9,
            "new_string": "x\n"
        }))
        .await;
    assert_eq!(result["ok"], false);
    assert_eq!(result["error_meta"]["code"], "TOOL_EDIT_RANGE_INVALID");
    assert_eq!(result["data"]["file_lines"], json!(2));
    assert_eq!(fixture.read("short.txt"), "a\nb\n");
}

#[tokio::test]
async fn missing_old_string_returns_the_nearest_fragment_and_a_range_hint() {
    let fixture = Fixture::new();
    fixture.write("sketch.py", "def draw(t):\n    x = 1\n    y = 2\n    return x + y\n");
    let result = fixture
        .call(json!({
            "file_path": "sketch.py",
            "old_string": "def draw(t):\n    x = 1\n    y = 3\n    return x + y\n",
            "new_string": "def draw(t):\n    return 0\n"
        }))
        .await;
    assert_eq!(result["error_meta"]["code"], "TOOL_EDIT_NO_MATCH");
    let nearest = result["data"]["nearest"].as_array().expect("nearest");
    assert_eq!(nearest[0]["line"], json!(1));
    assert_eq!(nearest[0]["end_line"], json!(4));
    let details = nearest[0]["differences"].as_array().expect("differences");
    assert!(
        details.iter().any(|note| note.as_str().unwrap_or("").contains("第 3 行")),
        "{nearest:?}"
    );
    let hint = result["error_meta"]["hint"].as_str().unwrap_or("");
    assert!(hint.contains("start_line=1"), "{hint}");
}

#[tokio::test]
async fn dry_run_previews_a_unified_diff_without_writing_or_snapshotting() {
    let fixture = Fixture::new();
    fixture.write("sketch.py", "alpha\nbeta\ngamma\n");
    let result = fixture
        .call(json!({
            "file_path": "sketch.py",
            "start_line": 2,
            "end_line": 2,
            "new_string": "BETA\n",
            "dry_run": true
        }))
        .await;
    assert_ok(&result);
    assert_eq!(result["state"], "dry_run");
    let unified = result["data"]["unified_diff"].as_str().expect("unified");
    assert!(unified.contains("-beta") && unified.contains("+BETA"), "{unified}");
    assert_eq!(fixture.read("sketch.py"), "alpha\nbeta\ngamma\n");
    assert!(result["data"].get("history").is_none());
}

#[tokio::test]
async fn write_file_returns_a_receipt_a_snapshot_and_a_preview_diff() {
    let fixture = Fixture::new();
    fixture.write("sketch.py", "x = 1\n");
    let result = fixture
        .call_tool(
            "写入文件",
            json!({"path": "sketch.py", "content": "x = 2\ny = 3\n"}),
        )
        .await;
    assert_ok(&result);
    assert_eq!(result["data"]["lines"], json!(2));
    let receipt = result["data"]["receipt"].as_str().expect("receipt");
    assert!(receipt.contains("file: sketch.py (2 lines"), "{receipt}");
    assert!(receipt.contains("sha256:"), "{receipt}");
    let snapshot = result["data"]["history"].as_str().expect("history").to_string();
    assert_eq!(
        std::fs::read_to_string(fixture.path(&snapshot)).unwrap(),
        "x = 1\n"
    );
    let unified = result["data"]["unified_diff"].as_str().expect("unified");
    assert!(unified.contains("-x = 1"), "{unified}");
    assert!(unified.contains("+x = 2"), "{unified}");
    assert_eq!(fixture.read("sketch.py"), "x = 2\ny = 3\n");

    let preview = fixture
        .call_tool(
            "写入文件",
            json!({"path": "sketch.py", "content": "z = 9\n", "dry_run": true}),
        )
        .await;
    assert_ok(&preview);
    assert_eq!(preview["state"], "dry_run");
    let preview_diff = preview["data"]["unified_diff"].as_str().expect("preview diff");
    assert!(preview_diff.contains("+z = 9"), "{preview_diff}");
    assert_eq!(fixture.read("sketch.py"), "x = 2\ny = 3\n");
    assert!(preview["data"].get("history").is_none());
}

/// 批量替换超过结构化块预算（12 块）时，回执里的 unified diff 仍要完整列出
/// 每一处命中：模型确认改动靠的是这段文本，不是 `diff_blocks` 的抽样结果。
#[tokio::test]
async fn unified_diff_lists_every_replacement_past_the_block_budget() {
    let fixture = Fixture::new();
    let mut source = String::new();
    for index in 0..30 {
        if index % 2 == 0 {
            source.push_str(&format!("token {index}\n"));
        } else {
            source.push_str(&format!("keep {index}\n"));
        }
    }
    fixture.write("sample.txt", &source);
    let result = fixture
        .call(json!({
            "file_path": "sample.txt",
            "old_string": "token",
            "new_string": "TOKEN",
            "replace_all": true
        }))
        .await;
    assert_ok(&result);
    assert_eq!(result["data"]["hunks_applied"], json!(15));
    assert!(result["data"]["diff_lines_omitted"].as_u64().unwrap_or(0) > 0);
    let unified = result["data"]["unified_diff"]
        .as_str()
        .expect("unified diff");
    assert_eq!(unified.matches("@@ -").count(), 15, "{unified}");
    assert!(unified.contains("+TOKEN 28"), "{unified}");
    // 边界行只出现一次；已改行不会再被当成下一块的上下文重复打印。
    assert_eq!(unified.matches(" keep 1\n").count(), 1, "{unified}");
    assert_eq!(unified.matches(" TOKEN 2\n").count(), 0, "{unified}");
    assert_eq!(result["data"]["unified_diff_truncated"], json!(false));
}
