//! Follow-ups typed while a turn is still running.
//!
//! The TUI holds them itself (codex's queued prompt), so the message the user
//! just typed is never dropped and the running card's state is never rewritten.
//! These tests pin the three facts a queue has to keep: the text is kept, the
//! transcript says so, and pulling it back gives the draft back.

use super::*;
use crate::args::Cli;
use clap::Parser;

async fn queue_fixture(tag: &str) -> (super::snapshot_tests::TempRoot, TuiApp) {
    let root = super::snapshot_tests::TempRoot::new(tag);
    let global = Cli::parse_from([
        "wunder-cli",
        "--lang",
        "en-US",
        "--user",
        "queue_user",
        "--temp-root",
        root.path().to_string_lossy().as_ref(),
    ])
    .global;
    let runtime = super::snapshot_tests::fixture_runtime(root.path());
    let (requester, _notifications) = crate::tui::frame_scheduler::spawn_frame_scheduler();
    let mut app = TuiApp::new(runtime, global, Some("queue-thread".to_string()), requester)
        .await
        .expect("queue fixture app");
    app.complete_startup_fills().await;
    (root, app)
}

/// A thread is mid-turn exactly when `thread_is_running` says so; the fixture
/// marks the visible thread busy and registers its live stream, which is the
/// same pair production sets before it admits the request.
fn mark_running(app: &mut TuiApp) {
    app.busy = true;
    app.active_stream_sessions.insert(app.session_id.clone());
}

fn tool_log_count(app: &TuiApp) -> usize {
    app.logs
        .iter()
        .filter(|entry| matches!(entry.kind, LogKind::Tool))
        .count()
}

fn last_log_text(app: &TuiApp) -> String {
    app.logs
        .last()
        .map(|entry| entry.text.clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn a_prompt_typed_mid_turn_is_queued_instead_of_dropped() {
    let (_root, mut app) = queue_fixture("queue_enqueue").await;
    mark_running(&mut app);
    let tools_before = tool_log_count(&app);
    let logs_before = app.logs.len();

    app.submit_line("second question".to_string())
        .await
        .expect("submit");

    assert_eq!(app.queued_prompts.len(), 1, "the follow-up is held");
    assert!(app.queued_prompts[0].prompt.contains("second question"));
    let notice = last_log_text(&app);
    assert!(
        notice.contains("queued #1"),
        "the transcript names the queued message: {notice}"
    );
    // A queued prompt is announced, not echoed as a user turn: the echo belongs
    // to the moment it is actually sent, otherwise the same text lands twice.
    assert!(
        !app.logs
            .iter()
            .any(|entry| matches!(entry.kind, LogKind::User)),
        "a held message must not print a user turn yet"
    );
    // Holding input must not touch the cards the running turn already drew.
    assert_eq!(tool_log_count(&app), tools_before);
    assert_eq!(app.logs.len(), logs_before + 1, "exactly one notice is added");
}

#[tokio::test]
async fn the_queue_keeps_order_and_its_cap() {
    let (_root, mut app) = queue_fixture("queue_cap").await;
    mark_running(&mut app);

    for index in 1..=8 {
        app.submit_line(format!("follow-up {index}"))
            .await
            .expect("submit");
    }
    assert_eq!(app.queued_prompts.len(), 8);
    assert!(app.queued_prompts[0].prompt.contains("follow-up 1"));
    assert!(app.queued_prompts[7].prompt.contains("follow-up 8"));

    // The ninth is refused with a notice instead of silently growing the queue.
    app.submit_line("follow-up 9".to_string())
        .await
        .expect("submit");
    assert_eq!(app.queued_prompts.len(), 8, "the cap holds");
    assert!(
        last_log_text(&app).contains("queue is full"),
        "the refusal is visible: {}",
        last_log_text(&app)
    );
}

#[tokio::test]
async fn alt_up_pulls_the_newest_queued_prompt_back_into_the_draft() {
    let (_root, mut app) = queue_fixture("queue_edit").await;
    mark_running(&mut app);
    app.submit_line("first follow-up".to_string())
        .await
        .expect("submit");
    app.submit_line("second follow-up".to_string())
        .await
        .expect("submit");
    assert_eq!(app.queued_prompts.len(), 2);

    app.input.clear();
    app.input_cursor = 0;
    app.edit_last_queued_prompt();

    assert_eq!(app.queued_prompts.len(), 1, "one entry left the queue");
    assert!(
        app.input.contains("second follow-up"),
        "the draft now holds it: {}",
        app.input
    );
    assert!(app.queued_prompts[0].prompt.contains("first follow-up"));
}

#[tokio::test]
async fn the_queue_waits_while_the_thread_is_busy() {
    let (_root, mut app) = queue_fixture("queue_hold").await;
    mark_running(&mut app);
    app.submit_line("held until idle".to_string())
        .await
        .expect("submit");

    app.flush_queued_prompts().await;

    assert_eq!(
        app.queued_prompts.len(),
        1,
        "a busy thread must not consume the queue"
    );
    assert!(app.queued_prompts[0].prompt.contains("held until idle"));
}

#[tokio::test]
async fn the_busy_row_reports_how_many_are_waiting() {
    let (_root, mut app) = queue_fixture("queue_row").await;
    mark_running(&mut app);
    app.submit_line("one".to_string()).await.expect("submit");
    app.submit_line("two".to_string()).await.expect("submit");

    let row = app.activity_line();
    assert!(row.contains("2 queued"), "busy row carries the count: {row}");
}

#[test]
fn a_preview_stays_on_one_short_line() {
    let flat = queued_prompt_preview("  first line\n second   line ");
    assert_eq!(flat, "first line second line");

    let long = queued_prompt_preview(&"点".repeat(80));
    assert!(long.ends_with('…'), "a long preview is clipped: {long}");
    assert_eq!(long.chars().count(), 49);
}

/// An interrupt means "hold on": the follow-ups typed during the stopped round come back
/// down into the composer (codex's `on_interrupted_turn`), and nothing is sent on the
/// strength of the stream closing.
#[tokio::test]
async fn an_interrupted_turn_hands_the_queue_back_to_the_composer() {
    let (_root, mut app) = queue_fixture("queue_interrupt").await;
    mark_running(&mut app);
    app.submit_line("first follow-up".to_string())
        .await
        .expect("submit");
    app.submit_line("second follow-up".to_string())
        .await
        .expect("submit");
    app.input = "draft in progress".to_string();
    app.input_cursor = app.input.len();

    app.turn_was_interrupted = true;
    let session_id = app.session_id.clone();
    app.handle_stream_message(StreamMessage::Done { session_id });

    assert!(
        app.queued_prompts.is_empty(),
        "the queue is drained into the draft, not kept for autosend"
    );
    assert_eq!(
        app.input, "first follow-up\nsecond follow-up\ndraft in progress",
        "queued order first, the draft last, joined by newlines"
    );
    assert_eq!(app.input_cursor, app.input.len(), "caret waits at the end");
    assert!(
        last_log_text(&app).contains("went back into the composer"),
        "the restore is visible: {}",
        last_log_text(&app)
    );
    assert!(
        !app.logs
            .iter()
            .any(|entry| matches!(entry.kind, LogKind::User)),
        "nothing was sent on the interrupted round"
    );
}

/// The queue still advances on its own when the round ended by itself, so the interrupt
/// handling above only applies to a turn the user actually stopped.
#[tokio::test]
async fn a_turn_that_ended_on_its_own_keeps_the_queue_for_autosend() {
    let (_root, mut app) = queue_fixture("queue_autosend").await;
    mark_running(&mut app);
    app.submit_line("sends on its own".to_string())
        .await
        .expect("submit");

    app.turn_was_interrupted = false;
    let session_id = app.session_id.clone();
    app.handle_stream_message(StreamMessage::Done { session_id });

    assert_eq!(
        app.queued_prompts.len(),
        1,
        "a natural end leaves the follow-up queued for the idle tick"
    );
    assert!(app.input.is_empty(), "the draft stays untouched");
}
