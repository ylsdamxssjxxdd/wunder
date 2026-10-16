//! Frame-level snapshots: the real `ui::draw` composed over a real `TuiApp`.
//!
//! The fixture builds `CliRuntime` directly instead of calling `CliRuntime::init`, because
//! that helper writes about a dozen process-global `WUNDER_*` environment variables which
//! would leak into every other test in this binary. Storage, sessions and the workspace all
//! point at a temp directory, and the session id is fixed.

use super::*;
use crate::args::Cli;
use crate::runtime::CliWorkspace;
use crate::tui::frame_scheduler::spawn_frame_scheduler;
use clap::Parser;
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use wunder_server::config::Config;
use wunder_server::config_store::ConfigStore;
use wunder_server::state::{AppState, AppStateInitOptions};

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let mut root = std::env::temp_dir();
        root.push(format!(
            "wunder_cli_frame_{tag}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(root.join("sessions").as_path()).expect("create sessions dir");
        fs::create_dir_all(root.join("workspace").as_path()).expect("create workspace dir");
        Self(root)
    }

    fn path(&self) -> &Path {
        self.0.as_path()
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.as_path());
    }
}

fn fixture_runtime(root: &Path) -> CliRuntime {
    let mut config = Config::default();
    config.server.mode = "cli".to_string();
    config.storage.backend = "sqlite".to_string();
    config.storage.db_path = root.join("snapshot.sqlite3").to_string_lossy().to_string();
    config.workspace.root = root.join("workspace").to_string_lossy().to_string();
    config.channels.enabled = false;
    config.gateway.enabled = false;
    config.agent_queue.enabled = false;
    config.cron.enabled = false;
    config.security.approval_mode = Some("suggest".to_string());

    // The store reads its file on construction, so the fixture writes what it just built;
    // the alternative would be a config error logged into every snapshot run.
    fs::write(
        root.join("config.yaml"),
        serde_yaml::to_string(&config).expect("serialize fixture config"),
    )
    .expect("write fixture config");
    let config_store = ConfigStore::new(root.join("config.yaml"));
    let state =
        AppState::new_with_options(config_store, config, AppStateInitOptions::cli_default())
            .expect("fixture state");
    let workspace_root = root.join("workspace");
    let workspace = CliWorkspace {
        workspace_id: "ws_snapshot".to_string(),
        root_path: workspace_root.clone(),
        name: "snapshot".to_string(),
    };
    state.workspace.register_workspace_root(
        workspace.workspace_id.as_str(),
        workspace_root.to_string_lossy().as_ref(),
    );
    CliRuntime {
        state: Arc::new(state),
        launch_dir: root.to_path_buf(),
        temp_root: root.to_path_buf(),
        wunder_home: root.to_path_buf(),
        repo_root: root.to_path_buf(),
        user_id: "snapshot_user".to_string(),
        workspace,
        user_config: Default::default(),
    }
}

/// A fictional but representative transcript: a user turn, a long reasoning block, a
/// finished command card, a patch card with a real hunk, and the final answer.
async fn fixture_app(root: &Path) -> TuiApp {
    let global = Cli::parse_from([
        "wunder-cli",
        "--lang",
        "en-US",
        "--user",
        "snapshot_user",
        "--temp-root",
        root.to_string_lossy().as_ref(),
    ])
    .global;
    let runtime = fixture_runtime(root);
    let (requester, _notifications) = spawn_frame_scheduler();
    let mut app = TuiApp::new(
        runtime,
        global,
        Some("snapshot-thread".to_string()),
        requester,
    )
    .await
    .expect("fixture app");
    // Production lets these land after the first frame; a snapshot needs them now.
    app.complete_startup_fills().await;

    app.push_log(LogKind::User, "summarise the flaky test".to_string());
    let reasoning = (1..=12)
        .map(|n| format!("reasoning line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.push_log(LogKind::Reasoning, reasoning);

    let command = build_completed_command_log(
        &json!({
            "result": {
                "ok": true,
                "data": {
                    "results": [{
                        "command": "cargo test -p demo",
                        "returncode": 0,
                        "stdout": "running 3 tests\ntest alpha ... ok\ntest beta ... ok\ntest gamma ... ok\ntest result: ok. 3 passed\n"
                    }]
                },
                "meta": { "duration_ms": 812 }
            }
        }),
        false,
    )
    .expect("command card");
    app.push_special_log(LogKind::Tool, command.summary_text(), command);

    let patch = build_pending_patch_log(
        &json!({
            "input": "*** Begin Patch\n*** Update File: src/demo.rs\n@@ -3,3 +3,3 @@\n let gate = 1;\n-let flaky = true;\n+let flaky = false;\n*** End Patch"
        }),
        false,
    )
    .expect("patch card");
    app.push_special_log(LogKind::Tool, patch.summary_text(), patch);

    app.push_log(
        LogKind::Assistant,
        "Three tests pass; the flaky gate is now disabled.".to_string(),
    );
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::CONTROL)
}

#[tokio::test]
async fn ctrl_r_searches_the_composer_history_and_escape_restores_the_draft() {
    let root = TempRoot::new("history_search");
    let mut app = fixture_app(root.path()).await;
    app.history = vec![
        "cargo test -p alpha".to_string(),
        "explain the flaky gate".to_string(),
        "cargo test -p beta".to_string(),
    ];
    app.input = "draft in progress".to_string();
    app.input_cursor = app.input.len();

    // Ctrl+R starts a reverse search on the newest entry.
    app.on_key(ctrl(KeyCode::Char('r'))).await.expect("ctrl+r");
    assert_eq!(app.input, "cargo test -p beta");
    assert!(app.history_search.is_some());

    // Typing narrows by substring; the preview follows the query.
    app.on_key(key(KeyCode::Char('a'))).await.expect("type a");
    app.on_key(key(KeyCode::Char('l'))).await.expect("type l");
    app.on_key(key(KeyCode::Char('p'))).await.expect("type p");
    assert_eq!(app.input, "cargo test -p alpha");

    // Stepping in reverse keeps looking older; there is nothing older to find.
    app.on_key(ctrl(KeyCode::Char('r'))).await.expect("step");
    assert_eq!(app.input, "cargo test -p alpha");

    // Esc restores whatever was being typed before the search.
    app.on_key(key(KeyCode::Esc)).await.expect("cancel");
    assert!(app.history_search.is_none());
    assert_eq!(app.input, "draft in progress");
}

#[tokio::test]
async fn ctrl_r_accepts_the_match_with_enter() {
    let root = TempRoot::new("history_accept");
    let mut app = fixture_app(root.path()).await;
    app.history = vec!["first command".to_string(), "second command".to_string()];
    app.input.clear();
    app.input_cursor = 0;

    app.on_key(ctrl(KeyCode::Char('r'))).await.expect("ctrl+r");
    app.on_key(key(KeyCode::Char('f'))).await.expect("type f");
    app.on_key(key(KeyCode::Enter)).await.expect("accept");

    assert!(app.history_search.is_none());
    assert_eq!(app.input, "first command");
}

#[tokio::test]
async fn the_exit_summary_names_a_stopped_round_and_the_resume_command() {
    let root = TempRoot::new("exit_summary");
    let mut app = fixture_app(root.path()).await;

    // An idle session still tells the shell how to come back to this thread.
    let idle = app.exit_summary();
    assert_eq!(idle.len(), 1, "{idle:?}");
    assert!(idle[0].starts_with("wunder-cli resume "), "{idle:?}");
    assert!(idle[0].ends_with(&app.session_id), "{idle:?}");

    // A running round that the user asked to stop is named as stopped.
    app.active_stream_sessions.insert(app.session_id.clone());
    app.turn_was_interrupted = true;
    let stopped = app.exit_summary();
    assert!(stopped[0].contains("stopped"), "{stopped:?}");
    assert!(stopped[1].starts_with("wunder-cli resume "), "{stopped:?}");
}

fn draw_rows(app: &mut TuiApp, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|row| {
            (0..width)
                .map(|column| buffer[(column, row)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn the_frame_composes_every_card_at_forty_eighty_and_one_hundred_twenty_columns() {
    let root = TempRoot::new("widths");
    for width in [40u16, 80, 120] {
        let mut app = fixture_app(root.path()).await;
        let rows = draw_rows(&mut app, width, 24);
        let frame = rows.join("\n");
        assert!(
            rows.iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no row may exceed the frame at {width} columns"
        );
        // The tail is what a narrow frame can always show: the newest cards and the
        // composer. Older entries scroll out, which is the transcript's job.
        assert!(frame.contains("Ran cargo test"), "command head: {frame}");
        assert!(
            frame.contains("test result: ok"),
            "command output tail: {frame}"
        );
        assert!(frame.contains("src/demo.rs"), "patch head: {frame}");
        assert!(
            frame.contains("Three tests pass"),
            "the answer is on screen: {frame}"
        );
        assert!(
            frame.contains("Ask directly"),
            "the composer keeps its placeholder: {frame}"
        );
        assert!(
            frame.contains('?'),
            "footer keeps the shortcut key: {frame}"
        );
    }

    // Only a frame wide enough to hold the whole transcript can be checked for what the
    // folds hide and what the head entries look like.
    let mut app = fixture_app(root.path()).await;
    let frame = draw_rows(&mut app, 120, 40).join("\n");
    assert!(
        frame.contains("▌ summarise"),
        "user row keeps its bar: {frame}"
    );
    assert!(
        frame.contains("+ Show thinking (enter)"),
        "reasoning folds with a key hint: {frame}"
    );
    assert!(
        !frame.contains("reasoning line 12"),
        "a folded block must not leak its tail: {frame}"
    );
    assert!(
        !frame.contains("reasoning line 5"),
        "and keeps only the head: {frame}"
    );
}

#[tokio::test]
async fn the_working_line_and_the_transcript_view_reach_the_real_frame() {
    let root = TempRoot::new("activity");
    let mut app = fixture_app(root.path()).await;

    app.busy = true;
    let frame = draw_rows(&mut app, 80, 24).join("\n");
    assert!(
        frame.contains("Working"),
        "the activity row draws for any running turn: {frame}"
    );

    // The Ctrl+T overlay is a modal reader over the same entries: what the folded frame
    // hid must appear, and the folded hint must be gone.
    let reasoning_index = app
        .logs
        .iter()
        .position(|entry| entry.kind == LogKind::Reasoning)
        .expect("reasoning entry");
    assert!(
        app.handle_transcript_view_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
    );
    let opened = draw_rows(&mut app, 80, 24).join("\n");
    assert!(opened.contains("reasoning line 12"), "{opened}");
    assert!(
        opened.contains("transcript "),
        "the view states its own extent: {opened}"
    );

    // Closing returns the frame to the folded measurement; the cache invalidation is the
    // part that used to be forgotten.
    assert!(app.handle_transcript_view_key(KeyCode::Esc.into()));
    assert!(!app.transcript_view_open());
    let closed = draw_rows(&mut app, 80, 24).join("\n");
    assert!(!closed.contains("reasoning line 12"), "{closed}");

    // Enter on a selected card opens just that card.
    app.transcript_selected = Some(reasoning_index);
    assert!(app.toggle_selected_card());
    let expanded = draw_rows(&mut app, 80, 24).join("\n");
    assert!(expanded.contains("reasoning line 12"), "{expanded}");
    assert!(
        expanded.contains("Ran cargo test"),
        "the rest of the transcript stays put"
    );
}

fn feed_stream_event(app: &mut TuiApp, event: &str, mut data: Value, seq: i64) {
    data.as_object_mut()
        .expect("object payload")
        .insert("event_id".to_string(), json!(seq));
    app.apply_stream_event(StreamEvent {
        event: event.to_string(),
        data,
        id: None,
        timestamp: None,
    });
}

fn plain_lines(app: &mut TuiApp, index: usize, width: u16) -> String {
    app.render_entry_lines(index, false, width)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The event path, not just the card model: reads join the group at the tail, anything
/// else closes it, and a result still finds its own child after the group stopped being
/// the newest entry.
#[tokio::test]
async fn a_run_of_workspace_reads_shares_one_exploring_card() {
    let root = TempRoot::new("exploring");
    let mut app = fixture_app(root.path()).await;

    feed_stream_event(
        &mut app,
        "tool_call",
        json!({"tool": "read_file", "args": {"path": "src/alpha.rs"}, "tool_call_id": "call-1", "turn_id": "turn-1"}),
        101,
    );
    // The first tool call of a turn also leaves a phase notice behind, so the card indexes
    // are measured from what the transcript holds after the first read rather than from the
    // fixture length.
    let group_index = app.logs.len() - 1;
    let base = app.logs.len();

    feed_stream_event(
        &mut app,
        "tool_call",
        json!({"tool": "list_files", "args": {"path": "src"}, "tool_call_id": "call-2", "turn_id": "turn-1"}),
        102,
    );
    assert_eq!(app.logs.len(), base, "two reads share one card");

    feed_stream_event(
        &mut app,
        "tool_result",
        json!({"tool": "list_files", "tool_call_id": "call-2", "turn_id": "turn-1",
               "result": {"ok": true, "data": {"entries": ["a.rs", "b.rs"]}}}),
        103,
    );
    feed_stream_event(
        &mut app,
        "tool_call",
        json!({"tool": "write_file", "args": {"path": "src/gamma.rs"}, "tool_call_id": "call-3", "turn_id": "turn-1"}),
        104,
    );
    assert_eq!(app.logs.len(), base + 1, "a write is not a read");
    assert!(
        plain_lines(&mut app, base, 80).contains("src/gamma.rs"),
        "the write keeps its own card"
    );

    feed_stream_event(
        &mut app,
        "tool_result",
        json!({"tool": "read_file", "tool_call_id": "call-1", "turn_id": "turn-1",
               "result": {"ok": false, "error": "file is too large to read"}}),
        105,
    );

    let group = plain_lines(&mut app, group_index, 80);
    assert!(
        group.contains("Explored · 1 failed"),
        "the header names how many calls broke: {group}"
    );
    assert!(
        group.contains("Read src/alpha.rs failed · file is too large to read"),
        "the result lands on its own child: {group}"
    );
    assert!(group.contains("List src"), "{group}");
    assert!(
        !plain_lines(&mut app, base, 80).contains("too large"),
        "and never on its sibling"
    );

    let frame = draw_rows(&mut app, 80, 40).join("\n");
    assert!(
        frame.contains("Explored · 1 failed"),
        "the counted header reaches the frame: {frame}"
    );
    assert!(frame.contains("└ Read"), "{frame}");
}
#[tokio::test]
async fn the_warning_notice_and_its_panel_reach_the_frame() {
    let root = TempRoot::new("notices");
    let mut app = fixture_app(root.path()).await;

    let frame = draw_rows(&mut app, 80, 24).join("\n");
    assert!(!frame.contains('\u{26a0}'), "no signal, no bar: {frame}");

    app.note_session_warning("approval denied: write src/demo.rs");
    app.note_session_warning("approval denied: write src/demo.rs");
    let frame = draw_rows(&mut app, 80, 24).join("\n");
    assert!(
        frame.contains("\u{26a0} 2 warnings \u{b7} f2 to view"),
        "the notice states the count and the key: {frame}"
    );

    // Narrow frames keep the number and lose the sentence, in that order.
    let narrow = draw_rows(&mut app, 20, 24).join("\n");
    assert!(narrow.contains("\u{26a0}2"), "{narrow}");
    assert!(!narrow.contains("f2"), "{narrow}");

    // The real key path opens and closes the list, and the notice stays as the entry point.
    app.on_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE))
        .await
        .expect("f2 opens the panel");
    assert!(app.warning_panel_open());
    let panel = draw_rows(&mut app, 80, 24).join("\n");
    assert!(
        panel.contains("approval denied: write src/demo.rs \u{d7}2"),
        "{panel}"
    );
    assert!(panel.contains("f2 or esc to close"), "{panel}");

    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc closes the panel");
    assert!(!app.warning_panel_open());
    let closed = draw_rows(&mut app, 80, 24).join("\n");
    assert!(!closed.contains("\u{d7}2"), "{closed}");
    assert!(closed.contains("\u{26a0} 2 warnings"), "{closed}");

    // A notice costs a row inside the composer band; a short screen must still keep the
    // input and the footer beside it.
    let short = draw_rows(&mut app, 32, 12);
    assert_eq!(short.len(), 12);
    assert!(
        short.iter().any(|row| row.contains('\u{26a0}')),
        "{short:?}"
    );
    assert!(
        short.iter().any(|row| row.contains('\u{203a}')),
        "the composer prompt survives: {short:?}"
    );
}

#[tokio::test]
async fn the_footer_names_the_task_and_sheds_it_before_the_keys() {
    let root = TempRoot::new("footer_title");
    let mut app = fixture_app(root.path()).await;

    let items = app.composer_footer_items();
    assert!(
        items
            .iter()
            .any(|(key, label)| key.is_empty() && label.contains("summarise the flaky test")),
        "the first user turn titles the thread: {items:?}"
    );

    let frame = draw_rows(&mut app, 40, 24).join("\n");
    assert!(frame.contains('\u{2190}'), "the keys stay: {frame}");
    assert!(frame.contains('?'), "the keys stay: {frame}");
    assert!(
        !frame.contains("summarise the flaky test"),
        "and the description gives way first: {frame}"
    );

    let wide = draw_rows(&mut app, 120, 24).join("\n");
    assert!(wide.contains("summarise the flaky test"), "{wide}");
}

fn draw_buffer(app: &mut TuiApp, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, app))
        .expect("draw");
    terminal.backend().buffer().clone()
}

/// The composer is the one editable region: codex paints its band and keeps the
/// prompt glyph bold in both focus states, so the eye lands there without reading.
#[tokio::test]
async fn the_composer_band_is_filled_and_the_prompt_keeps_its_weight() {
    use ratatui::style::Modifier;

    let root = TempRoot::new("composer_fill");
    let mut app = fixture_app(root.path()).await;
    let width_for_fill = 90u16;
    let buffer = draw_buffer(&mut app, width_for_fill, 24);

    let mut prompt_cell = None;
    for (index, cell) in buffer.content().iter().enumerate() {
        if cell.symbol() == "›" {
            prompt_cell = Some(index);
            assert!(
                cell.style().add_modifier.contains(Modifier::BOLD),
                "the prompt glyph keeps its weight"
            );
        }
    }
    let prompt_index = prompt_cell.expect("one prompt glyph on screen");
    // The band, not just the glyph: the whole composer row shares the surface tone.
    // Compared as one value so the assertion also holds on a NO_COLOR terminal, where
    // the theme degrades every hue to Reset.
    let prompt_row = prompt_index / usize::from(width_for_fill);
    let row_start = prompt_row * usize::from(width_for_fill);
    let band = buffer.content()[row_start..row_start + usize::from(width_for_fill)]
        .iter()
        .map(|cell| cell.style().bg)
        .collect::<Vec<_>>();
    assert_eq!(
        band[prompt_index - row_start],
        buffer.content()[prompt_index].style().bg,
        "the prompt sits on the same surface as the row"
    );
    assert!(
        band.windows(2).all(|pair| pair[0] == pair[1]),
        "the composer row is one filled band: {:?}",
        band.iter().flatten().next()
    );
    assert!(
        band[0].is_some(),
        "and the band is a surface, not an unpainted row of defaults"
    );
}

/// codex 用 Esc 中断当前轮次。舵机照做，但将「退出」仍只留给 Ctrl+C：按 Esc 永远不会
/// 关掉会话，也不会武装退出窗口。
#[tokio::test]
async fn esc_interrupts_the_running_round_and_never_closes_the_session() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let root = TempRoot::new("esc_interrupt");
    let mut app = fixture_app(root.path()).await;
    let session = app.session_id.clone();
    // The visible thread is mid-round, but no cancellable round exists in the monitor,
    // which is the honest local case: the notice must say so instead of going silent.
    app.busy = true;
    app.active_stream_sessions.insert(session.clone());

    let logs_before = app.logs.len();
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc is handled");
    assert_eq!(app.logs.len(), logs_before + 1, "the key answers back");
    assert!(
        app.logs[logs_before]
            .text
            .contains("no cancellable round found; the thread is finishing on its own"),
        "{}",
        app.logs[logs_before].text
    );
    assert!(!app.should_quit(), "esc never closes the session");
    assert!(
        app.ctrl_c_hint_deadline.is_none(),
        "esc never arms the exit window"
    );
    assert!(
        app.busy,
        "and the round keeps running until the engine stops it"
    );

    // The working line states the key it actually listens to, on screen.
    let frame = draw_rows(&mut app, 80, 24).join("\n");
    assert!(frame.contains("esc to interrupt"), "{frame}");
    assert!(!frame.contains("ctrl+c to interrupt"), "{frame}");

    // With nothing running, Esc goes back to clearing the draft.
    app.active_stream_sessions.remove(&session);
    app.busy = false;
    app.input = "half a thought".to_string();
    app.input_cursor = app.input.len();
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
        .await
        .expect("esc clears the draft");
    assert!(app.input.is_empty(), "esc yields the draft back when idle");

    // And the shortcuts panel names both keys honestly.
    let lines = app.shortcuts_lines().join("\n");
    assert!(lines.contains("esc to interrupt"), "{lines}");
    assert!(lines.contains("ctrl + c to exit"), "{lines}");
}

/// §11.2 呈现模板基线：一次帧里同时出现成功命令卡（头部 + `└` 尾行 + `…` 折叠 + 披露）、
/// 失败命令卡（`Failed (exit N)` 内联）、编辑卡（`(+n -m)` + 行号槽 + hunk 分隔）、运行行、
/// composer 与 footer。样张里的每一行都必须能在真实帧里找到，否则 §11 只是逐条自说自话。
#[tokio::test]
async fn the_acceptance_sample_composes_the_documented_template() {
    let root = TempRoot::new("template");
    let mut app = fixture_app(root.path()).await;
    app.logs.clear();
    app.invalidate_transcript_metrics();

    let long_stdout = (1..=200)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let ran = build_completed_command_log(
        &json!({"result": {
            "ok": true,
            "data": {"results": [{
                "command": "cargo test --workspace",
                "returncode": 0,
                "stdout": format!("{long_stdout}\ntest result: ok. 176 passed")
            }]},
            "meta": {"duration_ms": 1_200}
        }}),
        false,
    )
    .expect("success command card");
    app.push_special_log(LogKind::Tool, ran.summary_text(), ran);

    let failed = build_completed_command_log(
        &json!({"result": {
            "ok": false,
            "data": {"results": [{
                "command": "npx playwright test --grep flaky",
                "returncode": 1,
                "stdout": "1 failed\nexpected true to be false"
            }]},
            "meta": {"duration_ms": 640}
        }}),
        false,
    )
    .expect("failed command card");
    app.push_special_log(LogKind::Tool, failed.summary_text(), failed);

    let hunks = (1..=12)
        .map(|index| format!("+  let added_{index} = true;"))
        .collect::<Vec<_>>()
        .join("\n");
    let removed = (1..=3)
        .map(|index| format!("-  let removed_{index} = false;"))
        .collect::<Vec<_>>()
        .join("\n");
    let patch = build_pending_patch_log(
        &json!({"input": format!(
            "*** Begin Patch\n*** Update File: src/main.rs\n@@ -10,6 +10,15 @@\n keep\n{removed}\n{hunks}\n@@ -40,2 +49,2 @@\n keep tail\n*** End Patch"
        )}),
        false,
    )
    .expect("pending patch card");
    // The applied line counts come from the tool result, and the completed card keeps
    // the diff the pending card already showed.
    let mut edited = build_completed_patch_log(
        &json!({"result": {"ok": true, "data": {
            "added_lines": 12,
            "deleted_lines": 3,
            "changed_files": 1,
            "hunks_applied": 2,
            "files": [{"action": "update", "path": "src/main.rs", "hunks": 2,
                       "added_lines": 12, "deleted_lines": 3}]
        }}}),
        false,
    );
    edited.inherit_patch_preview_from(&patch);
    app.push_special_log(LogKind::Tool, edited.summary_text(), edited);

    app.push_log(
        LogKind::Assistant,
        "The flaky gate is now disabled.".to_string(),
    );
    app.busy = true;
    app.active_stream_sessions.insert(app.session_id.clone());

    let rows = draw_rows(&mut app, 80, 60);
    let frame = rows.join("\n");
    for expected in [
        "Ran cargo test --workspace",
        "└ ",
        "lines",
        "lines (enter to expand)",
        "Failed (exit 1)",
        "Edited src/main.rs (+12 -3 2 hunks)",
        "esc to interrupt",
        "›",
    ] {
        assert!(frame.contains(expected), "missing {expected:?}:\n{frame}");
    }

    // The hunk gutter is what makes a diff readable without color: the row number sits
    // in the gutter and the sign follows it, so both columns stay aligned per row.
    assert!(
        frame.contains("11 - ") && frame.contains("11 + "),
        "numbered rows keep the gutter before the sign: {frame}"
    );
    assert!(
        frame.contains("10   keep"),
        "a context row keeps the sign column empty instead of collapsing it: {frame}"
    );
    assert!(
        frame.contains('+') && frame.contains('-'),
        "diff markers survive: {frame}"
    );
    // The two fold marks stay distinct: `…` is output the stored window lost, the counted
    // disclosure row is what Enter brings back.
    assert!(
        frame.contains('\u{2026}'),
        "hidden output uses the sample's ellipsis: {frame}"
    );
    assert!(
        frame.contains("lines (enter to expand)"),
        "the diff's fold is counted on the row that opens it: {frame}"
    );

    // Same sample at a narrow frame: nothing may exceed the columns it was given, and the
    // fold disclosure stays reachable.
    let narrow_width = 40u16;
    let narrow = draw_rows(&mut app, narrow_width, 60);
    assert!(
        narrow
            .iter()
            .all(|row| row.chars().count() <= usize::from(narrow_width)),
        "narrow frame overflow: {narrow:?}"
    );
    let narrow_text = narrow.join("\n");
    assert!(narrow_text.contains("Failed (exit 1)"), "{narrow_text}");
    assert!(
        narrow_text.contains("Edited src/main.rs"),
        "the edited file keeps its name when the summary is dropped: {narrow_text}"
    );
    assert!(narrow_text.contains("esc to interrupt"), "{narrow_text}");
}

/// The number column belongs to the tinted row: codex paints the whole diff line, so a
/// dim-only gutter would read as a stripe-shaped hole in the middle of the tint. The
/// assertion compares styles to each other instead of to a hue, so it also holds on a
/// NO_COLOR terminal where every hue degrades to Reset.
#[tokio::test]
async fn a_diff_row_keeps_its_tint_across_the_number_column() {
    let root = TempRoot::new("diff_tint");
    let mut app = fixture_app(root.path()).await;
    app.logs.clear();
    app.invalidate_transcript_metrics();

    let pending = build_pending_patch_log(
        &json!({"input": "*** Begin Patch\n*** Update File: src/main.rs\n@@ -10,6 +10,9 @@\n keep\n-gone\n-also\n-third\n+here\n+two\n+three\n*** End Patch"}),
        false,
    )
    .expect("pending patch card");
    let mut completed = build_completed_patch_log(
        &json!({"result": {"ok": true, "data": {
            "added_lines": 3, "deleted_lines": 3, "changed_files": 1, "hunks_applied": 1,
            "files": [{"action": "update", "path": "src/main.rs", "hunks": 1,
                       "added_lines": 3, "deleted_lines": 3}]
        }}}),
        false,
    );
    completed.inherit_patch_preview_from(&pending);
    app.push_special_log(LogKind::Tool, completed.summary_text(), completed);

    let width = usize::from(80u16);
    let buffer = draw_buffer(&mut app, 80, 30);
    let mut tinted_rows = 0usize;
    for y in 0..buffer.area.height as usize {
        let row = &buffer.content()[y * width..(y + 1) * width];
        let Some(sign) = row
            .iter()
            .position(|cell| matches!(cell.symbol(), "+" | "-") && cell.style().bg.is_some())
        else {
            continue;
        };
        if row[0].style().bg.is_none() {
            continue;
        }
        tinted_rows += 1;
        let sign_bg = row[sign].style().bg;
        assert!(
            row[..=sign].iter().all(|cell| cell.style().bg == sign_bg),
            "row {y} loses the tint between the prefix and the sign: {:?}",
            row[..=sign]
                .iter()
                .map(|cell| (cell.symbol(), cell.style().bg))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        tinted_rows >= 2,
        "both an added and a deleted row should carry the tint, found {tinted_rows}"
    );
}

/// §7 的快照矩阵要求 command center 的宽/窄布局与空态都落在真帧上：120 列是列表 + 分隔线
/// + 详情双栏，60 列退回单栏，24 列直接拒绝而不是挤成一团；没有线程时给的是空态而不是报错。
#[tokio::test]
async fn the_command_center_lays_out_for_every_sampled_terminal_width() {
    let root = TempRoot::new("command_center");
    let mut app = fixture_app(root.path()).await;
    let first = app.session_id.clone();
    crate::ensure_cli_session_record(&app.runtime, first.as_str(), Some("first observation"))
        .await
        .expect("seed first thread");
    let second = format!("{first}b");
    crate::ensure_cli_session_record(&app.runtime, second.as_str(), Some("second observation"))
        .await
        .expect("seed second thread");

    app.open_command_center()
        .await
        .expect("open command center");
    assert!(app.command_center_open());

    let wide = draw_rows(&mut app, 120, 30);
    assert!(
        wide.iter().all(|row| row.chars().count() <= 120),
        "wide overflow: {wide:?}"
    );
    let wide_text = wide.join("\n");
    assert!(
        wide_text.contains("Threads") || wide_text.contains("任务中心"),
        "{wide_text}"
    );
    assert!(
        wide_text.contains('│'),
        "the wide frame keeps its two-column divider: {wide_text}"
    );
    assert!(
        wide_text.contains("Task details") || wide_text.contains("线程详情"),
        "the wide frame shows the selected thread's detail pane: {wide_text}"
    );
    assert!(wide_text.contains('›'), "one row is selected: {wide_text}");

    let medium = draw_rows(&mut app, 60, 30);
    assert!(
        medium.iter().all(|row| row.chars().count() <= 60),
        "medium overflow: {medium:?}"
    );
    let medium_text = medium.join("\n");
    assert!(
        medium_text.contains("Threads") || medium_text.contains("任务中心"),
        "{medium_text}"
    );
    assert!(
        !medium_text.contains("Task details") && !medium_text.contains("线程详情"),
        "a 60-column frame drops the detail column instead of truncating both: {medium_text}"
    );

    let narrow = draw_rows(&mut app, 24, 30);
    assert!(
        narrow
            .iter()
            .all(|row| row.chars().count() <= usize::from(24u16)),
        "narrow overflow: {narrow:?}"
    );
    let narrow_text = narrow.join("\n");
    assert!(
        narrow_text.contains("Terminal too small") || narrow_text.contains("终端过窄"),
        "below the usable width it says so plainly: {narrow_text}"
    );
    assert!(
        !narrow_text.contains("Filter") && !narrow_text.contains("筛选"),
        "and does not draw the list anyway: {narrow_text}"
    );
}

/// 空态也要有帧可查：没有可访问线程时，`←` 不开一层空列表，而是把原因落到转录里并留在
/// 画面上，同时整屏仍然守宽。
#[tokio::test]
async fn an_empty_thread_directory_says_why_it_did_not_open() {
    let root = TempRoot::new("command_center_empty");
    let mut app = fixture_app(root.path()).await;
    let logs_before = app.logs.len();
    app.open_command_center().await.expect("a soft stop");

    assert!(
        !app.command_center_open(),
        "nothing to pick, so nothing opens"
    );
    assert_eq!(app.logs.len(), logs_before + 1);
    assert!(
        app.logs[logs_before].text.contains("no tasks to show")
            || app.logs[logs_before].text.contains("没有可显示的任务"),
        "{}",
        app.logs[logs_before].text
    );

    let frame = draw_rows(&mut app, 100, 24);
    assert!(
        frame.iter().all(|row| row.chars().count() <= 100),
        "{frame:?}"
    );
    let text = frame.join("\n");
    assert!(
        text.contains("no tasks to show") || text.contains("没有可显示的任务"),
        "the reason stays on screen: {text}"
    );
}

/// §3.1: the first frame must not wait for the directory walk, the popup
/// catalogs, the model status or the session stats — the composer is editable
/// before any of them land, and they arrive afterwards.
#[tokio::test]
async fn the_first_frame_does_not_wait_for_background_fills() {
    let root = TempRoot::new("startup_fill");
    fs::write(root.path().join("probe.txt"), "x").expect("workspace file");
    let global = Cli::parse_from([
        "wunder-cli",
        "--lang",
        "en-US",
        "--user",
        "snapshot_user",
        "--temp-root",
        root.path().to_string_lossy().as_ref(),
    ])
    .global;
    let runtime = fixture_runtime(root.path());
    let (requester, _notifications) = spawn_frame_scheduler();
    let mut app = TuiApp::new(runtime, global, Some("thread".to_string()), requester)
        .await
        .expect("app");

    // The first frame already has its content and a live composer; only the
    // deferred facts are missing.
    assert!(
        !app.logs.is_empty(),
        "the banner is part of the first frame"
    );
    assert!(
        app.input_focus_active(),
        "the composer owns focus from the start"
    );
    assert!(
        app.workspace_files.is_empty(),
        "the file index must not be built on the first frame"
    );

    app.complete_startup_fills().await;
    assert!(
        app.workspace_files
            .iter()
            .any(|file| file.path.ends_with("probe.txt")),
        "the deferred walk still fills the mention index"
    );
    assert!(
        !app.app_hints.is_empty() || app.enabled_skill_names.is_empty(),
        "the catalog fill ran"
    );
}

/// §7 的矩阵还点名了帮助与确认流程：`?` 面板要在窄屏里仍然读得出键位，审批模态要把三个
/// 选项摆出来，两者都不许把行甩出画面。
#[tokio::test]
async fn the_help_panel_and_the_approval_modal_survive_a_narrow_screen() {
    use wunder_server::approval::{ApprovalRequest, ApprovalRequestKind};

    let root = TempRoot::new("modals");
    let mut app = fixture_app(root.path()).await;

    app.shortcuts_visible = true;
    // The shortcuts list is two columns per row; at 40 columns every entry wraps to two
    // screen rows, so the panel needs the height of a real terminal to show all of them.
    let help = draw_rows(&mut app, 40, 30);
    assert!(
        help.iter().all(|row| row.chars().count() <= 40),
        "help overflow: {help:?}"
    );
    let help_text = help.join("\n");
    assert!(help_text.contains("ctrl + t"), "{help_text}");
    assert!(help_text.contains("esc to interrupt"), "{help_text}");
    // A screen too short to hold the panel must still never spill outside its columns.
    let short = draw_rows(&mut app, 40, 16);
    assert!(
        short.iter().all(|row| row.chars().count() <= 40),
        "help overflow on a short screen: {short:?}"
    );
    app.shortcuts_visible = false;

    let (tx, _rx) = tokio::sync::oneshot::channel();
    app.active_approval = Some(ApprovalRequest {
        id: "req-frame-1".to_string(),
        kind: ApprovalRequestKind::Exec,
        tool: "execute_command".to_string(),
        args: json!({"command": "echo demo"}),
        summary: "run a demonstration command".to_string(),
        detail: json!({}),
        respond_to: tx,
    });
    let approval = draw_rows(&mut app, 40, 30);
    assert!(
        approval.iter().all(|row| row.chars().count() <= 40),
        "approval overflow: {approval:?}"
    );
    let approval_text = approval.join("\n");
    assert!(
        approval_text.contains("Yes, run it once"),
        "the options are on screen, not behind a key: {approval_text}"
    );
    assert!(approval_text.contains("No, and tell"), "{approval_text}");
}

fn seeded_answer(
    session: &str,
    turn_id: &str,
    content: &str,
    reasoning: &str,
    status: &str,
) -> Value {
    json!({
        "session_id": session, "turn_id": turn_id, "item_id": "answer-1",
        "kind": "assistant_message", "role": "assistant", "visibility": "user",
        "status": status, "content": content, "reasoning_content": reasoning
    })
}

fn conversation_texts(app: &TuiApp) -> Vec<String> {
    app.logs
        .iter()
        .filter(|entry| matches!(entry.kind, LogKind::Assistant | LogKind::Reasoning))
        .map(|entry| entry.text.clone())
        .collect()
}

/// 根治方案 §10 完成定义：断线续传、去重、排序与快照恢复的结果，必须与该会话按
/// `change_seq` 重放的 durable 帧逐文本一致。测试自己从 change 帧读提交行，作为
/// 独立于被测折叠逻辑的参照。
#[tokio::test]
async fn durable_replay_and_snapshot_recovery_land_the_same_text() {
    let root = TempRoot::new("durable_parity");
    let mut app = fixture_app(root.path()).await;
    let session = app.session_id.clone();
    let owner = app.runtime.user_id.clone();
    let storage = app.runtime.state.storage.clone();
    let turn = storage
        .accept_thread_turn(&owner, &session, &json!({"content": "first observation"}))
        .expect("seed turn");
    let turn_id = turn["turn_id"].as_str().expect("turn id").to_string();

    // The interrupted stream left a partly-filled reasoning cell and answer cell
    // behind; replay completes them in place instead of appending a second copy.
    app.logs.clear();
    app.invalidate_transcript_metrics();
    app.push_log(LogKind::Reasoning, "check".to_string());
    app.push_log(LogKind::Assistant, "partial".to_string());

    let revision_one = seeded_answer(&session, &turn_id, "partial", "checking the log", "running");
    storage
        .append_thread_item(&owner, &revision_one)
        .expect("seed answer revision 1");
    let revision_two = seeded_answer(
        &session,
        &turn_id,
        "partial answer: the retry gate",
        "checking the log",
        "completed",
    );
    storage
        .append_thread_item(&owner, &revision_two)
        .expect("seed answer revision 2");
    storage
        .append_thread_item(
            &owner,
            &json!({"session_id": &session, "turn_id": &turn_id, "item_id": "scratch-1",
                "kind": "assistant_message", "role": "assistant",
                "visibility": "model_internal", "status": "completed",
                "content": "private scratch"}),
        )
        .expect("seed private item");
    storage
        .append_thread_item(
            &owner,
            &json!({"session_id": &session, "turn_id": &turn_id, "item_id": "answer-2",
                "kind": "assistant_message", "role": "assistant", "visibility": "user",
                "status": "completed", "content": "the gate is fixed", "reasoning": ""}),
        )
        .expect("seed second answer");
    storage
        .upsert_thread_text_block(
            &owner,
            &session,
            &json!({"item_id": "answer-2", "field": "content", "block_index": 0,
                "event_id": 7, "data": {"content": "the gate is", "field": "content",
                    "block_index": 0, "content_offset": 0}}),
        )
        .expect("seed durable block");

    let catalog = wunder_server::ThreadCatalogService::new((*app.runtime.state).clone());
    let frames = catalog
        .changes(&session, 0, 200)
        .await
        .expect("durable frames");
    let watermark = app
        .runtime
        .state
        .workspace
        .latest_thread_change_seq(&session)
        .expect("watermark");

    // Reference transcript: fold the committed rows of the change frames, letting a
    // later revision of an item replace the earlier text at the same position.
    let mut expected: Vec<(String, LogKind, String)> = Vec::new();
    for frame in &frames {
        let data = &frame["data"];
        if data["change_type"] != json!("item_upsert") {
            continue;
        }
        let row = &data["payload"];
        if row["visibility"] != json!("user") || row["kind"] != json!("assistant_message") {
            continue;
        }
        let item_id = row["item_id"].as_str().unwrap_or_default().to_string();
        let body = &row["payload"];
        let mut lines: Vec<(LogKind, String)> = Vec::new();
        let reasoning = body["reasoning_content"]
            .as_str()
            .or_else(|| body["reasoning"].as_str())
            .unwrap_or_default();
        if !reasoning.trim().is_empty() {
            lines.push((LogKind::Reasoning, reasoning.trim().to_string()));
        }
        let content = body["content"].as_str().unwrap_or_default();
        if !content.trim().is_empty() {
            lines.push((LogKind::Assistant, content.trim().to_string()));
        }
        for (kind, text) in lines {
            match expected
                .iter_mut()
                .find(|(id, entry_kind, _)| *id == item_id && *entry_kind == kind)
            {
                Some(slot) => slot.2 = text,
                None => expected.push((item_id.clone(), kind, text)),
            }
        }
    }
    let expected_texts: Vec<String> = expected.iter().map(|(_, _, text)| text.clone()).collect();
    assert_eq!(
        expected_texts,
        vec![
            "checking the log".to_string(),
            "partial answer: the retry gate".to_string(),
            "the gate is fixed".to_string()
        ],
        "the fixture must cover revisions, a second item and a private item"
    );

    assert!(
        app.thread_registry.flag_durable_lag(&session, watermark),
        "the catalog watermark must demand one replay pass"
    );
    app.replay_thread_events_if_needed(&session).await;
    assert_eq!(app.thread_registry.durable_cursor(&session), watermark);
    assert_eq!(
        conversation_texts(&app),
        expected_texts,
        "断线续传须与 change_seq 重放逐文本一致"
    );

    // Folding the same frames again changes nothing (I6 幂等).
    app.apply_durable_heal_frames(&session, &frames);
    assert_eq!(
        conversation_texts(&app),
        expected_texts,
        "重复重放不得二次写入"
    );

    // Snapshot recovery (I5) of the same thread yields the same text.
    assert!(
        app.reload_transcript_from_snapshot(&session).await,
        "快照恢复可用"
    );
    assert_eq!(
        conversation_texts(&app),
        expected_texts,
        "快照恢复须与重放逐文本一致"
    );
    let users: Vec<&str> = app
        .logs
        .iter()
        .filter(|entry| matches!(entry.kind, LogKind::User))
        .map(|entry| entry.text.as_str())
        .collect();
    assert_eq!(
        users,
        vec!["first observation"],
        "用户轮次同样按 change_seq 落位"
    );
    assert!(
        !app.logs
            .iter()
            .any(|entry| entry.text.contains("private scratch")),
        "model-private items never reach the transcript"
    );
}

/// §3.2/§3.3：舵机直连 runtime 共享 feeder。基线之后的提交即使实时通道没送到，
/// durable 帧也会折叠进转录；一个线程只允许一个写者。
#[tokio::test]
async fn the_durable_feed_folds_commits_behind_the_start_baseline() {
    let root = TempRoot::new("durable_feed");
    let mut app = fixture_app(root.path()).await;
    let session = app.session_id.clone();
    let owner = app.runtime.user_id.clone();
    let storage = app.runtime.state.storage.clone();
    let turn = storage
        .accept_thread_turn(&owner, &session, &json!({"content": "first observation"}))
        .expect("seed turn");
    let turn_id = turn["turn_id"].as_str().expect("turn id").to_string();
    let baseline = app.durable_watermark(&session).await;
    app.logs.clear();
    app.invalidate_transcript_metrics();
    // A feed only survives for a thread with live work; emulate the send.
    app.active_stream_sessions.insert(session.clone());

    app.follow_durable_thread(session.as_str(), baseline).await;
    assert!(app.thread_registry.durable_feed_attached(&session));
    app.follow_durable_thread(session.as_str(), baseline).await;
    assert_eq!(
        app.thread_registry.durable_feed_session_ids().len(),
        1,
        "a thread must never hold two durable writers"
    );

    storage
        .append_thread_item(
            &owner,
            &json!({"session_id": &session, "turn_id": &turn_id, "item_id": "answer-9",
                "kind": "assistant_message", "role": "assistant", "visibility": "user",
                "status": "completed", "content": "the feed delivered this", "reasoning": ""}),
        )
        .expect("seed commit");

    let mut arrived = false;
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        app.drain_durable_feeds();
        if app
            .logs
            .iter()
            .any(|entry| entry.text == "the feed delivered this")
        {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "durable frames must fold into the transcript");
    assert_eq!(
        app.thread_registry.durable_cursor(&session),
        app.durable_watermark(&session).await,
        "游标须跟到最后一条 durable change"
    );

    // Nothing running and caught up: no idle feeder keeps polling.
    app.active_stream_sessions.remove(&session);
    app.drain_durable_feeds();
    assert!(
        !app.thread_registry.durable_feed_attached(&session),
        "a caught-up thread releases its feed"
    );
}

/// §3.4 慢客户端：本帧预算跟不上水位时停止订阅，改为一次有界 durable 回放。
#[tokio::test]
async fn a_slow_client_stops_the_feed_and_catches_up_by_replay() {
    let root = TempRoot::new("durable_slow");
    let mut app = fixture_app(root.path()).await;
    let session = app.session_id.clone();
    let owner = app.runtime.user_id.clone();
    let storage = app.runtime.state.storage.clone();
    let turn = storage
        .accept_thread_turn(&owner, &session, &json!({"content": "first observation"}))
        .expect("seed turn");
    let turn_id = turn["turn_id"].as_str().expect("turn id").to_string();
    for index in 0..300 {
        storage
            .append_thread_item(
                &owner,
                &json!({"session_id": &session, "turn_id": &turn_id,
                    "item_id": format!("slow-{index}"), "kind": "assistant_message",
                    "role": "assistant", "visibility": "user", "status": "completed",
                    "content": format!("durable {index}"), "reasoning": ""}),
            )
            .expect("seed durable commit");
    }
    let watermark = app.durable_watermark(&session).await;
    app.logs.clear();
    app.invalidate_transcript_metrics();
    app.active_stream_sessions.insert(session.clone());

    app.follow_durable_thread(session.as_str(), 0).await;
    // The feeder channel is 256 deep (根治方案 §3.3); a full channel means the
    // feeder itself is blocked on this client.
    let mut deep = false;
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if app.thread_registry.durable_feed_depth(&session) >= 256 {
            deep = true;
            break;
        }
    }
    assert!(deep, "the feed must fill before the client drains it");
    app.drain_durable_feeds();
    assert!(
        !app.thread_registry.durable_feed_attached(&session),
        "the slow side stops subscribing"
    );
    assert!(
        app.thread_registry.needs_replay(&session),
        "and demands one bounded replay pass"
    );

    app.replay_thread_events_if_needed(&session).await;
    assert!(
        !app.thread_registry.needs_replay(&session),
        "one pass closes the gap"
    );
    assert_eq!(
        app.thread_registry.durable_cursor(&session),
        watermark,
        "replay closes what the live feed could not"
    );
    app.active_stream_sessions.remove(&session);
}

async fn inquiry_fixture_app(root: &Path) -> TuiApp {
    let global = Cli::parse_from([
        "wunder-cli",
        "--lang",
        "en-US",
        "--user",
        "snapshot_user",
        "--temp-root",
        root.to_string_lossy().as_ref(),
    ])
    .global;
    let runtime = fixture_runtime(root);
    let (requester, _notifications) = spawn_frame_scheduler();
    TuiApp::new(
        runtime,
        global,
        Some("inquiry-thread".to_string()),
        requester,
    )
    .await
    .expect("app")
}

/// §问询面板: a multi-question payload numbers its routes globally, maps an
/// answer back to its own question and keeps the legacy flat shape readable.
#[tokio::test]
async fn inquiry_panel_numbers_routes_across_questions_and_answers_each_one() {
    let root = TempRoot::new("inquiry_panel");
    let mut app = inquiry_fixture_app(root.path()).await;
    let panel = app
        .parse_inquiry_panel_state(&json!({
            "questions": [
                {"question": "Pick a cadence", "multiple": true, "options": [
                    {"label": "Fixed", "description": "matches the frame pump", "recommended": true},
                    {"label": "Adaptive"},
                ]},
                {"question": "Pick a scope", "options": [
                    {"label": "Cards"}, {"label": "Rows"},
                ]},
            ]
        }))
        .expect("panel");
    assert_eq!(
        panel.route_count(),
        4,
        "the numbering runs past one question"
    );
    app.activate_inquiry_panel(panel, false);

    let lines = app.inquiry_modal_lines().expect("modal lines");
    assert_eq!(
        lines.first().map(String::as_str),
        Some("Question 1/2: Pick a cadence")
    );
    assert!(
        lines.iter().any(|line| line.starts_with("› 1. Fixed")),
        "the recommended route starts highlighted: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.ends_with("4. Rows")),
        "the second question keeps the global numbering: {lines:?}"
    );

    // Global indexes map back to the question they came from; the untouched
    // question answers with 无偏好 instead of swallowing the whole reply.
    assert_eq!(
        app.try_convert_inquiry_input("1, 4"),
        Some(
            "[Inquiry Panel Selection]\nQuestion 1/2: Pick a cadence\n- Fixed: matches the frame pump\nQuestion 2/2: Pick a scope\n- Rows"
                .to_string()
        )
    );
    assert!(
        app.active_inquiry_panel.is_none(),
        "an answered panel closes"
    );
}

/// A single-select question rejects two picks, and the pre-multi payload (one
/// flat question, `routes` as the option key) still drives the same modal.
#[tokio::test]
async fn inquiry_panel_keeps_single_select_honest_and_reads_legacy_payloads() {
    let root = TempRoot::new("inquiry_legacy");
    let mut app = inquiry_fixture_app(root.path()).await;
    let panel = app
        .parse_inquiry_panel_state(&json!({
            "question": "Pick one",
            "routes": [{"label": "Alpha"}, {"label": "Beta", "recommended": true}],
        }))
        .expect("legacy panel");
    assert_eq!(panel.questions.len(), 1);
    app.activate_inquiry_panel(panel, false);

    assert_eq!(
        app.try_convert_inquiry_input("1, 2"),
        Some(String::new()),
        "two picks of one single-select question are not an answer"
    );
    assert_eq!(
        app.logs.last().map(|entry| entry.kind),
        Some(LogKind::Error)
    );
    assert!(
        app.active_inquiry_panel.is_some(),
        "a rejected answer keeps the panel open"
    );

    // Enter answers with the highlighted route, which is the recommended one.
    assert_eq!(
        app.try_handle_inquiry_panel_navigation_key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE
        )),
        Some(Some("2".to_string()))
    );
    assert_eq!(
        app.try_convert_inquiry_input("2"),
        Some("[Inquiry Panel Selection]\nQuestion: Pick one\n- Beta".to_string())
    );
}

/// Pointer parity with codex: a click on a card's disclosure row folds or unfolds that
/// card, and the row that opened it is the row that closes it.
#[tokio::test]
async fn a_click_on_the_disclosure_row_toggles_the_card_it_belongs_to() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let root = TempRoot::new("disclosure_click");
    let mut app = fixture_app(root.path()).await;
    let width = 80u16;
    let height = 30u16;

    let folded = draw_rows(&mut app, width, height);
    let disclosure_row = folded
        .iter()
        .position(|row| row.contains("+ Show thinking (enter)"))
        .unwrap_or_else(|| panic!("a folded card should offer details:\n{folded:?}"));
    assert!(
        !folded.iter().any(|row| row.contains("reasoning line 12")),
        "the folded frame must not already show the tail"
    );

    let mut press = |row: usize| {
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 4,
            row: u16::try_from(row).expect("row fits"),
            modifiers: KeyModifiers::NONE,
        })
    };
    press(disclosure_row);

    let opened = draw_rows(&mut app, width, height);
    assert!(
        opened.iter().any(|row| row.contains("reasoning line 12")),
        "the click unfolded the card:\n{opened:?}"
    );
    let collapse_row = opened
        .iter()
        .position(|row| row.contains("\u{2212} Show less"))
        .unwrap_or_else(|| panic!("an open card offers to close itself:\n{opened:?}"));

    // And clicking that same row again folds it back.
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: u16::try_from(collapse_row).expect("row fits"),
        modifiers: KeyModifiers::NONE,
    });
    let refolded = draw_rows(&mut app, width, height);
    assert!(
        !refolded.iter().any(|row| row.contains("reasoning line 12")),
        "the second click folded the card back:\n{refolded:?}"
    );
}

/// The composer answers the pointer before the transcript does, the way codex routes the
/// textarea first: a click inside the draft puts the caret under the pointer.
#[tokio::test]
async fn a_click_in_the_composer_places_the_caret_under_the_pointer() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let root = TempRoot::new("composer_click");
    let mut app = fixture_app(root.path()).await;
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);

    let area = app.input_hit_area;
    assert!(area.width > 10, "the composer drew a text area: {area:?}");
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + 6,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.input_cursor, 6, "six columns in is six chars in");
    assert!(matches!(app.focus_area, FocusArea::Input));
}

/// A tool card is a disclosure subject like any other: it previews the head of its result
/// and holds the overflow — and a remote call's arguments — behind the same row.
#[tokio::test]
async fn a_tool_card_previews_its_rows_and_holds_the_rest_behind_the_disclosure() {
    let root = TempRoot::new("tool_fold");
    let mut app = fixture_app(root.path()).await;
    app.logs.clear();
    let content = (1..=5)
        .map(|n| json!({ "type": "text", "text": format!("row {n}") }))
        .collect::<Vec<_>>();
    let card = build_completed_tool_log(
        "crm@lookup",
        &json!({ "result": { "ok": true, "args": { "customer": "42" }, "data": {
            "server": "crm", "tool": "lookup", "result": { "content": content }
        }} }),
        false,
        None,
    );
    let summary = card.summary_text();
    app.push_special_log(LogKind::Tool, summary, card);

    let folded = draw_rows(&mut app, 60, 16);
    assert!(
        folded.iter().any(|row| row.contains("row 1")),
        "a result is readable without asking for it: {folded:?}"
    );
    assert!(
        !folded.iter().any(|row| row.contains("row 5")),
        "the overflow stays folded: {folded:?}"
    );
    assert!(
        folded
            .iter()
            .any(|row| row.contains("+2 lines (enter to expand)")),
        "the row counts exactly what Enter adds: {folded:?}"
    );
    assert!(
        !folded.iter().any(|row| row.contains("Arguments:")),
        "the arguments wait for the open card: {folded:?}"
    );

    app.transcript_selected = Some(app.logs.len() - 1);
    assert!(
        app.toggle_selected_card(),
        "Enter has something to reveal on this card"
    );
    let opened = draw_rows(&mut app, 60, 16);
    assert!(
        opened.iter().any(|row| row.contains("row 5")),
        "Enter revealed the overflow: {opened:?}"
    );
    assert!(
        opened.iter().any(|row| row.contains("customer")),
        "and the remote call's arguments: {opened:?}"
    );
    assert!(
        opened.iter().any(|row| row.contains("\u{2212} Show less")),
        "and the open card can close itself: {opened:?}"
    );
}

/// Fold state is keyed to the card, not to its slot in the transcript: trimming older
/// cards must not silently move an opening onto a different one.
#[tokio::test]
async fn an_opened_card_stays_open_when_older_cards_are_trimmed() {
    let root = TempRoot::new("disclosure_identity");
    let mut app = fixture_app(root.path()).await;

    let index = app
        .logs
        .iter()
        .position(|entry| entry.kind == LogKind::Reasoning)
        .expect("the fixture has a thinking block");
    app.transcript_selected = Some(index);
    assert!(app.toggle_selected_card(), "the card opens");
    let cell_id = app.logs[index].cell_id;

    app.pop_oldest_log_entry();
    let shifted = index.saturating_sub(1);
    assert_eq!(
        app.logs[shifted].cell_id, cell_id,
        "same card, earlier slot"
    );
    assert!(
        app.card_is_expanded(shifted),
        "the fold state followed the card, not the slot"
    );
    assert_eq!(
        app.expanded_cards.len(),
        1,
        "trimming does not wipe every opening, and leaves nothing orphaned"
    );

    // Clearing the transcript is a different event and does drop them.
    app.clear_transcript();
    assert!(app.expanded_cards.is_empty());
}

/// A modal surface owns the screen: the click belongs to it, never to the cards it hides.
#[tokio::test]
async fn a_click_while_a_modal_is_up_does_not_reach_the_cards_behind_it() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

    let root = TempRoot::new("modal_click");
    let mut app = fixture_app(root.path()).await;
    let folded = draw_rows(&mut app, 80, 30);
    let disclosure_row = folded
        .iter()
        .position(|row| row.contains("+ Show thinking (enter)"))
        .expect("the fixture shows a folded thinking card");

    app.shortcuts_visible = true;
    app.on_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 4,
        row: u16::try_from(disclosure_row).expect("row fits"),
        modifiers: KeyModifiers::NONE,
    });

    assert!(
        app.expanded_cards.is_empty(),
        "no card was unfolded behind the panel"
    );
    assert!(
        app.transcript_selected.is_none(),
        "no card was selected behind the panel"
    );
}

/// Clipboard writes a test triggers are recorded here instead of hitting the machine's
/// real clipboard.
static COPIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The sink is process-wide, so the tests that read it take turns: two of them in flight
/// would otherwise count each other's copies.
static CLIPBOARD_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn record_clipboard_write(text: &str) -> anyhow::Result<()> {
    COPIED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(text.to_string());
    Ok(())
}

struct ClipboardSeam;

impl ClipboardSeam {
    /// Hold the returned guard until the copy has been read back.
    fn arm() -> std::sync::MutexGuard<'static, ()> {
        let turn = CLIPBOARD_TURN
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *CLIPBOARD_WRITER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(record_clipboard_write);
        Self::drain();
        turn
    }

    fn drain() {
        COPIED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn copied() -> Vec<String> {
        COPIED
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

fn mouse(app: &mut TuiApp, kind: crossterm::event::MouseEventKind, row: usize, column: u16) {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let _ = (MouseButton::Left, MouseEventKind::Up(MouseButton::Left));
    app.on_mouse(MouseEvent {
        kind,
        column,
        row: u16::try_from(row).expect("row fits"),
        modifiers: KeyModifiers::NONE,
    });
}

#[tokio::test]
async fn a_drag_across_the_transcript_marks_rows_and_the_release_copies_them() {
    use crossterm::event::{MouseButton, MouseEventKind};
    use ratatui::style::Modifier;

    let root = TempRoot::new("drag_selection");
    let mut app = fixture_app(root.path()).await;
    let before = draw_rows(&mut app, 80, 24);
    let start = before
        .iter()
        .position(|row| row.contains("Ran cargo test"))
        .expect("the command card is on screen");
    let _clipboard_turn = ClipboardSeam::arm();

    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), start, 2);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        start + 1,
        20,
    );

    // The mark is a style, never a rewrite: every column of every row still reads the same.
    let marked = draw_rows(&mut app, 80, 24);
    assert_eq!(marked, before, "highlighting may not move a character");

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &mut app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    assert!(
        (0..24u16).any(|row| (0..80u16)
            .any(|column| buffer[(column, row)].modifier.contains(Modifier::REVERSED))),
        "the drag left a visible mark"
    );

    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        start + 1,
        20,
    );
    let copied = ClipboardSeam::copied();
    assert_eq!(copied.len(), 1, "one release copies once: {copied:?}");
    let lines = copied[0].lines().count();
    assert!(lines >= 2, "the drag covered two rows: {:?}", copied[0]);
    assert!(copied[0].contains("Ran cargo test"), "{:?}", copied[0]);
}

/// Two clicks take the word under the pointer, three take the line, and each release
/// copies what it picked up, which is what copy-on-select means.
#[tokio::test]
async fn double_and_triple_clicks_take_the_word_and_the_line() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("click_select");
    let mut app = fixture_app(root.path()).await;
    let before = draw_rows(&mut app, 80, 24);
    let row = before
        .iter()
        .position(|line| line.contains("Ran cargo test"))
        .expect("the command card is on screen");
    let column = before[row].find("cargo").expect("the word is on that row") as u16 + 1;
    let _clipboard_turn = ClipboardSeam::arm();

    for _ in 0..2 {
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            row,
            column,
        );
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, column);
    }
    let copied = ClipboardSeam::copied();
    assert_eq!(
        copied.last().map(String::as_str),
        Some("cargo"),
        "{copied:?}"
    );

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        column,
    );
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, column);
    let copied = ClipboardSeam::copied();
    assert!(
        copied
            .last()
            .is_some_and(|text| text.contains("Ran cargo test")),
        "the third click took the whole line: {copied:?}"
    );
}

/// The horizontal arrows enforce a card's fold state the way codex's list keys do, and
/// keep their scroll meaning on a card that has nothing to fold.
#[tokio::test]
async fn the_horizontal_arrows_open_and_close_the_selected_card() {
    let root = TempRoot::new("arrow_fold");
    let mut app = fixture_app(root.path()).await;
    let index = app
        .logs
        .iter()
        .position(|entry| entry.kind == LogKind::Reasoning)
        .expect("the fixture has a thinking block");
    app.focus_area = FocusArea::Transcript;
    app.transcript_selected = Some(index);

    let folded = draw_rows(&mut app, 80, 30);
    assert!(
        !folded.iter().any(|row| row.contains("reasoning line 12")),
        "the block starts folded: {folded:?}"
    );

    app.on_key(key(KeyCode::Right))
        .await
        .expect("right opens the card");
    let opened = draw_rows(&mut app, 80, 30);
    assert!(
        opened.iter().any(|row| row.contains("reasoning line 12")),
        "right expanded it: {opened:?}"
    );

    app.on_key(key(KeyCode::Left))
        .await
        .expect("left closes the card");
    let refolded = draw_rows(&mut app, 80, 30);
    assert!(
        !refolded.iter().any(|row| row.contains("reasoning line 12")),
        "left folded it back: {refolded:?}"
    );

    // A plain answer has no fold, so the arrows stay scroll keys there.
    let plain = app.logs.len() - 1;
    app.transcript_selected = Some(plain);
    assert!(
        !app.set_selected_card_expanded(true),
        "nothing to open on a card that hides nothing"
    );
}

/// A tab in a transcript row is drawn as projected text: the row that really renders at
/// eight-column stops, not one where the tab sits as a single 1-column cell.
#[tokio::test]
async fn a_tab_in_a_transcript_row_projects_to_the_next_eight_column_stop() {
    let root = TempRoot::new("tab_display");
    let mut app = fixture_app(root.path()).await;
    // Raw scrollback keeps the entry text verbatim, so the frame shows exactly this row.
    app.logs.clear();
    app.raw_output = true;
    app.push_log(LogKind::User, "gate\tvalue".to_string());

    let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &mut app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    let row = (0..12u16)
        .find(|&row| buffer[(0, row)].symbol() == "g")
        .expect("the tab row is on screen");

    assert_eq!(buffer[(3, row)].symbol(), "e", "gate sits at 0..4");
    for column in 4..8u16 {
        assert_eq!(
            buffer[(column, row)].symbol(),
            " ",
            "the tab pads its stop with spaces at column {column}"
        );
    }
    assert_eq!(
        buffer[(8, row)].symbol(),
        "v",
        "value resumes at the next eight-column stop"
    );
}

/// The pointer answers in projected display columns: padding right of a tab belongs to
/// the tab, so a double click there is between words and takes the word to its left,
/// while the copy still carries the logical text with the tab inside it.
#[tokio::test]
async fn a_double_click_past_a_tab_hits_the_projected_columns() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("tab_hit");
    let mut app = fixture_app(root.path()).await;
    app.logs.clear();
    app.raw_output = true;
    app.push_log(LogKind::User, "gate\tvalue".to_string());
    let before = draw_rows(&mut app, 40, 12);
    let row = before
        .iter()
        .position(|line| line.starts_with("gate"))
        .expect("the tab row is on screen");
    let _clipboard_turn = ClipboardSeam::arm();

    // Buffer column 6 is tab padding (gate covers 0..4, the tab covers 4..8). Counting
    // the tab as one column would place column 6 inside "value" instead.
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 7);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 7);
    }
    let copied = ClipboardSeam::copied();
    assert_eq!(
        copied.last().map(String::as_str),
        Some("gate"),
        "the padding double click takes the word to the left: {copied:?}"
    );

    // A third click on the same spot takes the whole line, and that copy keeps the tab.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 7);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 7);
    let copied = ClipboardSeam::copied();
    assert_eq!(
        copied.last().map(String::as_str),
        Some("gate\tvalue"),
        "the whole-line copy is logical text, tab and all: {copied:?}"
    );
}

/// A pointer event that says which keys were down: shift+click is its own gesture, so a
/// test has to be able to ask for it.
fn mouse_held(
    app: &mut TuiApp,
    kind: crossterm::event::MouseEventKind,
    row: usize,
    column: u16,
    modifiers: KeyModifiers,
) {
    app.on_mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row: u16::try_from(row).expect("row fits"),
        modifiers,
    });
}

/// The transcript as raw scrollback (`Alt+R`): one window line per card, text verbatim, so
/// a test can name a card by the text on its row and know which row is which card with no
/// renderer in between.
fn raw_single_line_cards(app: &mut TuiApp, texts: &[impl AsRef<str>]) {
    app.logs.clear();
    app.raw_output = true;
    for text in texts {
        app.push_log(LogKind::User, text.as_ref().to_string());
    }
    app.transcript_offset_from_bottom = 0;
    app.transcript_selected = None;
}

/// The screen rows carrying the selection mark — the reversed video `theme::selection_mark`
/// puts on the picked run — with the text drawn on them. Naming a mark by its text is what
/// lets a test say "the same text is still marked" across a scroll that moved the rows.
fn marked_rows(app: &mut TuiApp, width: u16, height: u16) -> Vec<(usize, String)> {
    use ratatui::style::Modifier;

    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| crate::tui::ui::draw(frame, app))
        .expect("draw");
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .filter_map(|row| {
            let text: String = (0..width)
                .map(|column| buffer[(column, row)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string();
            let marked = (0..width)
                .any(|column| buffer[(column, row)].modifier.contains(Modifier::REVERSED));
            marked.then_some((usize::from(row), text))
        })
        .collect()
}

/// Only the rows this fixture drew, so a decoration elsewhere cannot be mistaken for a mark.
fn marked_cards(app: &mut TuiApp, width: u16, height: u16) -> Vec<(usize, String)> {
    marked_rows(app, width, height)
        .into_iter()
        .filter(|(_, text)| text.contains("item-"))
        .collect()
}

/// The rows inside the transcript band that show one of the fixture's cards, in order.
/// Only the band the layout recorded answers to the pointer and the wheel, and the footer
/// echoes a card's text too, so a row picked by content alone can be outside the band.
fn banded_card_rows(rows: &[String], region: Rect) -> Vec<(usize, String)> {
    let top = usize::from(region.y);
    let bottom = (top + usize::from(region.height)).min(rows.len());
    (top..bottom)
        .filter(|row| rows[*row].starts_with("item-"))
        .map(|row| (row, rows[row].trim_end().to_string()))
        .collect()
}

/// A mark is booked against the card and the line of it the reader pressed, so the window
/// can scroll away and back and the same text is still marked and still copies the same.
#[tokio::test]
async fn a_mark_survives_scrolling_away_and_back_to_the_same_text() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_scroll");
    let mut app = fixture_app(root.path()).await;
    // One tall card and a short one: a stack of single-line cards is archived into the
    // terminal's own scrollback as it stops fitting, which leaves the in-app window with
    // nothing to scroll. A card taller than the band stays, with its own lines to slide.
    let body: Vec<String> = (1..=30).map(|n| format!("item-a{n:02}")).collect();
    raw_single_line_cards(&mut app, &[body.join("\n").as_str(), "item-tail"]);
    let width = 60u16;
    let height = 24u16;
    let before = draw_rows(&mut app, width, height);
    let band = banded_card_rows(&before, app.transcript_mouse_region);
    let a28 = band
        .iter()
        .find(|(_, text)| text == "item-a28")
        .expect("line 28 of the tall card is on screen");
    let a30 = band
        .iter()
        .find(|(_, text)| text == "item-a30")
        .expect("line 30 of the tall card is on screen");
    let a29 = band
        .iter()
        .find(|(_, text)| text == "item-a29")
        .expect("line 29 of the tall card is on screen");
    assert_eq!(
        (a29.0, a30.0),
        (a28.0 + 1, a28.0 + 2),
        "the three rows are next to each other"
    );

    let _clipboard_turn = ClipboardSeam::arm();
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), a28.0, 2);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), a30.0, 6);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), a30.0, 6);
    let expected = format!("{}\n{}\n{}", &a28.1[2..], &a29.1, &a30.1[..6]);
    let first_copy = ClipboardSeam::copied()
        .pop()
        .expect("the release copied the drag");
    assert_eq!(
        first_copy, expected,
        "the drag runs from column 2 of line 28 to column 6 of line 30"
    );
    let mark_before = marked_cards(&mut app, width, height);
    assert!(!mark_before.is_empty(), "the drag left a visible mark");
    let anchor_before = app.transcript_selection.expect("a mark is open").anchor;
    assert_eq!(
        anchor_before.cell_id, app.logs[0].cell_id,
        "the anchor names the tall card"
    );
    assert_eq!(
        anchor_before.line_within_cell, 27,
        "and the twenty-eighth line of it, not a window row"
    );

    // The wheel moves the window and nothing else. A frame is drawn while the picked
    // lines are off the window: there is nothing to mark there, and that is no reason to
    // forget where the mark was.
    let inside = usize::from(app.transcript_mouse_region.y + 1);
    for _ in 0..6 {
        mouse(&mut app, MouseEventKind::ScrollUp, inside, 4);
    }
    let away = marked_cards(&mut app, width, height);
    assert!(
        away.is_empty(),
        "the picked lines left the window, so nothing is drawn: {away:?}"
    );
    assert!(
        app.transcript_selection.is_some(),
        "scrolling away may not drop the mark"
    );
    for _ in 0..6 {
        mouse(&mut app, MouseEventKind::ScrollDown, inside, 4);
    }

    let selection = app.transcript_selection.expect("the mark is still open");
    assert_eq!(
        selection.anchor, anchor_before,
        "a scroll must not re-point the anchor"
    );
    let mark_after = marked_cards(&mut app, width, height);
    assert_eq!(
        mark_after, mark_before,
        "the same text is marked on the same rows once the window is back"
    );

    // A right click copies what is marked without moving the gesture.
    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        mark_after[0].0,
        6,
        KeyModifiers::NONE,
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some(first_copy.as_str()),
        "the copy after a scroll reads exactly as it did before"
    );
}

/// The tail growing while a mark is open — a streamed answer, a new card — neither drops
/// the mark nor shifts it onto other text.
#[tokio::test]
async fn the_mark_still_names_its_own_text_while_the_transcript_grows() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_append");
    let mut app = fixture_app(root.path()).await;
    let cards: Vec<String> = (1..=18).map(|n| format!("item-{n:02}")).collect();
    raw_single_line_cards(&mut app, &cards);
    let before = draw_rows(&mut app, 60, 24);
    let band = banded_card_rows(&before, app.transcript_mouse_region);
    let row = band.last().expect("the transcript band shows cards").0;
    assert!(row >= 1, "the drag needs a row above it: {row}");

    let _clipboard_turn = ClipboardSeam::arm();
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row - 1,
        0,
    );
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), row, 4);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 4);
    let first_copy = ClipboardSeam::copied().pop().expect("the release copied");
    let anchor_before = app.transcript_selection.expect("a mark is open").anchor;
    let texts_before: Vec<String> = marked_cards(&mut app, 60, 24)
        .into_iter()
        .map(|(_, text)| text)
        .collect();
    assert!(!texts_before.is_empty(), "the drag left a visible mark");

    // A new card lands at the tail, which is what a streaming round does to the window.
    app.push_log(LogKind::User, "item-19".to_string());
    let mark_after = marked_cards(&mut app, 60, 24);
    let selection = app
        .transcript_selection
        .expect("an append must not drop the mark");
    assert_eq!(
        selection.anchor, anchor_before,
        "the anchor still names the same card and line"
    );
    let texts_after: Vec<String> = mark_after.into_iter().map(|(_, text)| text).collect();
    assert_eq!(
        texts_after, texts_before,
        "the growing tail may neither drop the mark nor move it onto other cards"
    );
    assert!(
        texts_after.iter().any(|text| text.starts_with("item-18")),
        "the card the reader pressed is still one of them: {texts_after:?}"
    );

    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        row,
        2,
        KeyModifiers::NONE,
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some(first_copy.as_str()),
        "the copy still reads what the reader picked"
    );
}

/// A card that leaves the transcript for good takes its mark with it, and the neighbour
/// that slides into its slot is never mistaken for the picked text.
#[tokio::test]
async fn trimming_the_selected_card_clears_the_mark_and_a_neighbour_keeps_its_own() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_trim");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, &["item-01", "item-02", "item-03", "item-04"]);
    let before = draw_rows(&mut app, 60, 24);
    let first = before
        .iter()
        .position(|line| line.starts_with("item-01"))
        .expect("the oldest card is on screen");
    assert!(
        before[first + 1].starts_with("item-02"),
        "one raw line per card: {before:?}"
    );

    let _clipboard_turn = ClipboardSeam::arm();
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), first, 0);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        first + 1,
        7,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        first + 1,
        7,
    );
    assert_eq!(
        app.transcript_selection
            .expect("a mark is open")
            .anchor
            .cell_id,
        app.logs[0].cell_id,
        "the anchor is booked against the oldest card"
    );

    app.pop_oldest_log_entry();
    assert!(
        app.transcript_selection.is_none(),
        "the card is gone, so the mark goes rather than re-pointing"
    );

    // A mark on the second card survives that same trim, on its own text.
    raw_single_line_cards(&mut app, &["item-01", "item-02", "item-03", "item-04"]);
    let before = draw_rows(&mut app, 60, 24);
    let second = before
        .iter()
        .position(|line| line.starts_with("item-02"))
        .expect("the second card is on screen");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), second, 0);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), second, 7);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), second, 7);
    let picked = ClipboardSeam::copied()
        .pop()
        .expect("the drag copied its own card");
    assert_eq!(picked, "item-02");

    app.pop_oldest_log_entry();
    let selection = app
        .transcript_selection
        .expect("a trim of a different card keeps the mark");
    assert_eq!(
        selection.anchor.cell_id, app.logs[0].cell_id,
        "the picked card is now the oldest, and still the same card"
    );
    let marked = marked_cards(&mut app, 60, 24);
    assert!(
        marked.iter().any(|(_, text)| text == "item-02"),
        "the mark followed the card into its new row: {marked:?}"
    );
    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        marked[0].0,
        2,
        KeyModifiers::NONE,
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some("item-02"),
        "and copies the text it was put on, not the neighbour that took its row"
    );
}

/// Shift+click is codex's extend gesture: the anchor the reader pressed stays, the free
/// end moves, and the click never feeds the double or triple click counter.
#[tokio::test]
async fn a_shift_click_moves_the_free_end_and_leaves_the_anchor_put_down() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_shift");
    let mut app = fixture_app(root.path()).await;
    let cards: Vec<String> = (1..=16).map(|n| format!("item-{n:02}")).collect();
    raw_single_line_cards(&mut app, &cards);
    let before = draw_rows(&mut app, 60, 24);
    let band = banded_card_rows(&before, app.transcript_mouse_region);
    assert!(
        band.len() >= 6,
        "the gesture needs six card rows inside the band: {band:?}"
    );
    // A row with room both above and below it inside the band: only the band answers to
    // the pointer, so a row outside it is not a gesture at all.
    let row = band[2].0;
    assert_eq!(
        (band[3].0, band[4].0),
        (row + 1, row + 2),
        "one raw line per card keeps the rows next to each other: {band:?}"
    );

    let _clipboard_turn = ClipboardSeam::arm();
    // Press and drag a short way, so a range is open to extend.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 0);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), row, 3);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 3);
    let short = ClipboardSeam::copied().pop().expect("the drag copied");
    let anchor_before = app.transcript_selection.expect("a mark is open").anchor;
    assert_eq!(short, "ite", "three columns of one line: {short:?}");

    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row + 2,
        5,
        KeyModifiers::SHIFT,
    );
    assert_eq!(
        app.pointer_click_count, 0,
        "a shift+click is not a click toward a double click"
    );
    mouse_held(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row + 2,
        5,
        KeyModifiers::SHIFT,
    );

    let selection = app.transcript_selection.expect("still the same range");
    assert_eq!(selection.anchor, anchor_before, "the anchor stayed put");
    assert_ne!(selection.head, anchor_before, "the free end moved");
    let landed = before[row + 2].trim_end();
    let landed_card = app
        .logs
        .iter()
        .find(|entry| entry.text == landed)
        .expect("the card the shift landed on");
    assert_eq!(
        selection.head.cell_id, landed_card.cell_id,
        "and the free end is booked against the card it landed on"
    );
    let tail: String = before[row + 2].chars().take(5).collect();
    let expected = format!(
        "{}\n{}\n{}",
        before[row].trim_end(),
        before[row + 1].trim_end(),
        tail.trim_end()
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some(expected.as_str()),
        "the copy runs from the anchor to the new end"
    );

    // Shifting to the other side of the anchor flips the range instead of emptying it.
    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row - 1,
        4,
        KeyModifiers::SHIFT,
    );
    mouse_held(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row - 1,
        4,
        KeyModifiers::SHIFT,
    );
    let copied = ClipboardSeam::copied().pop().expect("the copy arrived");
    let backwards: String = before[row - 1].chars().skip(4).collect();
    assert_eq!(
        copied,
        backwards.trim_end(),
        "a backwards shift reads the range the other way"
    );
    assert_eq!(
        app.transcript_selection.expect("still open").anchor,
        anchor_before,
        "and even then the anchor is where it was pressed"
    );
}

/// On a row carrying a tab, a shift+click and a double click both count columns the way
/// the frame drew them: the tab carries the line to its next eight-column stop, so a
/// pointer inside its padding is not inside the word that follows it.
#[tokio::test]
async fn shift_and_double_clicks_on_a_tab_row_answer_at_the_eight_column_stops() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_shift_tab");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, &["gate\tvalue"]);
    let before = draw_rows(&mut app, 40, 12);
    let row = before
        .iter()
        .position(|line| line.starts_with("gate"))
        .expect("the tab row is on screen");

    let _clipboard_turn = ClipboardSeam::arm();
    // gate covers 0..4, the tab its padding 4..8, value sits at 8..13.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 0);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), row, 3);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 3);
    ClipboardSeam::drain();

    for (head, expect) in [
        (6usize, "gate"),
        (8usize, "gate"),
        (9usize, "gate\tv"),
        (13usize, "gate\tvalue"),
    ] {
        mouse_held(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            row,
            u16::try_from(head).expect("column fits"),
            KeyModifiers::SHIFT,
        );
        mouse_held(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            row,
            u16::try_from(head).expect("column fits"),
            KeyModifiers::SHIFT,
        );
        assert_eq!(
            ClipboardSeam::copied().last().map(String::as_str),
            Some(expect),
            "a shift+click at column {head} cuts at the stop, not at the character count: {:?}",
            ClipboardSeam::copied()
        );
    }
    ClipboardSeam::drain();

    // A range that starts inside the padding drops the tab: the tab's own column is
    // before the anchor, so it never travels to the clipboard.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 6);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), row, 7);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 7);
    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        13,
        KeyModifiers::SHIFT,
    );
    mouse_held(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row,
        13,
        KeyModifiers::SHIFT,
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some("value"),
        "column 6 is the tab's padding, so the copy starts at v: {:?}",
        ClipboardSeam::copied()
    );
    ClipboardSeam::drain();

    // Two clicks on that padding take the word to their left, and the word anchor keeps
    // its own bound when a shift+click walks the free end into the next word.
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 6);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 6);
    }
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some("gate"),
        "the padding double click is between words and takes the left one: {:?}",
        ClipboardSeam::copied()
    );
    mouse_held(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        12,
        KeyModifiers::SHIFT,
    );
    mouse_held(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row,
        12,
        KeyModifiers::SHIFT,
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some("gate\tvalue"),
        "the free end snaps to the word it landed in, eight-column stops and all: {:?}",
        ClipboardSeam::copied()
    );
}

/// A mark is drawn by patching spans, never by rewrapping, so at any sampled width every
/// row still fits the frame with a selection open.
#[tokio::test]
async fn a_marked_transcript_stays_inside_the_frame_at_every_sampled_width() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_widths");
    let mut app = fixture_app(root.path()).await;
    let cards: Vec<String> = (1..=12).map(|n| format!("item-{n:02}")).collect();
    raw_single_line_cards(&mut app, &cards);
    let _clipboard_turn = ClipboardSeam::arm();

    for width in [20u16, 24, 40, 80, 120] {
        let height = 24u16;
        let before = draw_rows(&mut app, width, height);
        let band = banded_card_rows(&before, app.transcript_mouse_region);
        let row = band.last().expect("the band shows cards").0;
        assert!(
            row >= 1,
            "the drag needs a row above it at {width}: {band:?}"
        );
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 1);
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            row - 1,
            width.saturating_sub(1),
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            row - 1,
            width.saturating_sub(1),
        );
        let rows = draw_rows(&mut app, width, height);
        assert!(
            rows.iter()
                .all(|line| line.chars().count() <= usize::from(width)),
            "no row may exceed the frame at {width} columns with a mark open"
        );
        assert!(
            !marked_cards(&mut app, width, height).is_empty(),
            "the mark is drawn at {width} columns"
        );
        app.transcript_selection = None;
    }
}

/// What a press books is the card's identity, and a press that never moved is still a
/// click rather than a copy.
#[tokio::test]
async fn a_mark_is_booked_against_the_card_not_the_row_it_was_pressed_on() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("selection_identity");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, &["item-01", "item-02", "item-03"]);
    let before = draw_rows(&mut app, 60, 24);
    let row = before
        .iter()
        .position(|line| line.starts_with("item-02"))
        .expect("the middle card is on screen");

    let _clipboard_turn = ClipboardSeam::arm();
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 2);
    let selection = app.transcript_selection.expect("a press opens a mark");
    assert_eq!(
        selection.anchor.cell_id, app.logs[1].cell_id,
        "the anchor names the card, not a window row"
    );
    assert_eq!(
        selection.head.cell_id, app.logs[1].cell_id,
        "and so does the free end it started on"
    );
    assert_eq!(
        selection.unit,
        SelectionUnit::Character,
        "one click is a character pick"
    );

    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 2);
    assert!(
        ClipboardSeam::copied().is_empty(),
        "a press that never moved is a click, not a copy: {:?}",
        ClipboardSeam::copied()
    );
}

/// The composer projects tabs before drawing: a draft carrying a literal tab reaches the
/// next eight-column stop on the frame, exactly like the transcript's projection — and the
/// draft itself keeps the tab.
#[tokio::test]
async fn a_tab_in_the_draft_reaches_the_next_eight_column_stop_on_the_frame() {
    let root = TempRoot::new("composer_tab_stop");
    let mut app = fixture_app(root.path()).await;
    app.input = "gate\tvalue".to_string();
    app.input_cursor = app.input.len();
    let rows = draw_rows(&mut app, 80, 24);
    let draft_row = rows
        .iter()
        .find(|row| row.contains("gate") && row.contains("value"))
        .expect("the draft is on screen");
    assert!(
        draft_row.contains("gate    value"),
        "the tab pads columns 4..8 on the frame: {draft_row:?}"
    );

    // The projection is display-only: the draft and the view handed to the painter keep
    // the logical text, tab included, so copy and submit round-trip what the user typed.
    assert_eq!(app.input, "gate\tvalue", "the draft keeps its literal tab");
    let area = app.input_hit_area;
    let view = app.input_view(area.width.max(1), area.height.max(1));
    let joined = view
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(
        joined.contains("gate\tvalue"),
        "the view hands the composer logical text: {joined:?}"
    );
    assert!(
        !joined.contains("gate    value"),
        "and never a pre-projected copy: {joined:?}"
    );
}

/// Hit testing answers in the projected column space: clicking the first cell of a word
/// that sits behind a tab's padding lands the caret on that word, not on whatever the
/// one-column-tab arithmetic would have counted there.
#[tokio::test]
async fn a_click_on_a_tab_words_projected_column_lands_on_that_word() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("composer_tab_hit");
    let mut app = fixture_app(root.path()).await;
    app.input = "gate\tvalue".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;

    // "gate" owns columns 0..4, the tab pads 4..8, "value" starts at column 8. The bytes
    // are g0 a1 t2 e3 \t4 v5 — a projected column 8 must give byte 5.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 8,
    );
    assert_eq!(
        app.input_cursor, 5,
        "the click landed on the word the pointer was over"
    );

    // A column inside the tab's padding belongs to the tab, so the caret sits before it.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 6,
    );
    assert_eq!(app.input_cursor, 4, "tab padding is the tab");
}

/// A tabbed draft that has to wrap breaks where the *projected* columns say it should. A
/// tab counted as one column gains three characters on the first row and splits the next
/// word mid-character, so this is the assertion that pins the wrap measurement to the same
/// eight-column stop the frame renders.
#[tokio::test]
async fn a_tabbed_draft_wraps_at_the_projected_columns_not_one_per_tab() {
    let root = TempRoot::new("composer_tab_wrap");
    let mut app = fixture_app(root.path()).await;
    app.input = "gate\tvalue and a tail that goes on".to_string();
    app.input_cursor = app.input.len();
    let rows = draw_rows(&mut app, 20, 24);
    let area = app.input_hit_area;
    let first = rows
        .get(usize::from(area.y))
        .expect("the composer row is on screen");
    assert!(
        first.contains("gate"),
        "the draft opens in the composer row: {first:?}"
    );
    let continuation = rows
        .get(usize::from(area.y) + 1)
        .expect("the wrapped draft has a second row");
    assert!(
        continuation.trim_start().starts_with("a tail"),
        "the tab must cost columns 4..8, so the break falls before \"a\": {continuation:?}"
    );
}

/// Up/down in a multi-line draft with tabs keeps the *display* column: the caret returns
/// to the same visual cell, which under eight-column stops is not the character index the
/// old one-column-tab counting would have aimed at.
#[tokio::test]
async fn arrow_keys_in_a_tabbed_draft_hold_the_display_column_not_the_char_index() {
    let root = TempRoot::new("composer_vertical_tab");
    let mut app = fixture_app(root.path()).await;
    // Line 0: gate(0..4) tab(4..8) value(8..13). Line 1: hh(0..2) tab(2..8) target(8..14).
    app.input = "gate\tvalue\nhh\ttarget".to_string();
    let _ = draw_rows(&mut app, 80, 24);

    // Caret before the 't' of "target" (byte 14), i.e. display column 8 of line 1.
    app.input_cursor = 14;
    app.on_key(key(KeyCode::Up)).await.expect("up");
    assert_eq!(
        app.input_cursor, 5,
        "column 8 on line 0 is the 'v' of value; one-column tabs would have aimed at 3"
    );

    app.on_key(key(KeyCode::Down)).await.expect("down");
    assert_eq!(
        app.input_cursor, 14,
        "and the way back lands on the same visual cell"
    );
}

/// The composer must survive the same width sweep as the transcript: a draft mixing a
/// long sentence, fullwidth characters and tabs never puts a glyph outside the frame at
/// any sampled width, and the caret stays inside the visible band.
#[tokio::test]
async fn the_composer_stays_inside_the_frame_at_every_sampled_width() {
    let root = TempRoot::new("composer_widths");
    let mut app = fixture_app(root.path()).await;
    app.input = "gate\t你好 item-1 value\ttail\n中\tsecond line with a long stretch of filler text that must wrap cleanly at every sampled width".to_string();
    // Caret at the head so the first visible rows always carry the draft's opening.
    app.input_cursor = 0;

    for width in [20u16, 24, 40, 80, 120] {
        let height = 24u16;
        let rows = draw_rows(&mut app, width, height);
        assert!(
            rows.iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no composer row may exceed the frame at {width} columns: {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("gate")),
            "the draft head is still on screen at {width} columns"
        );

        let area = app.input_hit_area;
        assert!(
            area.width > 0 && area.height > 0,
            "the composer kept a text area at {width} columns"
        );
        let view = app.input_view(area.width.max(1), area.height.max(1));
        assert!(
            view.cursor_x < area.width && view.cursor_y < area.height,
            "the caret stays inside the visible band at {width} columns: {:?}",
            (view.cursor_x, view.cursor_y, area)
        );
    }
}

// ------------------------------------------------------------ composer pointer gestures

/// The rows the draft is painted on, out of a `marked_rows` sweep: a mark anywhere else on
/// the screen belongs to another region and must not be read as the composer's.
fn marked_draft_rows(app: &mut TuiApp, width: u16, height: u16) -> Vec<(usize, String)> {
    let area = app.input_hit_area;
    let top = usize::from(area.y);
    let bottom = (top + usize::from(area.height)).min(usize::from(height));
    marked_rows(app, width, height)
        .into_iter()
        .filter(|(row, _)| *row >= top && *row < bottom)
        .collect()
}

/// The text the draft band shows, top row first: the window the wheel and an edge drag move.
/// The prompt glyph and the gutter it reserves are trimmed, so a row reads as draft text.
fn draft_band_rows(app: &mut TuiApp, width: u16, height: u16) -> Vec<String> {
    let rows = draw_rows(app, width, height);
    let area = app.input_hit_area;
    let top = usize::from(area.y);
    let bottom = (top + usize::from(area.height)).min(rows.len());
    rows[top..bottom]
        .iter()
        .map(|row| {
            row.trim_start_matches(|cell| cell == ' ' || cell == '\u{203a}')
                .to_string()
        })
        .collect()
}

/// A prepared attachment, built the way the placeholder module's own fixtures are: the draft
/// carries the token text and the payload never leaves the test.
fn fixture_attachment(source: &str) -> crate::attachments::PreparedAttachment {
    crate::attachments::PreparedAttachment {
        source: source.to_string(),
        payload: wunder_server::schemas::AttachmentPayload {
            name: Some(source.to_string()),
            content: None,
            content_type: None,
            public_path: None,
        },
        kind: crate::attachments::AttachmentKind::Image,
        size_bytes: 1,
        detail: None,
    }
}

/// The mark a composer drag leaves is a style, never a rewrite; the keystroke after the
/// release replaces the marked run and the caret ends up past what it typed, which is the
/// rule that an edit continues from the far end of the pick.
#[tokio::test]
async fn a_drag_across_the_draft_marks_the_run_and_the_next_keystroke_replaces_it() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("draft_drag_mark");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let before = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let row = usize::from(area.y);

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        area.x + 10,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        row,
        area.x + 16,
    );
    assert_eq!(
        app.input_selection_range(),
        Some((10, 16)),
        "columns 10..16 of an ASCII draft are the same bytes"
    );

    let marked = draw_rows(&mut app, 80, 24);
    assert_eq!(marked, before, "highlighting may not move a character");
    let marks = marked_draft_rows(&mut app, 80, 24);
    assert_eq!(marks.len(), 1, "one draft row carries the mark: {marks:?}");
    assert!(marks[0].1.contains("summarise"), "{marks:?}");

    // The release ends the gesture and copies what it picked, the way the transcript does.
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row,
        area.x + 16,
    );
    assert_eq!(
        ClipboardSeam::copied(),
        vec!["the fl".to_string()],
        "the release copies the marked run"
    );
    assert!(
        app.has_input_selection(),
        "and the mark stays until an edit retires it"
    );

    app.on_key(key(KeyCode::Char('x')))
        .await
        .expect("type over the mark");
    assert_eq!(app.input, "summarise xaky gate", "the run is replaced");
    assert_eq!(
        app.input_cursor, 11,
        "the caret lands after the replacement"
    );
    assert!(!app.has_input_selection());
    assert!(
        marked_draft_rows(&mut app, 80, 24).is_empty(),
        "and the frame stops marking it"
    );
}

/// Backspace and Delete take a marked run whole instead of eating one character of it.
#[tokio::test]
async fn backspace_and_delete_on_a_marked_run_remove_the_whole_selection() {
    let root = TempRoot::new("draft_delete_mark");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let row = usize::from(area.y);

    let drag = |app: &mut TuiApp, from: u16, to: u16| {
        use crossterm::event::{MouseButton as MB, MouseEventKind as MK};
        mouse(app, MK::Down(MB::Left), row, area.x + from);
        mouse(app, MK::Drag(MB::Left), row, area.x + to);
    };

    drag(&mut app, 10, 16);
    assert_eq!(app.input_selection_range(), Some((10, 16)));
    app.on_key(key(KeyCode::Backspace))
        .await
        .expect("backspace over the mark");
    assert_eq!(app.input, "summarise aky gate");
    assert_eq!(app.input_cursor, 10, "the caret parks at the run's start");
    assert!(!app.has_input_selection());

    // A fresh draft, drawn again so the pointer answers against the wrap it will see.
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);

    drag(&mut app, 13, 19);
    assert_eq!(app.input_selection_range(), Some((13, 19)));
    app.on_key(key(KeyCode::Delete))
        .await
        .expect("delete over the mark");
    assert_eq!(app.input, "summarise the gate");
    assert_eq!(app.input_cursor, 13);
    assert!(marked_draft_rows(&mut app, 80, 24).is_empty());
}

/// An arrow key collapses the mark and keeps the caret where the drag left it: the gesture
/// is over, the position is not.
#[tokio::test]
async fn an_arrow_key_after_a_drag_drops_the_mark_and_keeps_the_caret() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("draft_mark_motion");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let row = usize::from(area.y);

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        area.x + 10,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        row,
        area.x + 16,
    );
    assert_eq!(app.input_cursor, 16);
    assert!(app.has_input_selection());

    app.on_key(key(KeyCode::Left)).await.expect("left");
    assert_eq!(app.input_cursor, 15, "the caret moves from the drag's end");
    assert!(
        !app.has_input_selection(),
        "and the mark it was carrying is dropped"
    );
    assert!(marked_draft_rows(&mut app, 80, 24).is_empty());

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        area.x + 4,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        row,
        area.x + 8,
    );
    app.on_key(key(KeyCode::Right)).await.expect("right");
    assert_eq!(app.input_cursor, 9, "the same for the other direction");
    assert!(!app.has_input_selection());
}

/// Two clicks take the word, three take the whole logical line, and the columns that carry
/// them are the projected ones: a tab pads to its eight-column stop, and a click inside that
/// padding belongs to the character before it. A click past the row's last character is on
/// padding, so repeating it stays an insert point instead of turning into a pick.
#[tokio::test]
async fn a_double_click_in_the_draft_takes_the_word_and_a_triple_the_logical_line() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("draft_click_select");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    // gate 0..4, tab 4..8, 中 8..10, tab 10..16, value 16..21, space, item-2 22..28.
    app.input = "gate\t中\tvalue item-2".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let row = usize::from(area.y);

    let clicks = |app: &mut TuiApp, times: u8, column: u16| {
        use crossterm::event::{MouseButton as MB, MouseEventKind as MK};
        for _ in 0..times {
            mouse(app, MK::Down(MB::Left), row, area.x + column);
            mouse(app, MK::Up(MB::Left), row, area.x + column);
        }
    };

    // The fullwidth character, hit on one of its own columns.
    clicks(&mut app, 2, 9);
    assert_eq!(
        app.input_selection_range(),
        Some((5, 8)),
        "the one-character word 中 is the three bytes it occupies"
    );
    assert_eq!(
        ClipboardSeam::copied()
            .last()
            .map(String::as_str)
            .expect("a word pick copies on release"),
        "中"
    );

    // The second tab's padding belongs to the character before it: the same word again.
    ClipboardSeam::drain();
    clicks(&mut app, 2, 12);
    assert_eq!(app.input_selection_range(), Some((5, 8)));

    // The word that sits behind the padding, and then the whole line.
    clicks(&mut app, 2, 17);
    assert_eq!(app.input_selection_range(), Some((9, 14)), "value");
    clicks(&mut app, 3, 17);
    assert_eq!(
        app.input_selection_range(),
        Some((0, 21)),
        "three clicks take the logical line, not the wrap row"
    );

    // Padding past the last character is not text: three clicks there still just place the
    // caret, and nothing is marked.
    clicks(&mut app, 3, 30);
    assert_eq!(
        app.input_cursor, 21,
        "the caret sits at the end of the draft"
    );
    assert!(
        !app.has_input_selection(),
        "and a click on trailing padding never selects"
    );

    // A logical line means "up to the hard newline": the middle row of a three-line draft.
    app.input = "item-1\nitem-2\nitem-3".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let second = usize::from(area.y) + 1;
    for _ in 0..3 {
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            second,
            area.x + 2,
        );
        mouse(
            &mut app,
            MouseEventKind::Up(MouseButton::Left),
            second,
            area.x + 2,
        );
    }
    assert_eq!(
        app.input_selection_range(),
        Some((7, 13)),
        "the second logical line only"
    );
    assert_eq!(
        ClipboardSeam::copied().last().map(String::as_str),
        Some("item-2"),
        "and it copies that line"
    );
}

/// The wheel over the draft scrolls the draft, not the caret: the character the caret points
/// at stays the one it points at, and the transcript is untouched. Any caret motion hands the
/// band back to auto-follow.
#[tokio::test]
async fn the_wheel_over_the_draft_scrolls_it_without_moving_the_caret() {
    use crossterm::event::MouseEventKind;

    let root = TempRoot::new("draft_wheel");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    let _ = draw_rows(&mut app, 80, 24);
    let band = usize::from(app.input_hit_area.height.max(1));
    let lines = (1..=(band + 2))
        .map(|index| format!("item-{index}"))
        .collect::<Vec<_>>();
    app.input = lines.join("\n");
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let transcript_offset = app.transcript_offset_from_bottom;

    let before = draft_band_rows(&mut app, 80, 24);
    assert_eq!(
        before.first().map(String::as_str),
        Some("item-1"),
        "the band opens on the draft's first row: {before:?}"
    );

    mouse(
        &mut app,
        MouseEventKind::ScrollDown,
        usize::from(area.y) + 1,
        area.x + 2,
    );
    let travelled = lines.len() - band;
    assert_eq!(
        app.input_scroll,
        Some(travelled.min(3)),
        "one notch is three rows, clamped to the draft's own travel"
    );
    let after = draft_band_rows(&mut app, 80, 24);
    assert_ne!(after, before, "the content moved under the band: {after:?}");
    assert_eq!(
        after.first().map(String::as_str),
        lines.get(3.min(travelled)).map(String::as_str),
        "and it advanced to the row the offset names"
    );
    assert_eq!(
        app.input_cursor, 0,
        "the caret never left the first character"
    );
    assert!(
        app.input.starts_with("item-1"),
        "so the text it points at is still the draft's opening"
    );
    assert_eq!(
        app.transcript_offset_from_bottom, transcript_offset,
        "and the wheel over the draft did not scroll the transcript"
    );

    // A caret motion is the reader asking to see the caret: the band follows it again.
    app.on_key(key(KeyCode::Right)).await.expect("right");
    assert_eq!(app.input_scroll, None, "the manual offset is released");
    let followed = draft_band_rows(&mut app, 80, 24);
    assert_eq!(
        followed.first().map(String::as_str),
        Some("item-1"),
        "and the band is back on the caret's row: {followed:?}"
    );
}

/// A drag that runs off the top or bottom of the band keeps extending: the draft advances one
/// row per event and the mark grows by exactly that row, then stops at the draft's end.
#[tokio::test]
async fn a_drag_past_the_draft_band_edge_extends_the_mark_one_row_at_a_time() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("draft_edge_drag");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    let _ = draw_rows(&mut app, 80, 24);
    let band = usize::from(app.input_hit_area.height.max(1));
    let lines = (1..=(band + 2))
        .map(|index| format!("item-{index}"))
        .collect::<Vec<_>>();
    app.input = lines.join("\n");
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    // Every row is six characters plus a newline, so one row is seven bytes.
    let row_start = |row: usize| 7 * row + 2;

    // Anchor on the band's last visible row, then drag one row below it.
    let last = usize::from(area.y) + band - 1;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        last,
        area.x + 2,
    );
    assert_eq!(app.input_cursor, row_start(band - 1));
    let anchor = app.input_cursor;

    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        last + 1,
        area.x + 2,
    );
    assert_eq!(
        app.input_scroll,
        Some(1),
        "the draft advanced exactly one row"
    );
    assert_eq!(
        app.input_selection_range(),
        Some((anchor, row_start(band))),
        "and the mark grew by exactly the row that came into view"
    );

    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        last + 1,
        area.x + 2,
    );
    assert_eq!(app.input_scroll, Some(2), "one more row, one more event");
    assert_eq!(
        app.input_selection_range(),
        Some((anchor, row_start(band + 1)))
    );

    // The draft is out of rows to show: the gesture stops growing instead of racing away.
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        last + 1,
        area.x + 2,
    );
    assert_eq!(app.input_scroll, Some(2), "no travel left below the band");
    assert_eq!(
        app.input_selection_range(),
        Some((anchor, row_start(band + 1)))
    );

    // Now the other edge: anchor on the band's first visible row and drag above it.
    app.input_selection = None;
    app.input_cursor = 0;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 2,
    );
    let anchor = app.input_cursor;
    assert_eq!(
        anchor,
        row_start(2),
        "the anchor is the row the band opens on"
    );

    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        usize::from(area.y) - 1,
        area.x + 2,
    );
    assert_eq!(app.input_scroll, Some(1), "the draft went back one row");
    assert_eq!(app.input_selection_range(), Some((row_start(1), anchor)));

    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        usize::from(area.y) - 1,
        area.x + 2,
    );
    assert_eq!(app.input_scroll, Some(0), "to the top of the draft");
    assert_eq!(app.input_selection_range(), Some((row_start(0), anchor)));

    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        usize::from(area.y) - 1,
        area.x + 2,
    );
    assert_eq!(app.input_scroll, Some(0), "and no further");

    let marks = marked_draft_rows(&mut app, 80, 24);
    assert!(
        !marks.is_empty(),
        "a run that spans the band is visible on the frame"
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        usize::from(area.y) - 1,
        area.x + 2,
    );
    let copied = ClipboardSeam::copied()
        .last()
        .cloned()
        .expect("the release copies the grown run");
    assert_eq!(copied.lines().count(), 3, "{copied:?}");
    assert!(copied.contains("item-2"), "{copied:?}");
}

/// The right button copies what is marked in either region and reaches for the clipboard only
/// when nothing is: the transcript's rule, now the draft's too.
#[tokio::test]
async fn a_right_click_copies_a_marked_draft_run_and_pastes_when_there_is_none() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("draft_right_click");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    let row = usize::from(area.y);

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        area.x + 10,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        row,
        area.x + 16,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        row,
        area.x + 16,
    );
    ClipboardSeam::drain();

    app.focus_area = FocusArea::Transcript;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        row,
        area.x + 12,
    );
    assert_eq!(
        ClipboardSeam::copied(),
        vec!["the fl".to_string()],
        "the marked run goes to the clipboard"
    );
    assert_eq!(
        app.focus_area,
        FocusArea::Transcript,
        "and copying is not a focus change"
    );

    // A plain click leaves no run, so the button keeps its old job, which is paste. The
    // system read is production's own path; what a test can pin is that no mark was copied.
    // Somewhere else along the row: a second click on the same cell would be a double click.
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        area.x + 20,
    );
    assert!(
        !app.has_input_selection(),
        "a single click marks nothing, wherever it lands"
    );
    ClipboardSeam::drain();
    app.focus_area = FocusArea::Transcript;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Right),
        row,
        area.x + 20,
    );
    assert!(
        ClipboardSeam::copied().is_empty(),
        "with nothing marked the right button does not copy"
    );
    assert!(
        matches!(app.focus_area, FocusArea::Input),
        "and it falls through to the paste path"
    );
}

/// A draft replaced wholesale is not the text a gesture was made on. Every rewrite path
/// retires the mark rather than re-pointing it at bytes that name different words now.
#[tokio::test]
async fn a_rewritten_draft_retires_the_mark_instead_of_pointing_at_the_wrong_bytes() {
    let root = TempRoot::new("draft_rewrite_drops_mark");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();

    // Each draft has to be drawn before the pointer answers against its wrap, and the band
    // can move a row when a counter appears above it, so the area is read after every draw.
    let mark = |app: &mut TuiApp, draft: &str| {
        use crossterm::event::{MouseButton as MB, MouseEventKind as MK};
        app.input = draft.to_string();
        app.input_cursor = 0;
        let _ = draw_rows(app, 80, 24);
        let area = app.input_hit_area;
        let row = usize::from(area.y);
        mouse(app, MK::Down(MB::Left), row, area.x + 2);
        mouse(app, MK::Drag(MB::Left), row, area.x + 9);
        assert!(app.has_input_selection(), "the gesture marked a run first");
    };

    // Esc drops the draft.
    mark(&mut app, "summarise the flaky gate");
    assert!(app.clear_input_draft());
    assert!(!app.has_input_selection(), "a cleared draft keeps no mark");

    // A history step replaces it.
    app.history = vec!["older draft of the gate".to_string()];
    mark(&mut app, "summarise the flaky gate");
    app.history_up();
    assert_eq!(app.input, "older draft of the gate");
    assert!(!app.has_input_selection(), "a stepped draft keeps no mark");

    // Walking back down replaces it again.
    mark(&mut app, "summarise the flaky gate");
    app.history_down();
    assert!(!app.has_input_selection(), "and so does the way back");

    // Prefilling from a card is a foreign draft.
    mark(&mut app, "summarise the flaky gate");
    let user_card = app
        .logs
        .iter()
        .position(|entry| entry.kind == LogKind::User)
        .expect("the fixture has a user card");
    app.transcript_selected = Some(user_card);
    assert!(app.prefill_selected_user_message());
    assert!(
        !app.has_input_selection(),
        "a prefilled draft keeps no mark"
    );

    // Relabelling attachment placeholders rewrites the draft under the mark.
    app.pending_attachments = vec![
        fixture_attachment("gate.png"),
        fixture_attachment("item-2.png"),
    ];
    mark(&mut app, "[Image #1] [Image #2] tail");
    app.input = "[Image #1] tail".to_string();
    app.sync_attachment_placeholders();
    assert_eq!(
        app.pending_attachments.len(),
        1,
        "the placeholder that vanished retired its attachment"
    );
    assert!(
        !app.has_input_selection(),
        "and the relabel dropped the mark"
    );
    app.pending_attachments.clear();

    // Enter submits: the draft is taken away, so the mark goes with it. A whitespace-only
    // draft never reaches the engine, so the assertion stays about the draft.
    mark(&mut app, "         ");
    app.on_key(key(KeyCode::Enter)).await.expect("enter");
    assert!(app.input.is_empty(), "the draft was submitted away");
    assert!(
        !app.has_input_selection() && app.input_scroll.is_none(),
        "and the gesture went with it"
    );
}

/// The mark has to survive the width sweep the rest of the frame is held to: at every sampled
/// width a selected draft stays inside the band, moves no character, and keeps the caret in
/// the rows the reader can see.
#[tokio::test]
async fn a_marked_draft_stays_inside_the_frame_at_every_sampled_width() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("marked_draft_widths");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    for width in [20u16, 24, 40, 80, 120] {
        let height = 24u16;
        app.input = "gate\t你好 item-1 value\ttail\n中\tsecond line with a long stretch of filler text that must wrap cleanly at every sampled width".to_string();
        app.input_cursor = 0;
        app.input_selection = None;
        app.input_scroll = None;
        let before = draw_rows(&mut app, width, height);
        assert!(
            before
                .iter()
                .any(|row| row.contains("gate") && row.contains("item-")),
            "the unmarked frame opens on the draft's own text at {width} columns: {before:?}"
        );
        let area = app.input_hit_area;
        assert!(
            area.width > 0 && area.height > 0,
            "the composer kept a text area at {width} columns"
        );

        // The last row the band shows *and* the draft fills: a short draft at a wide frame
        // leaves rows below it that no pointer can answer on.
        let band = usize::from(area.height.max(1));
        let rows = app.input_hit_lines.len().max(1);
        let last_row = usize::from(area.y) + band.min(rows) - 1;
        mouse(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            usize::from(area.y),
            area.x + 2,
        );
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            last_row,
            area.x + area.width.saturating_sub(2),
        );
        assert!(
            app.has_input_selection(),
            "a drag across the band marks a run at {width} columns: caret {}, band {band}, rows {rows}, last_row {last_row}, gesture {:?}",
            app.input_cursor,
            app.input_selection,
        );

        let marked = draw_rows(&mut app, width, height);
        assert!(
            marked
                .iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no composer row may exceed the frame at {width} columns: {marked:?}"
        );
        let marks = marked_draft_rows(&mut app, width, height);
        assert!(
            !marks.is_empty(),
            "and the mark is on the draft band at {width} columns"
        );

        // The mark is a style and nothing else: with the same caret in the same window, the
        // frame the mark was taken off has to read exactly like the one it was put on.
        app.clear_input_selection();
        let unmarked = draw_rows(&mut app, width, height);
        assert_eq!(
            marked, unmarked,
            "the mark may not move a character at {width} columns"
        );

        let view = app.input_view(area.width.max(1), area.height.max(1));
        assert!(
            view.cursor_x < area.width && view.cursor_y < area.height,
            "the caret stays inside the visible band at {width} columns: {:?}",
            (view.cursor_x, view.cursor_y, area)
        );
    }
}

// ------------------------------------------------------------ the composer gap

/// The row the gap band occupies, trimmed: `None` when the band painted nothing, which is
/// also the only case in which it may ask for a row of its own.
fn gap_text(app: &mut TuiApp, width: u16, height: u16) -> Option<String> {
    let rows = draw_rows(app, width, height);
    let area = app.gap_hit_area();
    if area.is_empty() {
        return None;
    }
    Some(rows[usize::from(area.y)].trim().to_string())
}

/// Take the reader off the tail for real: anchor the tail count on a frame the reader
/// actually saw at the bottom, scroll away, then let the tail gain rows behind them.
fn detach_from_tail(app: &mut TuiApp, rows: usize) {
    let _ = draw_rows(app, 80, 24);
    app.scroll_transcript_up(4);
    assert!(
        !app.is_at_transcript_tail(),
        "the transcript can scroll at this size"
    );
    for index in 0..rows {
        app.push_log(
            LogKind::Assistant,
            format!("item-{index} gate value stays on one row"),
        );
    }
}

/// Away from the tail the band counts the rows it gained; back at the tail it is gone.
#[tokio::test]
async fn the_gap_counts_the_rows_the_tail_gained_while_the_reader_was_away() {
    let root = TempRoot::new("gap_unseen");
    let mut app = fixture_app(root.path()).await;
    // One frame at the tail anchors the baseline, and shows the band is absent there.
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "a following reader gets no band"
    );

    app.scroll_transcript_up(4);
    assert!(
        !app.is_at_transcript_tail(),
        "the transcript can scroll at this size"
    );
    let before = app.total_transcript_lines();
    for index in 0..3 {
        app.push_log(
            LogKind::Assistant,
            format!("item-{index} gate value stays on one row"),
        );
    }
    let gained = app.total_transcript_lines().saturating_sub(before);
    assert!(gained > 0, "the tail really did gain rows");

    let text = gap_text(&mut app, 80, 24).expect("the band shows once the tail is away");
    assert!(
        text.contains(&gained.to_string()),
        "the band names the {gained} rows it gained: {text:?}"
    );
    assert!(text.contains("new") && text.contains("End"), "{text:?}");

    // Returning to the tail clears the count and the band with it, at any count. The
    // count is spent when the tail has actually been painted, not on the scroll itself.
    app.scroll_transcript_to_bottom();
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "nothing to return to means no band"
    );
    assert_eq!(app.unseen_tail_lines, 0, "the count is spent at the tail");
}

/// A mark, the Ctrl+T view or a modal owns the reader's attention, so the control yields.
#[tokio::test]
async fn a_mark_in_either_region_retires_the_gap_control() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("gap_hidden");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 3);
    assert!(
        gap_text(&mut app, 80, 24).is_some(),
        "the band is up before the marks arrive"
    );

    // A drag over the transcript marks rows and stays marked without a release.
    let before = draw_rows(&mut app, 80, 24);
    let start = before
        .iter()
        .position(|row| row.contains("Ran cargo test"))
        .expect("the command card is on screen");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), start, 2);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        start + 1,
        20,
    );
    assert!(
        app.transcript_selection.is_some(),
        "the transcript carries a mark"
    );
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "a marked transcript retires the control"
    );

    // The composer's own mark does the same, and it is a different region's mark.
    app.transcript_selection = None;
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    let area = app.input_hit_area;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 1,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        usize::from(area.y),
        area.x + 8,
    );
    assert!(app.has_input_selection(), "the draft carries a mark");
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "a marked draft retires the control too"
    );

    // Ctrl+T and any modal surface own the screen, so the control is decided away even
    // though those frames never reach the composer's own band.
    app.input_selection = None;
    app.on_key(ctrl(KeyCode::Char('t')))
        .await
        .expect("open the transcript view");
    assert!(app.transcript_view_open(), "the full-transcript view is up");
    assert!(
        app.composer_gap_line(80, false).is_none(),
        "the view owns the frame"
    );
    app.on_key(ctrl(KeyCode::Char('t')))
        .await
        .expect("close the transcript view");
    app.shortcuts_visible = true;
    assert!(
        app.composer_gap_line(80, false).is_none(),
        "a modal owns the frame"
    );
    app.shortcuts_visible = false;
    assert!(
        app.composer_gap_line(80, false).is_some(),
        "and it comes back when nothing owns it"
    );
}

/// The band's rectangle is the control's hit area: a press-release on it follows the tail
/// again, and the band is gone from the frame that answers.
#[tokio::test]
async fn a_click_on_the_gap_control_follows_the_tail_again() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("gap_click");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 3);
    let _ = draw_rows(&mut app, 80, 24);

    let area = app.gap_hit_area();
    assert!(
        !area.is_empty() && area.height == 1,
        "the band owns one row"
    );
    let centre = area.x + area.width / 2;
    mouse(&mut app, MouseEventKind::Moved, usize::from(area.y), centre);
    let hovered = draw_rows(&mut app, 80, 24);
    assert!(
        marked_rows(&mut app, 80, 24)
            .iter()
            .any(|(row, _)| *row == usize::from(area.y)),
        "a pointer on the band is a state the reader can see: {hovered:?}"
    );

    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        centre,
    );
    assert!(app.gap_pressed, "the press is the control's own");
    assert!(
        app.transcript_selected.is_none(),
        "a press on the band is not a pick on a card"
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        usize::from(area.y),
        centre,
    );
    assert!(!app.gap_pressed, "and the release leaves no button held");
    assert!(app.is_at_transcript_tail(), "the release follows the tail");
    assert_eq!(app.unseen_tail_lines, 0);
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "the band is gone from the frame that answered"
    );
}

/// A hidden control paints no rectangle, so the row it would have used keeps answering
/// the region that actually owns it — here the draft under it.
#[tokio::test]
async fn a_hidden_band_leaves_its_row_to_the_draft_under_it() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("gap_passthrough");
    let mut app = fixture_app(root.path()).await;
    app.input = "summarise the flaky gate".to_string();
    app.input_cursor = 0;
    let _ = draw_rows(&mut app, 80, 24);
    assert!(
        app.gap_hit_area().is_empty(),
        "the band is hidden while the tail is visible"
    );

    let area = app.input_hit_area;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 6,
    );
    assert_eq!(
        app.input_cursor, 6,
        "the click reached the draft instead of being eaten by an invisible control"
    );
}

/// A copy answers in the band for a moment, then gives the row back. One release still
/// writes the clipboard exactly once, and its content is untouched by the feedback.
#[tokio::test]
async fn a_copy_shows_its_count_in_the_band_then_yields_the_row() {
    use crossterm::event::{MouseButton, MouseEventKind};

    let root = TempRoot::new("gap_copy_feedback");
    let mut app = fixture_app(root.path()).await;
    let _clipboard_turn = ClipboardSeam::arm();
    let before = draw_rows(&mut app, 80, 24);
    let start = before
        .iter()
        .position(|row| row.contains("Ran cargo test"))
        .expect("the command card is on screen");

    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), start, 2);
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        start + 1,
        40,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        start + 1,
        40,
    );
    let copied = ClipboardSeam::copied();
    assert_eq!(copied.len(), 1, "one release writes once: {copied:?}");
    let characters = copied[0].chars().count();

    let text = gap_text(&mut app, 80, 24).expect("the copy answers in the band");
    assert!(
        text.contains(&characters.to_string()) && text.contains("Copied"),
        "the band carries the count it copied: {text:?}"
    );

    // The mark is still up, so the control cannot show; expire the result and the row is
    // simply not there.
    app.copy_feedback
        .as_mut()
        .expect("a live copy result")
        .expires_at = Some(Instant::now());
    assert_eq!(
        gap_text(&mut app, 80, 24),
        None,
        "an expired result leaves nothing behind"
    );

    // Clear the mark, leave the tail and let it gain rows: the spent copy result yields
    // the row back to the reading control.
    app.transcript_selection = None;
    app.scroll_transcript_up(4);
    app.push_log(
        LogKind::Assistant,
        "item-9 gate value stays on one row".to_string(),
    );
    let text = gap_text(&mut app, 80, 24).expect("the control takes the row back");
    assert!(text.contains("new"), "{text:?}");
    assert!(
        !text.contains("Copied"),
        "the copy result is spent, not queued: {text:?}"
    );
}

/// An empty draft plus a detached tail is a request to come back, not a send.
#[tokio::test]
async fn an_empty_enter_from_a_detached_tail_returns_to_latest() {
    let root = TempRoot::new("gap_enter");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 3);
    let entries = app.logs.len();

    app.on_key(key(KeyCode::Enter)).await.expect("enter");
    assert!(
        app.is_at_transcript_tail(),
        "the empty press follows the tail"
    );
    assert_eq!(app.logs.len(), entries, "and sends nothing");
    assert!(app.input.is_empty());

    // A draft with text in it still submits, tail or not.
    app.scroll_transcript_up(4);
    app.input = "gate".to_string();
    app.input_cursor = app.input.len();
    app.on_key(key(KeyCode::Enter))
        .await
        .expect("submit a real draft");
    assert!(app.input.is_empty(), "the draft went out");
    assert_eq!(app.logs.len(), entries + 1, "one entry per send");
}

/// The band never costs the draft its rows and never exceeds the columns it was given.
#[tokio::test]
async fn the_gap_band_stays_inside_the_frame_at_every_sampled_width() {
    let root = TempRoot::new("gap_widths");
    for width in [20u16, 24, 40, 80, 120] {
        let height = 24u16;
        let mut app = fixture_app(root.path()).await;
        detach_from_tail(&mut app, 4);
        let rows = draw_rows(&mut app, width, height);
        let area = app.gap_hit_area();
        assert!(
            !area.is_empty(),
            "the band is up at {width} columns: {rows:?}"
        );
        assert_eq!(area.height, 1, "one row, never a wrapped second one");
        assert!(
            area.x + area.width <= width && area.y + 1 <= height,
            "the band stays inside the frame at {width} columns: {area:?}"
        );
        assert!(
            rows.iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no row may exceed the frame at {width} columns: {rows:?}"
        );
        assert!(
            app.input_hit_area.height >= 1,
            "the draft keeps a row of its own at {width} columns: {:?}",
            app.input_hit_area
        );
        let text = rows[usize::from(area.y)].trim();
        assert!(
            !text.is_empty() && UnicodeWidthStr::width(text) <= usize::from(width),
            "the band's own text fits at {width} columns: {text:?}"
        );
    }
}

/// What the band says and when it says it is decided by text and by the mark of a
/// rectangle, never by a hue: under a colourless terminal the state still reads.
#[tokio::test]
async fn the_gap_carries_its_state_without_color() {
    use crossterm::event::MouseEventKind;
    use ratatui::style::Modifier;

    let root = TempRoot::new("gap_colorless");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 3);
    assert!(
        app.unseen_tail_lines == 0,
        "the count only lands on a frame the reader was shown"
    );

    let line = app
        .composer_gap_line(80, false)
        .expect("the control is decided by state alone");
    let style = line.spans[0].style;
    assert!(
        style.fg.is_none() && style.bg.is_none(),
        "nothing for a colourless terminal to degrade: {style:?}"
    );

    let plain = draw_rows(&mut app, 80, 24);
    let area = app.gap_hit_area();
    let row = usize::from(area.y);

    // The hover state itself: reversed video and weight only, so it survives a terminal
    // where every colour degrades to Reset.
    let patch = crate::tui::theme::selection_mark().add_modifier(Modifier::BOLD);
    assert!(
        patch.fg.is_none() && patch.bg.is_none(),
        "the hover patch is a modifier, not a colour: {patch:?}"
    );
    mouse(&mut app, MouseEventKind::Moved, row, area.x + 1);
    let hovered = draw_rows(&mut app, 80, 24);
    assert_eq!(
        plain[row], hovered[row],
        "hover may not move a character of the band's text"
    );
    assert!(
        marked_rows(&mut app, 80, 24)
            .iter()
            .any(|(marked_row, text)| *marked_row == row && text.contains("new")),
        "a pointer inside the band marks its own row"
    );

    // The rectangle is the whole rule: leave it and the same row reads as unmarked again.
    mouse(&mut app, MouseEventKind::Moved, row + 1, area.x + 1);
    let elsewhere = marked_rows(&mut app, 80, 24);
    assert!(
        !elsewhere.iter().any(|(marked_row, _)| *marked_row == row),
        "a pointer off the band leaves it unmarked: {elsewhere:?}"
    );
}

// --------------------------------------------------------- transcript search
//
// The search editor is entered from the transcript focus with `/`, its scan advances one
// bounded step per frame, and its row in the composer gap reports the query and the counts.
// The helpers below drive exactly that: every keystroke is followed by a real frame, because
// the frame is the only thing that moves the scan forward.

use super::transcript_search::SearchProgress;
use super::transcript_search::SEARCH_MATCH_CAP;

/// Open the search the way the reader does: take the transcript focus, then press `/`.
async fn open_search(app: &mut TuiApp) {
    app.focus_area = FocusArea::Transcript;
    app.on_key(key(KeyCode::Char('/')))
        .await
        .expect("`/` opens the transcript search");
    assert!(app.transcript_search.editing, "the search editor is up");
}

/// One query character at a time, each followed by a frame: the count the band shows after a
/// keystroke is the count that frame's bounded scan collected.
async fn type_query(app: &mut TuiApp, text: &str, width: u16, height: u16) {
    for ch in text.chars() {
        app.on_key(key(KeyCode::Char(ch)))
            .await
            .expect("a query character");
        let _ = draw_rows(app, width, height);
    }
}

/// Type a query one key at a time *without* drawing: the scan then stands where the test
/// leaves it, so the bounded steps can be counted instead of guessed at.
async fn write_query(app: &mut TuiApp, text: &str) {
    for ch in text.chars() {
        app.on_key(key(KeyCode::Char(ch)))
            .await
            .expect("a query character");
    }
}

/// Press `/` while the transcript holds the keyboard, and Esc out again: the slash must not
/// become draft text, and the reading position the reader parked on must come back.
#[tokio::test]
async fn slash_opens_the_search_and_escape_restores_the_reading_position() {
    let root = TempRoot::new("search_enter");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 4);
    app.scroll_transcript_to_top();
    let origin = app.transcript_offset_from_bottom;
    assert!(origin > 0, "the reader is parked away from the tail");

    open_search(&mut app).await;
    assert!(
        app.input.is_empty(),
        "the slash did not fall into the draft"
    );
    assert_eq!(
        app.transcript_search.origin_offset, origin,
        "the origin is the scroll from the tail the reader left"
    );

    type_query(&mut app, "item-3", 80, 24).await;
    assert_eq!(app.transcript_search.query, "item-3");
    assert!(
        app.input.is_empty(),
        "a typed query never reaches the draft: {:?}",
        app.input
    );
    assert_eq!(
        app.transcript_search.matches.len(),
        1,
        "one card carries the query"
    );

    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit");
    assert_eq!(app.transcript_search.current, Some(0));
    assert!(
        app.is_at_transcript_tail(),
        "the newest card's line is on screen at the tail"
    );
    assert_ne!(
        app.transcript_offset_from_bottom, origin,
        "the jump really did move the window"
    );

    app.on_key(key(KeyCode::Esc))
        .await
        .expect("leave the search");
    assert!(
        !app.transcript_search.editing,
        "the editor is closed and the keys are the composer's again"
    );
    assert_eq!(
        app.transcript_offset_from_bottom, origin,
        "Esc hands the reading position back"
    );
    assert!(app.transcript_search.query.is_empty(), "nothing is left");
}

/// The count narrows as the query grows; Enter walks the hits and wraps at either end,
/// Shift+Enter walks back, and a query edit drops the accepted hit rather than keeping it.
#[tokio::test]
async fn the_query_narrows_the_hits_and_enter_walks_them_in_a_circle() {
    let root = TempRoot::new("search_walk");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(
        &mut app,
        &[
            "gate item-1 value",
            "gate item-12 value",
            "gate item-2 value",
            "no hit here",
        ],
    );
    let _ = draw_rows(&mut app, 80, 24);
    open_search(&mut app).await;

    type_query(&mut app, "item", 80, 24).await;
    assert_eq!(
        app.transcript_search.matches.len(),
        3,
        "four cards, three of them holding the query"
    );

    for expected in [1usize, 2, 3, 1] {
        app.on_key(key(KeyCode::Enter))
            .await
            .expect("step to the next hit");
        assert_eq!(
            app.transcript_search.current,
            Some(expected - 1),
            "Enter walks forward and wraps at the end"
        );
    }
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT))
        .await
        .expect("step back");
    assert_eq!(
        app.transcript_search.current,
        Some(2),
        "Shift+Enter steps back and wraps at the start"
    );

    // A longer query narrows the list, and the hit the reader stood on goes with it.
    type_query(&mut app, "-1", 80, 24).await;
    assert_eq!(
        app.transcript_search.matches.len(),
        2,
        "the query reached two cards"
    );
    assert_eq!(
        app.transcript_search.current, None,
        "a refined query has no accepted hit yet"
    );
    type_query(&mut app, "2", 80, 24).await;
    assert_eq!(
        app.transcript_search.matches.len(),
        1,
        "one card still carries the longer query"
    );

    // Backspace widens it again rather than only ever shrinking the text.
    app.on_key(key(KeyCode::Backspace))
        .await
        .expect("one character off the query");
    let _ = draw_rows(&mut app, 80, 24);
    assert_eq!(
        app.transcript_search.matches.len(),
        2,
        "the shortened query collects its hits again"
    );
}

/// Stepping to a hit scrolls its line into the band the reader sees: the row is absent from
/// the frame before the step and present after it.
#[tokio::test]
async fn stepping_to_a_hit_brings_its_line_onto_the_screen() {
    let root = TempRoot::new("search_jump");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 8);
    app.scroll_transcript_to_top();
    let before = draw_rows(&mut app, 80, 24);
    assert!(
        !before.iter().any(|row| row.contains("item-7")),
        "the newest card is off screen while the reader is at the top: {before:?}"
    );

    open_search(&mut app).await;
    type_query(&mut app, "item-7", 80, 24).await;
    assert_eq!(app.transcript_search.matches.len(), 1);

    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit");
    let after = draw_rows(&mut app, 80, 24);
    assert!(
        after.iter().any(|row| row.contains("item-7")),
        "the hit's line is in the frame after the step: {after:?}"
    );
}

/// A search editor owns the gap row: the reading control steps aside for it, and the row
/// reports without answering a pointer as a button would.
#[tokio::test]
async fn the_reading_control_gives_the_band_row_to_the_search_editor() {
    use crossterm::event::MouseButton;
    use crossterm::event::MouseEventKind;

    let root = TempRoot::new("search_gap");
    let mut app = fixture_app(root.path()).await;
    detach_from_tail(&mut app, 3);
    let text = gap_text(&mut app, 80, 24).expect("the control is up away from the tail");
    assert!(
        text.contains("new"),
        "the reading control owns the row: {text}"
    );

    open_search(&mut app).await;
    assert!(
        app.gap_search_editing(),
        "the gap asks the search editor, not a constant"
    );
    let text = gap_text(&mut app, 80, 24).expect("the band keeps its row for the query");
    assert!(
        text.contains("0/0"),
        "an empty query reports that nothing has been collected yet: {text}"
    );
    assert!(
        !text.contains("new") && !text.contains("latest"),
        "the reading control is hidden while a search owns the keyboard: {text}"
    );

    let area = app.gap_hit_area();
    mouse(
        &mut app,
        MouseEventKind::Moved,
        usize::from(area.y),
        area.x + 1,
    );
    assert!(
        !app.gap_hovered(area),
        "a reporting row carries no hover state: {area:?}"
    );
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        usize::from(area.y),
        area.x + 1,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        usize::from(area.y),
        area.x + 1,
    );
    assert!(
        !app.is_at_transcript_tail(),
        "and a press there did not send the reader to the tail"
    );
    assert!(
        app.transcript_search.editing,
        "the editor still owns the keyboard"
    );
}

/// The hit list stops at its ceiling and tells the reader so: the scan is finished, the count
/// it collected is the count it kept, and the band writes it as a floor.
#[tokio::test]
async fn the_hit_list_stops_at_its_ceiling_and_shows_it_as_a_floor() {
    let root = TempRoot::new("search_cap");
    let cards: Vec<String> = (0..(SEARCH_MATCH_CAP + 20))
        .map(|index| format!("item-{index} gate value"))
        .collect();
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, cards.as_slice());
    app.set_transcript_viewport(80, 24);
    open_search(&mut app).await;
    write_query(&mut app, "gate").await;

    // No real time and no frame guessing: the scan is stepped until it says it is through.
    loop {
        match app.advance_transcript_search(usize::MAX) {
            SearchProgress::Done => break,
            SearchProgress::More => continue,
            SearchProgress::Idle => panic!("the search closed itself mid-scan"),
        }
    }
    assert_eq!(
        app.transcript_search.matches.len(),
        SEARCH_MATCH_CAP,
        "the ceiling is the whole list, not a hint"
    );
    assert!(
        app.transcript_search.has_more,
        "the scan stopped early, which is exactly what the flag is for"
    );

    // The row is asked for directly: a frame here would hand the head of a 220-card
    // transcript to scrollback, and the addressable document is what the list covers.
    let line = app
        .composer_gap_line(80, false)
        .expect("the search row is up");
    let text = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    assert!(
        text.contains(&format!("{SEARCH_MATCH_CAP}+")),
        "the band reports the count as a floor: {text}"
    );
    assert!(
        !text.contains(&format!("{SEARCH_MATCH_CAP}/")),
        "never a count that reads as exact: {text}"
    );
}

/// One frame eats a bounded piece of the document, and the bounded frames together collect
/// exactly what a single unbounded pass collects.
#[tokio::test]
async fn one_frame_scans_a_bounded_piece_and_the_frames_together_scan_the_document() {
    let root = TempRoot::new("search_frames");
    let cards: Vec<String> = (0..60)
        .map(|index| format!("item-{index} gate value"))
        .collect();

    // One pass over the whole document, as the reference list.
    let mut once = fixture_app(root.path()).await;
    raw_single_line_cards(&mut once, cards.as_slice());
    once.set_transcript_viewport(80, 24);
    open_search(&mut once).await;
    write_query(&mut once, "gate").await;
    assert_eq!(
        once.advance_transcript_search(usize::MAX),
        SearchProgress::Done,
        "an unbounded budget reads the document in one step"
    );
    let reference = once.transcript_search.matches.clone();
    assert_eq!(reference.len(), 60, "one hit per card");

    // The same document, one small budget at a time.
    let mut stepped = fixture_app(root.path()).await;
    raw_single_line_cards(&mut stepped, cards.as_slice());
    stepped.set_transcript_viewport(80, 24);
    open_search(&mut stepped).await;
    write_query(&mut stepped, "gate").await;

    assert_eq!(
        stepped.advance_transcript_search(64),
        SearchProgress::More,
        "a bounded frame hands the document back unfinished"
    );
    let first_frame = stepped.transcript_search.matches.len();
    assert!(
        first_frame > 0 && first_frame < reference.len(),
        "the frame collected something but not everything: {first_frame}"
    );
    assert!(
        !stepped.transcript_search.scan_done,
        "and it left the cursor inside the document"
    );

    let mut frames = 1usize;
    loop {
        match stepped.advance_transcript_search(64) {
            SearchProgress::Done => break,
            SearchProgress::More => frames += 1,
            SearchProgress::Idle => panic!("the search closed itself mid-scan"),
        }
    }
    assert!(
        frames > 2,
        "the same document took {frames} bounded frames, never one"
    );
    assert_eq!(
        stepped.transcript_search.matches, reference,
        "frame by frame must equal one pass, in the same order"
    );
}

/// A scan is retired when the document it read moved — an append or a trim — and is kept when
/// only the window moved. Scroll position is not document identity.
#[tokio::test]
async fn a_moved_document_rescans_and_a_moved_window_does_not() {
    let root = TempRoot::new("search_stale");
    let cards: Vec<String> = (0..30)
        .map(|index| format!("item-{index} gate value"))
        .collect();
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, cards.as_slice());
    app.set_transcript_viewport(80, 24);
    open_search(&mut app).await;
    write_query(&mut app, "gate").await;
    assert_eq!(
        app.advance_transcript_search(usize::MAX),
        SearchProgress::Done
    );
    let hits = app.transcript_search.matches.len();
    assert_eq!(hits, 30);

    // A scroll moves the window, not the document: the finished scan stands, and a frame with
    // a one-byte budget proves there is nothing left to read rather than a restart.
    let revision = app.transcript_render_revision;
    app.scroll_transcript_up(1);
    assert!(
        !app.is_at_transcript_tail(),
        "the window really did move off the tail"
    );
    assert_eq!(
        app.transcript_render_revision, revision,
        "a scroll is not a document change"
    );
    assert_eq!(
        app.advance_transcript_search(1),
        SearchProgress::Done,
        "a finished scan is not run again by a scroll"
    );
    assert_eq!(app.transcript_search.matches.len(), hits);

    // An append is a document change: the list restarts and reads the new card.
    app.push_log(LogKind::User, "gate item-30 value".to_string());
    assert_ne!(
        app.transcript_render_revision, revision,
        "an append bumps the document revision"
    );
    assert_eq!(
        app.advance_transcript_search(1),
        SearchProgress::More,
        "the scan starts over, so one byte is not the whole document"
    );
    assert!(
        app.transcript_search.matches.len() < hits + 1,
        "nothing stale is left from the previous pass: {}",
        app.transcript_search.matches.len()
    );
    loop {
        match app.advance_transcript_search(usize::MAX) {
            SearchProgress::Done => break,
            SearchProgress::More => continue,
            SearchProgress::Idle => panic!("the search closed itself mid-scan"),
        }
    }
    assert_eq!(
        app.transcript_search.matches.len(),
        hits + 1,
        "the appended card is collected"
    );

    // A trim is one too: the hit that named the trimmed card has no text left to name.
    app.pop_oldest_log_entry();
    loop {
        match app.advance_transcript_search(usize::MAX) {
            SearchProgress::Done => break,
            SearchProgress::More => continue,
            SearchProgress::Idle => panic!("the search closed itself mid-scan"),
        }
    }
    assert_eq!(
        app.transcript_search.matches.len(),
        hits,
        "the trimmed card's hit went with it"
    );
}

/// The row carries the query and its place in the hits at every sampled width: it sheds the
/// query's tail before it sheds the row, never spills past the frame, and never takes the
/// draft's last row.
#[tokio::test]
async fn the_search_row_carries_the_query_and_the_counts_at_every_sampled_width() {
    let root = TempRoot::new("search_widths");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(
        &mut app,
        &[
            "gate item-1 value",
            "gate item-2 value",
            "a plain card with no query in it",
        ],
    );
    let _ = draw_rows(&mut app, 80, 24);
    open_search(&mut app).await;

    // Nothing typed yet, at the narrowest sampled frame: still one row, still readable.
    let text = gap_text(&mut app, 20, 24).expect("the row is up with an empty query");
    assert!(
        text.contains("0/0"),
        "an empty query reports no hits rather than nothing: {text}"
    );

    type_query(&mut app, "gate item", 20, 24).await;
    assert_eq!(app.transcript_search.matches.len(), 2);
    for width in [20u16, 24, 40, 80, 120] {
        let rows = draw_rows(&mut app, width, 24);
        let area = app.gap_hit_area();
        assert!(!area.is_empty(), "the search row is up at {width} columns");
        assert_eq!(area.height, 1, "one row, never a wrapped second one");
        assert!(
            area.x + area.width <= width && area.y + 1 <= 24,
            "the row stays inside the frame at {width} columns: {area:?}"
        );
        assert!(
            rows.iter()
                .all(|row| row.chars().count() <= usize::from(width)),
            "no row may exceed the frame at {width} columns: {rows:?}"
        );
        assert!(
            app.input_hit_area.height >= 1,
            "the draft keeps a row of its own at {width} columns: {:?}",
            app.input_hit_area
        );
        let text = rows[usize::from(area.y)].trim();
        assert!(
            UnicodeWidthStr::width(text) <= usize::from(width),
            "the row's own text fits at {width} columns: {text:?}"
        );
        assert!(
            text.contains("0/2"),
            "the counts are the scan's own at {width} columns: {text:?}"
        );
    }

    // The accepted hit's ordinal rides along, and a query too long for the frame sheds its
    // tail instead of spilling or disappearing. The frame stays at the loop's last width: a
    // resize re-wraps the document, which restarts the scan and retires the accepted hit.
    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit");
    let text = gap_text(&mut app, 120, 24).expect("the row is up");
    assert!(
        text.contains("1/2"),
        "the band shows the hit's place: {text}"
    );

    write_query(&mut app, " a long query that runs past the frame").await;
    let rows = draw_rows(&mut app, 20, 24);
    let area = app.gap_hit_area();
    assert!(!area.is_empty(), "a long query still owns its row");
    let text = rows[usize::from(area.y)].trim();
    assert!(
        text.contains('\u{2026}'),
        "the query sheds its tail before the row goes: {text:?}"
    );
    assert!(
        UnicodeWidthStr::width(text) <= 20,
        "and the shed row stays inside twenty columns: {text:?}"
    );
}

// ------------------------------------------------ transcript search: presentation
//
// A collected hit has to be *readable*: the frame marks the columns the query covers, the hit
// the reader stepped to carries more weight than the rest, a hit a fold is hiding opens that
// card for the length of the search, and an open pointer selection owns the text it covers.
// Every claim below is about cells a real `ui::draw` painted, and about modifiers rather than
// colours, so the same reading holds when `NO_COLOR` is set.

use super::transcript_search::SEARCH_SCAN_BYTES;

/// One cell as the terminal received it: what is drawn there, and the two weights a mark can
/// put on it. Reversed video is the mark itself, bold is the extra weight the accepted hit
/// carries. No colour is read, because a colour is what `NO_COLOR` takes away.
struct FrameCell {
    symbol: String,
    marked: bool,
    heavy: bool,
}

/// A drawn frame, kept column by column, with the rows the transcript band occupies. A footer
/// that echoes a card's text and the search row that echoes the query are outside that range,
/// so a mark found inside it is a mark on the document.
struct Frame {
    band: std::ops::Range<usize>,
    rows: Vec<Vec<FrameCell>>,
}

impl Frame {
    fn capture(app: &mut TuiApp, width: u16, height: u16) -> Self {
        use ratatui::style::Modifier;
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|f| crate::tui::ui::draw(f, app))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let rows = (0..height)
            .map(|row| {
                (0..width)
                    .map(|column| {
                        let cell = &buffer[(column, row)];
                        FrameCell {
                            symbol: cell.symbol().to_string(),
                            marked: cell.modifier.contains(Modifier::REVERSED),
                            heavy: cell.modifier.contains(Modifier::BOLD),
                        }
                    })
                    .collect()
            })
            .collect::<Vec<_>>();
        let area = app.transcript_mouse_region;
        let top = usize::from(area.y).min(usize::from(height));
        let bottom = (top + usize::from(area.height)).min(usize::from(height));
        Self {
            band: top..bottom,
            rows,
        }
    }

    /// The text of one frame row, trailing padding shed.
    fn text(&self, row: usize) -> String {
        self.rows[row]
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    fn rows_text(&self) -> Vec<String> {
        (0..self.rows.len()).map(|row| self.text(row)).collect()
    }

    /// The document rows the band is showing, empty padding shed. Two captures of the same
    /// document read the same whether or not anything is marked — and only the band is
    /// compared, because a frame that gained a status row lays the transcript out taller or
    /// shorter than the frame before it.
    fn card_rows(&self) -> Vec<String> {
        self.band
            .clone()
            .map(|row| self.text(row))
            .filter(|text| !text.is_empty())
            .collect()
    }

    /// Every run of `needle` inside the band: the row it is on, the column it starts at, and
    /// the marks the cells that cover it carry. The needles a fixture writes are one-column
    /// characters, so a column and a character index are the same number here.
    fn runs(&self, needle: &str) -> Vec<(usize, usize, Vec<(bool, bool)>)> {
        let mut found = Vec::new();
        let needle_chars: Vec<char> = needle.chars().collect();
        for row in self.band.clone() {
            let cells = &self.rows[row];
            let parts = cells
                .iter()
                .map(|cell| cell.symbol.as_str())
                .collect::<Vec<_>>();
            let mut start = 0usize;
            while start + needle_chars.len() <= parts.len() {
                if parts[start..start + needle_chars.len()]
                    .iter()
                    .map(|symbol| symbol.chars().next())
                    .eq(needle_chars.iter().copied().map(Some))
                {
                    found.push((
                        row,
                        start,
                        cells[start..start + needle_chars.len()]
                            .iter()
                            .map(|cell| (cell.marked, cell.heavy))
                            .collect(),
                    ));
                }
                start += 1;
            }
        }
        found
    }

    /// Whether nothing in the band carries a mark at all.
    fn is_clean(&self) -> bool {
        self.band
            .clone()
            .all(|row| self.rows[row].iter().all(|cell| !cell.marked))
    }
}

/// A one-line drag across the row that shows `needle`, from its first column to the last
/// column of the word a search would be marking. Returns what the release put on the
/// clipboard, so a test can compare the copy with and without a search painting behind it.
fn drag_over_run(app: &mut TuiApp, needle: &str, width: u16, height: u16) -> String {
    use crossterm::event::{MouseButton, MouseEventKind};
    let frame = Frame::capture(app, width, height);
    let (row, start) = frame
        .runs(needle)
        .first()
        .map(|(row, start, _)| (*row, *start))
        .unwrap_or_else(|| {
            panic!(
                "{needle} is not on a transcript row: {:?}",
                frame.rows_text()
            )
        });
    let end = (start + needle.chars().count()) as u16;
    let region = app.transcript_mouse_region;
    ClipboardSeam::drain();
    mouse(
        app,
        MouseEventKind::Down(MouseButton::Left),
        row,
        region.x + start as u16,
    );
    mouse(
        app,
        MouseEventKind::Drag(MouseButton::Left),
        row,
        region.x + end,
    );
    mouse(
        app,
        MouseEventKind::Up(MouseButton::Left),
        row,
        region.x + end,
    );
    ClipboardSeam::copied()
        .pop()
        .unwrap_or_else(|| panic!("the release copied the drag"))
}

/// A document with one card that hides most of itself: a finished thinking block, whose folded
/// rows are a head plus two hint rows, and a tail card with nothing to fold.
fn folded_thinking_card(app: &mut TuiApp) {
    app.logs.clear();
    app.raw_output = false;
    app.reset_scrollback_archive();
    let body = (1..=12)
        .map(|index| format!("reasoning line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.push_log(LogKind::Reasoning, body);
    app.push_log(LogKind::Assistant, "a tail card with no query".to_string());
    app.transcript_offset_from_bottom = 0;
    app.transcript_selected = None;
    app.expanded_cards.clear();
    app.search_expanded_cards.clear();
}

/// The frame marks every run the query covers, and the run the reader stepped to carries more
/// weight: reversed video everywhere, bold on the accepted hit only, and the weight moves when
/// Enter moves. A mark never moves a character, and leaving the search paints nothing.
#[tokio::test]
async fn the_frame_marks_every_hit_and_the_accepted_one_carries_more_weight() {
    let root = TempRoot::new("search_mark_weight");
    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(
        &mut app,
        &[
            "gate item-1 value",
            "a card with nothing to find",
            "gate item-2 value",
        ],
    );
    let plain = Frame::capture(&mut app, 80, 24);
    assert!(plain.is_clean(), "nothing is marked before a search");
    let plain_rows = plain.card_rows();
    open_search(&mut app).await;
    type_query(&mut app, "gate", 80, 24).await;
    assert_eq!(app.transcript_search.matches.len(), 2, "one hit per card");

    let marked = Frame::capture(&mut app, 80, 24);
    assert_eq!(
        marked.card_rows(),
        plain_rows,
        "a mark is a style, never a rewrite"
    );
    let runs = marked.runs("gate");
    assert_eq!(runs.len(), 2, "both hits are on screen: {runs:?}");
    for (row, start, cells) in &runs {
        assert!(
            cells.iter().all(|(cell_marked, _)| *cell_marked),
            "row {row} column {start} reads as a hit: {cells:?}"
        );
        assert!(
            cells.iter().all(|(_, heavy)| !heavy),
            "nothing is accepted yet, so nothing is heavier: {cells:?}"
        );
        assert!(
            !marked.rows[*row][start + cells.len()].marked,
            "the mark is the run the query covers, not the row it sits on: row {row}"
        );
    }

    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the first hit");
    let runs = Frame::capture(&mut app, 80, 24).runs("gate");
    assert!(
        runs[0]
            .2
            .iter()
            .all(|(cell_marked, heavy)| *cell_marked && *heavy),
        "the accepted hit is the mark plus weight: {:?}",
        runs[0].2
    );
    assert!(
        runs[1]
            .2
            .iter()
            .all(|(cell_marked, heavy)| *cell_marked && !heavy),
        "and every other hit keeps the plain mark: {:?}",
        runs[1].2
    );

    // Shift+Enter walks back, and the weight goes with it.
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT))
        .await
        .expect("step back");
    let runs = Frame::capture(&mut app, 80, 24).runs("gate");
    assert!(
        runs[0].2.iter().all(|(_, heavy)| !heavy),
        "the hit the reader left is plain again: {:?}",
        runs[0].2
    );
    assert!(
        runs[1]
            .2
            .iter()
            .all(|(cell_marked, heavy)| *cell_marked && *heavy),
        "the weight is on the hit they are standing on now: {:?}",
        runs[1].2
    );

    // The frame budget the scan is bounded by is still the one the scan runs on, so a mark
    // cannot be paid for by reading the whole document in a frame.
    assert_eq!(app.transcript_search.frame_budget, SEARCH_SCAN_BYTES);

    app.on_key(key(KeyCode::Esc))
        .await
        .expect("leave the search");
    let after = Frame::capture(&mut app, 80, 24);
    assert!(after.is_clean(), "a finished search paints nothing");
    assert_eq!(after.card_rows(), plain_rows, "the frame reads as it did");
}

/// An open pointer selection owns the text it covers: a hit inside it shows the selection's
/// mark and none of the search's weight, because the selection is what the reader copies. The
/// copy is the same text with a search up as with none, and a hit the selection never reaches
/// still shows.
#[tokio::test]
async fn an_open_selection_paints_over_a_hit_and_copies_the_same_text() {
    let root = TempRoot::new("search_mark_selection");
    let _clipboard_turn = ClipboardSeam::arm();

    // The same gesture on the same document, once with no search and once with a search
    // painting behind it: the clipboard cannot tell the difference.
    let mut plain = fixture_app(root.path()).await;
    raw_single_line_cards(&mut plain, &["gate item-1 value", "gate item-2 value"]);
    let copied_plain = drag_over_run(&mut plain, "gate", 80, 24);

    let mut app = fixture_app(root.path()).await;
    raw_single_line_cards(&mut app, &["gate item-1 value", "gate item-2 value"]);
    let before_rows = Frame::capture(&mut app, 80, 24).card_rows();
    open_search(&mut app).await;
    type_query(&mut app, "gate", 80, 24).await;
    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the first hit");
    let runs = Frame::capture(&mut app, 80, 24).runs("gate");
    assert!(
        runs[0]
            .2
            .iter()
            .all(|(cell_marked, heavy)| *cell_marked && *heavy),
        "the accepted hit carries its weight before anything is picked up: {:?}",
        runs[0].2
    );

    let copied_searched = drag_over_run(&mut app, "gate", 80, 24);
    assert_eq!(
        copied_searched, copied_plain,
        "the copy is the selection's text; a search mark never adds to it"
    );

    let after = Frame::capture(&mut app, 80, 24);
    let runs = after.runs("gate");
    assert!(
        runs[0].2.iter().all(|(cell_marked, _)| *cell_marked),
        "the picked-up text is still marked: {:?}",
        runs[0].2
    );
    assert!(
        runs[0].2.iter().all(|(_, heavy)| !heavy),
        "and the selection is the only weight on it — the mark that wins is the one the \
         reader copies: {:?}",
        runs[0].2
    );
    assert!(
        runs[1].2.iter().all(|(cell_marked, _)| *cell_marked),
        "the hit the selection never reaches keeps its mark"
    );
    assert_eq!(
        after.card_rows(),
        before_rows,
        "and neither mark moved a character"
    );
}

/// A hit the fold is hiding opens its card for as long as the search lasts, and the card is
/// folded again on the way out. The reader's own disclosure ledger is never written.
#[tokio::test]
async fn a_hit_behind_a_fold_opens_its_card_and_the_search_closes_it_again() {
    let root = TempRoot::new("search_reveal");
    let mut app = fixture_app(root.path()).await;
    folded_thinking_card(&mut app);
    let folded = draw_rows(&mut app, 80, 24);
    assert!(
        !folded.iter().any(|row| row.contains("reasoning line 12")),
        "the card starts folded: {folded:?}"
    );
    assert!(app.expanded_cards.is_empty(), "the reader opened nothing");

    open_search(&mut app).await;
    type_query(&mut app, "show", 80, 24).await;
    assert_eq!(
        app.transcript_search.matches.len(),
        1,
        "the hint row a fold leaves behind is the one hit in the document"
    );
    assert!(
        app.search_expanded_cards.is_empty(),
        "a hit is not opened until the reader walks to it"
    );

    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit");
    assert_eq!(
        app.search_expanded_cards.len(),
        1,
        "the search opened the card it walked into"
    );
    assert!(
        app.expanded_cards.is_empty(),
        "and it never wrote the reader's own ledger"
    );
    let opened = draw_rows(&mut app, 80, 24);
    assert!(
        opened.iter().any(|row| row.contains("reasoning line 12")),
        "the rest of the card is readable now: {opened:?}"
    );

    // A fold the reader closes by hand takes the borrowed opening back with it: two ledgers
    // must not keep a card open behind their back. The disclosure row is the pointer's entry
    // to the same control the arrow keys answer, and it stays live while the editor owns the
    // keyboard.
    let (row, _) = {
        let frame = Frame::capture(&mut app, 80, 24);
        let runs = frame.runs("Show");
        (runs[0].0, runs[0].1)
    };
    use crossterm::event::{MouseButton, MouseEventKind};
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), row, 2);
        mouse(&mut app, MouseEventKind::Up(MouseButton::Left), row, 2);
        let _ = draw_rows(&mut app, 80, 24);
    }
    assert!(
        app.expanded_cards.is_empty(),
        "the reader's own ledger is back to nothing"
    );
    assert!(
        app.search_expanded_cards.is_empty(),
        "and the opening the search borrowed went with the hand that closed it"
    );
    let closed = draw_rows(&mut app, 80, 24);
    assert!(
        !closed.iter().any(|row| row.contains("reasoning line 12")),
        "the card is folded again under their hand: {closed:?}"
    );

    app.on_key(key(KeyCode::Esc))
        .await
        .expect("leave the search");
    assert!(
        app.search_expanded_cards.is_empty(),
        "an opening the search borrowed is handed back"
    );
    assert!(
        app.expanded_cards.is_empty(),
        "the reader's ledger is empty, as it always was"
    );
    let refolded = draw_rows(&mut app, 80, 24);
    assert!(
        !refolded.iter().any(|row| row.contains("reasoning line 12")),
        "the card is folded again: {refolded:?}"
    );
}

/// The card goes, the borrowed opening goes with it: a trimmed card leaves nothing in the
/// search's ledger to point at whatever now sits where it was.
#[tokio::test]
async fn a_trimmed_card_takes_its_borrowed_opening_with_it() {
    let root = TempRoot::new("search_reveal_trim");
    let mut app = fixture_app(root.path()).await;
    folded_thinking_card(&mut app);
    open_search(&mut app).await;
    type_query(&mut app, "show", 80, 24).await;
    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit");
    let opened = *app
        .search_expanded_cards
        .iter()
        .next()
        .expect("the search opened the card");
    assert_eq!(
        Some(opened),
        app.logs.first().map(|entry| entry.cell_id),
        "and it is the card the hit is on"
    );

    app.pop_oldest_log_entry();
    assert!(
        app.search_expanded_cards.is_empty(),
        "the card is gone, so the opening it held is gone: {:?}",
        app.search_expanded_cards
    );
    assert!(
        app.expanded_cards.is_empty(),
        "and the reader's ledger never heard about either"
    );
    let frame = Frame::capture(&mut app, 80, 24);
    assert!(
        frame.is_clean(),
        "nothing stale is left marked: {:?}",
        frame.rows_text()
    );
    assert!(
        !frame
            .rows_text()
            .iter()
            .any(|row| row.contains("reasoning line")),
        "and nothing of the trimmed card is left on screen: {:?}",
        frame.rows_text()
    );
}

/// Opening a card moves the lines under the hit it opened for, so the scan that found that hit
/// is retired and run again — and the mark the reader walks to lands on the query's own text,
/// not on whatever the old numbering put at the old line.
#[tokio::test]
async fn opening_a_card_for_a_hit_renumbers_the_hits_and_the_mark_follows_the_text() {
    let root = TempRoot::new("search_refold");
    let mut app = fixture_app(root.path()).await;
    folded_thinking_card(&mut app);
    open_search(&mut app).await;
    type_query(&mut app, "show", 80, 24).await;
    let folded = Frame::capture(&mut app, 80, 24);
    let runs = folded.runs("Show");
    assert_eq!(runs.len(), 1, "the hint row is the hit: {runs:?}");
    assert!(
        runs[0].2.iter().all(|(cell_marked, _)| *cell_marked),
        "and it is marked where it stands: {:?}",
        runs[0].2
    );
    let hint_before = folded.text(runs[0].0);
    assert!(
        hint_before.contains("Show thinking"),
        "folded, the hint row offers to open: {hint_before}"
    );

    app.on_key(key(KeyCode::Enter))
        .await
        .expect("step to the hit, opening the card");
    // The frame the step painted is the frame the fold retired: its hits are the old
    // numbering's, and a hit the text no longer agrees with paints nothing rather than marking
    // the wrong row. The rescan lands on the next frame, and the mark comes back on the text.
    let _ = draw_rows(&mut app, 80, 24);
    let opened = Frame::capture(&mut app, 80, 24);
    let runs = opened.runs("Show");
    assert_eq!(
        runs.len(),
        1,
        "the query still names one row, now the one that closes the card: {runs:?}"
    );
    let hint_after = opened.text(runs[0].0);
    assert!(
        hint_after.contains("Show less"),
        "the card is open, so the hint row reads the other way: {hint_after}"
    );
    assert!(
        runs[0].2.iter().all(|(cell_marked, _)| *cell_marked),
        "the mark is on that row, the one that carries the query: {:?}",
        runs[0].2
    );
    assert!(
        !opened
            .runs("reasoning")
            .into_iter()
            .any(|(_, _, cells)| cells.iter().any(|(cell_marked, _)| *cell_marked)),
        "and not on one body row the new numbering put at the old line index"
    );

    // The reader's place survives the rescan rather than resetting under their cursor.
    assert_eq!(
        app.transcript_search.current,
        Some(0),
        "the accepted hit came back with the rebuilt list"
    );
    assert!(
        app.transcript_search.resume.is_none(),
        "and the place it was handed back from is spent"
    );

    // A scan the fold retired is a scan that ran again on the frame budget, not on the whole
    // document at once.
    assert_eq!(app.transcript_search.frame_budget, SEARCH_SCAN_BYTES);
    // And the numbering really did move, asserted by text because a line number means nothing
    // across a fold: the row the folded hint occupied is a body row of the opened card now,
    // and a body row carries no mark.
    let body = opened.runs("reasoning line 5");
    assert_eq!(
        body.len(),
        1,
        "the opened card shows the row the hint used to be numbered against: {:?}",
        opened.rows_text()
    );
    assert!(
        body[0].2.iter().all(|(cell_marked, _)| !*cell_marked),
        "and nothing marks it: {:?}",
        body[0].2
    );
}

/// A query and an opened card at every width the client lays out for: the transcript never
/// spills past the columns the terminal gave it, the draft is never squeezed out, and the band
/// keeps its row.
#[tokio::test]
async fn a_query_and_an_opened_card_stay_inside_every_sampled_width() {
    let root = TempRoot::new("search_reveal_widths");
    for width in [20u16, 24, 40, 80, 120] {
        let mut app = fixture_app(root.path()).await;
        folded_thinking_card(&mut app);
        app.push_log(
            LogKind::Assistant,
            "value with a longer body that has to wrap at every one of these widths, and keeps \
             going so even twenty columns has several rows of it to lay out"
                .to_string(),
        );
        open_search(&mut app).await;
        type_query(&mut app, "show", width, 24).await;
        app.on_key(key(KeyCode::Enter))
            .await
            .expect("step to the hit, opening the card");
        assert_eq!(
            app.search_expanded_cards.len(),
            1,
            "the card is open at {width} columns"
        );
        let rows = draw_rows(&mut app, width, 24);
        assert!(
            rows.iter()
                .all(|row| UnicodeWidthStr::width(row.as_str()) <= usize::from(width)),
            "no row may exceed the frame at {width} columns: {rows:?}"
        );
        assert!(
            app.input_hit_area.height >= 1,
            "the draft keeps a row of its own at {width} columns: {:?}",
            app.input_hit_area
        );
        let area = app.gap_hit_area();
        assert!(
            !area.is_empty() && area.x + area.width <= width,
            "the search row is up and inside the frame at {width} columns: {area:?}"
        );
        let frame = Frame::capture(&mut app, width, 24);
        let runs = frame.runs("Show");
        assert!(
            runs.iter().all(|(row, start, cells)| *row < 24
                && start + cells.len() <= usize::from(width)
                && cells.iter().all(|(cell_marked, _)| *cell_marked)),
            "a mark stays inside the frame and inside the text it marks at {width} columns: \
             {runs:?}"
        );
    }
}

#[tokio::test]
async fn model_add_registers_the_endpoint_in_the_shared_store() {
    let root = TempRoot::new("model_add_line");
    let mut app = fixture_app(root.path()).await;
    let before = app
        .runtime
        .state
        .config_store
        .get()
        .await
        .llm
        .default
        .clone();

    app.handle_slash_command(
        "/model add https://api.example.test/v1 sk-fixture-model demo-chat-model 32000".to_string(),
    )
    .await
    .expect("model add");

    let config = app.runtime.state.config_store.get().await;
    let entry = config
        .llm
        .models
        .get("demo-chat-model")
        .expect("the registered entry");
    assert_eq!(
        entry.base_url.as_deref(),
        Some("https://api.example.test/v1")
    );
    assert_eq!(entry.api_key.as_deref(), Some("sk-fixture-model"));
    assert_eq!(entry.model.as_deref(), Some("demo-chat-model"));
    assert_eq!(entry.model_type.as_deref(), Some("llm"));
    assert_eq!(entry.enable, Some(true));
    assert_eq!(entry.max_context, Some(32000));
    assert_eq!(config.llm.default, "demo-chat-model");
    assert_ne!(
        before, config.llm.default,
        "a new model becomes the default"
    );

    // The user layer has to state the same endpoint, otherwise the next start
    // projects a config without it and the entry reads as unreachable.
    let user_toml = fs::read_to_string(root.path().join("config.toml")).unwrap_or_default();
    assert!(user_toml.contains("demo-chat-model"), "{user_toml}");
    assert!(
        user_toml.contains("https://api.example.test/v1"),
        "the endpoint must land in the user layer: {user_toml}"
    );
}

#[tokio::test]
async fn model_add_collects_the_missing_fields_one_answer_at_a_time() {
    fn asked(app: &TuiApp, step: &str) -> bool {
        app.logs
            .iter()
            .any(|entry| entry.text.contains(&format!("step {step}/4")))
    }

    let root = TempRoot::new("model_add_wizard");
    let mut app = fixture_app(root.path()).await;

    app.handle_slash_command("/model add".to_string())
        .await
        .expect("open the wizard");
    assert!(
        app.model_wizard.is_some(),
        "/model add must open the wizard"
    );
    assert!(asked(&app, "1"), "the first question is base_url");

    app.handle_model_wizard_input("https://api.example.test/v1")
        .await
        .expect("base_url");
    assert!(asked(&app, "2"), "the second question is api_key");
    app.handle_model_wizard_input("sk-fixture-model")
        .await
        .expect("api_key");
    assert!(asked(&app, "3"), "the third question is the model name");
    app.handle_model_wizard_input("wizard-chat-model")
        .await
        .expect("model name");
    assert!(asked(&app, "4"), "the last question is optional");
    app.handle_model_wizard_input("16000")
        .await
        .expect("max_context");

    assert!(app.model_wizard.is_none(), "a finished wizard closes");
    let config = app.runtime.state.config_store.get().await;
    assert_eq!(config.llm.default, "wizard-chat-model");
    assert_eq!(
        config
            .llm
            .models
            .get("wizard-chat-model")
            .and_then(|entry| entry.max_context),
        Some(16000)
    );
}

#[tokio::test]
async fn model_add_seeds_the_wizard_with_what_the_caller_already_typed() {
    let root = TempRoot::new("model_add_seed");
    let mut app = fixture_app(root.path()).await;

    app.handle_slash_command("/model add https://api.example.test/v1 sk-fixture-model".to_string())
        .await
        .expect("partial endpoint");

    let wizard = app.model_wizard.as_ref().expect("seeded wizard");
    assert_eq!(
        wizard.base_url.as_deref(),
        Some("https://api.example.test/v1")
    );
    assert_eq!(wizard.api_key.as_deref(), Some("sk-fixture-model"));
    assert!(wizard.model_name.is_none());
    let texts: Vec<String> = app.logs.iter().map(|entry| entry.text.clone()).collect();
    let asked = |step: &str| {
        texts
            .iter()
            .any(|text| text.contains(&format!("step {step}/4")))
    };
    assert!(
        asked("3"),
        "the wizard must ask for the model name: {texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|text| text.contains("input model name (empty line to cancel)")),
        "the question must name the field: {texts:?}"
    );
    assert!(
        !asked("1"),
        "a seeded base_url must not be asked again: {texts:?}"
    );
    assert!(
        !asked("2"),
        "a seeded api_key must not be asked again: {texts:?}"
    );
}

#[tokio::test]
async fn a_model_registered_for_another_job_is_not_switchable() {
    let root = TempRoot::new("model_switch_guard");
    let mut app = fixture_app(root.path()).await;
    // 蛹 writes embedding and speech entries into the same engine file.
    app.runtime
        .state
        .config_store
        .update(|config| {
            config.llm.models.insert(
                "fixture-embedder".to_string(),
                wunder_server::config::LlmModelConfig {
                    model_type: Some("embedding".to_string()),
                    ..Default::default()
                },
            );
        })
        .await
        .expect("seed a non-chat entry");
    let before = app
        .runtime
        .state
        .config_store
        .get()
        .await
        .llm
        .default
        .clone();

    app.handle_slash_command("/model fixture-embedder".to_string())
        .await
        .expect("switch attempt");

    assert_eq!(
        app.runtime.state.config_store.get().await.llm.default,
        before,
        "a non-chat entry must not become the session model"
    );
    assert!(
        !crate::sorted_model_names(&app.runtime.state.config_store.get().await)
            .contains(&"fixture-embedder".to_string()),
        "the model list must not offer an embedding entry"
    );
}

#[tokio::test]
async fn config_no_longer_registers_an_endpoint() {
    let root = TempRoot::new("config_endpoint_retired");
    let mut app = fixture_app(root.path()).await;

    app.handle_slash_command(
        "/config https://api.example.test/v1 sk-fixture-model retired-model 32000".to_string(),
    )
    .await
    .expect("retired form");

    let config = app.runtime.state.config_store.get().await;
    assert!(
        !config.llm.models.contains_key("retired-model"),
        "/config must not write model entries any more"
    );
    assert!(
        app.logs
            .iter()
            .any(|entry| entry.text.contains("/model add")),
        "the refusal must name the command that replaced it"
    );
}

#[tokio::test]
async fn model_add_refuses_an_endpoint_that_breaks_the_shared_bounds() {
    let root = TempRoot::new("model_add_bounds");
    let mut app = fixture_app(root.path()).await;
    let long_key = format!("sk-{}", "x".repeat(8_192));

    let outcome = app
        .handle_slash_command(format!(
            "/model add https://api.example.test/v1 {long_key} demo-chat-model 32000"
        ))
        .await;

    assert!(outcome.is_err(), "an over-long key must be refused");
    let config = app.runtime.state.config_store.get().await;
    assert!(
        !config.llm.models.contains_key("demo-chat-model"),
        "a refused endpoint must not reach the shared engine file"
    );
    assert!(
        !config.llm.default.contains("demo-chat-model"),
        "a refused endpoint must not become the default"
    );
    let user_toml = fs::read_to_string(root.path().join("config.toml")).unwrap_or_default();
    assert!(
        !user_toml.contains(&long_key),
        "a refused endpoint must not reach the user layer either"
    );
}

#[tokio::test]
async fn model_add_registers_without_hijacking_a_working_default() {
    let root = TempRoot::new("model_add_keep_default");
    let mut app = fixture_app(root.path()).await;
    app.runtime
        .state
        .config_store
        .update(|config| {
            config.llm.default = "standing-chat-model".to_string();
            config.llm.models.insert(
                "standing-chat-model".to_string(),
                wunder_server::config::LlmModelConfig {
                    model_type: Some("llm".to_string()),
                    base_url: Some("https://standing.example.test/v1".to_string()),
                    ..Default::default()
                },
            );
        })
        .await
        .expect("seed a working model");

    app.handle_slash_command(
        "/model add https://other.example.test/v1 sk-other second-chat-model 32000".to_string(),
    )
    .await
    .expect("model add");

    let config = app.runtime.state.config_store.get().await;
    assert!(
        config.llm.models.contains_key("second-chat-model"),
        "the new model is registered"
    );
    assert_eq!(
        config.llm.default, "standing-chat-model",
        "a usable default must not be replaced by a registration"
    );
    assert!(
        app.logs
            .iter()
            .any(|entry| entry.text.contains("switch with /model second-chat-model")),
        "the reply must name the way to switch"
    );
    let user_toml = fs::read_to_string(root.path().join("config.toml")).unwrap_or_default();
    assert!(
        !user_toml.contains("second-chat-model"),
        "config.toml describes the default model's endpoint only: {user_toml}"
    );
}
