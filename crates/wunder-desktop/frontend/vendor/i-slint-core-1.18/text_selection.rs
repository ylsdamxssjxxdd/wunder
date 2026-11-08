// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Mouse selection over the `StyledText` blocks of a scrollable view.
//!
//! A press in a selectable text starts a selection and grabs the pointer, so the drag keeps
//! arriving at that item even once it has left it; this module then works out which text is
//! under the pointer. The selection itself is kept here, not in the items: it is a pair of
//! `(text, byte offset)` ends, which is enough for every text to work out what of it is covered
//! when it is drawn, in any order, and without the items having to know about each other.
//!
//! Reading order is the layout's: blocks stack top to bottom, so comparing the window-space
//! top-left corner of two items orders them. Texts level with each other (a table row, say)
//! are joined by a space when copied, the rest by a line break.
//!
//! An end also remembers its ordinal in that order. A streaming view replaces every block item
//! on each update, which kills the `ItemWeak` but keeps the ordinals — so an open selection is
//! re-bound to the new items instead of disappearing mid-answer.
//!
//! Only `StyledText` takes part. `TextInput` has its own selection, and plain `Text` is used
//! for UI chrome that nobody selects.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::ops::Range;
use core::pin::Pin;
use core::time::Duration;

use crate::Property;
use crate::graphics::Color;
use crate::item_tree::{ItemRc, ItemWeak, ParentItemTraversalMode};
use crate::items::{Flickable, StyledTextItem};
use crate::lengths::{LogicalPoint, LogicalRect};
use crate::platform::Clipboard;
use crate::textlayout::sharedparley::TextUnit;
use crate::timers::{Timer, TimerMode};
use crate::window::{WindowAdapter, WindowInner};

/// One end of a selection: a text, and a byte offset into its plain text.
#[derive(Clone)]
struct Edge {
    item: ItemWeak,
    /// Where that text sat in reading order, counted among the texts of the same scope. Only
    /// used when `item` no longer upgrades, which a view rebuild does to every block.
    ordinal: usize,
    offset: usize,
}

struct Selection {
    /// The container the selection runs through, so a text elsewhere in the window is unaffected.
    scope: ItemWeak,
    start: Edge,
    end: Edge,
    color: Color,
}

struct Drag {
    scope: ItemWeak,
    /// The texts of the view in reading order, as they were when the press began. The view does
    /// not scroll itself during a drag (see `drag_in`), so the order stays valid; the rectangles
    /// are read fresh each time, because the auto-scroll below does move the content.
    parts: Vec<ItemRc>,
    anchor: Edge,
    color: Color,
    /// Where the pointer was last seen, in window coordinates. Held down at the edge of the
    /// viewport, the auto-scroll timer replays this until the button comes up.
    pointer: LogicalPoint,
    /// Repeating while the drag lasts; dropped — and so stopped — with the `Drag`. Never read:
    /// it is here for its `Drop`.
    #[allow(dead_code)]
    autoscroll: Timer,
}

/// How often the auto-scroll advances, and how far per tick at full speed.
const AUTOSCROLL_TICK: Duration = Duration::from_millis(16);
/// How close to the viewport edge the pointer has to be for the content to start moving.
const AUTOSCROLL_EDGE: f32 = 26.0;
const AUTOSCROLL_MAX_STEP: f32 = 26.0;

std::thread_local! {
    static SELECTION: RefCell<Option<Selection>> = const { RefCell::new(None) };
    static DRAG: RefCell<Option<Drag>> = const { RefCell::new(None) };
    /// The text the context menu was opened on, with the colour to highlight with. `select-all`
    /// is called from the menu and has no item of its own, so it works from here.
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
    /// Bumped whenever the selection changes. Drawing reads it, which is what schedules the
    /// repaint of the texts whose highlight differs.
    static CHANGES: Pin<Box<Property<u32>>> = Box::pin(Property::new(0));
}

/// What the context menu was opened on: the item whose view `select-all` walks, and the text to
/// copy. The text is snapshotted because pressing a menu item is itself a left press, and that
/// ends the selection before the item's action runs.
struct Context {
    item: ItemWeak,
    color: Color,
    text: Option<String>,
}

/// Makes the current evaluation (a draw, in practice) depend on the selection.
pub(crate) fn track() {
    CHANGES.with(|changes| {
        changes.as_ref().get();
    });
}

fn changed() {
    CHANGES.with(|changes| {
        changes.as_ref().set(changes.as_ref().get_untracked().wrapping_add(1));
    });
}

fn window_rect(item: &ItemRc) -> LogicalRect {
    let geometry = item.geometry();
    LogicalRect::new(item.map_to_window(geometry.origin), geometry.size)
}

fn corner(rect: LogicalRect) -> (f32, f32) {
    (rect.min_y(), rect.min_x())
}

fn is_text(item: &ItemRc) -> bool {
    item.downcast::<StyledTextItem>().is_some()
}

fn collect(scope: &ItemRc, parts: &mut Vec<ItemRc>) {
    fn push(parts: &mut Vec<ItemRc>, item: &ItemRc) {
        // Every text of the view, on screen or not: a scrolled-out block still has its place in
        // reading order, and leaving it out would both shift the ordinals and make a selection
        // that reaches past the viewport unrepresentable.
        if is_text(item) {
            parts.push(item.clone());
        }
    }
    push(parts, scope);
    scope.visit_descendants(|item| {
        push(parts, item);
        core::ops::ControlFlow::<()>::Continue(())
    });
    // Stable, so texts that share a corner (cells of one table row, or a view that has not been
    // laid out yet) keep the item tree's order, which is the document's.
    parts.sort_by(|a, b| {
        corner(window_rect(a)).partial_cmp(&corner(window_rect(b))).unwrap_or(core::cmp::Ordering::Equal)
    });
}

/// Binds an end to a live item. After a rebuild the weak reference is dead but the ordinal still
/// names the same place in the document, so the selection moves onto the new items instead of
/// being lost every time a streaming answer adds a block.
fn resolve(edge: &mut Edge, scope: &ItemRc, parts: &mut Vec<ItemRc>) -> Option<ItemRc> {
    if let Some(item) = edge.item.upgrade() {
        return Some(item);
    }
    if parts.is_empty() {
        collect(scope, parts);
    }
    let item = parts.get(edge.ordinal)?.clone();
    edge.item = item.downgrade();
    Some(item)
}

/// The scroll container a text sits in, if any: a selection runs through its siblings only.
fn scope_of(item: &ItemRc) -> ItemRc {
    let mut parent = item.parent_item(ParentItemTraversalMode::StopAtPopups);
    while let Some(current) = parent {
        if current.downcast::<Flickable>().is_some() {
            return current;
        }
        parent = current.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    item.clone()
}

fn set(selection: Selection) {
    SELECTION.with(|current| *current.borrow_mut() = Some(selection));
    changed();
}

/// Forgets the selection unless a press is building a new one.
pub(crate) fn clear() {
    if DRAG.with(|drag| drag.borrow().is_some()) {
        return;
    }
    if SELECTION.with(|current| current.borrow_mut().take()).is_some() {
        changed();
    }
}

/// A left press anywhere ends the previous drag (which may have lost its text) and its selection.
pub(crate) fn pressed() {
    DRAG.with(|drag| drag.borrow_mut().take());
    clear();
}

/// Which end of the selection a handle drag moves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionHandle {
    Start,
    End,
}

/// How close a press has to be to a handle to belong to the shell's knob
/// instead of starting a new selection. It covers the whole 40px touch box of
/// the knob, so a press anywhere on it never clears the selection first.
const HANDLE_HIT_RADIUS: f32 = 40.0;

/// Window-space points of the two selection handles: the caret line centre at
/// each edge. `None` when nothing is selected. Touch shells overlay draggable
/// knobs on these points; the core stays the single owner of the selection.
pub fn handle_positions(window: &crate::api::Window) -> Option<(LogicalPoint, LogicalPoint)> {
    let (start_item, start_offset, end_item, end_offset) = SELECTION.with(|current| {
        let mut slot = current.borrow_mut();
        let selection = slot.as_mut()?;
        let scope = selection.scope.upgrade()?;
        let mut parts = Vec::new();
        let start = resolve(&mut selection.start, &scope, &mut parts)?;
        let end = resolve(&mut selection.end, &scope, &mut parts)?;
        Some((start, selection.start.offset, end, selection.end.offset))
    })?;
    Some((
        edge_handle(
            window,
            &start_item,
            start_offset,
            crate::items::TextCursorAffinity::NextCharacter,
        )?,
        edge_handle(
            window,
            &end_item,
            end_offset,
            crate::items::TextCursorAffinity::PreviousCharacter,
        )?,
    ))
}

/// One handle: the centre of the caret line at `offset` in its text block.
fn edge_handle(
    window: &crate::api::Window,
    item: &ItemRc,
    offset: usize,
    affinity: crate::items::TextCursorAffinity,
) -> Option<LogicalPoint> {
    let text = item.downcast::<StyledTextItem>()?;
    let rect = crate::textlayout::sharedparley::styled_text_cursor_rect_for_byte_offset(
        window,
        text.as_pin_ref(),
        item,
        offset,
        affinity,
    )?;
    // The knobs sit at the caret line's vertical centre: the line bottom is
    // already outside the hit band of the shaped paragraph. The rectangle is
    // item-local and `map_to_window` takes parent coordinates, so fold in the
    // item's origin.
    let local = item.geometry().origin + rect.center().to_vector();
    Some(item.map_to_window(local))
}

/// The selection may survive a press on one of its handles: the shell's knob
/// owns that drag, so the edge can move instead of the selection starting over.
pub(crate) fn pressed_at(window: &crate::api::Window, position: LogicalPoint) {
    if let Some((start, end)) = handle_positions(window) {
        let near = |handle: LogicalPoint| {
            let dx = position.x - handle.x;
            let dy = position.y - handle.y;
            dx * dx + dy * dy <= HANDLE_HIT_RADIUS * HANDLE_HIT_RADIUS
        };
        if near(start) || near(end) {
            return;
        }
    }
    pressed();
}

/// Move one edge to the text under `position` (window coordinates). The edge
/// is clamped at the opposite edge, so a drag shrinks at most to an empty
/// selection and never inverts it.
pub fn move_handle(
    window: &crate::api::Window,
    handle: SelectionHandle,
    position: LogicalPoint,
) {
    let window_adapter = WindowInner::from_pub(window).window_adapter();
    let Some((parts, start_item, end_item, start_offset, end_offset)) =
        SELECTION.with(|current| {
            let mut slot = current.borrow_mut();
            let selection = slot.as_mut()?;
            let scope = selection.scope.upgrade()?;
            let mut parts = Vec::new();
            // `resolve` only fills `parts` when an edge's item was rebuilt, so
            // always collect: the handle drag needs the reading order.
            collect(&scope, &mut parts);
            let start = resolve(&mut selection.start, &scope, &mut parts)?;
            let end = resolve(&mut selection.end, &scope, &mut parts)?;
            Some((parts, start, end, selection.start.offset, selection.end.offset))
        })
    else {
        return;
    };

    let Some(target) = item_at(&parts, position) else { return };
    let Some(text) = target.downcast::<StyledTextItem>() else { return };
    let local = window_to_item_position(&target, position);
    let Some(range) =
        text.as_pin_ref()
            .range_at(&target, &window_adapter, local, TextUnit::Character)
    else {
        return;
    };
    let Some(ordinal) = parts.iter().position(|part| part == &target) else { return };
    let start_ordinal = parts.iter().position(|part| part == &start_item).unwrap_or(0);
    let end_ordinal = parts.iter().position(|part| part == &end_item).unwrap_or(0);

    let new_edge = match handle {
        SelectionHandle::Start if (ordinal, range.start) > (end_ordinal, end_offset) => Edge {
            item: end_item.downgrade(),
            ordinal: end_ordinal,
            offset: end_offset,
        },
        SelectionHandle::End if (ordinal, range.start) < (start_ordinal, start_offset) => Edge {
            item: start_item.downgrade(),
            ordinal: start_ordinal,
            offset: start_offset,
        },
        _ => Edge { item: target.downgrade(), ordinal, offset: range.start },
    };
    SELECTION.with(|current| {
        let mut slot = current.borrow_mut();
        let Some(selection) = slot.as_mut() else { return };
        match handle {
            SelectionHandle::Start => selection.start = new_edge,
            SelectionHandle::End => selection.end = new_edge,
        }
    });
    changed();
}

/// The text block under `position`, or the nearest one vertically when the
/// pointer is beyond the first or last block: a handle drag past the viewport
/// keeps extending the edge instead of stopping.
fn item_at(parts: &[ItemRc], position: LogicalPoint) -> Option<ItemRc> {
    if let Some(item) = parts
        .iter()
        .find(|part| is_text(part) && window_rect(part).contains(position))
    {
        return Some(item.clone());
    }
    parts
        .iter()
        .filter(|part| is_text(part))
        .min_by(|a, b| {
            let da = vertical_distance(window_rect(a), position);
            let db = vertical_distance(window_rect(b), position);
            da.partial_cmp(&db).unwrap_or(core::cmp::Ordering::Equal)
        })
        .cloned()
}

fn vertical_distance(rect: LogicalRect, position: LogicalPoint) -> f32 {
    if position.y < rect.min_y() {
        rect.min_y() - position.y
    } else if position.y > rect.max_y() {
        position.y - rect.max_y()
    } else {
        0.0
    }
}

/// Window point → the item's own coordinate system. The transcript has no
/// rotation or scale transforms, so folding the item and ancestor origins is
/// enough and is the inverse of what `map_to_window` added.
fn window_to_item_position(item: &ItemRc, position: LogicalPoint) -> LogicalPoint {
    let mut local = position;
    let mut current = Some(item.clone());
    while let Some(node) = current {
        local -= node.geometry().origin.to_vector();
        current = node.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    local
}

/// Starts selecting at `position`, in `text`'s coordinates. `click_count` counts repeats, so the
/// first press selects by character, the second by word and the third by paragraph.
pub(crate) fn press(
    item: &ItemRc,
    text: Pin<&StyledTextItem>,
    window_adapter: &Rc<dyn WindowAdapter>,
    position: LogicalPoint,
    click_count: u8,
) -> bool {
    let scope = scope_of(item);
    let mut parts = Vec::new();
    collect(&scope, &mut parts);
    let Some(index) = parts.iter().position(|part| part == item) else { return false };

    let unit = match click_count % 3 {
        0 => TextUnit::Character,
        1 => TextUnit::Word,
        _ => TextUnit::Paragraph,
    };
    let Some(range) = text.range_at(item, window_adapter, position, unit) else { return false };

    let color = text.link_color().with_alpha(0.35);
    // A left press only needs the scope for `Ctrl+A` to walk; the menu's copy
    // buffer is filled by `right_press`.
    let context = Context { item: item.downgrade(), color, text: None };
    CONTEXT.with(|slot| *slot.borrow_mut() = Some(context));
    let anchor = Edge { item: item.downgrade(), ordinal: index, offset: range.start };
    let empty = range.start == range.end;
    if !empty {
        set(Selection {
            scope: scope.downgrade(),
            start: Edge { item: item.downgrade(), ordinal: index, offset: range.start },
            end: Edge { item: item.downgrade(), ordinal: index, offset: range.end },
            color,
        });
    }

    // One timer for the whole drag: while the pointer rests near the edge of the viewport it
    // keeps scrolling, so a selection can reach past what is on screen. Dropped in `release`.
    let autoscroll = Timer::default();
    let weak_window = Rc::downgrade(window_adapter);
    let weak_scope = scope.downgrade();
    autoscroll.start(TimerMode::Repeated, AUTOSCROLL_TICK, move || {
        let (Some(window), Some(scope)) = (weak_window.upgrade(), weak_scope.upgrade()) else {
            return;
        };
        autoscroll_tick(&scope, &window);
    });

    DRAG.with(|drag| {
        *drag.borrow_mut() = Some(Drag {
            scope: scope.downgrade(),
            parts,
            anchor,
            color,
            pointer: item.map_to_window(position),
            autoscroll,
        })
    });
    true
}

/// Records a right press on `item` and drops the selection unless the press landed inside it —
/// a right click outside the highlight starts from nothing, the way it does in a browser.
/// Returns whether there is still something to copy.
pub(crate) fn right_press(
    item: &ItemRc,
    window_adapter: &Rc<dyn WindowAdapter>,
    position: LogicalPoint,
) -> bool {
    let color = item.downcast::<StyledTextItem>().map_or(
        Color::from_argb_u8(90, 0, 120, 215),
        |text| text.as_pin_ref().link_color().with_alpha(0.35),
    );
    let inside = item
        .downcast::<StyledTextItem>()
        .and_then(|text| {
            text.as_pin_ref().range_at(item, window_adapter, position, TextUnit::Character)
        })
        .and_then(|range| covered(item).map(|(span, _)| span.contains(&range.start)))
        .unwrap_or(false);
    if !inside {
        clear();
    }
    // Recorded after the drop, so the snapshot matches what the menu is told:
    // a right click outside the highlight leaves nothing to copy.
    let context = Context {
        item: item.downgrade(),
        color,
        text: inside.then(selected_text).flatten(),
    };
    CONTEXT.with(|slot| *slot.borrow_mut() = Some(context));
    inside
}

/// `Ctrl+A` from the window level: a focused text input selects its own content, so it keeps
/// the shortcut; anywhere else the whole view the last press was in gets selected.
pub(crate) fn select_all_in_view(window: &WindowInner) -> bool {
    let input_focused = window
        .focus_item
        .borrow()
        .upgrade()
        .is_some_and(|item| item.downcast::<crate::items::TextInput>().is_some());
    if input_focused {
        return false;
    }
    select_all()
}

/// Selects every text of the view the context menu was opened on.
pub fn select_all() -> bool {
    let target = CONTEXT.with(|context| {
        let context = context.borrow();
        let context = context.as_ref()?;
        Some((scope_of(&context.item.upgrade()?), context.color))
    });
    let Some((scope, color)) = target else { return false };

    let mut parts = Vec::new();
    collect(&scope, &mut parts);
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else { return false };
    let last_len =
        last.downcast::<StyledTextItem>().map_or(0, |text| text.as_pin_ref().plain().len());
    set(Selection {
        scope: scope.downgrade(),
        start: Edge { item: first.downgrade(), ordinal: 0, offset: 0 },
        end: Edge { item: last.downgrade(), ordinal: parts.len() - 1, offset: last_len },
        color,
    });
    true
}

/// Whether a press is currently dragging a selection.
pub(crate) fn dragging() -> bool {
    DRAG.with(|drag| drag.borrow().is_some())
}

/// Whether a selection drag is running in `scope`, and if so whether it has selected anything
/// yet. The scroll container uses this to keep the pointer while a selection is being dragged.
pub(crate) fn drag_in(scope: &ItemRc) -> Option<bool> {
    DRAG.with(|drag| {
        let drag = drag.borrow();
        let drag = drag.as_ref()?;
        if drag.scope.upgrade().as_ref() != Some(scope) {
            return None;
        }
        Some(SELECTION.with(|selection| selection.borrow().is_some()))
    })
}

/// Whether `item` is still part of `scope`'s tree. A rebuild moves every block into a fresh
/// component, so the items collected when the press began can be stale even though they are
/// still alive (the drag itself holds them).
fn belongs_to(item: &ItemRc, scope: &ItemRc) -> bool {
    let mut current = Some(item.clone());
    while let Some(node) = current {
        if &node == scope {
            return true;
        }
        current = node.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    false
}

/// Re-reads the texts of the view when the ones from the press are gone.
fn refresh(drag: &mut Drag) {
    let Some(scope) = drag.scope.upgrade() else { return };
    // A model update replaces the rows wholesale, so one surviving text is enough to trust the
    // whole list; probing every part on each mouse move would cost a tree walk per block.
    if drag.parts.first().is_some_and(|first| belongs_to(first, &scope)) {
        return;
    }
    drag.parts.clear();
    collect(&scope, &mut drag.parts);
    // The press-time anchor died with the old items; its ordinal still names the same text.
    if drag.anchor.item.upgrade().is_none() {
        if let Some(item) = drag.parts.get(drag.anchor.ordinal) {
            drag.anchor.item = item.downgrade();
        }
    }
}

/// Extends the selection to `pointer`, in window coordinates.
pub(crate) fn drag_to(window_adapter: &Rc<dyn WindowAdapter>, pointer: LogicalPoint) {
    let target = DRAG.with(|drag| {
        let mut drag = drag.borrow_mut();
        let drag = drag.as_mut()?;
        refresh(drag);
        drag.pointer = pointer;
        Some((drag.scope.clone(), drag.parts.clone(), drag.anchor.clone(), drag.color))
    });
    let Some((scope, parts, anchor, color)) = target else { return };
    if parts.is_empty() {
        return;
    }

    // The text under the pointer, or else the closest one: past the last line of a block the
    // drag belongs to the next block, above the first one to its start.
    let mut hit: Option<(usize, LogicalPoint)> = None;
    for (index, part) in parts.iter().enumerate() {
        let rect = window_rect(part);
        if rect.contains(pointer) {
            hit = Some((index, pointer - rect.origin.to_vector()));
            break;
        }
    }
    let (index, local) = match hit {
        Some(hit) => hit,
        None => {
            let mut best = 0;
            for (index, part) in parts.iter().enumerate() {
                if window_rect(part).max_y() <= pointer.y {
                    best = index;
                }
            }
            let rect = window_rect(&parts[best]);
            let y = pointer.y.max(rect.min_y()).min(rect.max_y() - 1.0);
            (best, LogicalPoint::new(pointer.x, y) - rect.origin.to_vector())
        }
    };

    let part = &parts[index];
    let Some(text) = part.downcast::<StyledTextItem>() else { return };
    let Some(range) = text.as_pin_ref().range_at(part, window_adapter, local, TextUnit::Character)
    else {
        return;
    };
    let offset = range.start;

    let edge = Edge { item: part.downgrade(), ordinal: index, offset };
    let (start, end) = if index < anchor.ordinal || (index == anchor.ordinal && offset < anchor.offset)
    {
        (edge, anchor)
    } else {
        (anchor, edge)
    };
    set(Selection { scope, start, end, color });
}

/// Advances the content under a held drag. Returns whether anything moved, which is also the
/// signal that the selection has to be recomputed.
fn scroll_step(scope: &ItemRc, pointer: LogicalPoint) -> bool {
    let Some(flickable) = scope.downcast::<Flickable>() else { return false };
    let flickable = flickable.as_pin_ref();
    let rect = window_rect(scope);

    // Positive below the bottom edge, negative above the top one, and scaled by how far out the
    // pointer is, so the closer to the border the slower the creep.
    let overshoot = if pointer.y < rect.min_y() + AUTOSCROLL_EDGE {
        pointer.y - (rect.min_y() + AUTOSCROLL_EDGE)
    } else if pointer.y > rect.max_y() - AUTOSCROLL_EDGE {
        pointer.y - (rect.max_y() - AUTOSCROLL_EDGE)
    } else {
        return false;
    };
    let step = (overshoot * 0.4).clamp(-AUTOSCROLL_MAX_STEP, AUTOSCROLL_MAX_STEP);
    if step.abs() < 0.5 {
        return false;
    }

    let limit = (flickable.content_height().get() - scope.geometry().height()).max(0.);
    let current = flickable.content_y().get();
    // `content-y` is the content's offset inside the viewport: scrolling down moves the content
    // up, so it becomes more negative. `step` is positive below the viewport edge.
    let next = (current - step).clamp(-limit, 0.);
    if (next - current).abs() < 0.5 {
        return false;
    }
    flickable.content_y.set(crate::lengths::LogicalLength::new(next));
    true
}

fn autoscroll_tick(scope: &ItemRc, window: &Rc<dyn WindowAdapter>) {
    let pointer = DRAG.with(|drag| drag.borrow().as_ref().map(|drag| drag.pointer));
    let Some(pointer) = pointer else { return };
    if scroll_step(scope, pointer) {
        // The texts moved under a pointer that did not: what is selected changed with them.
        drag_to(window, pointer);
    }
}

/// Ends the drag. Returns whether anything is selected.
pub(crate) fn release() -> bool {
    DRAG.with(|drag| drag.borrow_mut().take());
    SELECTION.with(|current| current.borrow().is_some())
}

/// The selection as plain text, in reading order.
fn selected_text() -> Option<String> {
    let scope = SELECTION.with(|current| {
        let current = current.borrow();
        let current = current.as_ref()?;
        current.scope.upgrade()
    })?;
    let (start, start_offset, end, end_offset) = SELECTION.with(|current| {
        let mut slot = current.borrow_mut();
        let selection = slot.as_mut()?;
        let mut parts = Vec::new();
        let start = resolve(&mut selection.start, &scope, &mut parts)?;
        let end = resolve(&mut selection.end, &scope, &mut parts)?;
        Some((start, selection.start.offset, end, selection.end.offset))
    })?;
    let mut parts = Vec::new();
    collect(&scope, &mut parts);
    let index_of = |item: &ItemRc| parts.iter().position(|part| part == item);
    let (start_index, end_index) = (index_of(&start)?, index_of(&end)?);

    let mut text = String::new();
    let mut previous: Option<LogicalRect> = None;
    for index in start_index..=end_index {
        let part = &parts[index];
        let Some(item) = part.downcast::<StyledTextItem>() else { continue };
        let plain = item.as_pin_ref().plain();
        let len = plain.len();
        let from = if index == start_index { start_offset.min(len) } else { 0 };
        let to = if index == end_index { end_offset.min(len) } else { len };
        if from >= to {
            continue;
        }
        let rect = window_rect(part);
        if let Some(before) = previous {
            // Texts side by side on a line read apart; stacked ones read as a new line.
            let level = rect.min_y() < before.max_y() && before.min_y() < rect.max_y();
            text.push(if level { ' ' } else { '\n' });
        }
        text.push_str(plain.get(from..to).unwrap_or_default());
        previous = Some(rect);
    }
    (!text.is_empty()).then_some(text)
}

/// The selection as plain text, for a host that wants to show or act on it without going
/// through the clipboard.
pub fn selection_text() -> Option<String> {
    selected_text()
}

/// Copies the selection, unless a text input with a selection of its own has the focus: that
/// one copies itself. Returns whether the selection was copied.
pub(crate) fn copy(window: &WindowInner) -> bool {
    let input_selecting = window.focus_item.borrow().upgrade().is_some_and(|item| {
        item.downcast::<crate::items::TextInput>()
            .is_some_and(|input| input.as_pin_ref().has_selection())
    });
    if input_selecting {
        return false;
    }
    let Some(text) = selected_text() else { return false };
    window.context().platform().set_clipboard_text(&text, Clipboard::DefaultClipboard);
    true
}

/// Copies the current selection. Public for a host that owns the context menu and wants to run
/// the action from its own UI thread; `copy` above is the same thing from inside the window.
pub fn copy_selection(window: &crate::api::Window) -> bool {
    copy(WindowInner::from_pub(window))
}

/// Runs the context menu's `copy`. The menu item is a left press, which ends the selection before
/// its action runs, so this copies the text snapshotted when the menu opened instead of whatever
/// is selected now.
pub fn copy_context(window: &crate::api::Window) -> bool {
    let text = CONTEXT.with(|context| context.borrow().as_ref().and_then(|c| c.text.clone()));
    let Some(text) = text else { return false };
    WindowInner::from_pub(window)
        .context()
        .platform()
        .set_clipboard_text(&text, Clipboard::DefaultClipboard);
    true
}

/// What of `item` the selection covers, and the colour behind it. Called while drawing, so the
/// highlight follows the selection without anything having to invalidate the items.
pub(crate) fn covered(item: &ItemRc) -> Option<(Range<usize>, Color)> {
    track();
    let mut parts = Vec::new();
    let resolved = SELECTION.with(|current| {
        let mut slot = current.borrow_mut();
        let selection = slot.as_mut()?;
        let scope = selection.scope.upgrade()?;
        let start = resolve(&mut selection.start, &scope, &mut parts)?;
        let end = resolve(&mut selection.end, &scope, &mut parts)?;
        Some((start, selection.start.offset, end, selection.end.offset, selection.color))
    });
    let (start_item, start_offset, end_item, end_offset, color) = resolved?;

    let this = corner(window_rect(item));
    if item == &start_item && item == &end_item {
        let (from, to) = (start_offset.min(end_offset), start_offset.max(end_offset));
        return (from < to).then_some((from..to, color));
    }
    let len = item.downcast::<StyledTextItem>().map_or(0, |text| text.as_pin_ref().plain().len());
    let start = corner(window_rect(&start_item));
    let end = corner(window_rect(&end_item));

    if item == &start_item {
        return (start_offset < len).then_some((start_offset..len, color));
    }
    if item == &end_item {
        return (end_offset > 0).then_some((0..end_offset.min(len), color));
    }
    (this > start && this < end && len > 0).then_some((0..len, color))
}
