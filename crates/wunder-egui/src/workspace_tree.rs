//! Working-directory tree: flattened visible rows, expansion state and
//! per-file multi-select.
//!
//! Pure model: the panel asks for rows and paints them. Keeping the flatten and
//! the indent arithmetic here means the pointer, keyboard and drag paths cannot
//! disagree about which row is where.
//!
//! Nothing here touches `egui`: row positions are described in row indices and
//! logical pixels, so the same rules are testable without a frame and the web
//! shell can reuse them.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::WorkspaceFileView;

/// Height of one tree row, matching the hive (web) list so both shells scroll
/// and hit-test at the same rhythm.
pub const ROW_HEIGHT: f32 = 26.0;
/// Left padding of a top-level row.
pub const INDENT_BASE: f32 = 6.0;
/// Extra padding per nesting level.
pub const INDENT_STEP: f32 = 12.0;
/// Width reserved for the leading file/folder glyph.
pub const ICON_SIZE: f32 = 14.0;

/// Left padding for a row at `depth`.
///
/// The row's own expander sits at this offset, so hit-testing a chevron only
/// needs the row index and the depth the flatten reported.
pub fn indent(depth: usize) -> f32 {
    INDENT_BASE + INDENT_STEP * depth as f32
}

/// Hard stop for descending into cached children.
///
/// The ancestor check in [`Tree::flatten`] already breaks cycles exactly; this
/// cap only exists so a pathologically deep (or corrupt) cache cannot exhaust
/// the stack. Real directory trees are far shallower.
const MAX_DEPTH: usize = 128;

/// Expansion state plus the children loaded so far.
///
/// `children` is the cache: a directory listed once is not re-requested after
/// collapsing, because re-expanding a folder should be instant. `expanded`
/// records intent (what the user opened) and is independent of whether the
/// listing arrived yet, which is what lets an error stay visible in place.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    expanded: BTreeSet<String>,
    children: BTreeMap<String, Vec<WorkspaceFileView>>,
    loading: BTreeSet<String>,
    failed: BTreeSet<String>,
}

impl Tree {
    pub fn is_expanded(&self, path: &str) -> bool {
        self.expanded.contains(path)
    }

    /// Mark `path` expanded and report whether its children must be requested.
    ///
    /// The request bit is the only thing the panel needs: cached children mean
    /// no round-trip, and an in-flight request means a second one would only
    /// duplicate the reply. A failed directory still asks again, so clicking a
    /// retry row re-runs the fetch.
    pub fn expand(&mut self, path: &str) -> bool {
        self.expanded.insert(path.to_owned());
        !self.children.contains_key(path) && !self.loading.contains(path)
    }

    /// Collapse without dropping the cache: re-expanding is then free.
    pub fn collapse(&mut self, path: &str) {
        self.expanded.remove(path);
    }

    /// Flip expansion; returns the [`Self::expand`] request bit, or `false`
    /// when the directory was just collapsed.
    pub fn toggle(&mut self, path: &str) -> bool {
        if self.is_expanded(path) {
            self.collapse(path);
            false
        } else {
            self.expand(path)
        }
    }

    /// Store a listing. Any pending or failed flag for the path is resolved
    /// here, so the row stops showing a spinner or an error line.
    pub fn set_children(&mut self, path: &str, items: Vec<WorkspaceFileView>) {
        self.loading.remove(path);
        self.failed.remove(path);
        self.children.insert(path.to_owned(), items);
    }

    /// Record a failed listing.
    ///
    /// The directory stays expanded on purpose: the failure is painted as a
    /// child row under it, and collapsing would hide the retry affordance the
    /// user is looking at.
    pub fn set_error(&mut self, path: &str) {
        self.loading.remove(path);
        self.failed.insert(path.to_owned());
    }

    /// Mark a request in flight and forget an older error, so a retry does not
    /// show the spinner and the previous message at once.
    pub fn set_loading(&mut self, path: &str) {
        self.loading.insert(path.to_owned());
        self.failed.remove(path);
    }

    pub fn is_loading(&self, path: &str) -> bool {
        self.loading.contains(path)
    }

    pub fn has_failed(&self, path: &str) -> bool {
        self.failed.contains(path)
    }

    /// Drop every expansion and every cached child.
    ///
    /// Called when the root listing is replaced: keeping old subtrees would
    /// splice children of a previous workspace under a fresh root, or show a
    /// stale listing under a directory whose contents changed.
    pub fn collapse_all(&mut self) {
        self.expanded.clear();
        self.children.clear();
        self.loading.clear();
        self.failed.clear();
    }

    /// Drop state for directories that no longer exist.
    ///
    /// `keep` answers "is this directory path still present after the
    /// refresh?". All four sets are pruned together: a cache entry without a
    /// directory is dead weight, and a surviving `expanded`/`loading` flag for
    /// a vanished path would either expand a recreated directory without
    /// loading it or leave a spinner nobody clears.
    pub fn retain_loaded(&mut self, keep: impl Fn(&str) -> bool) {
        self.children.retain(|path, _| keep(path));
        self.expanded.retain(|path| keep(path));
        self.loading.retain(|path| keep(path));
        self.failed.retain(|path| keep(path));
    }

    /// Walk the visible tree into paint order.
    ///
    /// Ordering is computed here rather than trusted from the backend: the
    /// directory listing is not guaranteed stable, and a list that reshuffles
    /// between refreshes would move rows under the pointer mid-click.
    ///
    /// A directory that is expanded but not loaded yields only its own row;
    /// `loading`/`failed` describe that row and never create placeholder rows,
    /// so row indices stay equal to what the model can address.
    pub fn flatten<'a>(&'a self, root: &'a [WorkspaceFileView]) -> Vec<TreeRow<'a>> {
        let mut rows = Vec::with_capacity(root.len());
        let mut ancestors: Vec<&'a str> = Vec::new();
        self.flatten_level(root, 0, &mut ancestors, &mut rows);
        rows
    }

    fn flatten_level<'a>(
        &'a self,
        items: &'a [WorkspaceFileView],
        depth: usize,
        ancestors: &mut Vec<&'a str>,
        rows: &mut Vec<TreeRow<'a>>,
    ) {
        let mut ordered: Vec<&'a WorkspaceFileView> = items.iter().collect();
        ordered.sort_by(|left, right| {
            // Directories first, then by name; the path breaks ties so two
            // entries with the same label still have one fixed order.
            right
                .is_directory
                .cmp(&left.is_directory)
                .then_with(|| left.name.cmp(&right.name))
                .then_with(|| left.path.cmp(&right.path))
        });

        for item in ordered {
            let path = item.path.as_str();
            // A path already on the ancestor chain means the cache points back
            // at itself (duplicate or cyclic paths from a bad listing). Skip it
            // instead of recursing: the same path twice in one branch is never
            // renderable, and this keeps `flatten` bounded without a timeout.
            if ancestors.contains(&path) {
                continue;
            }
            let expanded = item.is_directory && self.is_expanded(path);
            rows.push(TreeRow {
                path,
                name: item.name.as_str(),
                depth,
                is_directory: item.is_directory,
                size: item.size,
                expanded,
                loading: self.is_loading(path),
                failed: self.has_failed(path),
            });
            if !expanded || depth >= MAX_DEPTH {
                continue;
            }
            let Some(children) = self.children.get(path) else {
                continue;
            };
            ancestors.push(path);
            self.flatten_level(children, depth + 1, ancestors, rows);
            ancestors.pop();
        }
    }
}

/// One visible row, borrowed from the root listing or the child cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeRow<'a> {
    pub path: &'a str,
    pub name: &'a str,
    pub depth: usize,
    pub is_directory: bool,
    pub size: u64,
    pub expanded: bool,
    pub loading: bool,
    pub failed: bool,
}

/// Multi-select state for the file tree.
///
/// `selected` is a `BTreeSet` so [`FileSelection::ids`] has a stable order and
/// bulk actions walk the same sequence twice. The mode flag is separate from
/// the set so "clear" can empty the selection while leaving the selection bar
/// on screen.
#[derive(Clone, Debug, Default)]
pub struct FileSelection {
    active: bool,
    selected: BTreeSet<String>,
}

impl FileSelection {
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Enter multi-select without preselecting anything; the user picks the
    /// first row afterwards.
    pub fn enter(&mut self) {
        self.active = true;
    }

    /// Leave multi-select and drop the selection, so a later `enter` cannot
    /// resurrect paths that may no longer exist.
    pub fn exit(&mut self) {
        self.active = false;
        self.selected.clear();
    }

    /// Flip membership of `path` and return the new state.
    ///
    /// The mode is not changed here: a checkbox click must not silently pull
    /// the panel into multi-select. Toggling twice restores the prior state.
    pub fn toggle(&mut self, path: &str) -> bool {
        if self.selected.remove(path) {
            false
        } else {
            self.selected.insert(path.to_owned());
            true
        }
    }

    /// Replace the selection with exactly `paths`, which must be the visible
    /// rows (usually `flatten` output): selecting hidden or collapsed entries
    /// would act on files the user cannot see.
    ///
    /// `paths` is an iterator rather than a slice so callers can feed row
    /// paths without collecting them first; duplicates collapse in the set.
    pub fn select_all<'a>(&mut self, paths: impl Iterator<Item = &'a str>) {
        self.selected = paths.map(str::to_owned).collect();
    }

    /// Empty the selection but stay in multi-select.
    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn contains(&self, path: &str) -> bool {
        self.selected.contains(path)
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Selected paths in sorted order. Callers may borrow for the frame; any
    /// command that outlives it clones.
    pub fn ids(&self) -> Vec<&str> {
        self.selected.iter().map(String::as_str).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str, path: &str) -> WorkspaceFileView {
        WorkspaceFileView {
            name: name.to_owned(),
            path: path.to_owned(),
            is_directory: true,
            size: 0,
        }
    }

    fn file(name: &str, path: &str, size: u64) -> WorkspaceFileView {
        WorkspaceFileView {
            name: name.to_owned(),
            path: path.to_owned(),
            is_directory: false,
            size,
        }
    }

    fn paths<'a>(rows: &[TreeRow<'a>]) -> Vec<&'a str> {
        rows.iter().map(|row| row.path).collect()
    }

    #[test]
    fn indent_starts_at_the_base_and_grows_by_step() {
        assert_eq!(indent(0), 6.0);
        assert_eq!(indent(1), 18.0);
        assert_eq!(indent(3), 42.0);
        // The step must be a whole number of rows' worth of separation, or
        // chevrons from adjacent levels would overlap.
        assert_eq!(indent(1) - indent(0), INDENT_STEP);
    }

    #[test]
    fn collapsed_directory_hides_children_and_rows_sort_dirs_first() {
        let root = vec![
            file("b.md", "b.md", 10),
            dir("zeta", "zeta"),
            file("a.md", "a.md", 5),
            dir("alpha", "alpha"),
        ];
        let mut tree = Tree::default();

        let rows = tree.flatten(&root);
        // Directories first, each group by name; the backend order is ignored.
        assert_eq!(paths(&rows), vec!["alpha", "zeta", "a.md", "b.md"]);
        assert!(!rows[0].expanded);

        assert!(tree.expand("alpha"));
        tree.set_children("alpha", vec![file("inner.md", "alpha/inner.md", 1)]);
        assert_eq!(
            paths(&tree.flatten(&root)),
            vec!["alpha", "alpha/inner.md", "zeta", "a.md", "b.md"]
        );
    }

    #[test]
    fn expand_only_requests_until_the_listing_is_cached() {
        let mut tree = Tree::default();
        // First open needs a round-trip.
        assert!(tree.expand("notes"));
        assert!(tree.is_expanded("notes"));

        // Once the request is in flight a second one would duplicate it.
        tree.set_loading("notes");
        assert!(tree.is_loading("notes"));
        assert!(!tree.expand("notes"));

        tree.set_children("notes", vec![file("plan.md", "notes/plan.md", 3)]);
        assert!(!tree.is_loading("notes"));

        // Collapsing must not throw the cache away.
        tree.collapse("notes");
        assert!(!tree.is_expanded("notes"));
        assert!(!tree.expand("notes"));
        assert!(tree.is_expanded("notes"));
    }

    #[test]
    fn toggle_flips_expansion_and_reports_only_on_open() {
        let mut tree = Tree::default();
        assert!(tree.toggle("docs"));
        assert!(tree.is_expanded("docs"));
        assert!(!tree.toggle("docs"));
        assert!(!tree.is_expanded("docs"));
    }

    #[test]
    fn set_error_keeps_the_directory_expanded_for_retry() {
        let mut tree = Tree::default();
        tree.expand("broken");
        tree.set_loading("broken");
        tree.set_error("broken");

        assert!(tree.has_failed("broken"));
        assert!(!tree.is_loading("broken"));
        // Still open: the error row lives under the directory, so collapsing
        // it would hide the retry the user is looking at.
        assert!(tree.is_expanded("broken"));
        // A failed directory is requested again on the next click.
        assert!(tree.expand("broken"));

        tree.set_loading("broken");
        assert!(!tree.has_failed("broken"));
    }

    #[test]
    fn flatten_reports_depth_and_indent_increases_with_it() {
        let root = vec![dir("src", "src")];
        let mut tree = Tree::default();
        tree.set_children("src", vec![dir("ui", "src/ui")]);
        tree.set_children("src/ui", vec![file("view.rs", "src/ui/view.rs", 7)]);
        tree.expand("src");
        tree.expand("src/ui");

        let rows = tree.flatten(&root);
        assert_eq!(paths(&rows), vec!["src", "src/ui", "src/ui/view.rs"]);
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[2].depth, 2);
        assert!(indent(rows[0].depth) < indent(rows[1].depth));
        assert!(indent(rows[1].depth) < indent(rows[2].depth));
        // A file carries no expansion and no flags of its own.
        assert!(!rows[2].is_directory);
        assert!(!rows[2].expanded);
        assert!(!rows[2].loading);
        assert!(!rows[2].failed);
        assert_eq!(rows[2].size, 7);
    }

    #[test]
    fn empty_root_flattens_to_no_rows() {
        let tree = Tree::default();
        let rows = tree.flatten(&[]);
        assert!(rows.is_empty());
    }

    #[test]
    fn files_only_root_needs_no_expansion() {
        let root = vec![file("readme.md", "readme.md", 1)];
        let mut tree = Tree::default();
        let rows = tree.flatten(&root);
        assert_eq!(paths(&rows), vec!["readme.md"]);
        assert_eq!(rows.len(), 1);
        // Expanding a file path never produces children.
        assert!(tree.expand("readme.md"));
        assert_eq!(tree.flatten(&root).len(), 1);
    }

    #[test]
    fn directories_only_root_collapses_to_one_row_each() {
        let root = vec![dir("a", "a"), dir("b", "b")];
        let tree = Tree::default();
        let rows = tree.flatten(&root);
        assert_eq!(paths(&rows), vec!["a", "b"]);
        assert!(rows.iter().all(|row| row.is_directory));
        assert!(rows.iter().all(|row| !row.expanded));
    }

    #[test]
    fn deep_nesting_flattens_every_level() {
        let root = vec![dir("d0", "d0")];
        let mut tree = Tree::default();
        let mut parent = String::from("d0");
        tree.expand(&parent);
        for level in 1..6 {
            let child = format!("d{level}");
            let child_path = format!("{parent}/{child}");
            tree.set_children(&parent, vec![dir(&child, &child_path)]);
            tree.expand(&child_path);
            parent = child_path;
        }

        let rows = tree.flatten(&root);
        assert_eq!(rows.len(), 6);
        for (level, row) in rows.iter().enumerate() {
            assert_eq!(row.depth, level);
        }
        assert_eq!(rows[5].path, "d0/d1/d2/d3/d4/d5");
    }

    #[test]
    fn self_referencing_children_do_not_recurse_forever() {
        let root = vec![dir("a", "a")];
        let mut tree = Tree::default();
        // Corrupt data: `a` lists itself, and `b` lists `a` back.
        tree.set_children("a", vec![dir("a", "a"), dir("b", "a/b")]);
        tree.set_children("a/b", vec![dir("a", "a")]);
        tree.expand("a");
        tree.expand("a/b");

        let rows = tree.flatten(&root);
        // `a` -> `a/b` -> back to `a` is cut at the ancestor check, so the
        // walk terminates after the two distinct paths.
        assert_eq!(paths(&rows), vec!["a", "a/b"]);
    }

    #[test]
    fn collapse_all_drops_every_cached_subtree() {
        let root = vec![dir("a", "a")];
        let mut tree = Tree::default();
        tree.set_children("a", vec![file("x.md", "a/x.md", 1)]);
        tree.expand("a");
        tree.set_loading("a");
        tree.set_error("a");
        assert_eq!(tree.flatten(&root).len(), 2);

        tree.collapse_all();
        assert!(!tree.is_expanded("a"));
        assert!(!tree.is_loading("a"));
        assert!(!tree.has_failed("a"));
        assert_eq!(paths(&tree.flatten(&root)), vec!["a"]);
        // The cache is gone too, so opening it again must refetch.
        assert!(tree.expand("a"));
    }

    #[test]
    fn retain_loaded_forgets_directories_missing_from_the_refresh() {
        let mut tree = Tree::default();
        tree.set_children("keep", vec![file("k.md", "keep/k.md", 1)]);
        tree.set_children("gone", vec![file("g.md", "gone/g.md", 1)]);
        tree.expand("gone");

        tree.retain_loaded(|path| path == "keep");
        assert!(!tree.is_expanded("gone"));
        // `keep` was retained, so expanding it still needs no request.
        assert!(!tree.expand("keep"));
        // `gone` lost its cache, so a fresh open refetches.
        assert!(tree.expand("gone"));
    }

    #[test]
    fn flatten_marks_flags_on_the_directory_row_only() {
        let root = vec![dir("a", "a"), dir("b", "b")];
        let mut tree = Tree::default();
        tree.expand("a");
        tree.set_loading("a");
        tree.expand("b");
        tree.set_error("b");

        let rows = tree.flatten(&root);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].expanded);
        assert!(rows[0].loading);
        assert!(!rows[0].failed);
        assert!(rows[1].expanded);
        assert!(rows[1].failed);
        assert!(!rows[1].loading);
    }

    #[test]
    fn selection_enter_toggle_and_exit_keep_counts_straight() {
        let mut selection = FileSelection::default();
        assert!(!selection.is_active());
        assert!(selection.is_empty());

        selection.enter();
        assert!(selection.is_active());
        assert_eq!(selection.len(), 0);

        assert!(selection.toggle("a.md"));
        assert_eq!(selection.len(), 1);
        // Toggling twice is idempotent and returns the membership.
        assert!(!selection.toggle("a.md"));
        assert_eq!(selection.len(), 0);

        assert!(selection.toggle("b.md"));
        assert!(selection.toggle("a.md"));
        assert_eq!(selection.ids(), vec!["a.md", "b.md"]);
        assert!(selection.contains("a.md"));

        selection.exit();
        assert!(!selection.is_active());
        assert!(selection.is_empty());
        // Re-entering must not resurrect the old ids.
        selection.enter();
        assert_eq!(selection.len(), 0);
    }

    #[test]
    fn select_all_replaces_dedupes_and_clear_keeps_the_mode() {
        let mut selection = FileSelection::default();
        selection.enter();
        selection.toggle("stale.md");

        selection.select_all(["b.md", "a.md", "b.md"].into_iter());
        assert_eq!(selection.len(), 2);
        assert_eq!(selection.ids(), vec!["a.md", "b.md"]);
        assert!(!selection.contains("stale.md"));

        // Running it again with the same visible rows changes nothing.
        selection.select_all(["b.md", "a.md"].into_iter());
        assert_eq!(selection.len(), 2);

        selection.select_all(std::iter::empty());
        assert!(selection.is_empty());
        assert!(selection.is_active());

        selection.select_all(["a.md"].into_iter());
        selection.clear();
        assert!(selection.is_empty());
        assert!(selection.is_active());
    }
}
