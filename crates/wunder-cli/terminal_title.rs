//! The OS window title of the terminal the 命令行 runs in.
//!
//! Several shells usually sit side by side, so the window is named after the
//! thread on screen, not after the program. The name is the one the session
//! record already holds (first user message, `/rename`, `/compact`), which is
//! also what the thread center and the other forms show — one owner, one name.

use crossterm::execute;
use crossterm::terminal::SetTitle;
use std::io::{self, IsTerminal, Write};
use std::sync::Mutex;

/// The last value pushed to the terminal. The title is synced on every painted
/// frame; without this the same escape sequence would be re-sent each time.
static LAST_TITLE: Mutex<Option<String>> = Mutex::new(None);

/// A window title is a handle, not a paragraph: some terminals reflow the tab
/// bar by it, and a session name is already capped by the engine.
const MAX_TITLE_CHARS: usize = 120;

/// Point the window title at `title`.
///
/// A redirected stdout has no window to name, and a non-TTY frame writer must
/// keep its bytes clean, so both bail out before anything is written.
pub(crate) fn set(title: &str) {
    let cleaned = sanitize(title);
    if cleaned.is_empty() {
        return;
    }
    let mut stdout = io::stdout();
    if !stdout.is_terminal() {
        return;
    }
    let mut last = LAST_TITLE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if last.as_deref() == Some(cleaned.as_str()) {
        return;
    }
    if write_title(&mut stdout, cleaned.as_str()).is_ok() {
        *last = Some(cleaned);
    }
}

/// The one place the sequence is written. `execute!` writes the OSC sequence on
/// a VT-capable console and falls back to `SetConsoleTitleW` on Windows 7, where
/// the escape would be printed as literal text into the viewport.
fn write_title<W: Write>(writer: &mut W, title: &str) -> io::Result<()> {
    execute!(writer, SetTitle(title))
}

/// One line, no control characters: a newline or an escape inside the name
/// would end the sequence early and let the rest run as terminal input.
fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_TITLE_CHARS * 4));
    let mut kept = 0usize;
    let mut pending_space = false;
    for ch in raw.chars() {
        if ch.is_control() || ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if kept >= MAX_TITLE_CHARS {
            break;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
        kept += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{sanitize, write_title};

    #[test]
    fn flattens_the_name_into_one_safe_line() {
        assert_eq!(sanitize("  配色\n观察\t临时  "), "配色 观察 临时");
        // An ESC would end the OSC sequence and the remainder would be read as
        // terminal input, so every control character is dropped.
        let spoofed = sanitize("a\u{1b}]0;spoof\u{7}b");
        assert!(!spoofed.chars().any(char::is_control), "{spoofed:?}");
        assert_eq!(spoofed, "a ]0;spoof b");
        assert_eq!(sanitize("\r\n\t "), "");
    }

    #[cfg(unix)]
    #[test]
    fn writes_the_osc_title_sequence() {
        let mut out: Vec<u8> = Vec::new();
        write_title(&mut out, "配色观察").expect("write title");
        assert_eq!(
            String::from_utf8(out).expect("utf8 title"),
            format!("\u{1b}]0;配色观察\u{7}")
        );
    }

    #[test]
    fn caps_the_name_length() {
        let long = "会话".repeat(200);
        let cleaned = sanitize(long.as_str());
        assert_eq!(cleaned.chars().count(), 120);
    }
}
