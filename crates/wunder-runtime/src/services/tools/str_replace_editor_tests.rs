use super::*;

#[test]
fn command_requested_only_for_non_empty_command() {
    assert!(command_requested(&json!({"command": "view"})));
    assert!(!command_requested(&json!({"command": "   "})));
    assert!(!command_requested(&json!({"command": null})));
    assert!(!command_requested(&json!({})));
}

#[test]
fn view_range_parser_rejects_non_pairs() {
    assert!(parse_view_range(&json!({})).unwrap().is_none());
    assert!(parse_view_range(&json!({"view_range": null}))
        .unwrap()
        .is_none());
    assert!(parse_view_range(&json!({"view_range": [11, 12]}))
        .unwrap()
        .is_some());
    assert!(parse_view_range(&json!({"view_range": [11]})).is_err());
    assert!(parse_view_range(&json!({"view_range": [11, "x"]})).is_err());
}

#[test]
fn file_view_numbers_lines_and_applies_range() {
    let rendered = format_file_view("f.txt", "alpha\nbeta\ngamma\n", None).unwrap();
    assert!(rendered.contains("     1  alpha"));
    assert!(rendered.contains("     3  gamma"));
    let ranged = format_file_view("f.txt", "alpha\nbeta\ngamma\n", Some(vec![2, 3])).unwrap();
    assert!(ranged.contains("with view_range=[2, 3]"));
    assert!(ranged.contains("     2  beta"));
    assert!(!ranged.contains("alpha"));
}

#[test]
fn file_view_uses_actual_lines_and_bounds_long_line_rendering() {
    let rendered = format_file_view("sample.txt", "alpha\r\nbeta\r\n", None).unwrap();
    assert!(rendered.contains("total of 2 lines"));
    assert!(!rendered.contains('\r'));
    assert!(format_file_view("sample.txt", "alpha\n", Some(vec![2, -1])).is_err());
    assert!(format_file_view("sample.txt", "", None)
        .unwrap()
        .contains("total of 0 lines"));
    let long = "文".repeat(MAX_OUTPUT_CHARS * 4);
    let clipped = format_file_view("sample.txt", &long, None).unwrap();
    assert!(clipped.ends_with(TRUNCATED_MESSAGE));
    assert_eq!(
        clipped.chars().count(),
        MAX_OUTPUT_CHARS + TRUNCATED_MESSAGE.chars().count()
    );
}

#[test]
fn truncation_appends_marker() {
    let out = maybe_truncate("0123456789", 4);
    assert!(out.starts_with("0123"));
    assert!(out.contains("<response clipped>"));
}

#[test]
fn str_replace_data_carries_the_diff_contract() {
    let before = "alpha\nbeta\ngamma\n";
    let after = "alpha\nBETA\ngamma\n";
    let (diff, unavailable) = super::super::line_diff::diff_literal_replaces(
        before,
        after,
        &[6],
        "beta".len(),
        "BETA",
    );
    assert!(unavailable.is_none());
    let data = build_edit_data("f.txt", "update", &diff, unavailable);
    let mut keys: Vec<&str> = data
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "added",
            "added_lines",
            "changed_files",
            "deleted",
            "deleted_lines",
            "diff_lines_omitted",
            "files",
            "hunks_applied",
            "moved",
            "path",
            "updated",
        ]
    );
    assert_eq!(data["files"][0]["action"], json!("update"));
    assert_eq!(data["added_lines"], json!(1));
    assert_eq!(data["deleted_lines"], json!(1));
    assert_eq!(data["hunks_applied"], json!(1));
    let block = &data["files"][0]["diff_blocks"][0];
    assert_eq!(block["header"], json!("@@ -2,1 +2,1 @@"));
    assert_eq!(block["lines"][0]["kind"], json!("delete"));
    assert_eq!(block["lines"][0]["old_line"], json!(2));
    assert!(block["lines"][0]["new_line"].is_null());
    assert_eq!(block["lines"][0]["text"], json!("beta"));
    assert_eq!(block["lines"][1]["kind"], json!("add"));
    assert_eq!(block["lines"][1]["new_line"], json!(2));
    assert_eq!(block["lines"][1]["text"], json!("BETA"));
}

use crate::storage::SqliteStorage;
use std::collections::HashMap;
use tempfile::tempdir;

fn sorted_keys(value: &Value) -> Vec<String> {
    let mut keys: Vec<String> = value.as_object().expect("object").keys().cloned().collect();
    keys.sort();
    keys
}

/// 真实工作区夹具：直接跑 `run_editor` 分派，覆盖写盘与 diff 组装全链路。
fn editor_workspace(root: &Path) -> Arc<WorkspaceManager> {
    let storage = Arc::new(SqliteStorage::new(
        root.join("state.sqlite3").to_string_lossy().to_string(),
    ));
    Arc::new(WorkspaceManager::new(
        root.join("workspaces").to_string_lossy().as_ref(),
        storage,
        0,
        &HashMap::new(),
    ))
}

fn run(ws: &Arc<WorkspaceManager>, command: &str, args: &Value) -> Value {
    debug_assert_eq!(args["command"].as_str(), Some(command));
    run_editor(
        Arc::clone(ws),
        "tester".to_string(),
        Vec::new(),
        command,
        args.get("file_path").and_then(Value::as_str).unwrap_or(""),
        args,
    )
    .expect("editor dispatch")
    .value
}

fn create_args(path: &str, text: &str) -> Value {
    json!({ "command": "create", "file_path": path, "file_text": text })
}

#[test]
fn create_emits_one_all_add_block_with_new_side_numbering() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    let result = run(
        &ws,
        "create",
        &create_args("notes.txt", "one\ntwo\nthree\n"),
    );
    assert_eq!(result["action"], json!("create"));
    assert_eq!(result["ok"], json!(true));
    let data = &result["data"];
    assert_eq!(data["path"], json!("notes.txt"));
    assert_eq!(data["added_lines"], json!(3));
    assert_eq!(data["deleted_lines"], json!(0));
    assert_eq!(data["changed_files"], json!(1));
    assert_eq!(data["added"], json!(1));
    assert_eq!(data["updated"], json!(0));
    assert_eq!(data["deleted"], json!(0));
    assert_eq!(data["moved"], json!(0));
    assert_eq!(data["hunks_applied"], json!(1));
    assert_eq!(data["diff_lines_omitted"], json!(0));
    assert!(data.get("diff_unavailable").is_none());
    let file = &data["files"][0];
    assert_eq!(file["action"], json!("add"));
    assert_eq!(file["path"], json!("notes.txt"));
    assert!(file["to_path"].is_null());
    assert_eq!(file["hunks"], json!(1));
    assert_eq!(file["diff_blocks"].as_array().expect("blocks").len(), 1);
    let block = &file["diff_blocks"][0];
    assert_eq!(block["header"], json!("new file"));
    assert_eq!(block["start_line_before"], json!(0));
    assert_eq!(block["end_line_before"], json!(0));
    assert_eq!(block["start_line_after"], json!(1));
    assert_eq!(block["end_line_after"], json!(3));
    let rows = block["lines"].as_array().expect("rows");
    assert_eq!(rows.len(), 3);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row["kind"], json!("add"));
        assert!(row["old_line"].is_null());
        assert_eq!(row["new_line"], json!(index + 1));
    }
    assert_eq!(rows[0]["text"], json!("one"));
    assert_eq!(rows[2]["text"], json!("three"));

    // 落盘内容不变：view 仍能看到整份文件。
    let viewed = run(
        &ws,
        "view",
        &json!({ "command": "view", "file_path": "notes.txt" }),
    );
    assert!(viewed["data"]["content"]
        .as_str()
        .expect("content")
        .contains("     1  one"));
}

#[test]
fn create_refuses_existing_path_without_claiming_a_diff() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    run(&ws, "create", &create_args("notes.txt", "a\n"));
    let again = run(&ws, "create", &create_args("notes.txt", "b\n"));
    assert_eq!(again["ok"], json!(false));
    assert_eq!(again["error_meta"]["code"], json!("TOOL_SRE_FILE_EXISTS"));
    assert!(again["data"].get("files").is_none());
}

#[test]
fn insert_emits_add_only_block_at_the_insertion_point() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    run(
        &ws,
        "create",
        &create_args("notes.txt", "alpha\nbeta\ngamma\n"),
    );
    let result = run(
        &ws,
        "insert",
        &json!({
            "command": "insert",
            "file_path": "notes.txt",
            "insert_line": 2,
            "new_string": "X\nY",
        }),
    );
    assert_eq!(result["action"], json!("insert"));
    let data = &result["data"];
    assert_eq!(data["path"], json!("notes.txt"));
    assert_eq!(data["added_lines"], json!(2));
    assert_eq!(data["deleted_lines"], json!(0));
    assert_eq!(data["updated"], json!(1));
    assert_eq!(data["added"], json!(0));
    assert_eq!(data["hunks_applied"], json!(1));
    assert_eq!(data["changed_files"], json!(1));
    let file = &data["files"][0];
    assert_eq!(file["action"], json!("update"));
    assert_eq!(file["hunks"], json!(1));
    assert_eq!(file["deleted_lines"], json!(0));
    let block = &file["diff_blocks"][0];
    assert_eq!(file["diff_blocks"].as_array().expect("blocks").len(), 1);
    assert_eq!(block["header"], json!("@@ -3,0 +3,2 @@"));
    assert_eq!(block["start_line_after"], json!(3));
    assert_eq!(block["end_line_after"], json!(4));
    let rows = block["lines"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    for row in rows.iter() {
        assert_eq!(row["kind"], json!("add"));
        assert!(row["old_line"].is_null());
    }
    assert_eq!(rows[0]["new_line"], json!(3));
    assert_eq!(rows[0]["text"], json!("X"));
    assert_eq!(rows[1]["new_line"], json!(4));
    assert_eq!(rows[1]["text"], json!("Y"));

    let viewed = run(
        &ws,
        "view",
        &json!({ "command": "view", "file_path": "notes.txt" }),
    );
    let content = viewed["data"]["content"].as_str().expect("content");
    assert!(content.contains("     3  X"), "{content}");
    assert!(content.contains("     5  gamma"), "{content}");
}

/// `create` 不允许覆盖既有路径（见 `create_refuses_existing_path_*`），
/// 覆盖写语义属于 `写入文件`；这里钉住家族里 `update` 动作两侧编号的形态，
/// 保证若日后放开覆盖，渲染侧契约不需要再改。
#[test]
fn update_action_numbers_both_sides_for_an_overwrite_shape() {
    let data = build_edit_data(
        "notes.txt",
        "update",
        &diff_replaced("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\nDELTA\n"),
        None,
    );
    assert_eq!(data["updated"], json!(1));
    assert_eq!(data["added"], json!(0));
    assert_eq!(data["deleted_lines"], json!(2));
    assert_eq!(data["added_lines"], json!(3));
    let file = &data["files"][0];
    assert_eq!(file["action"], json!("update"));
    let rows = file["diff_blocks"][0]["lines"].as_array().expect("rows");
    let deletes: Vec<&Value> = rows
        .iter()
        .filter(|row| row["kind"] == json!("delete"))
        .collect();
    let adds: Vec<&Value> = rows
        .iter()
        .filter(|row| row["kind"] == json!("add"))
        .collect();
    assert_eq!(deletes.len(), 2);
    assert_eq!(adds.len(), 3);
    for (index, row) in deletes.iter().enumerate() {
        assert_eq!(row["old_line"], json!(index + 2));
        assert!(row["new_line"].is_null());
    }
    for (index, row) in adds.iter().enumerate() {
        assert_eq!(row["new_line"], json!(index + 2));
        assert!(row["old_line"].is_null());
    }
}

#[test]
fn create_and_insert_data_pin_the_patch_family_key_set() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    let created = run(&ws, "create", &create_args("a.txt", "one\n"));
    // 新建文件没有旧内容可留快照，所以 history 只在覆盖类写入里出现；
    // 其余回执/diff 家族键两条路径必须完全一致，模型才只需认一套口径。
    let base = vec![
        "added",
        "added_lines",
        "changed_files",
        "check",
        "deleted",
        "deleted_lines",
        "diff_lines_omitted",
        "dry_run",
        "files",
        "hunks_applied",
        "lines",
        "moved",
        "path",
        "receipt",
        "sha256",
        "syntax",
        "unified_diff",
        "unified_diff_truncated",
        "updated",
    ];
    assert_eq!(sorted_keys(&created["data"]), base);
    let mut expected = base.clone();
    expected.insert(9, "history");
    expected.insert(10, "history_rev");
    let inserted = run(
        &ws,
        "insert",
        &json!({
            "command": "insert",
            "file_path": "a.txt",
            "insert_line": 1,
            "new_string": "two",
        }),
    );
    assert_eq!(sorted_keys(&inserted["data"]), expected);
    let block = &inserted["data"]["files"][0]["diff_blocks"][0];
    assert_eq!(
        sorted_keys(block),
        vec![
            "end_line_after",
            "end_line_before",
            "header",
            "lines",
            "start_line_after",
            "start_line_before"
        ]
    );
    assert_eq!(
        sorted_keys(&block["lines"][0]),
        vec!["kind", "new_line", "old_line", "text"]
    );
    for row in block["lines"].as_array().expect("rows") {
        let kind = row["kind"].as_str().expect("kind");
        assert!(matches!(kind, "meta" | "add" | "delete"), "{kind}");
        let text = row["text"].as_str().expect("text");
        assert!(!text.starts_with('+') && !text.starts_with('-'), "{text}");
    }
}

#[test]
fn insert_degrades_to_counts_only_for_oversized_sources() {
    use super::super::line_diff::MAX_DIFF_SOURCE_BYTES;
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    let big = "x\n".repeat(MAX_DIFF_SOURCE_BYTES / 2 + 1);
    run(&ws, "create", &create_args("big.txt", &big));
    let result = run(
        &ws,
        "insert",
        &json!({
            "command": "insert",
            "file_path": "big.txt",
            "insert_line": 1,
            "new_string": "y",
        }),
    );
    let data = &result["data"];
    assert_eq!(data["diff_unavailable"], json!("oversize"));
    assert_eq!(data["files"][0]["action"], json!("update"));
    assert_eq!(data["files"][0]["added_lines"], json!(0));
    assert!(data["files"][0]["diff_blocks"]
        .as_array()
        .expect("blocks")
        .is_empty());
}
#[test]
fn undo_and_redo_walk_the_history_instead_of_toggling() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    run(&ws, "create", &create_args("notes.txt", "A\n"));
    let target = ws.resolve_path("tester", "notes.txt").expect("path");
    let read = || std::fs::read_to_string(&target).expect("read");
    let step = |command: &str| {
        run(
            &ws,
            command,
            &json!({ "command": command, "file_path": "notes.txt" }),
        )
    };

    // A → A,B → A,B,C：每次写入前压栈，栈里是 A 与 A,B。
    run(
        &ws,
        "insert",
        &json!({ "command": "insert", "file_path": "notes.txt", "insert_line": 1, "new_string": "B" }),
    );
    run(
        &ws,
        "insert",
        &json!({ "command": "insert", "file_path": "notes.txt", "insert_line": 2, "new_string": "C" }),
    );
    assert_eq!(read(), "A\nB\nC\n");

    // 撤销是退游标，不是两版翻转：C → A,B → A。
    let first = step("undo_edit");
    assert_eq!(first["ok"], json!(true));
    assert_eq!(first["data"]["changed_files"], json!(1));
    assert_eq!(first["data"]["cursor"], json!(1));
    assert!(first["data"]["restored_from"]
        .as_str()
        .expect("snapshot")
        .starts_with("temp/.history/"));
    assert_eq!(read(), "A\nB\n");

    let second = step("undo_edit");
    assert_eq!(second["ok"], json!(true));
    assert_eq!(read(), "A\n");

    // 到栈底就停住，不再翻回上一态。
    let third = step("undo_edit");
    assert_eq!(third["ok"], json!(false));
    assert_eq!(third["error_meta"]["code"], json!("TOOL_SRE_NO_HISTORY"));
    assert_eq!(read(), "A\n");

    // 重做原路返回：A → A,B → A,B,C，再往后到顶停住。
    let redo = step("redo");
    assert_eq!(redo["ok"], json!(true));
    assert_eq!(redo["data"]["cursor"], json!(1));
    assert_eq!(read(), "A\nB\n");

    let redo = step("redo");
    assert_eq!(redo["data"]["cursor"], json!(2));
    assert_eq!(read(), "A\nB\nC\n");

    let done = step("redo");
    assert_eq!(done["ok"], json!(false));
    assert_eq!(done["error_meta"]["code"], json!("TOOL_SRE_NO_REDO"));
    assert_eq!(read(), "A\nB\nC\n");
}

#[test]
fn undo_edit_without_history_fails_cleanly() {
    let dir = tempdir().expect("tempdir");
    let ws = editor_workspace(dir.path());
    run(&ws, "create", &create_args("fresh.txt", "a\n"));
    let result = run(
        &ws,
        "undo_edit",
        &json!({ "command": "undo_edit", "file_path": "fresh.txt" }),
    );
    assert_eq!(result["ok"], json!(false));
    assert_eq!(result["error_meta"]["code"], json!("TOOL_SRE_NO_HISTORY"));
    assert_eq!(
        std::fs::read_to_string(ws.resolve_path("tester", "fresh.txt").expect("path"))
            .expect("read"),
        "a\n"
    );
}
