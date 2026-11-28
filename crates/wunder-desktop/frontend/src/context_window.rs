//! Projection of the engine's recorded request composition onto the context
//! window card: legend rows, the contents list, the window map and its grid.
//!
//! The card reads one recorded snapshot per thread. The engine records it at
//! the request gate and rescales it to the vendor's own count once that
//! arrives, so nothing here estimates tokens; this module only turns the
//! snapshot into the rows and rectangles the card draws.
//!
//! The map layout follows magpie's (Bruls/Huizing/van Wijk squarified
//! treemap), solved for the map's real pixel size the shell reports: the used
//! share takes a band along the long side — eased by `share^0.35` so an almost
//! empty window still leaves its parts readable — and the free space takes the
//! rest. A part lays its own items inside its rectangle; items too small to
//! read gather into one "more" tile.
use crate::{ContextItemRow, ContextPartRow, ContextTile, ContextTipRow, MainWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wunder_desktop::context_map::{fmt_k, layout, listed_items, pct, Record, Tip};
use wunder_desktop::native::NativeContextWindow;
use wunder_desktop::NativeDesktop;

/// The map box before the shell has reported its size; the first layout request
/// replaces it with the real one.
const DEFAULT_W: f64 = 880.0;
const DEFAULT_H: f64 = 232.0;

thread_local! {
    /// The snapshot the card was last filled from. Filter changes re-project
    /// the rows from it instead of re-reading the workspace on every click.
    static RECORD: RefCell<Option<Record>> = const { RefCell::new(None) };
    /// Hover cards, one per tile in the same order as the tiles model.
    static TIPS: RefCell<Vec<Tip>> = const { RefCell::new(Vec::new()) };
    /// The map box the tiles were solved for; the shell reports it when the
    /// card opens or the window resizes.
    static MAP: Cell<(f64, f64)> = const { Cell::new((DEFAULT_W, DEFAULT_H)) };
}

pub fn install(app: &MainWindow, desktop: Arc<NativeDesktop>) {
    let load_desktop = Arc::clone(&desktop);
    let weak = app.as_weak();
    app.on_load_context_window(move || {
        let Some(app) = weak.upgrade() else { return };
        let session = app.get_active_session_id().to_string();
        let native = if session.trim().is_empty() {
            NativeContextWindow::default()
        } else {
            load_desktop.context_window(&session).unwrap_or_default()
        };
        let record = Record::from_native(native);
        project(&app, &record);
        RECORD.with(|slot| *slot.borrow_mut() = Some(record));
    });

    // The map is laid out for its real size, like the reference does in CSS
    // pixels: reporting the size again re-solves the rectangles.
    let weak = app.as_weak();
    app.on_context_map_resized(move |width, height| {
        let Some(app) = weak.upgrade() else { return };
        let width = width as f64;
        let height = height as f64;
        if width < 120.0 || height < 60.0 {
            return;
        }
        let old = MAP.with(|size| size.get());
        if (old.0 - width).abs() < 0.5 && (old.1 - height).abs() < 0.5 {
            return;
        }
        MAP.with(|size| size.set((width, height)));
        let record = RECORD.with(|slot| slot.borrow().clone());
        if let Some(record) = record {
            project_tiles(&app, &record, width, height);
            close_tip(&app);
        }
    });

    let weak = app.as_weak();
    app.on_context_tile_hover(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let tip = TIPS.with(|slot| slot.borrow().get(index.max(0) as usize).cloned());
        if let Some(tip) = tip {
            show_tip(&app, &tip);
        }
    });
    let weak = app.as_weak();
    app.on_context_tile_leave(move || {
        if let Some(app) = weak.upgrade() {
            close_tip(&app);
        }
    });

    let text_desktop = Arc::clone(&desktop);
    let weak = app.as_weak();
    app.on_context_tile_open(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let tip = TIPS.with(|slot| slot.borrow().get(index.max(0) as usize).cloned());
        let Some(tip) = tip else { return };
        let record = RECORD.with(|slot| slot.borrow().clone());
        let Some(record) = record else { return };
        if tip.kind == "item" {
            let session = app.get_active_session_id().to_string();
            let text =
                text_desktop.context_text(&session, tip.slot, &tip.tag, &tip.name, tip.count);
            open_item_detail(
                &app, &record, tip.slot, &tip.tag, &tip.name, tip.count, text,
            );
        } else if tip.kind == "part" {
            open_part_detail(&app, &record, tip.slot);
        }
        close_tip(&app);
    });

    let weak = app.as_weak();
    app.on_context_legend_open(move |slot| {
        let Some(app) = weak.upgrade() else { return };
        let record = RECORD.with(|cell| cell.borrow().clone());
        let Some(record) = record else { return };
        open_part_detail(&app, &record, slot);
    });

    let text_desktop = Arc::clone(&desktop);
    let weak = app.as_weak();
    app.on_context_detail_row_toggle(move |index, slot, tag, name, count| {
        let Some(app) = weak.upgrade() else { return };
        if app.get_context_detail_row_open() == index {
            app.set_context_detail_row_open(-1);
            app.set_context_detail_row_text("".into());
            return;
        }
        let session = app.get_active_session_id().to_string();
        let text = text_desktop.context_text(&session, slot, &tag, &name, count as i64);
        app.set_context_detail_row_open(index);
        app.set_context_detail_row_text(text.unwrap_or_default().into());
    });

    let weak = app.as_weak();
    app.on_context_detail_dismissed(move || {
        if let Some(app) = weak.upgrade() {
            clear_detail_text(&app);
            app.set_context_detail_open(false);
        }
    });
}

fn show_tip(app: &MainWindow, tip: &Tip) {
    app.set_context_tip_open(true);
    app.set_context_tip_kind(tip.kind.as_str().into());
    app.set_context_tip_slot(tip.slot);
    app.set_context_tip_tag(tip.tag.as_str().into());
    app.set_context_tip_name(tip.name.as_str().into());
    app.set_context_tip_count(tip.count.min(i32::MAX as i64) as i32);
    app.set_context_tip_value(tip.value.as_str().into());
    app.set_context_tip_pct(tip.pct.as_str().into());
    app.set_context_tip_rows(ModelRc::new(VecModel::from(
        tip.rows
            .iter()
            .map(|row| ContextTipRow {
                tag: row.tag.as_str().into(),
                name: row.name.as_str().into(),
                count: row.count.min(i32::MAX as i64) as i32,
                value: row.value.as_str().into(),
                on: false,
            })
            .collect::<Vec<_>>(),
    )));
    app.set_context_tip_more(tip.more);
}

fn close_tip(app: &MainWindow) {
    app.set_context_tip_open(false);
}

/// A thread switch: the card closes and its last projection is dropped, so the
/// next open reflects the new thread rather than the previous one.
pub fn reset(app: &MainWindow) {
    RECORD.with(|slot| *slot.borrow_mut() = None);
    TIPS.with(|slot| slot.borrow_mut().clear());
    app.set_context_window_open(false);
    app.set_context_has_data(false);
    app.set_context_used_tokens(0);
    app.set_context_capacity_tokens(0);
    app.set_context_used_text("—".into());
    app.set_context_capacity_text("—".into());
    app.set_context_free_text("—".into());
    app.set_context_measured(false);
    app.set_context_turns(0);
    app.set_context_model_label("".into());
    app.set_context_parts(ModelRc::new(VecModel::from(Vec::<ContextPartRow>::new())));
    app.set_context_tiles(ModelRc::new(VecModel::from(Vec::<ContextTile>::new())));
    close_tip(app);
    clear_detail_text(app);
    app.set_context_detail_open(false);
}

fn project(app: &MainWindow, record: &Record) {
    let has_data = record.has_data();
    let window = record.window.max(0);
    let used = if has_data { record.tokens } else { 0 };
    let free = if window > 0 {
        (window - used).max(0)
    } else {
        0
    };
    let capacity_text = if window > 0 {
        fmt_k(window)
    } else {
        "—".into()
    };
    let used_text = if has_data { fmt_k(used) } else { "—".into() };
    let free_text = if window > 0 {
        fmt_k(free)
    } else {
        "—".into()
    };

    app.set_context_has_data(has_data);
    app.set_context_used_tokens(used.min(i32::MAX as i64) as i32);
    app.set_context_capacity_tokens(window.min(i32::MAX as i64) as i32);
    app.set_context_used_text(used_text.into());
    app.set_context_capacity_text(capacity_text.into());
    app.set_context_free_text(free_text.into());
    app.set_context_measured(record.measured);
    app.set_context_turns(record.turns.min(i32::MAX as i64) as i32);
    app.set_context_model_label(record.model_label().into());
    app.set_context_parts(ModelRc::new(VecModel::from(part_rows(record))));
    let (width, height) = MAP.with(|size| size.get());
    project_tiles(app, record, width, height);
    close_tip(app);
    app.set_context_detail_open(false);
}

fn part_rows(record: &Record) -> Vec<ContextPartRow> {
    record
        .parts
        .iter()
        .map(|part| ContextPartRow {
            slot: part.slot,
            tokens: part.tokens.min(i32::MAX as i64) as i32,
            tokens_text: fmt_k(part.tokens).into(),
        })
        .collect()
}

fn project_tiles(app: &MainWindow, record: &Record, width: f64, height: f64) {
    let layout = layout(record, width, height, true);
    let tiles = layout
        .tiles
        .into_iter()
        .map(|tile| ContextTile {
            x: tile.x,
            y: tile.y,
            w: tile.w,
            h: tile.h,
            slot: tile.slot,
            kind: tile.kind.as_str().into(),
            tokens_text: tile.display_value(record).into(),
            tag: tile.tag.into(),
            name: tile.name.into(),
            count: tile.count.clamp(0, i32::MAX as i64) as i32,
            item: tile.item,
            show_name: tile.show_name,
            show_value: tile.show_value,
        })
        .collect::<Vec<_>>();
    app.set_context_tiles(ModelRc::new(VecModel::from(tiles)));
    TIPS.with(|slot| *slot.borrow_mut() = layout.tips);
}

fn open_part_detail(app: &MainWindow, record: &Record, slot: i32) {
    let Some(part) = record.parts.iter().find(|part| part.slot == slot) else {
        return;
    };
    let listed = listed_items(part);
    let most = listed
        .iter()
        .map(|item| item.tokens)
        .max()
        .unwrap_or(1)
        .max(1);
    let rows: Vec<ContextItemRow> = listed
        .iter()
        .enumerate()
        .map(|(rank, item)| ContextItemRow {
            slot: part.slot,
            tag: item.tag.as_str().into(),
            name: item.name.as_str().into(),
            count: item.count.min(i32::MAX as i64) as i32,
            tokens_text: fmt_k(item.tokens).into(),
            ratio: (item.tokens as f32 / most as f32).clamp(0.0, 1.0),
            item: rank.min(i32::MAX as usize) as i32,
        })
        .collect();
    app.set_context_detail_kind("part".into());
    app.set_context_detail_slot(part.slot);
    app.set_context_detail_tag("".into());
    app.set_context_detail_name("".into());
    app.set_context_detail_count(0);
    app.set_context_detail_value(fmt_k(part.tokens).into());
    app.set_context_detail_pct(pct(part.tokens, record.tokens).into());
    clear_detail_text(app);
    app.set_context_detail_rows(ModelRc::new(VecModel::from(rows)));
    app.set_context_detail_open(true);
}

fn open_item_detail(
    app: &MainWindow,
    record: &Record,
    slot: i32,
    tag: &str,
    name: &str,
    count: i64,
    text: Option<String>,
) {
    let Some(part) = record.parts.iter().find(|part| part.slot == slot) else {
        return;
    };
    let listed = listed_items(part);
    let Some(item) = listed
        .iter()
        .find(|item| item.tag == tag && item.name == name && item.count == count)
    else {
        return;
    };
    app.set_context_detail_kind("item".into());
    app.set_context_detail_slot(part.slot);
    app.set_context_detail_tag(item.tag.as_str().into());
    app.set_context_detail_name(item.name.as_str().into());
    app.set_context_detail_count(item.count.min(i32::MAX as i64) as i32);
    app.set_context_detail_value(fmt_k(item.tokens).into());
    app.set_context_detail_pct(pct(item.tokens, record.tokens).into());
    open_detail_text(app, text.unwrap_or_default());
    app.set_context_detail_row_open(-1);
    app.set_context_detail_row_text("".into());
    // A single item shows only itself: no list of the part's other items.
    app.set_context_detail_rows(ModelRc::new(VecModel::from(Vec::<ContextItemRow>::new())));
    app.set_context_detail_open(true);
}

/// Release the shell's copy of the detail text and any expanded part row. The
/// engine's cache still holds the request; this only clears the Slint shell.
fn clear_detail_text(app: &MainWindow) {
    app.set_context_detail_text("".into());
    app.set_context_detail_row_open(-1);
    app.set_context_detail_row_text("".into());
}

/// Show the whole item text at once. The card scrolls it; no part is hidden
/// behind a "show more" step.
fn open_detail_text(app: &MainWindow, body: String) {
    app.set_context_detail_text(body.into());
}
