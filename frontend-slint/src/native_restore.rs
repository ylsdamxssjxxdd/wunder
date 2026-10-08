//! Durable-restore acceptance: what a second, fresh process sees on disk.
//!
//! `check-native.py` runs this phase as `--native-restore <dir>` after the
//! streaming phase exits, and asserts that `restore.txt` starts with `PASS`.
//! The process is a completely new runtime against the same isolated SQLite
//! store, so everything it can observe got there through durability and nothing
//! else -- the mock model server is gone by then, which is what makes this
//! phase read-only by construction.
//!
//! The guard is deliberately *not* `native_smoke::check_runtime`: that one
//! writes probe fixtures (`create_session`, a "native-cancel" message), and a
//! check that mutates the store cannot prove anything about what survived it.
use crate::MainWindow;
use crate::native_smoke::ensure;
use slint::{ComponentHandle, Model};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};
use wunder_desktop::NativeDesktop;

/// Read-only liveness check: the runtime answers a listing and rejects an
/// unknown session, without creating anything.
fn check_restored_runtime(desktop: &NativeDesktop) -> Result<(), Box<dyn std::error::Error>> {
    ensure(
        desktop.get_session("restore-missing-session").is_err(),
        "an unknown session was accepted after restart",
    )?;
    ensure(
        !desktop.list_sessions(None)?.is_empty(),
        "the restart lost every session",
    )?;
    Ok(())
}

pub fn run(
    app: &MainWindow,
    directory: PathBuf,
    desktop: Arc<NativeDesktop>,
) -> Result<(), Box<dyn std::error::Error>> {
    let result = Rc::new(RefCell::new(None));
    let output = result.clone();
    let weak = app.as_weak();
    let timer = slint::Timer::default();
    let mut step = 0u8;
    let start = Instant::now();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(50),
        move || {
            let Some(app) = weak.upgrade() else { return };
            // The harness allows this phase 40s and it runs a debug build, so
            // the deadline here has to leave room for the runtime to open the
            // store and rehydrate, not just for the checks to run. A tie used to
            // surface as "the restored turn has no answer" instead of a clear
            // timeout.
            let checked = if start.elapsed() > Duration::from_secs(35) {
                Err(format!("restore step {step} timed out; {}", app.get_status()).into())
            } else {
                advance(&app, &directory, desktop.as_ref(), &mut step)
            };
            if matches!(checked, Ok(false)) {
                return;
            }
            let report = match &checked {
                Ok(_) => {
                    "PASS: durable restore -- sessions, transcript and settled turn state survived a fresh process\n"
                        .to_string()
                }
                Err(error) => format!("FAIL: step {step}: {error}\n"),
            };
            let checked = std::fs::write(directory.join("restore.txt"), report)
                .map_err(|error| error.to_string())
                .and(checked.map(|_| ()).map_err(|error| error.to_string()));
            *output.borrow_mut() = Some(checked);
            let _ = slint::quit_event_loop();
        },
    );
    app.run()?;
    timer.stop();
    result
        .borrow_mut()
        .take()
        .ok_or("native restore did not run")??;
    Ok(())
}

fn advance(
    app: &MainWindow,
    directory: &std::path::Path,
    desktop: &NativeDesktop,
    step: &mut u8,
) -> Result<bool, Box<dyn std::error::Error>> {
    match *step {
        0 => {
            check_restored_runtime(desktop)?;
        }
        1 => {
            // The conversation list is itself a projection of the durable store,
            // so an empty list after a restart means the store did not come back.
            if app.get_conversations().row_count() == 0 {
                ensure(
                    !app.get_status().starts_with("无法"),
                    "the conversation list failed to load",
                )?;
                return Ok(false);
            }
        }
        2 => {
            // The streamed turn is the one whose whole answer is known exactly:
            // the mock model streams DELTA 600 times for "native-long".
            // The streamed session is identified by the id phase 1 recorded,
            // not by its title: titles come from a session's first message, so
            // several harness turns share the prefix.
            let handoff = std::fs::read_to_string(directory.join("streamed-session.json"))?;
            let handoff: serde_json::Value = serde_json::from_str(&handoff)?;
            let session_id = handoff["session_id"]
                .as_str()
                .ok_or("streamed-session.json has no session id")?
                .to_string();
            let titles: Vec<String> = app
                .get_conversations()
                .iter()
                .map(|row| row.title.to_string())
                .collect();
            let Some(index) = app
                .get_conversations()
                .iter()
                .position(|row| row.id == session_id.as_str())
            else {
                return Err(format!(
                    "the streamed session {session_id} did not survive the restart; restored: {titles:?}"
                )
                .into());
            };
            // Selecting is asynchronous: the transcript only arrives on a later
            // tick, so the step has to wait for the switch to land instead of
            // falling through to the checks with an empty timeline.
            if app.get_active_session_id() != session_id.as_str() {
                if !app.get_session_loading() && !app.get_chat_loading() {
                    app.invoke_select_conversation(index as i32);
                }
                return Ok(false);
            }
        }
        3 => {
            // Wait for the transcript to rehydrate. The loading flags are not
            // enough on their own: the previous step already waited for the
            // session switch to land, and a timeline that is still empty after
            // that is either mid-load or genuinely has nothing. Keep asking
            // until the deadline so the failure that surfaces is the real one
            // (a wrong or missing answer in step 4) rather than a race.
            if app.get_session_loading() || app.get_chat_loading() {
                return Ok(false);
            }
            let counts = app.invoke_timeline_counts();
            if counts.answers == 0 {
                ensure(
                    !app.get_status().starts_with("无法"),
                    "the restored transcript failed to load",
                )?;
                return Ok(false);
            }
            ensure(
                counts.answers > 0,
                &format!(
                    "the restored turn has no answer: rows={} status={:?}",
                    counts.rows,
                    app.get_status(),
                ),
            )?;
        }
        4 => {
            // Compare against what phase 1 actually streamed, handed over in
            // the same file, rather than re-deriving the mock model's output.
            let handoff = std::fs::read_to_string(directory.join("streamed-session.json"))?;
            let handoff: serde_json::Value = serde_json::from_str(&handoff)?;
            let want = handoff["answer"]
                .as_str()
                .ok_or("streamed-session.json has no recorded answer")?
                .to_string();
            let actual = app.invoke_timeline_last_answer().to_string();
            ensure(
                actual.contains(want.as_str()),
                &format!(
                    "the restored answer differs from what was streamed: got {} bytes want {} bytes, got_start={:?}",
                    actual.len(),
                    want.len(),
                    &actual.chars().take(24).collect::<String>(),
                ),
            )?;
            crate::smoke::snapshot(app, &directory.join("native-restored.png"))?;
        }
        5 => {
            // The cancel turn stopped mid-flight in the previous process. After a
            // restart it must read as settled: a turn that is still armed would
            // block the next send.
            ensure(
                !app.get_busy() && !app.get_stopping(),
                &format!(
                    "a restored turn is still in flight: busy={} stopping={} status={:?}",
                    app.get_busy(),
                    app.get_stopping(),
                    app.get_status(),
                ),
            )?;
        }
        _ => return Ok(true),
    }
    *step += 1;
    Ok(false)
}
