use super::*;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use wunder_server::approval::ApprovalRequestKind;

#[test]
fn wrapped_input_lines_wrap_by_viewport_width() {
    let lines = build_wrapped_input_lines("abcdef", 3);
    assert_eq!(lines.len(), 2);
    assert_eq!((lines[0].start, lines[0].end), (0, 3));
    assert_eq!((lines[1].start, lines[1].end), (3, 6));
}

#[test]
fn cursor_visual_position_prefers_next_wrapped_line_boundary() {
    let text = "abcdef";
    let lines = build_wrapped_input_lines(text, 3);
    assert_eq!(cursor_visual_position(text, &lines, 2), (0, 2));
    assert_eq!(cursor_visual_position(text, &lines, 3), (1, 0));
}

#[test]
fn wrapped_input_lines_keep_explicit_newlines() {
    let text = "a

b";
    let lines = build_wrapped_input_lines(text, 8);
    assert_eq!(lines.len(), 3);
    assert_eq!((lines[0].start, lines[0].end), (0, 1));
    assert_eq!((lines[1].start, lines[1].end), (2, 2));
    assert_eq!((lines[2].start, lines[2].end), (3, 4));
    assert_eq!(cursor_visual_position(text, &lines, 2), (1, 0));
}

#[test]
fn move_cursor_vertical_uses_wrapped_lines_without_newline() {
    let text = "abcdef";
    assert_eq!(move_cursor_vertical(text, 3, 4, -1), 1);
    assert_eq!(move_cursor_vertical(text, 3, 1, 1), 4);
}

#[test]
fn move_cursor_vertical_clamps_to_line_end() {
    let text = "ab
cdef";
    assert_eq!(move_cursor_vertical(text, 16, 5, -1), 2);
    assert_eq!(move_cursor_vertical(text, 16, 1, 1), 4);
}

#[test]
fn cursor_visual_position_handles_cjk_width() {
    let text = "\u{4f60}\u{597d}a";
    let lines = build_wrapped_input_lines(text, 8);
    let cursor_after_nihao = "\u{4f60}\u{597d}".len();
    assert_eq!(
        cursor_visual_position(text, &lines, cursor_after_nihao),
        (0, 4)
    );
    assert_eq!(cursor_visual_position(text, &lines, text.len()), (0, 5));
}

#[test]
fn wrapped_input_lines_wrap_cjk_without_splitting_char() {
    let text = "\u{4f60}\u{597d}ab";
    let lines = build_wrapped_input_lines(text, 4);
    assert_eq!(lines.len(), 2);
    assert_eq!(&text[lines[0].start..lines[0].end], "\u{4f60}\u{597d}");
    assert_eq!(&text[lines[1].start..lines[1].end], "ab");
}

#[test]
fn normalize_wrapped_cursor_position_wraps_boundary_columns() {
    assert_eq!(normalize_wrapped_cursor_position((2, 3), 4), (2, 3));
    assert_eq!(normalize_wrapped_cursor_position((2, 4), 4), (3, 0));
    assert_eq!(normalize_wrapped_cursor_position((2, 9), 4), (4, 1));
}

#[test]
fn wrapped_visual_line_count_tracks_wrap_and_newlines() {
    assert_eq!(wrapped_visual_line_count("", 8), 1);
    assert_eq!(wrapped_visual_line_count("abcdef", 3), 2);
    assert_eq!(wrapped_visual_line_count("ab\ncd", 8), 2);
    assert_eq!(wrapped_visual_line_count("\u{4f60}\u{597d}\u{5417}", 4), 2);
}

#[test]
fn transcript_window_tail_view_uses_bottom_entries() {
    let counts = vec![2, 2, 2, 2];
    let window = compute_transcript_window_spec(&counts, 3, 0);
    assert_eq!(window.total_lines, 8);
    assert_eq!(window.start_entry, 2);
    assert_eq!(window.end_entry_exclusive, 4);
    assert_eq!(window.local_scroll, 1);
}

#[test]
fn transcript_window_scrolled_up_returns_expected_slice() {
    let counts = vec![2, 2, 2, 2];
    let window = compute_transcript_window_spec(&counts, 3, 2);
    assert_eq!(window.start_entry, 1);
    assert_eq!(window.end_entry_exclusive, 4);
    assert_eq!(window.local_scroll, 1);
}

#[test]
fn transcript_window_limits_rendered_entries() {
    let counts = vec![1; 200];
    let window = compute_transcript_window_spec(&counts, 6, 95);
    assert_eq!(window.start_entry, 99);
    assert_eq!(window.end_entry_exclusive, 106);
    assert_eq!(window.local_scroll, 0);
    assert!(
        window
            .end_entry_exclusive
            .saturating_sub(window.start_entry)
            < counts.len()
    );
}

#[test]
fn transcript_window_supports_large_scroll_offsets() {
    let counts = vec![1; 90_000];
    let window = compute_transcript_window_spec(&counts, 20, 80_000);
    assert_eq!(window.start_entry, 9_980);
    assert_eq!(window.end_entry_exclusive, 10_001);
    assert_eq!(window.local_scroll, 0);
    assert_eq!(window.total_lines, 90_000);
}

#[test]
fn paste_shortcut_accepts_ctrl_v_and_shift_insert() {
    assert!(is_paste_shortcut(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL
    )));
    assert!(is_paste_shortcut(KeyEvent::new(
        KeyCode::Char('V'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT
    )));
    assert!(is_paste_shortcut(KeyEvent::new(
        KeyCode::Insert,
        KeyModifiers::SHIFT
    )));
}

#[test]
fn paste_shortcut_rejects_plain_or_alt_modified_v() {
    assert!(!is_paste_shortcut(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::NONE
    )));
    assert!(!is_paste_shortcut(KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::ALT | KeyModifiers::CONTROL
    )));
}

#[test]
fn mouse_mode_capture_policy_matches_codex_like_behavior() {
    assert!(!MouseMode::Auto.captures_mouse());
    assert!(MouseMode::Scroll.captures_mouse());
    assert!(!MouseMode::Select.captures_mouse());
}

#[test]
fn sanitize_assistant_text_strips_tool_markup_blocks() {
    let raw = "before <tool_call>{\"name\":\"读取文件\"}</tool_call> after";
    assert_eq!(sanitize_assistant_text(raw), "before  after");
}

#[test]
fn sanitize_assistant_delta_filters_tool_payload_fragments() {
    assert!(sanitize_assistant_delta("<tool_call>{").is_empty());
    assert!(sanitize_assistant_delta("{\"name\":\"读取文件\",\"arguments\":{}}").is_empty());
}

#[test]
fn sanitize_assistant_delta_streaming_strips_split_tool_call_block() {
    let mut in_tool_markup = false;
    let first = sanitize_assistant_delta_streaming(
        "<tool_call>{\"name\":\"final_reply\",\"arguments\":{\"content\":\"",
        &mut in_tool_markup,
    );
    assert!(first.is_empty());
    assert!(in_tool_markup);

    let second =
        sanitize_assistant_delta_streaming("hello\"}}</tool_call>hello world", &mut in_tool_markup);
    assert_eq!(second, "hello world");
    assert!(!in_tool_markup);
}

#[test]
fn merge_stream_text_reuses_snapshot_without_duplicate_append() {
    let mut output = "hello".to_string();
    merge_stream_text(&mut output, "hello world");
    assert_eq!(output, "hello world");

    merge_stream_text(&mut output, "world");
    assert_eq!(output, "hello world");
}

#[test]
fn merge_stream_text_appends_non_overlapping_delta_without_newline() {
    let mut output = "hello".to_string();
    merge_stream_text(&mut output, " ");
    merge_stream_text(&mut output, "world");
    assert_eq!(output, "hello world");
}

#[test]
fn equivalent_text_ignores_whitespace_differences() {
    assert!(is_equivalent_text("hello  world", "hello world"));
    assert!(is_equivalent_text("- run command", "-run command"));
}

#[test]
fn payload_has_tool_calls_accepts_non_empty_array() {
    let payload = serde_json::json!({ "tool_calls": [{ "name": "读取文件" }] });
    assert!(payload_has_tool_calls(&payload));
}

#[test]
fn format_execute_command_result_lines_prioritizes_failure_output() {
    let payload = serde_json::json!({
        "data": {
            "results": [{
                "command": "pip list",
                "returncode": 1,
                "stdout": "",
                "stderr": "pip is not recognized as a cmdlet
    at line:1 char:1"
            }]
        },
        "meta": {
            "duration_ms": 15,
            "exit_code": 1
        }
    });

    let lines = format_execute_command_result_lines("exec", &payload);
    assert!(!lines.is_empty());
    assert!(lines[0].contains("failed"));
    assert!(lines.iter().any(|line| line.starts_with("  stderr:")));
    assert!(!lines
        .iter()
        .any(|line| line.starts_with("  output: <empty>")));
}

#[test]
fn format_tool_result_lines_formats_apply_patch_changes_with_markers() {
    let payload = serde_json::json!({
        "result": {
            "ok": true,
            "data": {
                "changed_files": 3,
                "hunks_applied": 4,
                "files": [
                    { "action": "add", "path": "src/new_file.rs", "hunks": 1 },
                    { "action": "update", "path": "src/existing.rs", "hunks": 2 },
                    { "action": "delete", "path": "src/old_file.rs", "hunks": 1 }
                ]
            }
        }
    });

    let lines = format_tool_result_lines("应用补丁", &payload);
    assert!(!lines.is_empty());
    assert!(lines[0].contains("已修改 3 个文件"));
    assert!(lines[0].contains("4 hunks"));
    assert!(lines.iter().any(|line| line.contains("A src/new_file.rs")));
    assert!(lines.iter().any(|line| line.contains("M src/existing.rs")));
    assert!(lines.iter().any(|line| line.contains("D src/old_file.rs")));
}

#[test]
fn format_tool_result_lines_surfaces_apply_patch_error_code_and_hint() {
    let payload = serde_json::json!({
        "result": {
            "ok": false,
            "error": "Patch apply failed",
            "data": {
                "error_code": "PATCH_CONTEXT_NOT_FOUND",
                "hint": "Read latest file content and regenerate patch"
            }
        }
    });

    let lines = format_tool_result_lines("apply_patch", &payload);
    assert!(lines.iter().any(|line| line.contains("failed")));
    assert!(lines
        .iter()
        .any(|line| line.contains("PATCH_CONTEXT_NOT_FOUND")));
    assert!(lines.iter().any(|line| line.contains("regenerate patch")));
}

#[test]
fn append_text_preview_truncates_long_output() {
    let mut lines = Vec::new();
    let value = "line1\nline2\nline3\nline4\nline5\nline6\nline7\n";
    let has_output = append_text_preview(&mut lines, "stdout", value, 4, 64);
    assert!(has_output);
    assert!(lines.iter().any(|line| line.contains("more lines")));
}

#[test]
fn compact_json_handles_multibyte_truncation() {
    let value = serde_json::json!({ "message": "你".repeat(400) });
    let output = compact_json(&value);
    assert!(output.ends_with("..."));
}

#[test]
fn backtrack_user_text_returns_trimmed_user_content() {
    let entry = LogEntry {
        kind: LogKind::User,
        text: "  hello world  ".to_string(),
        special: None,
        markdown_cache: None,
    };
    assert_eq!(backtrack_user_text(&entry), Some("hello world".to_string()));
}

#[test]
fn backtrack_user_text_ignores_non_user_or_empty() {
    let assistant_entry = LogEntry {
        kind: LogKind::Assistant,
        text: "hello".to_string(),
        special: None,
        markdown_cache: None,
    };
    assert_eq!(backtrack_user_text(&assistant_entry), None);

    let empty_user_entry = LogEntry {
        kind: LogKind::User,
        text: "   ".to_string(),
        special: None,
        markdown_cache: None,
    };
    assert_eq!(backtrack_user_text(&empty_user_entry), None);
}

#[test]
fn collect_recent_user_logs_returns_latest_first() {
    let logs = vec![
        LogEntry {
            kind: LogKind::User,
            text: "first".to_string(),
            special: None,
            markdown_cache: None,
        },
        LogEntry {
            kind: LogKind::Assistant,
            text: "reply".to_string(),
            special: None,
            markdown_cache: None,
        },
        LogEntry {
            kind: LogKind::User,
            text: "second".to_string(),
            special: None,
            markdown_cache: None,
        },
    ];
    assert_eq!(
        collect_recent_user_logs(&logs, 5),
        vec!["second".to_string(), "first".to_string()]
    );
}

#[test]
fn backtrack_preview_line_truncates() {
    let preview = backtrack_preview_line("abcdefghij", 5);
    assert_eq!(preview, "abcde...");
    assert_eq!(backtrack_preview_line("abcd", 8), "abcd");
}

#[test]
fn should_store_history_entry_skips_slash_commands() {
    assert!(should_store_history_entry("hello world"));
    assert!(should_store_history_entry("  你好"));
    assert!(!should_store_history_entry("   /help"));
    assert!(!should_store_history_entry("/attach image.png"));
    assert!(!should_store_history_entry("   "));
}

#[test]
fn detect_pasted_attachment_paths_recognizes_quoted_local_files() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base_dir = std::env::temp_dir().join(format!("wunder-cli-paste-{unique}"));
    fs::create_dir_all(&base_dir).unwrap();

    let first = base_dir.join("截图 one.png");
    let second = base_dir.join("第二张图.jpg");
    fs::write(&first, b"fake-image-a").unwrap();
    fs::write(&second, b"fake-image-b").unwrap();

    let pasted = format!("\"{}\" \"{}\"", first.display(), second.display());
    let detected = detect_pasted_attachment_paths(base_dir.as_path(), pasted.as_str()).unwrap();

    assert_eq!(
        detected,
        vec![
            first.to_string_lossy().to_string(),
            second.to_string_lossy().to_string(),
        ]
    );

    fs::remove_file(first).unwrap();
    fs::remove_file(second).unwrap();
    fs::remove_dir_all(base_dir).unwrap();
}

#[test]
fn detect_pasted_attachment_paths_leaves_plain_text_alone() {
    assert_eq!(
        detect_pasted_attachment_paths(Path::new("."), "hello codex-like world"),
        None
    );
}

#[test]
fn format_compose_attachment_hint_handles_single_and_multiple_items() {
    assert_eq!(format_compose_attachment_hint(&[], true), None);
    assert_eq!(
        format_compose_attachment_hint(&["diagram.png".to_string()], true),
        Some("已附加: diagram.png".to_string())
    );
    assert_eq!(
        format_compose_attachment_hint(&["diagram.png".to_string(), "notes.md".to_string()], false,),
        Some("attached 2 · diagram.png +1".to_string())
    );
}

#[test]
fn format_apply_patch_approval_lines_show_real_diff_preview() {
    let args = serde_json::json!({
        "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"
    });
    let lines = format_apply_patch_approval_lines(&args, false).expect("approval lines");
    assert_eq!(lines[0], "Patch preview: files=1, +1, -1");
    assert!(lines.iter().any(|line| line.trim() == "diff src/main.rs"));
    assert!(lines.iter().any(|line| line.trim() == "@@"));
    assert!(lines.iter().any(|line| line.trim() == "- old"));
    assert!(lines.iter().any(|line| line.trim() == "+ new"));
}

#[test]
fn approval_prompt_text_matches_request_kind() {
    let (tx, _rx) = tokio::sync::oneshot::channel();
    let request = ApprovalRequest {
        id: "req-1".to_string(),
        kind: ApprovalRequestKind::Patch,
        tool: "apply_patch".to_string(),
        args: serde_json::json!({}),
        summary: "edit files".to_string(),
        detail: serde_json::json!({}),
        respond_to: tx,
    };
    assert_eq!(
        approval_prompt_text(&request, false),
        "Would you like to make the following edits?"
    );
    assert_eq!(
        approval_prompt_text(&request, true),
        "是否允许应用以下修改？"
    );
}

#[test]
fn approval_option_labels_match_exec_request_kind() {
    let (tx, _rx) = tokio::sync::oneshot::channel();
    let request = ApprovalRequest {
        id: "req-2".to_string(),
        kind: ApprovalRequestKind::Exec,
        tool: "execute_command".to_string(),
        args: serde_json::json!({}),
        summary: "run command".to_string(),
        detail: serde_json::json!({}),
        respond_to: tx,
    };
    assert_eq!(
        approval_option_labels(&request, false),
        [
            "Yes, run it once".to_string(),
            "Yes, allow it for this session".to_string(),
            "No, and tell Wunder what to do differently".to_string(),
        ]
    );
}
#[test]
fn normalize_statusline_item_supports_cwd_aliases() {
    assert_eq!(normalize_statusline_item("cwd").as_deref(), Some("cwd"));
    assert_eq!(normalize_statusline_item("dir").as_deref(), Some("cwd"));
    assert_eq!(
        normalize_statusline_item("workspace").as_deref(),
        Some("cwd")
    );
}
#[test]
fn normalize_statusline_item_supports_project_and_branch_aliases() {
    assert_eq!(
        normalize_statusline_item("project").as_deref(),
        Some("project")
    );
    assert_eq!(
        normalize_statusline_item("repo").as_deref(),
        Some("project")
    );
    assert_eq!(
        normalize_statusline_item("branch").as_deref(),
        Some("branch")
    );
    assert_eq!(normalize_statusline_item("git").as_deref(), Some("branch"));
}
