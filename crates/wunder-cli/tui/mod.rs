mod activity_indicator;
mod app;
mod frame_scheduler;
mod highlight;
mod line_utils;
mod markdown;
mod markdown_render;
mod markdown_stream;
mod scrollback;
mod theme;
mod ui;
mod wrapping;

use anyhow::Result;
use app::TuiApp;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use frame_scheduler::spawn_frame_scheduler;
use frame_scheduler::FrameNotifications;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::{Terminal, TerminalOptions, Viewport};
use std::io;

use crate::args::GlobalArgs;
use crate::runtime::CliRuntime;

pub async fn run_main(
    runtime: &CliRuntime,
    global: &GlobalArgs,
    first_prompt: Option<String>,
    session_override: Option<String>,
) -> Result<()> {
    let (frame_requester, frame_notifications) = spawn_frame_scheduler();
    let mut terminal = setup_terminal()?;
    let mut app = TuiApp::new(
        runtime.clone(),
        global.clone(),
        session_override,
        frame_requester,
    )
    .await?;

    if let Some(prompt) = first_prompt
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        app.submit_line(prompt).await?;
    }

    app.request_redraw();

    let run_result = run_loop(&mut terminal, &mut app, frame_notifications).await;
    let restore_result = restore_terminal(&mut terminal);

    run_result?;
    restore_result?;
    println!();
    Ok(())
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    mut frame_notifications: FrameNotifications,
) -> Result<()> {
    let mut mouse_capture_enabled = None;
    let mut events = EventStream::new();

    loop {
        sync_mouse_mode(terminal, app, &mut mouse_capture_enabled)?;

        let mut should_draw = false;
        tokio::select! {
            maybe_draw = frame_notifications.recv() => {
                if maybe_draw.is_none() {
                    break;
                }
                should_draw = true;
            }
            maybe_event = events.next() => {
                let Some(event) = maybe_event else {
                    break;
                };
                match event? {
                    Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                        app.on_key(key).await?;
                    }
                    Event::Mouse(mouse) => {
                        app.on_mouse(mouse);
                    }
                    Event::Paste(text) => {
                        app.on_paste(text);
                    }
                    Event::FocusGained => {
                        app.set_terminal_focus(true);
                    }
                    Event::FocusLost => {
                        app.set_terminal_focus(false);
                    }
                    Event::Resize(_, _) => {}
                    _ => {}
                }
                app.request_redraw();
            }
        }

        if app.should_quit() {
            break;
        }

        if !should_draw {
            continue;
        }

        app.drain_stream_events().await;

        sync_mouse_mode(terminal, app, &mut mouse_capture_enabled)?;

        terminal.draw(|frame| ui::draw(frame, app))?;

        let pending_scrollback_lines = app.drain_pending_scrollback_lines();
        if !pending_scrollback_lines.is_empty() {
            scrollback::insert_history_lines(terminal, pending_scrollback_lines)?;
        }

        if app.should_quit() {
            break;
        }

        app.schedule_periodic_redraw_if_needed();
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    enable_bracketed_paste_if_supported(&mut stdout)?;

    // Use an inline viewport so the transcript stays in normal scrollback,
    // matching Codex-style wheel scrolling and native text selection.
    let (_, rows) = crossterm::terminal::size()?;
    let viewport = Viewport::Inline(rows.max(1));
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::with_options(backend, TerminalOptions { viewport })?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    disable_bracketed_paste_if_supported(terminal.backend_mut())?;
    if let Err(error) = execute!(terminal.backend_mut(), DisableMouseCapture) {
        if !cfg!(windows) || !error.to_string().contains("Initial console modes not set") {
            return Err(error.into());
        }
    }
    terminal.show_cursor()?;
    Ok(())
}

fn enable_bracketed_paste_if_supported<W: io::Write>(writer: &mut W) -> Result<()> {
    match execute!(writer, EnableBracketedPaste) {
        Ok(()) => Ok(()),
        Err(error) if should_ignore_bracketed_paste_error(&error) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn disable_bracketed_paste_if_supported<W: io::Write>(writer: &mut W) -> Result<()> {
    match execute!(writer, DisableBracketedPaste) {
        Ok(()) => Ok(()),
        Err(error) if should_ignore_bracketed_paste_error(&error) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn should_ignore_bracketed_paste_error(error: &io::Error) -> bool {
    cfg!(windows) && looks_like_legacy_windows_bracketed_paste_error(&error.to_string())
}

fn looks_like_legacy_windows_bracketed_paste_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    (message.contains("bracketed paste") || message.contains("bracked paste"))
        && message.contains("legacy windows api")
}

fn sync_mouse_mode(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &TuiApp,
    mouse_capture_enabled: &mut Option<bool>,
) -> Result<()> {
    let desired_mouse_capture = app.mouse_capture_enabled();
    if *mouse_capture_enabled == Some(desired_mouse_capture) {
        return Ok(());
    }
    if mouse_capture_enabled.is_none() && !desired_mouse_capture {
        *mouse_capture_enabled = Some(false);
        return Ok(());
    }
    if desired_mouse_capture {
        execute!(terminal.backend_mut(), EnableMouseCapture)?;
    } else {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    *mouse_capture_enabled = Some(desired_mouse_capture);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::looks_like_legacy_windows_bracketed_paste_error;

    #[test]
    fn detects_legacy_windows_bracketed_paste_errors() {
        assert!(looks_like_legacy_windows_bracketed_paste_error(
            "bracketed paste not implemented in the legacy windows api"
        ));
        assert!(looks_like_legacy_windows_bracketed_paste_error(
            "Bracketed Paste not implemented in the Legacy Windows API"
        ));
        assert!(looks_like_legacy_windows_bracketed_paste_error(
            "bracked paste not implemented in the legacy windows api"
        ));
        assert!(!looks_like_legacy_windows_bracketed_paste_error(
            "initial console modes not set"
        ));
    }
}
