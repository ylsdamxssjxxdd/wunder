//! Event-driven terminal projection. Parsing and shell I/O stay off the UI
//! thread; the mailbox retains one viewport and one queued UI notification.
use crate::{
    terminal_grid::{self, Span, TerminalGrid},
    TerminalWindow,
};
use std::{
    sync::{
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use wunder_desktop::{
    NativeDesktop, NativeTerminalFrame, NativeTerminalSignal, NativeTerminalWatch,
};

pub enum Control {
    Resize(u16, u16),
    Scroll(i32),
    ScrollTo(usize),
    Clear,
    Mouse(i32, i32, i32, i32),
}
enum Input {
    Bytes(Vec<u8>),
    Interrupt,
    Resize(u16, u16),
}

pub struct Projection {
    pub spans: Vec<Span>,
    pub rows: u16,
    pub cols: u16,
    pub caret: Option<(i32, i32)>,
    pub history: usize,
    pub offset: usize,
    pub mouse: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
    pub status: String,
    pub ready: bool,
    pub error: bool,
    pub ended: bool,
}
#[derive(Default)]
struct Mailbox {
    projection: Option<Projection>,
    scheduled: bool,
}

pub struct Session {
    controls: SyncSender<Control>,
    input: SyncSender<Input>,
    signal: NativeTerminalSignal,
    mailbox: Arc<Mutex<Mailbox>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.closed
            .store(true, std::sync::atomic::Ordering::Release);
        self.signal.wake();
    }
}
impl Session {
    pub fn start(
        desktop: Arc<NativeDesktop>,
        session: String,
        terminal: String,
        window: slint::Weak<TerminalWindow>,
        replay: String,
    ) -> Result<Self, String> {
        let watch = desktop
            .terminal_watch(&session, &terminal)
            .map_err(|e| e.to_string())?;
        let signal = watch.signal();
        let (input, input_rx) = mpsc::sync_channel(128);
        let (controls, control_rx) = mpsc::sync_channel(64);
        let (errors_tx, errors) = mpsc::sync_channel(1);
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_closed = closed.clone();
        let writer = desktop.clone();
        let write_session = session.clone();
        let write_terminal = terminal.clone();
        let write_signal = signal.clone();
        std::thread::spawn(move || {
            while let Ok(event) = input_rx.recv() {
                let result = match event {
                    Input::Bytes(bytes) => {
                        writer.terminal_send(&write_session, &write_terminal, &bytes)
                    }
                    Input::Interrupt => writer.terminal_cancel(&write_session, &write_terminal),
                    Input::Resize(cols, rows) => writer.terminal_resize(
                        &write_session,
                        &write_terminal,
                        cols.into(),
                        rows.into(),
                    ),
                };
                if let Err(error) = result {
                    let _ = errors_tx.try_send(format!("终端输入失败：{error}"));
                    write_signal.wake();
                    break;
                }
            }
        });
        let worker_mailbox = mailbox.clone();
        let worker_input = input.clone();
        std::thread::spawn(move || {
            run(
                watch,
                control_rx,
                errors,
                worker_input,
                worker_closed,
                worker_mailbox,
                window,
                replay,
            );
            let _ = desktop.terminal_close(&session, &terminal);
        });
        Ok(Self {
            controls,
            input,
            signal,
            mailbox,
            closed,
        })
    }
    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.len() > 64 * 1024 {
            return Err("终端输入过大，请分段粘贴".into());
        }
        self.input
            .try_send(Input::Bytes(bytes))
            .map_err(|_| "终端输入繁忙，请稍候".into())
    }
    pub fn interrupt(&self) -> Result<(), String> {
        self.input
            .try_send(Input::Interrupt)
            .map_err(|_| "终端输入繁忙，请稍候".into())
    }
    pub fn control(&self, event: Control) -> Result<(), String> {
        self.controls
            .try_send(event)
            .map_err(|_| "终端事件繁忙，请稍候".to_string())?;
        self.signal.wake();
        Ok(())
    }
    pub fn take(&self) -> Option<Projection> {
        let mut m = self.mailbox.lock().unwrap_or_else(|e| e.into_inner());
        m.scheduled = false;
        m.projection.take()
    }
}

fn publish(
    mailbox: &Mutex<Mailbox>,
    window: &slint::Weak<TerminalWindow>,
    projection: Projection,
) -> bool {
    let mut m = mailbox.lock().unwrap_or_else(|e| e.into_inner());
    m.projection = Some(projection);
    if m.scheduled {
        return true;
    }
    m.scheduled = true;
    drop(m);
    window
        .upgrade_in_event_loop(|w| w.invoke_frame_ready())
        .is_ok()
}

fn run(
    mut watch: NativeTerminalWatch,
    controls: mpsc::Receiver<Control>,
    errors: mpsc::Receiver<String>,
    input: SyncSender<Input>,
    closed: Arc<std::sync::atomic::AtomicBool>,
    mailbox: Arc<Mutex<Mailbox>>,
    window: slint::Weak<TerminalWindow>,
    replay: String,
) {
    let mut grid = TerminalGrid::new(24, 80);
    grid.feed(replay.as_bytes());
    let mut status = "启动中…".to_string();
    let mut ready = false;
    let mut error = false;
    let mut dirty = true;
    let mut ended = false;
    let mut loss_marked = false;
    // The first echo wakes immediately. Sustained output is capped at one
    // projection per display frame, rather than rebuilding once per byte.
    let mut painted = Instant::now() - Duration::from_millis(16);
    loop {
        if closed.load(std::sync::atomic::Ordering::Acquire) {
            break;
        }
        for event in controls.try_iter().take(64) {
            match event {
                Control::Resize(cols, rows) => {
                    if grid.size() != (rows, cols) {
                        grid.set_size(rows, cols);
                        let _ = input.try_send(Input::Resize(cols, rows));
                        dirty = true;
                    }
                }
                Control::Scroll(rows) => {
                    grid.scroll_by(rows);
                    dirty = true;
                }
                Control::ScrollTo(row) => {
                    grid.scroll_to_row(row);
                    dirty = true;
                }
                Control::Clear => {
                    grid.clear();
                    dirty = true;
                }
                Control::Mouse(action, button, col, row) => {
                    let action = match action {
                        0 => terminal_grid::MouseAction::Press,
                        1 => terminal_grid::MouseAction::Release,
                        2 => terminal_grid::MouseAction::Move,
                        3 => terminal_grid::MouseAction::WheelUp,
                        4 => terminal_grid::MouseAction::WheelDown,
                        _ => continue,
                    };
                    if let Some(bytes) = grid.mouse_input(
                        action,
                        (button >= 0).then_some(button as u8),
                        col.max(0) as u16,
                        row.max(0) as u16,
                    ) {
                        let _ = input.try_send(Input::Bytes(bytes));
                    }
                }
            }
        }
        if let Ok(message) = errors.try_recv() {
            status = message;
            error = true;
            ready = false;
            dirty = true;
            ended = true;
        }
        if dirty && painted.elapsed() >= Duration::from_millis(8) {
            let (rows, cols) = grid.size();
            if !publish(
                &mailbox,
                &window,
                Projection {
                    spans: grid.render(),
                    rows,
                    cols,
                    caret: grid.caret(),
                    history: grid.history_depth(),
                    offset: grid.view_offset(),
                    mouse: grid.mouse_reporting(),
                    application_cursor: grid.application_cursor(),
                    bracketed_paste: grid.bracketed_paste_enabled(),
                    status: status.clone(),
                    ready,
                    error,
                    ended,
                },
            ) {
                break;
            }
            painted = Instant::now();
            dirty = false;
        }
        if ended && !dirty {
            break;
        }
        let deadline = dirty.then(|| Duration::from_millis(8).saturating_sub(painted.elapsed()));
        match watch.next(deadline) {
            Ok(Some(frame)) => {
                if frame.truncated && !loss_marked {
                    grid.feed(b"\x1b[33m[earlier output truncated]\x1b[0m\r\n");
                    loss_marked = true;
                }
                if !frame.text.is_empty() {
                    grid.feed(frame.text.as_bytes());
                    dirty = true;
                }
                let next_status = describe(&frame);
                let next_ready = frame.status == "running"
                    && (frame.backend == "console" || frame.backend == "pty");
                dirty |= status != next_status || ready != next_ready;
                status = next_status;
                ready = next_ready;
                error = frame.status == "failed"
                    || (frame.status == "exited" && frame.exit_code.is_some_and(|c| c != 0));
                if frame.status == "running" && !next_ready {
                    status = "无法创建交互终端，请安装 Git 终端补充包后重新打开".into();
                    ready = false;
                    error = true;
                    ended = true;
                    dirty = true;
                }
                ended |= frame.status == "failed" || frame.status == "exited";
            }
            Ok(None) => {}
            Err(message) => {
                status = format!("终端读取失败：{message}");
                error = true;
                ready = false;
                dirty = true;
                ended = true;
            }
        }
    }
}
fn describe(f: &NativeTerminalFrame) -> String {
    match f.status.as_str() {
        "starting" => "启动中…".into(),
        "running" => "运行中".into(),
        "failed" => format!("启动失败：{}", f.error.as_deref().unwrap_or("未知原因")),
        "exited" => f
            .exit_code
            .map(|c| format!("已退出（{c}）"))
            .unwrap_or("已退出".into()),
        _ => f.status.clone(),
    }
}
