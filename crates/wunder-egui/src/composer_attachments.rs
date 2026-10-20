//! Attachment chips and the bounded prompt queue for the composer.
//!
//! Pure state: the composer draws it, `AppState` owns it.
//!
//! Nothing here touches egui. The composer's pending attachments, the queued
//! prompts and the up/down history all have limits that must be enforced on
//! every path (drag-drop, file picker, paste, keyboard), so the rules live in
//! one testable place instead of being re-checked inside the draw code.

/// How many chips may sit above the input box at once.
///
/// Eight is what still fits on one wrapped row pair at the minimum window
/// width; beyond that the chips start pushing the transcript out of view.
pub const MAX_ATTACHMENTS: usize = 8;

/// Upper bound for the summed bytes of the *file* attachments.
///
/// The whole set is handed to the runtime as file references, and the
/// composer must not become a way to pin a gigabyte of the disk in memory.
/// 64 MiB covers the documents and screenshots the desktop flow is built for.
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// How many prompts may wait behind the running turn.
///
/// Queued prompts are shown as a stack of removable rows; four keeps the
/// stack readable and stops a held-down enter key from building a backlog.
pub const MAX_QUEUED: usize = 4;

/// How many sent prompts the up/down history keeps.
///
/// Fifty is the shell convention and stays small enough that the list is
/// cheap to clone on every keystroke.
pub const MAX_HISTORY: usize = 50;

/// One pending attachment.
///
/// `size` is kept verbatim even for directories (where the picker may report
/// the entry size) so the UI can show it if it ever wants to; only
/// [`Attachments::total_bytes`] decides whether it counts against the budget.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attachment {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_directory: bool,
}

impl Attachment {
    /// Convenience constructor; the path doubles as the identity key.
    pub fn new(path: impl Into<String>, name: impl Into<String>, size: u64) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size,
            is_directory: false,
        }
    }

    pub fn directory(path: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            name: name.into(),
            size: 0,
            is_directory: true,
        }
    }
}

/// Why an attachment was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachError {
    Duplicate,
    TooMany,
    TooLarge,
}

impl AttachError {
    /// User-visible reason. The numbers are spelled out because this string is
    /// painted next to the composer, far away from the constants above; the
    /// `attach_error_messages_quote_the_limits` test keeps them in sync.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Duplicate => "该文件已在附件列表中",
            Self::TooMany => "附件数量已达上限（最多 8 个）",
            Self::TooLarge => "附件总大小超出上限（最多 64 MB）",
        }
    }
}

/// The pending attachment set for the composer.
///
/// Insertion order is preserved: chips read left to right in the order the
/// user added them, and the runtime receives them in the same order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attachments {
    items: Vec<Attachment>,
}

impl Attachments {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one attachment.
    ///
    /// Checks run in this order: duplicate, count, bytes. Duplicate first
    /// because it is the only recoverable no-op (the chip is already there),
    /// then count because that is the limit the user can see, then bytes.
    /// A duplicate is an error rather than a silent overwrite so a second
    /// drop of the same path cannot reset a size the user already reviewed.
    pub fn add(&mut self, attachment: Attachment) -> Result<(), AttachError> {
        if self.items.iter().any(|item| item.path == attachment.path) {
            return Err(AttachError::Duplicate);
        }
        if self.items.len() >= MAX_ATTACHMENTS {
            return Err(AttachError::TooMany);
        }
        if !attachment.is_directory
            && self.total_bytes().saturating_add(attachment.size) > MAX_TOTAL_BYTES
        {
            return Err(AttachError::TooLarge);
        }
        self.items.push(attachment);
        Ok(())
    }

    /// Drop one attachment by path. Returns whether anything was removed.
    ///
    /// Removal is never refused, including at the limit: the user must always
    /// be able to make room.
    pub fn remove(&mut self, path: &str) -> bool {
        match self.items.iter().position(|item| item.path == path) {
            Some(index) => {
                self.items.remove(index);
                true
            }
            None => false,
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Summed bytes of the file attachments; directories count as zero.
    ///
    /// Computed on demand because the set is tiny and a cached counter is one
    /// more thing that can drift from the vector.
    pub fn total_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter(|item| !item.is_directory)
            .fold(0u64, |sum, item| sum.saturating_add(item.size))
    }

    pub fn items(&self) -> &[Attachment] {
        &self.items
    }

    /// One-line status for the row above the chips, e.g. `3 个附件 · 1.2 MB`.
    ///
    /// Empty means "draw nothing": an empty attachment set gets no label at all.
    pub fn summary(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }
        format!(
            "{} 个附件 · {}",
            self.items.len(),
            format_size(self.total_bytes())
        )
    }

    /// Presentation rows for the chip strip.
    ///
    /// `removable` is always true today: removal must stay possible even when
    /// the set is at its limit. The field stays in the struct so the composer
    /// can key "the strip is full, drop something first" hints off it without
    /// re-deriving the state from counts.
    pub fn chips(&self) -> Vec<AttachmentChip> {
        self.items
            .iter()
            .map(|item| AttachmentChip {
                path: item.path.clone(),
                name: item.name.clone(),
                size_text: if item.is_directory {
                    "目录".to_owned()
                } else {
                    format_size(item.size)
                },
                removable: true,
            })
            .collect()
    }
}

/// A draw-ready attachment chip. The composer only positions these.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentChip {
    /// Stable key for the click handler; names are not unique.
    pub path: String,
    pub name: String,
    pub size_text: String,
    pub removable: bool,
}

/// Why a prompt was not queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueError {
    EmptyText,
    Full,
}

impl QueueError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::EmptyText => "内容为空，无法加入队列",
            Self::Full => "待发送队列已满（最多 4 条），请先发送或移除",
        }
    }
}

/// Prompts waiting behind the running turn.
///
/// FIFO: the head is the prompt that will be sent next, which is also the row
/// the composer marks as "next". Text is stored verbatim (only checked trimmed)
/// so leading indentation survives into the queued message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PromptQueue {
    items: Vec<String>,
}

impl PromptQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, text: String) -> Result<(), QueueError> {
        if text.trim().is_empty() {
            return Err(QueueError::EmptyText);
        }
        if self.items.len() >= MAX_QUEUED {
            return Err(QueueError::Full);
        }
        self.items.push(text);
        Ok(())
    }

    /// Take the oldest prompt, or `None` when the queue is empty.
    pub fn pop(&mut self) -> Option<String> {
        if self.items.is_empty() {
            None
        } else {
            Some(self.items.remove(0))
        }
    }

    /// Drop the prompt at `index` (the row's remove button).
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }
}

/// Up/down history for the composer input.
///
/// `cursor` is `None` while the live draft is in the box and `Some(i)` while
/// the box shows `entries[i]`. The live draft is captured on the first
/// [`History::previous`] so the caller can restore it after [`History::next`]
/// walks past the newest entry and returns `None`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    /// Oldest first; the last entry is what the first "up" shows.
    entries: Vec<String>,
    cursor: Option<usize>,
    /// The draft that was in the box before browsing started.
    draft: String,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a prompt that was actually sent.
    ///
    /// Adjacent duplicates collapse: re-sending the same text is common and
    /// would otherwise force two "up" presses to reach anything else. Pushing
    /// always returns the cursor to the live draft, because every stored index
    /// may shift when the oldest entry is trimmed.
    pub fn push(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if self.entries.last().map(String::as_str) != Some(text) {
            if self.entries.len() >= MAX_HISTORY {
                self.entries.remove(0);
            }
            self.entries.push(text.to_owned());
        }
        self.reset();
    }

    /// Step one entry older. Returns the text to put in the box.
    ///
    /// The first call stores `current` as the draft to come back to. At the
    /// oldest entry the cursor stays there and returns it again, so holding
    /// "up" cannot run off the end.
    pub fn previous(&mut self, current: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let index = match self.cursor {
            None => {
                self.draft = current.to_owned();
                self.entries.len() - 1
            }
            Some(index) => index.saturating_sub(1),
        };
        self.cursor = Some(index);
        self.entries.get(index).cloned()
    }

    /// Step one entry newer.
    ///
    /// `None` means "you are past the newest entry": the box should show
    /// [`History::draft`] again. Asking for "down" while already on the draft
    /// is also `None` and changes nothing. Named `next` on purpose: the
    /// composer pairs it with `previous` in the up/down key handler, and this
    /// is not an iterator (the cursor moves both ways and mutates a draft).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<String> {
        let index = self.cursor?;
        if index + 1 < self.entries.len() {
            self.cursor = Some(index + 1);
            self.entries.get(index + 1).cloned()
        } else {
            self.cursor = None;
            None
        }
    }

    /// The live draft captured when browsing started.
    ///
    /// Separate from [`History::next`] because `None` is already the "restore
    /// the draft" signal; handing the text back separately keeps that signal
    /// unambiguous.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// Leave browsing mode and forget the captured draft. Called after send
    /// and on escape, so a later "up" starts from the freshly typed text.
    pub fn reset(&mut self) {
        self.cursor = None;
        self.draft.clear();
    }
}

/// Human size for chips and the summary.
///
/// Base 1024 with the unit thresholds the rest of the app uses. KB is whole
/// numbers and MB/GB keep one decimal, matching the workspace file list. A
/// value that would round to `1024 KB` is promoted to MB instead of printing
/// a nonsense unit.
fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let value = bytes as f64;
    if value >= GB {
        format!("{:.1} GB", value / GB)
    } else if value >= MB {
        format!("{:.1} MB", value / MB)
    } else if value >= KB {
        let kb = (value / KB).round();
        if kb >= 1024.0 {
            format!("{:.1} MB", value / MB)
        } else {
            format!("{:.0} KB", kb)
        }
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> Attachment {
        Attachment::new(format!("C:/tmp/{name}"), name, size)
    }

    fn dir(name: &str) -> Attachment {
        Attachment::directory(format!("C:/tmp/{name}"), name)
    }

    fn filled(count: usize) -> Attachments {
        let mut set = Attachments::new();
        for index in 0..count {
            let added = set.add(file(&format!("f{index}.txt"), 1));
            assert_eq!(added, Ok(()));
        }
        set
    }

    #[test]
    fn add_rejects_duplicate_path_without_overwriting() {
        let mut set = Attachments::new();
        assert_eq!(set.add(file("a.txt", 10)), Ok(()));
        // Same path, different name and size: still the same attachment.
        let duplicate = Attachment::new("C:/tmp/a.txt", "renamed.txt", 999);
        assert_eq!(set.add(duplicate), Err(AttachError::Duplicate));
        assert_eq!(set.len(), 1);
        assert_eq!(set.total_bytes(), 10);
        let first = set.items().first().map(|item| item.name.as_str());
        assert_eq!(first, Some("a.txt"));
    }

    #[test]
    fn add_stops_at_the_count_limit() {
        let mut set = filled(MAX_ATTACHMENTS);
        assert_eq!(set.len(), MAX_ATTACHMENTS);
        assert_eq!(set.add(file("extra.txt", 1)), Err(AttachError::TooMany));
        assert_eq!(set.len(), MAX_ATTACHMENTS);
    }

    #[test]
    fn directories_take_a_slot_but_no_bytes() {
        let mut set = Attachments::new();
        assert_eq!(set.add(dir("src")), Ok(()));
        assert_eq!(set.len(), 1);
        assert_eq!(set.total_bytes(), 0);
        // Directories still cannot push the count past the limit.
        for index in 1..MAX_ATTACHMENTS {
            assert_eq!(set.add(dir(&format!("d{index}"))), Ok(()));
        }
        assert_eq!(set.add(dir("overflow")), Err(AttachError::TooMany));
    }

    #[test]
    fn add_stops_at_the_byte_limit() {
        let mut set = Attachments::new();
        // Exactly at the limit is allowed; one more byte is not.
        assert_eq!(set.add(file("full.bin", MAX_TOTAL_BYTES)), Ok(()));
        assert_eq!(set.add(file("one-more.bin", 1)), Err(AttachError::TooLarge));
        assert_eq!(set.total_bytes(), MAX_TOTAL_BYTES);

        let mut single = Attachments::new();
        assert_eq!(
            single.add(file("huge.bin", MAX_TOTAL_BYTES + 1)),
            Err(AttachError::TooLarge)
        );
        assert!(single.is_empty());
    }

    #[test]
    fn remove_and_clear_reset_the_accounting() {
        let mut set = filled(3);
        assert!(set.remove("C:/tmp/f1.txt"));
        assert_eq!(set.len(), 2);
        assert_eq!(set.total_bytes(), 2);
        // Removing an unknown path is a no-op, not an error.
        assert!(!set.remove("C:/tmp/f1.txt"));
        assert_eq!(set.len(), 2);
        set.clear();
        assert!(set.is_empty());
        assert_eq!(set.total_bytes(), 0);
        assert_eq!(set.summary(), "");
    }

    #[test]
    fn summary_reports_count_and_total() {
        let mut set = Attachments::new();
        assert_eq!(set.summary(), "");
        assert_eq!(set.add(file("a.txt", 12 * 1024)), Ok(()));
        assert_eq!(set.summary(), "1 个附件 · 12 KB");
        assert_eq!(set.add(dir("src")), Ok(()));
        // The directory raises the count but not the byte total.
        assert_eq!(set.summary(), "2 个附件 · 12 KB");
        assert_eq!(set.add(file("b.bin", 1024 * 1024 + 200 * 1024)), Ok(()));
        assert_eq!(set.summary(), "3 个附件 · 1.2 MB");
    }

    #[test]
    fn chips_carry_labels_and_stay_removable() {
        let mut set = Attachments::new();
        assert_eq!(set.add(file("a.txt", 12 * 1024)), Ok(()));
        assert_eq!(set.add(dir("src")), Ok(()));
        let chips = set.chips();
        assert_eq!(chips.len(), 2);
        let first = chips
            .first()
            .map(|chip| (chip.name.as_str(), chip.size_text.as_str()));
        assert_eq!(first, Some(("a.txt", "12 KB")));
        let second = chips
            .get(1)
            .map(|chip| (chip.size_text.as_str(), chip.removable));
        assert_eq!(second, Some(("目录", true)));

        // At the limit the chips stay removable so the user can make room.
        let full = filled(MAX_ATTACHMENTS).chips();
        assert!(full.iter().all(|chip| chip.removable));
        assert_eq!(full.len(), MAX_ATTACHMENTS);
    }

    #[test]
    fn size_text_switches_units_without_rounding_past_them() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(850), "850 B");
        assert_eq!(format_size(2048), "2 KB");
        assert_eq!(format_size(12 * 1024), "12 KB");
        assert_eq!(format_size(1024 * 1024 + 200 * 1024), "1.2 MB");
        assert_eq!(format_size(64 * 1024 * 1024), "64.0 MB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GB");
        // 1023.6 KB must not print as "1024 KB".
        assert_eq!(format_size(1023 * 1024 + 1024 * 6 / 10), "1.0 MB");
    }

    #[test]
    fn attach_error_messages_quote_the_limits() {
        assert!(AttachError::Duplicate.message().contains("已在附件列表"));
        let too_many = format!("{}", MAX_ATTACHMENTS);
        assert!(AttachError::TooMany.message().contains(&too_many));
        let too_large = format!("{}", MAX_TOTAL_BYTES / (1024 * 1024));
        assert!(AttachError::TooLarge.message().contains(&too_large));
    }

    #[test]
    fn queue_pops_in_insertion_order() {
        let mut queue = PromptQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.pop(), None);
        assert_eq!(queue.push("first".to_owned()), Ok(()));
        assert_eq!(queue.push("second".to_owned()), Ok(()));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.pop(), Some("first".to_owned()));
        assert_eq!(queue.pop(), Some("second".to_owned()));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn queue_rejects_blank_text_and_stores_the_rest_verbatim() {
        let mut queue = PromptQueue::new();
        assert_eq!(queue.push(String::new()), Err(QueueError::EmptyText));
        assert_eq!(queue.push("   \n\t".to_owned()), Err(QueueError::EmptyText));
        assert!(queue.is_empty());
        // Indentation is preserved; only the emptiness check trims.
        assert_eq!(queue.push("  indented".to_owned()), Ok(()));
        assert_eq!(queue.items(), ["  indented".to_owned()]);
    }

    #[test]
    fn queue_rejects_beyond_the_limit() {
        let mut queue = PromptQueue::new();
        for index in 0..MAX_QUEUED {
            assert_eq!(queue.push(format!("p{index}")), Ok(()));
        }
        assert_eq!(queue.push("extra".to_owned()), Err(QueueError::Full));
        assert_eq!(queue.len(), MAX_QUEUED);
        let full = format!("{}", MAX_QUEUED);
        assert!(QueueError::Full.message().contains(&full));
        assert!(!QueueError::EmptyText.message().is_empty());
    }

    #[test]
    fn queue_remove_drops_one_row() {
        let mut queue = PromptQueue::new();
        assert_eq!(queue.push("a".to_owned()), Ok(()));
        assert_eq!(queue.push("b".to_owned()), Ok(()));
        assert_eq!(queue.push("c".to_owned()), Ok(()));
        assert!(queue.remove(1));
        assert_eq!(queue.items(), ["a".to_owned(), "c".to_owned()]);
        // Out of range indices are refused instead of panicking.
        assert!(!queue.remove(2));
        assert!(!queue.remove(usize::MAX));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn history_walks_back_then_restores_the_draft() {
        let mut history = History::new();
        assert_eq!(history.previous("draft"), None);
        history.push("one");
        history.push("two");
        assert_eq!(history.previous("draft"), Some("two".to_owned()));
        assert_eq!(history.previous("two"), Some("one".to_owned()));
        // The captured draft is the text that was in the box on the first up.
        assert_eq!(history.draft(), "draft");
        assert_eq!(history.next(), Some("two".to_owned()));
        assert_eq!(history.next(), None);
        assert_eq!(history.draft(), "draft");
        // Already on the draft: further downs change nothing.
        assert_eq!(history.next(), None);
    }

    #[test]
    fn history_stops_at_both_ends() {
        let mut history = History::new();
        history.push("only");
        // Oldest end: the cursor stays put and keeps returning the same entry.
        assert_eq!(history.previous(""), Some("only".to_owned()));
        assert_eq!(history.previous("only"), Some("only".to_owned()));
        assert_eq!(history.previous("only"), Some("only".to_owned()));
        assert_eq!(history.next(), None);
        assert_eq!(history.next(), None);
    }

    #[test]
    fn history_collapses_adjacent_duplicates() {
        let mut history = History::new();
        history.push("same");
        history.push("same");
        history.push("other");
        history.push("other");
        assert_eq!(history.previous(""), Some("other".to_owned()));
        assert_eq!(history.previous("other"), Some("same".to_owned()));
        // Only one "same" was stored.
        assert_eq!(history.previous("same"), Some("same".to_owned()));
    }

    #[test]
    fn history_ignores_blank_sends_and_caps_the_entries() {
        let mut history = History::new();
        history.push("");
        history.push("  \t");
        assert_eq!(history.previous("draft"), None);

        for index in 0..MAX_HISTORY + 10 {
            history.push(&format!("p{index}"));
        }
        // The oldest entries were trimmed: 50 ups reach p59 down to p10 and
        // then keep returning p10 instead of walking off the end.
        let mut seen = Vec::new();
        let mut current = "draft".to_owned();
        for _ in 0..MAX_HISTORY {
            match history.previous(&current) {
                Some(entry) => {
                    current = entry.clone();
                    seen.push(entry);
                }
                None => break,
            }
        }
        assert_eq!(seen.len(), MAX_HISTORY);
        assert_eq!(seen.first().map(String::as_str), Some("p59"));
        assert_eq!(seen.last().map(String::as_str), Some("p10"));
        assert_eq!(history.previous(&current), Some("p10".to_owned()));
    }

    #[test]
    fn history_reset_returns_to_the_live_draft() {
        let mut history = History::new();
        history.push("sent");
        assert_eq!(history.previous("draft"), Some("sent".to_owned()));
        history.reset();
        assert_eq!(history.next(), None);
        assert_eq!(history.draft(), "");
        // After reset the next up captures the newly typed text.
        assert_eq!(history.previous("fresh"), Some("sent".to_owned()));
        assert_eq!(history.draft(), "fresh");
    }

    #[test]
    fn history_push_while_browsing_returns_to_the_draft() {
        let mut history = History::new();
        history.push("one");
        assert_eq!(history.previous("draft"), Some("one".to_owned()));
        history.push("two");
        // Sending clears browsing state, so the next up shows the new entry.
        assert_eq!(history.previous(""), Some("two".to_owned()));
        assert_eq!(history.draft(), "");
    }
}
