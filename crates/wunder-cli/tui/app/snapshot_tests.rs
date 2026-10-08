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
        "+ Show details (enter)",
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
    // The two fold glyphs stay distinct: `…` is hidden output on a command card, `⋮` is a
    // folded range inside a diff.
    assert!(
        frame.contains('\u{2026}'),
        "hidden output uses the sample's ellipsis: {frame}"
    );
    assert!(
        frame.contains('\u{22ee}'),
        "the diff keeps its range separator: {frame}"
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
