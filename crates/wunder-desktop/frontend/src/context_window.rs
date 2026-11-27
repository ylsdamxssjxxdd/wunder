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
use crate::{ContextCell, ContextItemRow, ContextPartRow, ContextTile, ContextTipRow, MainWindow};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::sync::Arc;
use wunder_desktop::native::NativeContextWindow;
use wunder_desktop::NativeDesktop;

/// Parts in the order the card lists them; the index is the palette slot the
/// .slint side colours by.
const PART_SLOTS: [(&str, i32); 5] = [
    ("system", 1),
    ("tools", 2),
    ("files", 3),
    ("results", 4),
    ("chat", 5),
];

/// The map box before the shell has reported its size; the first layout request
/// replaces it with the real one.
const DEFAULT_W: f64 = 880.0;
const DEFAULT_H: f64 = 232.0;

/// Below these sizes a tile cannot carry its text any more (magpie's rules).
const TILE_BARE_W: f64 = 54.0;
const TILE_BARE_H: f64 = 30.0;
const TILE_LOW_H: f64 = 46.0;
/// A tile is never laid smaller than this area, or thinner than this side.
const TILE_FLOOR: f64 = 700.0;
const TILE_SIDE: f64 = 16.0;
const TILE_MIN: f64 = 1500.0;
const TILE_MAX_ITEMS: usize = 12;
const TIP_ROWS: usize = 5;
const GRID_TIP_ROWS: usize = 4;
/// Cells in the grid look: each is a quarter of a percent of the window.
const CELLS: usize = 400;

#[derive(Clone, Debug, Default)]
struct ItemRecord {
    tag: String,
    name: String,
    count: i64,
    tokens: i64,
    /// Message-stream position written by the engine; 0 means an older
    /// snapshot that predates this ordering field.
    order: i64,
}

#[derive(Clone, Debug, Default)]
struct PartRecord {
    slot: i32,
    tokens: i64,
    items: Vec<ItemRecord>,
}

#[derive(Clone, Debug, Default)]
struct Record {
    model: String,
    provider: String,
    window: i64,
    tokens: i64,
    measured: bool,
    parts: Vec<PartRecord>,
}

#[derive(Clone, Debug, Default)]
struct Tip {
    kind: String,
    slot: i32,
    tag: String,
    name: String,
    count: i64,
    value: String,
    pct: String,
    rows: Vec<TipRow>,
    more: i32,
}

#[derive(Clone, Debug, Default)]
struct TipRow {
    tag: String,
    name: String,
    count: i64,
    value: String,
    on: bool,
}

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

    let weak = app.as_weak();
    app.on_context_filter_changed(move || {
        let Some(app) = weak.upgrade() else { return };
        let record = RECORD.with(|slot| slot.borrow().clone());
        if let Some(record) = record {
            project_rows(&app, &record);
        }
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
            let laid = tiles(&record, width, height);
            app.set_context_tiles(ModelRc::new(VecModel::from(laid.tiles)));
            TIPS.with(|slot| *slot.borrow_mut() = laid.tips);
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

    let weak = app.as_weak();
    app.on_context_cell_hover(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let record = RECORD.with(|slot| slot.borrow().clone());
        let Some(record) = record else { return };
        let cells = cells(&record);
        let Some(cell) = cells.get(index.max(0) as usize) else {
            return;
        };
        if cell.slot == 0 {
            let free = (record.window.max(record.tokens) - record.tokens).max(0);
            let tip = Tip {
                kind: "free".into(),
                value: fmt_k(free),
                pct: pct(free, record.window.max(record.tokens)),
                ..Default::default()
            };
            show_tip(&app, &tip);
            return;
        }
        let Some(part) = record.parts.iter().find(|part| part.slot == cell.slot) else {
            return;
        };
        let tip = part_tip(&record, part, cell.item, GRID_TIP_ROWS);
        show_tip(&app, &tip);
    });
    let weak = app.as_weak();
    app.on_context_cell_leave(move || {
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

    let text_desktop = Arc::clone(&desktop);
    let weak = app.as_weak();
    app.on_context_cell_open(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let record = RECORD.with(|slot| slot.borrow().clone());
        let Some(record) = record else { return };
        let cells = cells(&record);
        let Some(cell) = cells.get(index.max(0) as usize) else {
            return;
        };
        if cell.slot == 0 {
            return;
        }
        if cell.item >= 0 {
            let Some(part) = record.parts.iter().find(|part| part.slot == cell.slot) else {
                return;
            };
            let sorted = sorted_items(part);
            if let Some(item) = sorted.get(cell.item as usize) {
                let session = app.get_active_session_id().to_string();
                let text = text_desktop
                    .context_text(&session, part.slot, &item.tag, &item.name, item.count);
                open_item_detail(
                    &app, &record, part.slot, &item.tag, &item.name, item.count, text,
                );
            }
        } else {
            open_part_detail(&app, &record, cell.slot);
        }
        close_tip(&app);
    });

    let text_desktop = Arc::clone(&desktop);
    let weak = app.as_weak();
    app.on_context_row_open(move |slot, tag, name, count| {
        let Some(app) = weak.upgrade() else { return };
        let record = RECORD.with(|slot| slot.borrow().clone());
        let Some(record) = record else { return };
        let session = app.get_active_session_id().to_string();
        let text = text_desktop.context_text(&session, slot, &tag, &name, count as i64);
        open_item_detail(&app, &record, slot, &tag, &name, count as i64, text);
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
                on: row.on,
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
    app.set_context_tab(0);
    app.set_context_has_data(false);
    app.set_context_used_tokens(0);
    app.set_context_capacity_tokens(0);
    app.set_context_used_text("—".into());
    app.set_context_capacity_text("—".into());
    app.set_context_free_text("—".into());
    app.set_context_measured(false);
    app.set_context_model_label("".into());
    app.set_context_parts(ModelRc::new(VecModel::from(Vec::<ContextPartRow>::new())));
    app.set_context_items(ModelRc::new(VecModel::from(Vec::<ContextItemRow>::new())));
    app.set_context_tiles(ModelRc::new(VecModel::from(Vec::<ContextTile>::new())));
    app.set_context_cells(ModelRc::new(VecModel::from(Vec::<ContextCell>::new())));
    close_tip(app);
    clear_detail_text(app);
    app.set_context_detail_open(false);
}

impl Record {
    fn from_native(native: NativeContextWindow) -> Self {
        let mut parts = Vec::new();
        for (kind, slot) in PART_SLOTS {
            let Some(part) = native.parts.iter().find(|part| part.kind == kind) else {
                continue;
            };
            parts.push(PartRecord {
                slot,
                tokens: part.tokens,
                items: part
                    .items
                    .iter()
                    .map(|item| ItemRecord {
                        tag: item.tag.clone(),
                        name: item.name.clone(),
                        count: item.count,
                        tokens: item.tokens,
                        order: item.order,
                    })
                    .collect(),
            });
        }
        let tokens: i64 = parts.iter().map(|part| part.tokens).sum();
        Self {
            model: native.model,
            provider: native.provider,
            window: native.window,
            tokens,
            measured: native.measured,
            parts,
        }
    }

    fn has_data(&self) -> bool {
        !self.parts.is_empty() && self.tokens > 0
    }

    /// The model this request went to, "provider › model" like the reference.
    fn model_label(&self) -> String {
        let provider = self.provider.trim();
        let model = self.model.trim();
        match (provider.is_empty(), model.is_empty()) {
            (true, _) => model.to_string(),
            (false, true) => provider.to_string(),
            (false, false) => format!("{provider} › {model}"),
        }
    }
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
    app.set_context_model_label(record.model_label().into());
    app.set_context_parts(ModelRc::new(VecModel::from(part_rows(record))));
    let (width, height) = MAP.with(|size| size.get());
    let laid = tiles(record, width, height);
    app.set_context_tiles(ModelRc::new(VecModel::from(laid.tiles)));
    TIPS.with(|slot| *slot.borrow_mut() = laid.tips);
    app.set_context_cells(ModelRc::new(VecModel::from(cells(record))));
    project_rows(app, record);
    close_tip(app);
    app.set_context_detail_open(false);
}

/// The contents list: every item, or one part's, in the order the request
/// carried them. The list does not truncate: a tool-definition block must not
/// hide the turns below it, and there is no show-more step to expand.
fn project_rows(app: &MainWindow, record: &Record) {
    let tab = app.get_context_tab();
    let rows = list_rows(record, tab);
    let most = rows
        .iter()
        .map(|(_, _, item)| item.tokens)
        .max()
        .unwrap_or(1)
        .max(1);
    let projected: Vec<ContextItemRow> = rows
        .iter()
        .map(|(slot, rank, item)| ContextItemRow {
            slot: *slot,
            tag: item.tag.as_str().into(),
            name: item.name.as_str().into(),
            count: item.count.min(i32::MAX as i64) as i32,
            tokens_text: fmt_k(item.tokens).into(),
            ratio: (item.tokens as f32 / most as f32).clamp(0.0, 1.0),
            item: (*rank).min(i32::MAX as usize) as i32,
        })
        .collect();
    app.set_context_items(ModelRc::new(VecModel::from(projected)));
}

/// The rows shown by the contents list. The non-conversation parts keep the
/// part order; tool results and turns are interleaved by the message position
/// the engine recorded, so a result stays after the turn that called it rather
/// than jumping ahead of every turn. `rank` is the item's position in the
/// size-sorted view used by the map and grid.
fn list_rows<'a>(record: &'a Record, tab: i32) -> Vec<(i32, usize, &'a ItemRecord)> {
    if tab != 0 {
        return record
            .parts
            .iter()
            .filter(|part| part.slot == tab)
            .flat_map(rows_of_part)
            .collect();
    }
    let mut head = Vec::new();
    let mut talk = Vec::new();
    for part in &record.parts {
        let rows = rows_of_part(part);
        if part.slot == 4 || part.slot == 5 {
            talk.extend(rows);
        } else {
            head.extend(rows);
        }
    }
    // Older snapshots have no message position (all zero). Keep turns together
    // but place results after them, so a stale snapshot cannot show a tool
    // result ahead of every turn.
    if talk.iter().any(|(_, _, item)| item.order > 0) {
        talk.sort_by_key(|(_, _, item)| item.order);
    } else {
        talk.sort_by_key(|(slot, _, _)| i32::from(*slot == 4));
    }
    head.extend(talk);
    head
}

fn rows_of_part<'a>(part: &'a PartRecord) -> Vec<(i32, usize, &'a ItemRecord)> {
    let sorted = sorted_items(part);
    listed_items(part)
        .into_iter()
        .map(|item| {
            let rank = sorted
                .iter()
                .position(|candidate| std::ptr::eq(*candidate, item))
                .unwrap_or(0);
            (part.slot, rank, item)
        })
        .collect()
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TileKind {
    Part,
    Item,
    More,
    Free,
}

#[derive(Clone, Debug)]
struct Entry {
    value: f64,
    // Tokens the tile reports (an item's own, a part's, a free remainder).
    tokens: i64,
    kind: TileKind,
    slot: i32,
    tag: String,
    name: String,
    count: i64,
    // Index among the part's sorted items; -1 for a part, -2 for gathered
    // small items, -3 for free space.
    item: i32,
    // Sorted item indices this tile stands for, for its hover card.
    its: Vec<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Clone, Debug, Default)]
struct Laid {
    tiles: Vec<ContextTile>,
    tips: Vec<Tip>,
}

/// Squarified treemap (Bruls, Huizing, van Wijk), in the order given: a strip
/// is kept while adding to it lowers the worst aspect ratio, then laid along
/// the rectangle's short side.
fn squarify(entries: &[Entry], rect: Rect) -> Vec<(Rect, usize)> {
    let mut out = Vec::new();
    let live: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.value > 0.0)
        .map(|(index, _)| index)
        .collect();
    let total: f64 = live.iter().map(|index| entries[*index].value).sum();
    if total <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return out;
    }
    let k = rect.w * rect.h / total;
    let worst = |row: &[usize], side: f64| -> f64 {
        let mut sum = 0.0;
        let mut hi: f64 = 0.0;
        let mut lo = f64::INFINITY;
        for index in row {
            let scaled = entries[*index].value * k;
            sum += scaled;
            hi = hi.max(scaled);
            lo = lo.min(scaled);
        }
        if sum <= 0.0 || lo <= 0.0 {
            return f64::INFINITY;
        }
        ((side * side * hi) / (sum * sum)).max((sum * sum) / (side * side * lo))
    };
    let mut rect = rect;
    let mut row: Vec<usize> = Vec::new();
    for index in live {
        let side = rect.w.min(rect.h);
        if row.is_empty() {
            row.push(index);
            continue;
        }
        let mut candidate = row.clone();
        candidate.push(index);
        if worst(&candidate, side) <= worst(&row, side) {
            row = candidate;
            continue;
        }
        out.extend(lay_row(entries, &row, k, &mut rect));
        row = vec![index];
    }
    if !row.is_empty() {
        out.extend(lay_row(entries, &row, k, &mut rect));
    }
    out
}

fn lay_row(entries: &[Entry], row: &[usize], k: f64, rect: &mut Rect) -> Vec<(Rect, usize)> {
    let sum: f64 = row.iter().map(|index| entries[*index].value * k).sum();
    let mut out = Vec::with_capacity(row.len());
    if rect.w >= rect.h {
        let cw = sum / rect.h;
        let mut cy = rect.y;
        for index in row {
            let ch = entries[*index].value * k / cw;
            out.push((
                Rect {
                    x: rect.x,
                    y: cy,
                    w: cw,
                    h: ch,
                },
                *index,
            ));
            cy += ch;
        }
        rect.x += cw;
        rect.w -= cw;
    } else {
        let rh = sum / rect.w;
        let mut cx = rect.x;
        for index in row {
            let cw = entries[*index].value * k / rh;
            out.push((
                Rect {
                    x: cx,
                    y: rect.y,
                    w: cw,
                    h: rh,
                },
                *index,
            ));
            cx += cw;
        }
        rect.y += rh;
        rect.h -= rh;
    }
    out
}

/// Raise every value to at least the area share of `floor_px`, so a part with
/// a handful of tokens still owns a tile one can point at.
fn floor_areas(entries: &mut [Entry], area: f64, floor_px: f64) {
    let sum: f64 = entries.iter().map(|entry| entry.value).sum();
    if sum <= 0.0 || area <= 0.0 {
        return;
    }
    let lo = sum * floor_px / area;
    for entry in entries {
        entry.value = entry.value.max(lo);
    }
}

fn sorted_items(part: &PartRecord) -> Vec<&ItemRecord> {
    let mut items: Vec<&ItemRecord> = part.items.iter().filter(|item| item.tokens > 0).collect();
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    items
}

/// Items as the request carried them: system prompt and reminders, tools,
/// images, tool results, then the turns in conversation order. The map and
/// grid still sort by size; the contents list does not.
fn listed_items(part: &PartRecord) -> Vec<&ItemRecord> {
    part.items.iter().filter(|item| item.tokens > 0).collect()
}

fn tiles(record: &Record, width: f64, height: f64) -> Laid {
    let mut laid = Laid::default();
    if !record.has_data() {
        return laid;
    }
    let w = width.max(200.0);
    let h = height.max(60.0);
    let scale = record.window.max(record.tokens).max(1) as f64;
    let used = record.tokens as f64;
    let long = w.max(h);
    let across = w >= h;
    // An almost empty window still keeps its parts readable: the used band is
    // eased by share^0.35 (never less than 36px) and the free tile says its
    // own share in words.
    let used_len = if used < scale {
        (long * (used / scale).powf(0.35)).clamp(36.0_f64.min(long), long)
    } else {
        long
    };
    let (band, free_rect) = if across {
        (
            Rect {
                x: 0.0,
                y: 0.0,
                w: used_len,
                h,
            },
            Rect {
                x: used_len,
                y: 0.0,
                w: w - used_len,
                h,
            },
        )
    } else {
        (
            Rect {
                x: 0.0,
                y: 0.0,
                w,
                h: used_len,
            },
            Rect {
                x: 0.0,
                y: used_len,
                w,
                h: h - used_len,
            },
        )
    };

    let mut part_entries: Vec<Entry> = record
        .parts
        .iter()
        .map(|part| {
            let sorted = sorted_items(part);
            Entry {
                value: part.tokens as f64,
                tokens: part.tokens,
                kind: TileKind::Part,
                slot: part.slot,
                tag: String::new(),
                name: String::new(),
                count: 0,
                item: -1,
                its: (0..sorted.len()).collect(),
            }
        })
        .collect();
    floor_areas(&mut part_entries, band.w * band.h, TILE_FLOOR);
    let mut part_rects = squarify(&part_entries, band);
    // A part between large ones would be a strip too thin to point at: raise
    // its value until it is at least TILE_SIDE across both ways.
    for _ in 0..8 {
        let thin: Vec<usize> = part_rects
            .iter()
            .filter(|(rect, _)| rect.w.min(rect.h) < TILE_SIDE)
            .map(|(_, index)| *index)
            .collect();
        if thin.is_empty() {
            break;
        }
        for index in thin {
            let rect = part_rects
                .iter()
                .find(|(_, at)| *at == index)
                .map(|(rect, _)| *rect)
                .unwrap_or(Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                });
            let side = rect.w.min(rect.h).max(1.0);
            part_entries[index].value *= (TILE_SIDE / side) * 1.05;
        }
        part_rects = squarify(&part_entries, band);
    }

    for (rect, index) in part_rects {
        let Some(part) = record.parts.get(index) else {
            continue;
        };
        let entry = &part_entries[index];
        let sorted = sorted_items(part);
        emit_tile(
            &mut laid,
            rect,
            entry,
            &sorted,
            record.tokens,
            record.window.max(record.tokens),
            w,
            h,
        );
        if sorted.is_empty() {
            continue;
        }
        let area = rect.w * rect.h;
        if area < TILE_MIN * 2.0 {
            continue;
        }
        let isum: f64 = sorted.iter().map(|item| item.tokens as f64).sum();
        let mut kept: Vec<Entry> = Vec::new();
        let mut rest: Vec<usize> = Vec::new();
        for (rank, item) in sorted.iter().enumerate() {
            if kept.len() < TILE_MAX_ITEMS && (item.tokens as f64 / isum) * area >= TILE_MIN {
                kept.push(Entry {
                    value: item.tokens as f64,
                    tokens: item.tokens,
                    kind: TileKind::Item,
                    slot: part.slot,
                    tag: item.tag.clone(),
                    name: item.name.clone(),
                    count: item.count,
                    item: rank as i32,
                    its: vec![rank],
                });
            } else {
                rest.push(rank);
            }
        }
        if !rest.is_empty() {
            // One leftover item stays its own tile; several become "more".
            if kept.is_empty() && rest.len() == 1 {
                let rank = rest[0];
                let item = sorted[rank];
                kept.push(Entry {
                    value: item.tokens as f64,
                    tokens: item.tokens,
                    kind: TileKind::Item,
                    slot: part.slot,
                    tag: item.tag.clone(),
                    name: item.name.clone(),
                    count: item.count,
                    item: rank as i32,
                    its: vec![rank],
                });
            } else {
                let tokens: i64 = rest.iter().map(|rank| sorted[*rank].tokens).sum();
                kept.push(Entry {
                    value: tokens as f64,
                    tokens,
                    kind: TileKind::More,
                    slot: part.slot,
                    tag: String::new(),
                    name: String::new(),
                    count: rest.len() as i64,
                    item: -2,
                    its: rest,
                });
            }
        }
        floor_areas(&mut kept, area, TILE_FLOOR);
        let laid_items = squarify(&kept, rect);
        if laid_items.iter().any(|(rect, _)| rect.w.min(rect.h) < 14.0) {
            // Too thin to read as separate tiles: the part stays one tile.
            continue;
        }
        // The part tile is replaced by its items.
        if !laid_items.is_empty() {
            laid.tiles.pop();
            laid.tips.pop();
        }
        for (item_rect, item_index) in laid_items {
            let item_entry = &kept[item_index];
            emit_tile(
                &mut laid,
                item_rect,
                item_entry,
                &sorted,
                record.tokens,
                record.window.max(record.tokens),
                w,
                h,
            );
        }
    }

    if free_rect.w > 0.0 && free_rect.h > 0.0 {
        let free = (record.window.max(record.tokens) as i64 - record.tokens).max(0);
        let entry = Entry {
            value: free as f64,
            tokens: free,
            kind: TileKind::Free,
            slot: 0,
            tag: String::new(),
            name: String::new(),
            count: 0,
            item: -3,
            its: Vec::new(),
        };
        emit_tile(
            &mut laid,
            free_rect,
            &entry,
            &[],
            record.tokens,
            record.window.max(record.tokens),
            w,
            h,
        );
    }
    laid
}

fn emit_tile(
    laid: &mut Laid,
    rect: Rect,
    entry: &Entry,
    sorted: &[&ItemRecord],
    prompt_tokens: i64,
    scale_tokens: i64,
    map_w: f64,
    map_h: f64,
) {
    let bare = rect.w < TILE_BARE_W || rect.h < TILE_BARE_H;
    let low = rect.h < TILE_LOW_H;
    laid.tiles.push(ContextTile {
        x: (rect.x / map_w) as f32,
        y: (rect.y / map_h) as f32,
        w: (rect.w / map_w) as f32,
        h: (rect.h / map_h) as f32,
        slot: entry.slot,
        kind: match entry.kind {
            TileKind::Part => "part",
            TileKind::Item => "item",
            TileKind::More => "more",
            TileKind::Free => "free",
        }
        .into(),
        tag: entry.tag.as_str().into(),
        name: entry.name.as_str().into(),
        count: entry.count.min(i32::MAX as i64) as i32,
        tokens_text: fmt_k(entry.tokens).into(),
        item: entry.item,
        show_name: !bare,
        show_value: !bare && !low,
    });
    laid.tips
        .push(tip_of(entry, sorted, prompt_tokens, scale_tokens));
}

fn tip_of(entry: &Entry, sorted: &[&ItemRecord], prompt_tokens: i64, scale_tokens: i64) -> Tip {
    let mut tip = Tip {
        kind: match entry.kind {
            TileKind::Part | TileKind::More => "part",
            TileKind::Item => "item",
            TileKind::Free => "free",
        }
        .into(),
        slot: entry.slot,
        tag: entry.tag.clone(),
        name: entry.name.clone(),
        count: entry.count,
        value: fmt_k(entry.tokens),
        pct: pct(
            entry.tokens,
            if entry.kind == TileKind::Free {
                scale_tokens
            } else {
                prompt_tokens
            },
        ),
        ..Default::default()
    };
    if entry.kind == TileKind::Part || entry.kind == TileKind::More {
        let take = entry.its.len().min(TIP_ROWS);
        for rank in entry.its.iter().take(take) {
            if let Some(item) = sorted.get(*rank) {
                tip.rows.push(TipRow {
                    tag: item.tag.clone(),
                    name: item.name.clone(),
                    count: item.count,
                    value: fmt_k(item.tokens),
                    on: false,
                });
            }
        }
        tip.more = entry.its.len().saturating_sub(take) as i32;
    }
    tip
}

fn part_tip(record: &Record, part: &PartRecord, item: i32, rows: usize) -> Tip {
    let sorted = sorted_items(part);
    let mut tip = Tip {
        kind: "part".into(),
        slot: part.slot,
        value: fmt_k(part.tokens),
        pct: pct(part.tokens, record.tokens),
        ..Default::default()
    };
    let mut picked: Vec<usize> = (0..sorted.len()).take(rows).collect();
    if item >= 0 {
        let at = item as usize;
        if at < sorted.len() && !picked.contains(&at) {
            if picked.len() == rows && rows > 0 {
                picked[rows - 1] = at;
            } else {
                picked.push(at);
            }
        }
    }
    // A new turn is often far smaller than the older turns; keep the latest
    // turn in the grid tip even when it did not make the size-ranked top rows.
    if part.slot == 5 && rows > 0 {
        let listed = listed_items(part);
        if let Some(latest) = listed.last() {
            if let Some(at) = sorted
                .iter()
                .position(|candidate| std::ptr::eq(*candidate, *latest))
            {
                if !picked.contains(&at) {
                    picked[rows - 1] = at;
                }
            }
        }
    }
    for rank in &picked {
        if let Some(it) = sorted.get(*rank) {
            tip.rows.push(TipRow {
                tag: it.tag.clone(),
                name: it.name.clone(),
                count: it.count,
                value: fmt_k(it.tokens),
                on: item >= 0 && *rank == item as usize,
            });
        }
    }
    tip.more = sorted.len().saturating_sub(picked.len()) as i32;
    tip
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

/// The grid look: the window as 400 cells, one per quarter of a percent, each
/// carrying the part and the item its middle falls in. The chat part additionally
/// keeps one cell per turn so a newly arrived turn is visible instead of being
/// swallowed by the part's minimum cell. Tool results and turns are laid out
/// by message position after the fixed system/tool/files head.
fn cells(record: &Record) -> Vec<ContextCell> {
    let mut out = vec![ContextCell { slot: 0, item: -1 }; CELLS];
    if !record.has_data() {
        return out;
    }
    let scale = record.window.max(record.tokens).max(1) as f64;
    let used = ((record.tokens as f64 / scale) * CELLS as f64)
        .round()
        .clamp(0.0, CELLS as f64) as usize;
    if used == 0 {
        return out;
    }
    let parts: Vec<&PartRecord> = record.parts.iter().filter(|part| part.tokens > 0).collect();
    let sum: f64 = parts
        .iter()
        .map(|part| part.tokens as f64)
        .sum::<f64>()
        .max(1.0);
    let want: Vec<f64> = parts
        .iter()
        .map(|part| (part.tokens as f64 / sum) * used as f64)
        .collect();
    // Conversation turns get one cell each even when the whole chat part is
    // smaller than one window cell; otherwise a fresh turn can disappear into
    // the part's minimum cell and the grid looks unchanged.
    let mut minimum = vec![1usize; parts.len()];
    if let Some(chat) = parts.iter().position(|part| part.slot == 5) {
        let room = used.saturating_sub(parts.len().saturating_sub(1)).max(1);
        minimum[chat] = parts[chat].items.len().max(1).min(room);
    }
    let mut got: Vec<usize> = want
        .iter()
        .zip(minimum.iter())
        .map(|(value, floor)| (*value as usize).max(*floor))
        .collect();
    let mut left = used as i64 - got.iter().sum::<usize>() as i64;
    let mut order: Vec<(f64, usize)> = want
        .iter()
        .enumerate()
        .map(|(index, value)| (value - value.floor(), index))
        .collect();
    order.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut at = 0;
    while left > 0 && !order.is_empty() {
        got[order[at % order.len()].1] += 1;
        at += 1;
        left -= 1;
    }
    let mut from_end = got.len();
    while left < 0 && from_end > 0 {
        from_end -= 1;
        while left < 0 && got[from_end] > minimum[from_end] {
            got[from_end] -= 1;
            left += 1;
        }
    }
    let mut head = Vec::new();
    let mut talk = Vec::new();
    for (part_index, part) in parts.iter().enumerate() {
        let sorted = sorted_items(part);
        let target = if part.slot == 4 || part.slot == 5 {
            &mut talk
        } else {
            &mut head
        };
        for item in cell_items(part, &sorted, got[part_index]) {
            target.push(ContextCell { slot: part.slot, item });
        }
    }
    // Old snapshots have no message position: keep turns before results so a
    // result cannot jump ahead of every turn. New snapshots interleave both by
    // the recorded order.
    talk.sort_by_cached_key(|cell| {
        let order = record
            .parts
            .iter()
            .find(|part| part.slot == cell.slot)
            .and_then(|part| {
                sorted_items(part)
                    .get(cell.item.max(0) as usize)
                    .map(|item| item.order)
            })
            .unwrap_or(0);
        if order > 0 {
            (0, order, cell.slot)
        } else {
            (1, if cell.slot == 4 { 1 } else { 0 }, 0)
        }
    });
    let mut fill = 0;
    for cell in head.into_iter().chain(talk) {
        if fill < out.len() {
            out[fill] = cell;
            fill += 1;
        }
    }
    out
}

/// Split a part's cells between its items. Ordinary parts keep the proportional
/// midpoint split; the chat part gives every turn a cell and hands any extra
/// cells to the larger turns, in chronological cell order.
fn cell_items(part: &PartRecord, sorted: &[&ItemRecord], cells: usize) -> Vec<i32> {
    if cells == 0 {
        return Vec::new();
    }
    if sorted.is_empty() {
        return vec![-1; cells];
    }
    if part.slot == 5 && cells >= part.items.len() && !part.items.is_empty() {
        let listed = listed_items(part);
        let n = listed.len();
        let extra = cells - n;
        let sum: f64 = listed
            .iter()
            .map(|item| item.tokens.max(0) as f64)
            .sum::<f64>();
        let mut bonus = vec![0usize; n];
        if extra > 0 && sum > 0.0 {
            let mut remainders = Vec::with_capacity(n);
            for (index, item) in listed.iter().enumerate() {
                let exact = (item.tokens.max(0) as f64 / sum) * extra as f64;
                bonus[index] = exact.floor() as usize;
                remainders.push((exact - exact.floor(), index));
            }
            let mut assigned: usize = bonus.iter().sum();
            remainders.sort_by(|a, b| {
                b.0.partial_cmp(&a.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(b.1.cmp(&a.1))
            });
            let mut at = 0;
            while assigned < extra && !remainders.is_empty() {
                bonus[remainders[at % remainders.len()].1] += 1;
                assigned += 1;
                at += 1;
            }
        } else if extra > 0 {
            // No token weight to go by: let the newest turn carry the extras.
            bonus[n - 1] = extra;
        }
        let mut out = Vec::with_capacity(cells);
        for (index, item) in listed.iter().enumerate() {
            let rank = sorted
                .iter()
                .position(|candidate| std::ptr::eq(*candidate, *item))
                .unwrap_or(0) as i32;
            for _ in 0..1 + bonus[index] {
                out.push(rank);
            }
        }
        return out;
    }
    let isum: f64 = sorted
        .iter()
        .map(|item| item.tokens as f64)
        .sum::<f64>()
        .max(1.0);
    let mut item_at = 0;
    let mut acc = 0.0;
    let mut out = Vec::with_capacity(cells);
    for cell in 0..cells {
        let middle = ((cell as f64 + 0.5) / cells as f64) * isum;
        while item_at + 1 < sorted.len() && (acc + sorted[item_at].tokens as f64) < middle {
            acc += sorted[item_at].tokens as f64;
            item_at += 1;
        }
        out.push(item_at as i32);
    }
    out
}

/// magpie's token count: 992K, 8.3K, 1.25M — the unit already says the scale.
fn fmt_k(value: i64) -> String {
    let value = value.max(0);
    if value >= 1_000_000 {
        let digits = if value >= 10_000_000 { 1 } else { 2 };
        format!("{}M", trim_zeros(value as f64 / 1_000_000.0, digits))
    } else if value >= 1_000 {
        let digits = if value >= 100_000 { 0 } else { 1 };
        format!("{}K", trim_zeros(value as f64 / 1_000.0, digits))
    } else {
        value.to_string()
    }
}

fn pct(value: i64, total: i64) -> String {
    let total = total.max(1);
    format!(
        "{}%",
        ((value as f64 / total as f64) * 100.0).round() as i64
    )
}

fn trim_zeros(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(value: f64) -> Entry {
        Entry {
            value,
            tokens: value as i64,
            kind: TileKind::Item,
            slot: 1,
            tag: String::new(),
            name: String::new(),
            count: 0,
            item: 0,
            its: Vec::new(),
        }
    }

    #[test]
    fn contents_list_keeps_part_order_and_turns_together() {
        let record = Record {
            window: 262_144,
            tokens: 8_621,
            parts: vec![
                PartRecord {
                    slot: 1,
                    tokens: 3_422,
                    items: vec![ItemRecord {
                        tag: "prompt".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 3_422,
                        order: 0,
                    }],
                },
                PartRecord {
                    slot: 2,
                    tokens: 5_003,
                    items: (0..19)
                        .map(|index| ItemRecord {
                            tag: "tool".into(),
                            name: format!("tool{index}"),
                            count: 1,
                            // The later turns must not be broken up by these.
                            tokens: 263,
                            order: 0,
                        })
                        .collect(),
                },
                PartRecord {
                    slot: 5,
                    tokens: 196,
                    items: vec![
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 1,
                            tokens: 188,
                            order: 0,
                        },
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 2,
                            tokens: 8,
                            order: 0,
                        },
                    ],
                },
            ],
            ..Default::default()
        };
        let rows = list_rows(&record, 0);
        let chat: Vec<_> = rows.iter().filter(|(slot, _, _)| *slot == 5).collect();
        assert_eq!(chat.len(), 2);
        assert_eq!(chat[0].2.count, 1);
        assert_eq!(chat[1].2.count, 2);
        let first_chat = rows.iter().position(|(slot, _, _)| *slot == 5).unwrap();
        assert_eq!(
            rows[first_chat - 1].0,
            2,
            "all tools belong before the first turn"
        );
        assert!(rows[first_chat..].iter().all(|(slot, _, _)| *slot == 5));

        let chat_only = list_rows(&record, 5);
        assert_eq!(chat_only[0].2.count, 1);
        assert_eq!(chat_only[1].2.count, 2);
    }

    #[test]
    fn tool_results_follow_the_message_order_between_turns() {
        let record = Record {
            window: 8_000,
            tokens: 400,
            parts: vec![
                PartRecord {
                    slot: 1,
                    tokens: 150,
                    items: vec![ItemRecord {
                        tag: "prompt".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 150,
                        order: 0,
                    }],
                },
                PartRecord {
                    slot: 4,
                    tokens: 40,
                    items: vec![ItemRecord {
                        tag: "result".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 40,
                        order: 2,
                    }],
                },
                PartRecord {
                    slot: 5,
                    tokens: 200,
                    items: vec![
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 1,
                            tokens: 110,
                            order: 1,
                        },
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 2,
                            tokens: 90,
                            order: 3,
                        },
                    ],
                },
            ],
            ..Default::default()
        };
        let rows = list_rows(&record, 0);
        let conversation: Vec<_> = rows
            .iter()
            .filter(|(slot, _, _)| *slot == 4 || *slot == 5)
            .map(|(slot, _, item)| (*slot, item.tag.as_str(), item.count))
            .collect();
        assert_eq!(
            conversation,
            vec![(5, "turn", 1), (4, "result", 1), (5, "turn", 2)]
        );
    }

    #[test]
    fn grid_gives_every_turn_its_own_cell_at_the_end() {
        let record = Record {
            window: 262_144,
            tokens: 8_968,
            parts: vec![
                PartRecord {
                    slot: 1,
                    tokens: 3_402,
                    items: vec![ItemRecord {
                        tag: "prompt".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 3_402,
                        order: 0,
                    }],
                },
                PartRecord {
                    slot: 2,
                    tokens: 4_974,
                    items: vec![ItemRecord {
                        tag: "tool".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 4_974,
                        order: 0,
                    }],
                },
                PartRecord {
                    slot: 4,
                    tokens: 60,
                    items: vec![ItemRecord {
                        tag: "result".into(),
                        name: "list_files".into(),
                        count: 1,
                        tokens: 60,
                        order: 2,
                    }],
                },
                PartRecord {
                    slot: 5,
                    tokens: 532,
                    items: vec![
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 1,
                            tokens: 187,
                            order: 1,
                        },
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 2,
                            tokens: 92,
                            order: 3,
                        },
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 3,
                            tokens: 94,
                            order: 5,
                        },
                        ItemRecord {
                            tag: "turn".into(),
                            name: String::new(),
                            count: 4,
                            tokens: 159,
                            order: 7,
                        },
                    ],
                },
            ],
            ..Default::default()
        };
        let cells = cells(&record);
        let chat: Vec<_> = cells.iter().filter(|cell| cell.slot == 5).collect();
        assert!(chat.len() >= 4, "every turn keeps a cell");
        for rank in 0..4 {
            assert!(chat.iter().any(|cell| cell.item == rank));
        }
        // The result sits between the turns that surround it, not before all
        // of them, and the newest turn remains at the end of the grid.
        let result_at = cells.iter().position(|cell| cell.slot == 4).unwrap();
        let last_chat = cells.iter().rposition(|cell| cell.slot == 5).unwrap();
        assert!(result_at < last_chat);
        assert_eq!(chat.last().map(|cell| cell.item), Some(1));
    }

    #[test]
    fn squarify_covers_the_rect_without_overlap() {
        let entries = vec![entry(3000.0), entry(1000.0), entry(500.0), entry(200.0)];
        let rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 200.0,
        };
        let laid = squarify(&entries, rect);
        assert_eq!(laid.len(), entries.len());
        for (a, _) in &laid {
            assert!(a.x >= -0.001 && a.y >= -0.001);
            assert!(a.x + a.w <= 800.001 && a.y + a.h <= 200.001);
        }
        for (index, (a, _)) in laid.iter().enumerate() {
            for (b, _) in laid.iter().skip(index + 1) {
                let overlap_x = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                let overlap_y = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                assert!(overlap_x <= 0.001 || overlap_y <= 0.001);
            }
        }
    }

    #[test]
    fn tile_labels_are_bare_when_the_tile_cannot_hold_text() {
        let record = Record {
            window: 100_000,
            tokens: 20_000,
            parts: vec![PartRecord {
                slot: 2,
                tokens: 20_000,
                items: vec![],
            }],
            ..Default::default()
        };
        let tiles = tiles(&record, 880.0, 232.0).tiles;
        assert!(!tiles.is_empty());
        assert!(tiles.iter().all(|tile| tile.w > 0.0 && tile.h > 0.0));
        assert!(tiles.iter().any(|tile| tile.kind == "free"));
    }

    #[test]
    fn cells_round_the_used_share_and_keep_a_part_marker() {
        let record = Record {
            window: 4_000,
            tokens: 40,
            parts: vec![
                PartRecord {
                    slot: 1,
                    tokens: 20,
                    items: vec![],
                },
                PartRecord {
                    slot: 2,
                    tokens: 20,
                    items: vec![],
                },
            ],
            ..Default::default()
        };
        let cells = cells(&record);
        assert_eq!(cells.len(), CELLS);
        assert_eq!(cells.iter().filter(|cell| cell.slot == 1).count(), 2);
        assert_eq!(cells.iter().filter(|cell| cell.slot == 2).count(), 2);
        assert!(cells.iter().filter(|cell| cell.slot == 0).count() >= CELLS - 4);
    }

    #[test]
    fn token_counts_read_like_the_reference() {
        assert_eq!(fmt_k(8300), "8.3K");
        assert_eq!(fmt_k(992_000), "992K");
        assert_eq!(fmt_k(1_000_000), "1M");
        assert_eq!(fmt_k(1_250_000), "1.25M");
        assert_eq!(fmt_k(0), "0");
    }

    #[test]
    fn a_nearly_empty_window_keeps_the_system_part_readable() {
        let record = Record {
            window: 1_000_000,
            tokens: 8_300,
            parts: vec![
                PartRecord {
                    slot: 1,
                    tokens: 3_100,
                    items: vec![ItemRecord {
                        tag: "prompt".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 3_100,
                        order: 0,
                    }],
                },
                PartRecord {
                    slot: 2,
                    tokens: 4_400,
                    items: [
                        507, 466, 417, 322, 281, 260, 240, 220, 200, 180, 170, 160, 150, 140, 130,
                        120, 110, 100, 227,
                    ]
                    .iter()
                    .enumerate()
                    .map(|(i, tokens)| ItemRecord {
                        tag: "tool".into(),
                        name: format!("tool{i}"),
                        count: 1,
                        tokens: *tokens,
                        order: 0,
                    })
                    .collect(),
                },
                PartRecord {
                    slot: 5,
                    tokens: 748,
                    items: vec![ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 748,
                        order: 0,
                    }],
                },
            ],
            ..Default::default()
        };
        let laid = tiles(&record, 880.0, 232.0);
        let system = laid
            .tiles
            .iter()
            .find(|tile| tile.slot == 1)
            .expect("system tile");
        assert!(system.w * 880.0 > 100.0, "system tile width {}", system.w);
        assert!(system.h * 232.0 > 40.0, "system tile height {}", system.h);
        assert!(laid.tiles.iter().any(|tile| tile.kind == "free"));
        for tile in &laid.tiles {
            println!(
                "TILE x={:.4} y={:.4} w={:.4} h={:.4} slot={} kind={} tag={} name={} count={} text={} item={} name?{} value?{}",
                tile.x, tile.y, tile.w, tile.h, tile.slot, tile.kind, tile.tag, tile.name, tile.count,
                tile.tokens_text, tile.item, tile.show_name, tile.show_value
            );
        }
    }
}
