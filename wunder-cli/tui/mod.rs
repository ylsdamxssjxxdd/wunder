mod app;
mod frame_scheduler;
mod highlight;
mod line_utils;
mod markdown;
mod markdown_render;
mod markdown_stream;
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
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::Command;
use frame_scheduler::spawn_frame_scheduler;
use frame_scheduler::FrameNotifications;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::fmt;
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
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EnableAlternateScroll;

impl Command for EnableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "tried to execute EnableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DisableAlternateScroll;

impl Command for DisableAlternateScroll {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[?1007l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        Err(std::io::Error::other(
            "tried to execute DisableAlternateScroll using WinAPI; use ANSI instead",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut TuiApp,
    mut frame_notifications: FrameNotifications,
) -> Result<()> {
    let mut mouse_capture_enabled = false;
    let mut events = EventStream::new();

    loop {
        tokio::select! {
            maybe_draw = frame_notifications.recv() => {
                if maybe_draw.is_none() {
                    break;
                }
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

        app.drain_stream_events().await;

        let desired_mouse_capture = app.mouse_capture_enabled();
        if desired_mouse_capture != mouse_capture_enabled {
            if desired_mouse_capture {
                execute!(
                    terminal.backend_mut(),
                    DisableAlternateScroll,
                    EnableMouseCapture
                )?;
            } else {
                execute!(
                    terminal.backend_mut(),
                    DisableMouseCapture,
                    EnableAlternateScroll
                )?;
            }
            mouse_capture_enabled = desired_mouse_capture;
        }

        terminal.draw(|frame| ui::draw(frame, app))?;

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
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableAlternateScroll
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture,
        DisableAlternateScroll
    )?;
    terminal.show_cursor()?;
    Ok(())
}
