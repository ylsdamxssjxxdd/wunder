//! Context-window presentation data and treemap layout shared by both native shells.

use crate::native::NativeContextWindow;

/// Parts in the order the card lists them; the index is the palette slot the
/// .slint side colours by.
const PART_SLOTS: [(&str, i32); 5] = [
    ("system", 1),
    ("tools", 2),
    ("files", 3),
    ("results", 4),
    ("chat", 5),
];

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

#[derive(Clone, Debug, Default)]
pub struct ItemRecord {
    pub tag: String,
    pub name: String,
    pub count: i64,
    pub tokens: i64,
}

#[derive(Clone, Debug, Default)]
pub struct PartRecord {
    pub slot: i32,
    pub tokens: i64,
    pub items: Vec<ItemRecord>,
}

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub model: String,
    pub provider: String,
    pub window: i64,
    pub tokens: i64,
    pub measured: bool,
    pub turns: i64,
    pub parts: Vec<PartRecord>,
}

#[derive(Clone, Debug, Default)]
pub struct Tip {
    pub kind: String,
    pub slot: i32,
    pub tag: String,
    pub name: String,
    pub count: i64,
    pub value: String,
    pub pct: String,
    pub rows: Vec<TipRow>,
    pub more: i32,
}

#[derive(Clone, Debug, Default)]
pub struct TipRow {
    pub tag: String,
    pub name: String,
    pub count: i64,
    pub value: String,
}

impl Record {
    pub fn from_native(native: NativeContextWindow) -> Self {
        let mut native_parts = native.parts;
        let mut parts = Vec::new();
        for (kind, slot) in PART_SLOTS {
            let Some(index) = native_parts.iter().position(|part| part.kind == kind) else {
                continue;
            };
            let part = native_parts.swap_remove(index);
            parts.push(PartRecord {
                slot,
                tokens: part.tokens,
                items: part
                    .items
                    .into_iter()
                    .map(|item| ItemRecord {
                        tag: item.tag,
                        name: item.name,
                        count: item.count,
                        tokens: item.tokens,
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
            turns: native.turns,
            parts,
        }
    }

    pub fn has_data(&self) -> bool {
        !self.parts.is_empty() && self.tokens > 0
    }

    pub fn part(&self, slot: i32) -> Option<&PartRecord> {
        self.parts.iter().find(|part| part.slot == slot)
    }

    /// The model this request went to, "provider › model" like the reference.
    pub fn model_label(&self) -> String {
        let provider = self.provider.trim();
        let model = self.model.trim();
        match (provider.is_empty(), model.is_empty()) {
            (true, _) => model.to_string(),
            (false, true) => provider.to_string(),
            (false, false) => format!("{provider} › {model}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TileKind {
    Part,
    Item,
    More,
    Free,
}

impl TileKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Part => "part",
            Self::Item => "item",
            Self::More => "more",
            Self::Free => "free",
        }
    }
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
pub struct Layout {
    pub tiles: Vec<Tile>,
    pub tips: Vec<Tip>,
}

#[derive(Clone, Debug)]
pub struct Tile {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub slot: i32,
    pub kind: TileKind,
    pub tag: String,
    pub name: String,
    pub count: i64,
    pub tokens: i64,
    pub item: i32,
    pub show_name: bool,
    pub show_value: bool,
}

impl Tile {
    pub fn display_value(&self, record: &Record) -> String {
        let value = fmt_k(self.tokens);
        if self.kind == TileKind::Free {
            return format!("{value} · {}", pct(self.tokens, record.window));
        }
        value
    }
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
pub fn listed_items(part: &PartRecord) -> Vec<&ItemRecord> {
    part.items.iter().filter(|item| item.tokens > 0).collect()
}

pub fn layout(record: &Record, width: f64, height: f64, include_tips: bool) -> Layout {
    let mut laid = Layout::default();
    if !record.has_data()
        || !width.is_finite()
        || !height.is_finite()
        || width <= 0.0
        || height <= 0.0
    {
        return laid;
    }
    let (w, h) = (width, height);
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

    let parts: Vec<_> = record.parts.iter().filter(|part| part.tokens > 0).collect();
    let mut part_entries: Vec<Entry> = parts
        .iter()
        .map(|part| Entry {
            value: part.tokens as f64,
            tokens: part.tokens,
            kind: TileKind::Part,
            slot: part.slot,
            tag: String::new(),
            name: String::new(),
            count: 0,
            item: -1,
            its: Vec::new(),
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
        let Some(part) = parts.get(index) else {
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
            include_tips,
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
                include_tips,
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
            include_tips,
        );
    }
    laid
}

fn emit_tile(
    laid: &mut Layout,
    rect: Rect,
    entry: &Entry,
    sorted: &[&ItemRecord],
    prompt_tokens: i64,
    scale_tokens: i64,
    map_w: f64,
    map_h: f64,
    include_tips: bool,
) {
    // 安全盒保护：squarify 的浮点累加与局部提薄循环都可能让一块稍微探出
    // 地图盒。把瓦片裁进 [0, map_w] × [0, map_h]，换算成比例后保证
    // x+w ≤ 1.0、y+h ≤ 1.0，任何卡高下最后一行都不会被边缘切掉。
    let mut rect = rect;
    rect.x = rect.x.clamp(0.0, map_w);
    rect.y = rect.y.clamp(0.0, map_h);
    rect.w = rect.w.max(0.0).min(map_w - rect.x);
    rect.h = rect.h.max(0.0).min(map_h - rect.y);
    let bare = rect.w < TILE_BARE_W || rect.h < TILE_BARE_H;
    let low = rect.h < TILE_LOW_H;
    laid.tiles.push(Tile {
        x: (rect.x / map_w) as f32,
        y: (rect.y / map_h) as f32,
        w: (rect.w / map_w) as f32,
        h: (rect.h / map_h) as f32,
        slot: entry.slot,
        kind: entry.kind,
        tag: entry.tag.clone(),
        name: entry.name.clone(),
        count: entry.count,
        tokens: entry.tokens,
        item: entry.item,
        show_name: !bare,
        show_value: !bare && !low,
    });
    if include_tips {
        laid.tips
            .push(tip_of(entry, sorted, prompt_tokens, scale_tokens));
    }
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
    let (ranks, total) = match entry.kind {
        TileKind::Part => (
            (0..sorted.len().min(TIP_ROWS)).collect::<Vec<_>>(),
            sorted.len(),
        ),
        TileKind::More => (
            entry.its.iter().copied().take(TIP_ROWS).collect(),
            entry.its.len(),
        ),
        _ => (Vec::new(), 0),
    };
    tip.more = total.saturating_sub(ranks.len()).min(i32::MAX as usize) as i32;
    for rank in ranks {
        if let Some(item) = sorted.get(rank) {
            tip.rows.push(TipRow {
                tag: item.tag.clone(),
                name: item.name.clone(),
                count: item.count,
                value: fmt_k(item.tokens),
            });
        }
    }
    tip
}

pub fn fmt_k(value: i64) -> String {
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

pub fn pct(value: i64, total: i64) -> String {
    let total = total.max(1);
    format!(
        "{}%",
        ((value as f64 / total as f64) * 100.0).round() as i64
    )
}

fn trim_zeros(value: f64, digits: usize) -> String {
    let text = format!("{value:.digits$}");
    if digits == 0 {
        text
    } else {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests;
