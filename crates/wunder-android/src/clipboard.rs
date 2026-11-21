//! Clipboard payload policy for 安卓版.
//!
//! The system clipboard is written through a Binder transaction. A
//! work-process entry can carry the whole tool result — `read_file` alone
//! allows 1 MiB — and Java strings are parcelled as UTF-16, two bytes per
//! `char`, so an unbounded copy overflows the transaction buffer. The write is
//! then dropped, the previous clip stays on the clipboard, and the next paste
//! reports that stale content as unsupported ("不支持粘贴该内容") while the
//! shell still toasted "已复制".
//!
//! Bounding the payload here — once, for every copy button in the shell —
//! keeps the write inside the budget and lets the toast say exactly what
//! landed, instead of claiming a complete copy of a tool result the system
//! refused to store.

/// Maximum number of Unicode scalar values handed to the system clipboard.
/// Parcelled as UTF-16 this is about 200 KiB, far below the 1 MiB transaction
/// buffer the app shares with the rest of the process.
pub const MAX_CLIPBOARD_CHARS: usize = 100_000;

/// One prepared clipboard payload plus what the toast has to say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardPayload {
    pub text: String,
    /// Character count before clamping, so the toast can quote the real size.
    pub total_chars: usize,
    pub truncated: bool,
}

impl ClipboardPayload {
    /// Message shown after the clipboard read-back matched this payload.
    pub fn toast(&self) -> String {
        if self.truncated {
            // The toast is a 260px pill on a phone; one short line is enough and
            // the full size is not worth a second line of digits.
            format!("内容过长，已复制前 {} 字", MAX_CLIPBOARD_CHARS)
        } else {
            "已复制".to_string()
        }
    }
}

/// Shape one copy request for Android's clipboard.
pub fn prepare(text: &str) -> ClipboardPayload {
    let total_chars = text.chars().count();
    let (text, truncated) = if total_chars > MAX_CLIPBOARD_CHARS {
        (text.chars().take(MAX_CLIPBOARD_CHARS).collect(), true)
    } else {
        (text.to_string(), false)
    };
    ClipboardPayload {
        text,
        total_chars,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_copied_verbatim() {
        let payload = prepare("命令输出\n第二行");
        assert_eq!(payload.text, "命令输出\n第二行");
        assert_eq!(payload.total_chars, 8);
        assert!(!payload.truncated);
        assert_eq!(payload.toast(), "已复制");
    }

    #[test]
    fn oversized_text_is_cut_on_a_char_boundary() {
        let original = "命".repeat(MAX_CLIPBOARD_CHARS + 5);
        let payload = prepare(&original);
        assert_eq!(payload.text.chars().count(), MAX_CLIPBOARD_CHARS);
        assert!(payload.text.chars().all(|c| c == '命'));
        // The bytes of a UTF-16 payload stay well under the 1 MiB transaction
        // buffer, which is what used to be exceeded.
        assert_eq!(payload.text.encode_utf16().count(), MAX_CLIPBOARD_CHARS);
        assert_eq!(payload.total_chars, MAX_CLIPBOARD_CHARS + 5);
        assert!(payload.truncated);
        assert_eq!(
            payload.toast(),
            format!("内容过长，已复制前 {} 字", MAX_CLIPBOARD_CHARS)
        );
    }

    #[test]
    fn exactly_at_the_budget_is_not_truncated() {
        let payload = prepare(&"x".repeat(MAX_CLIPBOARD_CHARS));
        assert!(!payload.truncated);
        assert_eq!(payload.text.len(), MAX_CLIPBOARD_CHARS);
    }

    #[test]
    fn empty_text_still_produces_a_readable_toast() {
        let payload = prepare("");
        assert_eq!(payload.text, "");
        assert_eq!(payload.total_chars, 0);
        assert_eq!(payload.toast(), "已复制");
    }
}
