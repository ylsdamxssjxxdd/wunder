//! Thread list interaction model: multi-select and drag-over geometry.
//!
//! Lives outside `app.rs` so the pointer and selection rules can be tested
//! without running a frame. Everything here is pure presentation state: the
//! sidebar owns one [`ThreadSelection`] and calls these helpers from its draw
//! code, so the same rule is not re-implemented in the immediate-mode tree.
//! This module deliberately has no `Ui` and paints nothing.

use std::collections::BTreeSet;

/// A status is archivable only when no turn is still doing work. Archiving a
/// live thread would cut off a running turn, so the bulk action filters these
/// out and surfaces them separately instead of cancelling silently.
pub fn archivable(status: &str) -> bool {
    !matches!(status, "running" | "streaming" | "queued")
}

/// Multi-select state for the sidebar thread list.
///
/// `selected` is a `BTreeSet` so [`ThreadSelection::ids`] has a stable order;
/// tests and the bulk-archive command both rely on that. The mode flag is kept
/// separate from the set so "select all" can be cleared without leaving the
/// mode, and exiting always leaves an empty set behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadSelection {
    active: bool,
    selected: BTreeSet<String>,
}

impl ThreadSelection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Enter multi-select and select `id`.
    ///
    /// Activating again with another id keeps the existing selection, which is
    /// what a row checkbox needs.
    /// Enter multi-select without preselecting a row; the selection bar's
    /// "全选" button then decides the first batch.
    pub fn enter(&mut self) {
        self.active = true;
    }

    pub fn activate(&mut self, id: &str) {
        self.active = true;
        self.selected.insert(id.to_owned());
    }

    /// Flip membership of `id`.
    ///
    /// Toggling does not change the mode: the mode is owned by `activate` and
    /// `exit` so a checkbox click cannot leave the user in a mode they never
    /// entered. Toggling the same id twice restores the previous selection.
    pub fn toggle(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }

    /// Remove one id without flipping it back on.
    ///
    /// A successful archive must drop the row from the selection; using
    /// `toggle` here would re-add it if the same id showed up again in a later
    /// refresh.
    pub fn deselect(&mut self, id: &str) {
        self.selected.remove(id);
    }

    /// Replace the selection with exactly `ids`.
    ///
    /// Callers pass the list they consider selectable (archivable rows), not
    /// every session, so "select all" cannot revive a hidden or locked thread.
    /// The mode flag is untouched for the same reason as in [`Self::toggle`].
    pub fn select_all<I, S>(&mut self, ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.selected = ids.into_iter().map(|id| id.as_ref().to_owned()).collect();
    }

    /// Drop the selection but stay in multi-select. Used by the "clear" action
    /// that should leave the selection bar visible.
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    /// Leave multi-select. The selection is cleared as part of the transition
    /// so a later `activate` cannot resurrect stale ids.
    pub fn exit(&mut self) {
        self.active = false;
        self.selected.clear();
    }

    pub fn contains(&self, id: &str) -> bool {
        self.selected.contains(id)
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Selected ids in sorted order. The caller may keep the borrows for the
    /// frame; the bulk command clones them because it outlives the draw pass.
    pub fn ids(&self) -> Vec<&str> {
        self.selected.iter().map(String::as_str).collect()
    }

    /// Selected ids that may be archived, in the order `entries` yields them.
    ///
    /// The caller's order (the visible list order) is preserved instead of the
    /// `BTreeSet` order so the progress counter walks the list top to bottom.
    pub fn archive_targets<'a>(
        &self,
        entries: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> Vec<String> {
        self.partition_targets(entries).0
    }

    /// Selected ids that were filtered out of [`Self::archive_targets`].
    ///
    /// Exposed separately so the UI can say "N running threads skipped" rather
    /// than dropping them without a word.
    pub fn skipped<'a>(&self, entries: impl Iterator<Item = (&'a str, &'a str)>) -> Vec<String> {
        self.partition_targets(entries).1
    }

    /// One pass keeps `archive_targets` and `skipped` from disagreeing about
    /// which selected ids were filtered.
    fn partition_targets<'a>(
        &self,
        entries: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> (Vec<String>, Vec<String>) {
        let mut targets = Vec::new();
        let mut skipped = Vec::new();
        for (id, status) in entries {
            if !self.contains(id) {
                continue;
            }
            if archivable(status) {
                targets.push(id.to_owned());
            } else {
                skipped.push(id.to_owned());
            }
        }
        (targets, skipped)
    }
}

/// Insertion slot for a pointer at `pointer_y`, in `0..=count`.
///
/// Slot `i` means "before row `i`"; `count` means "after the last row". A row
/// is split at its vertical midpoint: the upper half inserts before it and the
/// lower half inserts after it, so dropping on the bottom half of row 0 lands
/// between rows 0 and 1. An exact midpoint counts as the lower half.
///
/// The pointer is clamped to the list bounds and an unusable row height
/// (non-positive or non-finite) falls back to the top slot instead of
/// producing a bogus index.
pub fn insertion_index(pointer_y: f32, list_top: f32, row_height: f32, count: usize) -> usize {
    if !pointer_y.is_finite() || !list_top.is_finite() || !row_height.is_finite() {
        return 0;
    }
    if row_height <= 0.0 {
        return 0;
    }
    let slot = ((pointer_y - list_top) / row_height).round();
    if slot <= 0.0 {
        0
    } else if slot >= count as f32 {
        count
    } else {
        slot as usize
    }
}

/// Y coordinate of the drop indicator line for an insertion slot.
///
/// `index` is expected to come from [`insertion_index`]; index 0 sits on
/// `list_top` and index `count` sits on the bottom edge of the last row, which
/// is where the "append at the end" line belongs.
pub fn drop_indicator_y(list_top: f32, row_height: f32, index: usize) -> f32 {
    list_top + row_height * index as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archivable_only_rejects_statuses_with_live_work() {
        assert!(!archivable("running"));
        assert!(!archivable("streaming"));
        assert!(!archivable("queued"));
        assert!(archivable("done"));
        assert!(archivable("failed"));
    }

    #[test]
    fn activate_enters_multi_select_and_selects_the_row() {
        let mut selection = ThreadSelection::new();
        assert!(!selection.is_active());

        selection.activate("thread-a");
        assert!(selection.is_active());
        assert!(selection.contains("thread-a"));
        assert_eq!(selection.len(), 1);

        selection.activate("thread-b");
        assert_eq!(selection.len(), 2);
        assert_eq!(selection.ids(), vec!["thread-a", "thread-b"]);
    }

    #[test]
    fn exit_clears_selection_and_leaves_multi_select() {
        let mut selection = ThreadSelection::new();
        selection.activate("thread-a");
        selection.activate("thread-b");

        selection.exit();
        assert!(!selection.is_active());
        assert!(selection.is_empty());
        // A stale id must not come back when the mode is entered again.
        selection.activate("thread-c");
        assert_eq!(selection.ids(), vec!["thread-c"]);
    }

    #[test]
    fn clear_keeps_the_mode_but_select_all_replaces_the_selection() {
        let mut selection = ThreadSelection::new();
        selection.activate("thread-old");
        selection.clear();
        assert!(selection.is_active());
        assert!(selection.is_empty());

        selection.activate("thread-old");
        selection.select_all(["thread-b", "thread-a", "thread-b"]);
        // Only the passed ids survive, deduplicated and in stable order.
        assert_eq!(selection.ids(), vec!["thread-a", "thread-b"]);
        assert!(!selection.contains("thread-old"));
    }

    #[test]
    fn toggle_is_idempotent_and_never_changes_the_mode() {
        let mut selection = ThreadSelection::new();
        selection.activate("thread-a");

        selection.toggle("thread-b");
        assert!(selection.contains("thread-b"));
        selection.toggle("thread-b");
        assert!(!selection.contains("thread-b"));
        // Toggling the last selected row off keeps the user in multi-select;
        // `exit` is the only way out.
        selection.toggle("thread-a");
        assert!(!selection.contains("thread-a"));
        assert!(selection.is_active());
        assert!(selection.is_empty());
    }

    #[test]
    fn deselect_drops_one_row_without_leaving_the_mode() {
        let mut selection = ThreadSelection::new();
        selection.activate("thread-a");
        selection.activate("thread-b");

        selection.deselect("thread-a");
        assert_eq!(selection.ids(), vec!["thread-b"]);
        assert!(selection.is_active());
        // Removing an id that is not selected is a no-op.
        selection.deselect("thread-missing");
        assert_eq!(selection.ids(), vec!["thread-b"]);
    }

    #[test]
    fn archive_targets_filter_live_threads_and_keep_input_order() {
        let rows = [
            ("z-done", "done"),
            ("m-running", "running"),
            ("a-done", "done"),
            ("s-streaming", "streaming"),
            ("q-queued", "queued"),
            ("u-unselected", "done"),
        ];
        let mut selection = ThreadSelection::new();
        for (id, _) in rows {
            if id != "u-unselected" {
                selection.activate(id);
            }
        }

        let targets = selection.archive_targets(rows.iter().copied());
        // Input order, not the BTreeSet order ("a-done", "z-done").
        assert_eq!(targets, vec!["z-done", "a-done"]);
        assert_eq!(
            selection.skipped(rows.iter().copied()),
            vec!["m-running", "s-streaming", "q-queued"]
        );
    }

    #[test]
    fn empty_selection_yields_no_targets_or_skips() {
        let rows = [("thread-a", "done"), ("thread-b", "running")];
        let selection = ThreadSelection::new();
        assert!(selection.archive_targets(rows.iter().copied()).is_empty());
        assert!(selection.skipped(rows.iter().copied()).is_empty());
    }

    #[test]
    fn insertion_index_clamps_to_the_list_bounds() {
        // An empty list has a single slot, wherever the pointer is.
        assert_eq!(insertion_index(500.0, 100.0, 28.0, 0), 0);
        // Above the first row and below the last row clamp to the ends.
        assert_eq!(insertion_index(0.0, 100.0, 28.0, 4), 0);
        assert_eq!(insertion_index(10_000.0, 100.0, 28.0, 4), 4);
        // A broken row height is not allowed to produce a random slot.
        assert_eq!(insertion_index(150.0, 100.0, 0.0, 3), 0);
        assert_eq!(insertion_index(150.0, 100.0, -28.0, 3), 0);
    }

    #[test]
    fn insertion_index_splits_rows_at_their_midpoint() {
        let top = 100.0;
        let height = 28.0;
        // Row 0 spans 100..128 and its midpoint is 114.
        assert_eq!(insertion_index(113.9, top, height, 4), 0);
        assert_eq!(insertion_index(114.1, top, height, 4), 1);
        // Row 1 spans 128..156, so its lower half appends after row 1.
        assert_eq!(insertion_index(141.9, top, height, 4), 1);
        assert_eq!(insertion_index(142.1, top, height, 4), 2);
        assert_eq!(insertion_index(155.9, top, height, 4), 2);
    }

    #[test]
    fn drop_indicator_y_follows_row_boundaries() {
        let top = 100.0;
        let height = 28.0;
        assert_eq!(drop_indicator_y(top, height, 0), 100.0);
        assert_eq!(drop_indicator_y(top, height, 1), 128.0);
        assert_eq!(drop_indicator_y(top, height, 2), 156.0);
        // The line after the last of four rows sits on its bottom edge.
        assert_eq!(drop_indicator_y(top, height, 4), 212.0);
    }
}
