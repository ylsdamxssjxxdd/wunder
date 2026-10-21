//! Device → workspace → thread projection for the left rail.
//!
//! The Slint 蛹 sidebar moved to three levels: a device selector decides which
//! tree is shown, every workspace owns its threads, and thread rows only appear
//! under an expanded workspace. Keeping that shape here means the drawing code
//! never re-derives ordering rules, and the rules stay testable without a
//! window.
//!
//! Only the local device is a real tree today: the egui shell has no cloud
//! thread channel yet, so `Device::Cloud` projects no rows instead of inventing
//! a cloud workspace the backend cannot serve. The device header still renders
//! the same way, which is what 蛹 shows while signed out.

use crate::model::{SessionView, WorkspaceView};

/// Device header height; keeps the 28px rhythm of the 蛹 rail.
pub const DEVICE_ROW_HEIGHT: f32 = 28.0;
/// Workspace rows are 34px and thread rows 28px, as in 蛹 and the web shell.
pub const WORKSPACE_ROW_HEIGHT: f32 = 34.0;
pub const THREAD_ROW_HEIGHT: f32 = 28.0;

/// Which device owns the visible tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Device {
    #[default]
    Local,
    Cloud,
}

impl Device {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "本地",
            Self::Cloud => "云端",
        }
    }

    /// Host name shown after the device label, matching `sb_device_*_host`.
    pub fn host(self) -> &'static str {
        match self {
            Self::Local => "蛹",
            Self::Cloud => "蜂巢",
        }
    }

    pub fn is_cloud(self) -> bool {
        matches!(self, Self::Cloud)
    }
}

/// One visible rail row. `expanded` is carried on the workspace row so the
/// painter never has to ask the tree again.
#[derive(Clone, Copy, Debug)]
pub enum Row<'a> {
    Workspace {
        workspace: &'a WorkspaceView,
        expanded: bool,
    },
    Thread {
        /// Index into the state's session list, so selection and the action
        /// handlers keep addressing the same thread after the tree filters.
        index: usize,
        session: &'a SessionView,
    },
}

/// Device choice plus the single expanded workspace.
///
/// 蛹 keeps exactly one workspace open at a time: opening another closes the
/// previous group. Two open groups would also break the row-ordered keyboard
/// navigation, which assumes one contiguous run of thread rows.
#[derive(Clone, Debug, Default)]
pub struct SidebarTree {
    device: Device,
    cloud_available: bool,
    expanded: Option<String>,
}

impl SidebarTree {
    pub fn device(&self) -> Device {
        self.device
    }

    pub fn cloud_available(&self) -> bool {
        self.cloud_available
    }

    /// Whether the header offers a device menu at all. A signed-out cloud
    /// target cannot host a tree, so 蛹 hides the affordance instead of
    /// letting a click land on an empty device.
    pub fn can_switch_device(&self) -> bool {
        self.cloud_available
    }

    pub fn set_cloud_available(&mut self, available: bool) {
        self.cloud_available = available;
        if !available && self.device.is_cloud() {
            self.set_device(Device::Local);
        }
    }

    /// Switch devices. Returns whether the tree changed; an unavailable cloud
    /// device is refused rather than shown empty.
    pub fn set_device(&mut self, device: Device) -> bool {
        if device.is_cloud() && !self.cloud_available {
            return false;
        }
        if self.device == device {
            return false;
        }
        self.device = device;
        // The other device's workspace is not in this projection, so its
        // expansion id would point at an invisible row.
        self.expanded = None;
        true
    }

    pub fn expanded(&self) -> Option<&str> {
        self.expanded.as_deref()
    }

    pub fn is_expanded(&self, workspace_id: &str) -> bool {
        self.expanded.as_deref() == Some(workspace_id)
    }

    /// Open or close one workspace. Returns the state after the toggle.
    pub fn toggle(&mut self, workspace_id: &str) -> bool {
        if self.is_expanded(workspace_id) {
            self.expanded = None;
            false
        } else {
            self.expanded = Some(workspace_id.to_owned());
            true
        }
    }

    /// Expand the workspace that owns a thread without closing it when already
    /// open. Selecting a thread from outside the rail (a queued prompt, a
    /// restore) must leave its row visible.
    pub fn reveal(&mut self, workspace_id: &str) {
        if workspace_id.is_empty() {
            return;
        }
        if !self.is_expanded(workspace_id) {
            self.expanded = Some(workspace_id.to_owned());
        }
    }

    pub fn collapse(&mut self) {
        self.expanded = None;
    }

    /// Drop an expansion that points at a workspace which no longer exists.
    /// A deleted workspace would otherwise keep the group marked open with no
    /// rows under it.
    pub fn retain_workspace(&mut self, workspace_id: &str) {
        if self.is_expanded(workspace_id) {
            self.expanded = None;
        }
    }

    /// Flatten the rail into rows in paint order.
    ///
    /// Threads are matched by `SessionView::workspace_id`. A thread without an
    /// owning workspace is filed under the first workspace, mirroring
    /// `NativeDesktop::create_session(None)`, which binds new threads to the
    /// default (first) workspace. Showing it under a workspace is wrong only in
    /// a way the user can immediately correct by moving the thread; hiding it
    /// would make an existing conversation unreachable.
    pub fn rows<'a>(
        &self,
        workspaces: &'a [WorkspaceView],
        sessions: &'a [SessionView],
    ) -> Vec<Row<'a>> {
        if self.device.is_cloud() {
            return Vec::new();
        }
        let mut rows = Vec::with_capacity(workspaces.len().saturating_mul(2) + sessions.len());
        let fallback = workspaces.first().map(|workspace| workspace.id.as_str());
        for workspace in workspaces {
            let expanded = self.is_expanded(&workspace.id);
            rows.push(Row::Workspace {
                workspace,
                expanded,
            });
            if !expanded {
                continue;
            }
            for (index, session) in sessions.iter().enumerate() {
                let owner = session
                    .workspace_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    .or(fallback);
                if owner == Some(workspace.id.as_str()) {
                    rows.push(Row::Thread { index, session });
                }
            }
        }
        rows
    }
}

/// Threads owned by one workspace, in list order. The workspace row shows this
/// count when the backend's own counter is stale (for example right after a
/// thread is created locally).
pub fn thread_count(sessions: &[SessionView], workspace_id: &str) -> usize {
    sessions
        .iter()
        .filter(|session| session.workspace_id.as_deref() == Some(workspace_id))
        .count()
}

/// Insertion slot for a dragged workspace row.
///
/// `pointer_y` is compared against each row's own span; the row's midpoint
/// decides whether the dropped row lands before or after it. Returning the
/// insertion index keeps the geometry out of the reorder itself.
pub fn drop_index(row_spans: &[(f32, f32)], pointer_y: f32) -> Option<usize> {
    if row_spans.is_empty() {
        return None;
    }
    let last = row_spans.len().saturating_sub(1);
    let clamped = pointer_y.clamp(row_spans[0].0, row_spans[last].1);
    for (index, (top, bottom)) in row_spans.iter().enumerate() {
        if clamped <= *bottom {
            return Some(if clamped < (top + bottom) / 2.0 {
                index
            } else {
                index + 1
            });
        }
    }
    Some(row_spans.len())
}

/// Move `moved` to `target` in the given id list.
///
/// `target` is an insertion index in the *original* list, so moving a row down
/// past its own slot has to account for the hole it leaves behind.
pub fn moved_order(ids: &[String], moved: &str, target: usize) -> Option<Vec<String>> {
    let from = ids.iter().position(|id| id == moved)?;
    let target = target.min(ids.len());
    let mut next = ids.to_vec();
    let item = next.remove(from);
    let insert = if target > from { target - 1 } else { target };
    next.insert(insert.min(next.len()), item);
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(id: &str, name: &str) -> WorkspaceView {
        WorkspaceView {
            id: id.to_owned(),
            name: name.to_owned(),
            root_path: format!("/tmp/{id}"),
            icon: "folder".to_owned(),
            color: "gray".to_owned(),
            thread_count: 0,
        }
    }

    fn thread(id: &str, workspace_id: Option<&str>) -> SessionView {
        SessionView {
            id: id.to_owned(),
            title: id.to_owned(),
            workspace_id: workspace_id.map(str::to_owned),
            ..Default::default()
        }
    }

    #[test]
    fn rows_show_only_workspace_headers_while_collapsed() {
        let workspaces = vec![workspace("a", "甲"), workspace("b", "乙")];
        let sessions = vec![thread("t1", Some("a"))];
        let tree = SidebarTree::default();
        let rows = tree.rows(&workspaces, &sessions);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| matches!(
            row,
            Row::Workspace {
                expanded: false,
                ..
            }
        )));
    }

    #[test]
    fn expanding_a_workspace_lists_only_its_threads_in_order() {
        let workspaces = vec![workspace("a", "甲"), workspace("b", "乙")];
        let sessions = vec![
            thread("t1", Some("a")),
            thread("t2", Some("b")),
            thread("t3", Some("a")),
        ];
        let mut tree = SidebarTree::default();
        assert!(tree.toggle("a"));
        let rows = tree.rows(&workspaces, &sessions);
        let ids: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Thread { session, .. } => Some(session.id.as_str()),
                Row::Workspace { .. } => None,
            })
            .collect();
        assert_eq!(ids, vec!["t1", "t3"]);
        // The thread index keeps pointing into the state's own list, not into
        // the filtered projection.
        let indexes: Vec<usize> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Thread { index, .. } => Some(*index),
                Row::Workspace { .. } => None,
            })
            .collect();
        assert_eq!(indexes, vec![0, 2]);
    }

    #[test]
    fn opening_another_workspace_closes_the_previous_one() {
        let workspaces = vec![workspace("a", "甲"), workspace("b", "乙")];
        let sessions = vec![thread("t1", Some("a")), thread("t2", Some("b"))];
        let mut tree = SidebarTree::default();
        assert!(tree.toggle("a"));
        assert!(tree.toggle("b"));
        assert!(!tree.is_expanded("a"));
        let open: Vec<&str> = tree
            .rows(&workspaces, &sessions)
            .iter()
            .filter_map(|row| match row {
                Row::Thread { session, .. } => Some(session.id.as_str()),
                Row::Workspace { .. } => None,
            })
            .collect();
        assert_eq!(open, vec!["t2"]);
    }

    #[test]
    fn toggling_the_open_workspace_closes_it() {
        let mut tree = SidebarTree::default();
        assert!(tree.toggle("a"));
        assert!(!tree.toggle("a"));
        assert_eq!(tree.expanded(), None);
    }

    #[test]
    fn cloud_device_is_refused_while_unavailable() {
        let mut tree = SidebarTree::default();
        assert!(!tree.set_device(Device::Cloud));
        assert_eq!(tree.device(), Device::Local);
        tree.set_cloud_available(true);
        assert!(tree.set_device(Device::Cloud));
        assert_eq!(tree.device(), Device::Cloud);
        // Losing the cloud target falls back to the local tree instead of
        // leaving the rail empty on a device that no longer exists.
        tree.set_cloud_available(false);
        assert_eq!(tree.device(), Device::Local);
    }

    #[test]
    fn switching_devices_drops_the_local_expansion() {
        let mut tree = SidebarTree::default();
        tree.set_cloud_available(true);
        assert!(tree.toggle("a"));
        assert!(tree.set_device(Device::Cloud));
        assert_eq!(tree.expanded(), None);
        assert!(tree.set_device(Device::Local));
        assert_eq!(tree.rows(&[], &[]).len(), 0);
    }

    #[test]
    fn cloud_device_projects_no_rows_without_a_cloud_channel() {
        let workspaces = vec![workspace("a", "甲")];
        let sessions = vec![thread("t1", Some("a"))];
        let mut tree = SidebarTree::default();
        tree.set_cloud_available(true);
        assert!(tree.set_device(Device::Cloud));
        assert!(tree.rows(&workspaces, &sessions).is_empty());
    }

    #[test]
    fn reveal_opens_the_owning_workspace_without_toggling() {
        let mut tree = SidebarTree::default();
        tree.reveal("a");
        assert!(tree.is_expanded("a"));
        tree.reveal("a");
        assert!(tree.is_expanded("a"));
        // An empty id cannot match a real workspace, so it must not open a
        // group that would then look permanently empty.
        tree.reveal("");
        assert!(tree.is_expanded("a"));
    }

    #[test]
    fn thread_without_a_workspace_falls_back_to_the_first_group() {
        let workspaces = vec![workspace("a", "甲"), workspace("b", "乙")];
        let sessions = vec![thread("orphan", None)];
        let mut tree = SidebarTree::default();
        assert!(tree.toggle("a"));
        let rows = tree.rows(&workspaces, &sessions);
        assert!(rows.iter().any(|row| matches!(
            row,
            Row::Thread { session, .. } if session.id == "orphan"
        )));
    }

    #[test]
    fn thread_count_filters_by_workspace() {
        let sessions = vec![
            thread("t1", Some("a")),
            thread("t2", Some("b")),
            thread("t3", Some("a")),
            thread("t4", None),
        ];
        assert_eq!(thread_count(&sessions, "a"), 2);
        assert_eq!(thread_count(&sessions, "b"), 1);
        assert_eq!(thread_count(&sessions, "missing"), 0);
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn moved_order_handles_both_directions() {
        let base = ids(&["a", "b", "c"]);
        assert_eq!(
            moved_order(&base, "a", 2),
            Some(ids(&["b", "a", "c"])),
            "dragging down past one row must not skip it"
        );
        assert_eq!(moved_order(&base, "c", 0), Some(ids(&["c", "a", "b"])));
        assert_eq!(
            moved_order(&base, "b", 1),
            Some(base.clone()),
            "dropping on your own slot is a no-op"
        );
        assert_eq!(moved_order(&base, "missing", 0), None);
        // An out-of-range target clamps to the end instead of panicking.
        assert_eq!(moved_order(&base, "a", 99), Some(ids(&["b", "c", "a"])));
    }

    #[test]
    fn drop_index_uses_row_midpoints_and_clamps_to_the_list() {
        let spans = [(0.0, 34.0), (34.0, 68.0), (68.0, 102.0)];
        assert_eq!(drop_index(&spans, 10.0), Some(0));
        assert_eq!(drop_index(&spans, 20.0), Some(1));
        assert_eq!(drop_index(&spans, 40.0), Some(1));
        assert_eq!(drop_index(&spans, 90.0), Some(3));
        // Above the first row and below the last both clamp to the ends.
        assert_eq!(drop_index(&spans, -50.0), Some(0));
        assert_eq!(drop_index(&spans, 500.0), Some(3));
        assert_eq!(drop_index(&[], 10.0), None);
    }

    /// Slot the rail would use for a pointer position. A `None` is asserted
    /// first and then replaced by a sentinel, so a geometry regression shows up
    /// as a wrong order below instead of a panicking test.
    fn slot_or_fail(spans: &[(f32, f32)], pointer_y: f32) -> usize {
        assert!(
            drop_index(spans, pointer_y).is_some(),
            "a pointer over or below a non-empty list always resolves a slot"
        );
        drop_index(spans, pointer_y).unwrap_or(usize::MAX)
    }

    #[test]
    fn rail_drag_composes_slot_and_order() {
        // The rail's own row height, three workspaces, as painted top to bottom.
        let spans: Vec<(f32, f32)> = (0..3)
            .map(|index| {
                let top = index as f32 * WORKSPACE_ROW_HEIGHT;
                (top, top + WORKSPACE_ROW_HEIGHT)
            })
            .collect();
        let base = ids(&["a", "b", "c"]);
        // Release just under the first row's midpoint: the dragged row has to
        // land above it, not on top of itself.
        assert_eq!(slot_or_fail(&spans, 4.0), 0);
        assert_eq!(
            moved_order(&base, "c", slot_or_fail(&spans, 4.0)),
            Some(ids(&["c", "a", "b"]))
        );
        // Release inside the second row, above its midpoint: between a and b.
        assert_eq!(slot_or_fail(&spans, WORKSPACE_ROW_HEIGHT + 4.0), 1);
        assert_eq!(
            moved_order(&base, "c", slot_or_fail(&spans, WORKSPACE_ROW_HEIGHT + 4.0)),
            Some(ids(&["a", "c", "b"]))
        );
        // Dragging the first row down past b lands between b and c.
        assert_eq!(slot_or_fail(&spans, WORKSPACE_ROW_HEIGHT * 2.0 + 4.0), 2);
        assert_eq!(
            moved_order(
                &base,
                "a",
                slot_or_fail(&spans, WORKSPACE_ROW_HEIGHT * 2.0 + 4.0)
            ),
            Some(ids(&["b", "a", "c"]))
        );
        // A release below the whole list clamps to the end.
        assert_eq!(slot_or_fail(&spans, 9_999.0), 3);
        assert_eq!(
            moved_order(&base, "a", slot_or_fail(&spans, 9_999.0)),
            Some(ids(&["b", "c", "a"]))
        );
    }
}
