//! 上下文窗口卡片的壳层投影（wunder 安卓版）。
//!
//! 引擎在请求发出前记下提示词构成，供应商回报真实 input 后再把各部位缩放到
//! 实测总数。本模块只把这份快照转成手机卡片要画的部位行、条目行、方块格与
//! 简化树图方块；不在壳层估算、重算或回填任何 token。口径与桌面版
//! `crates/wunder-desktop/frontend/src/context_window.rs` 一致。

use crate::{CtxCell, CtxItemRow, CtxPartRow, CtxTile};
use wunder_desktop::native::NativeContextWindow;

/// 部位固定顺序与调色板槽位（1..5），与引擎 `PART_KINDS` 对齐。
const PART_SLOTS: [(&str, i32); 5] = [
    ("system", 1),
    ("tools", 2),
    ("files", 3),
    ("results", 4),
    ("chat", 5),
];

/// 方块格总数：每格占窗口的 0.25%，与桌面版一致。
const CELLS: usize = 400;

/// 树图块不能再承载文字的下限（magpie 规则，与桌面版同值）。
const TILE_BARE_W: f64 = 54.0;
const TILE_BARE_H: f64 = 30.0;
const TILE_LOW_H: f64 = 46.0;
/// 块不允许比这块面积更小，或比这条边更窄。
const TILE_FLOOR: f64 = 700.0;
const TILE_SIDE: f64 = 16.0;
const TILE_MIN: f64 = 1500.0;
const TILE_MAX_ITEMS: usize = 12;

/// 预览/首次布局时树图盒子的默认像素尺寸；实际尺寸由卡片回传后重排。
const DEFAULT_MAP_W: f64 = 366.0;
const DEFAULT_MAP_H: f64 = 180.0;

#[derive(Clone, Debug, Default)]
struct ItemRecord {
    tag: String,
    name: String,
    count: i64,
    tokens: i64,
    order: i64,
}

#[derive(Clone, Debug, Default)]
struct PartRecord {
    slot: i32,
    tokens: i64,
    items: Vec<ItemRecord>,
}

/// 最近一次打开卡片时读到的快照缓存。切线程时整份丢弃；部位筛选只从这份
/// 缓存重投影，不再回读工作区。
#[derive(Clone, Debug, Default)]
pub struct CtxRecord {
    model: String,
    provider: String,
    window: i64,
    tokens: i64,
    measured: bool,
    turns: i64,
    parts: Vec<PartRecord>,
}

impl CtxRecord {
    /// 空快照（线程从未发出请求）：只显示窗口容量与提示，不伪造五部位构成。
    pub fn has_data(&self) -> bool {
        !self.parts.is_empty() && self.tokens > 0
    }

    pub fn measured(&self) -> bool {
        self.measured
    }

    pub fn total_tokens(&self) -> i64 {
        self.tokens
    }

    /// `provider › model`，与桌面版标签写法一致。
    fn model_label(&self) -> String {
        let provider = self.provider.trim();
        let model = self.model.trim();
        match (provider.is_empty(), model.is_empty()) {
            (true, _) => model.to_string(),
            (false, true) => provider.to_string(),
            (false, false) => format!("{provider} › {model}"),
        }
    }

    fn part(&self, slot: i32) -> Option<&PartRecord> {
        self.parts.iter().find(|part| part.slot == slot)
    }
}

pub fn from_native(native: NativeContextWindow) -> CtxRecord {
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
    let tokens = parts.iter().map(|part| part.tokens).sum();
    CtxRecord {
        model: native.model,
        provider: native.provider,
        window: native.window,
        tokens,
        measured: native.measured,
        turns: native.turns,
        parts,
    }
}

/// 摘要行文案。`used_text` / `capacity_text` 在没有数据时返回「—」，不拿
/// tokens 当分母。
pub fn summary(record: &CtxRecord) -> Summary {
    let has_data = record.has_data();
    let window = record.window.max(0);
    let used = if has_data { record.tokens } else { 0 };
    let free = if window > 0 {
        (window - used).max(0)
    } else {
        0
    };
    Summary {
        used_text: if has_data { fmt_k(used) } else { "—".into() },
        capacity_text: if window > 0 {
            fmt_k(window)
        } else {
            "—".into()
        },
        free_text: if window > 0 {
            fmt_k(free)
        } else {
            "—".into()
        },
        pct_text: if window > 0 {
            pct(used, window)
        } else {
            "—".into()
        },
        model_label: record.model_label(),
        turns_text: if record.turns > 0 {
            format!("承载 {} 轮对话", record.turns)
        } else {
            String::new()
        },
    }
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub used_text: String,
    pub capacity_text: String,
    pub free_text: String,
    pub pct_text: String,
    pub model_label: String,
    pub turns_text: String,
}

/// 固定五条图例；快照里缺的部位补 0 token / 0%，保证三端口径一致。空快照
/// 不调用本函数（图例整体隐藏）。
pub fn part_rows(record: &CtxRecord) -> Vec<CtxPartRow> {
    PART_SLOTS
        .iter()
        .map(|(_, slot)| {
            let tokens = record.part(*slot).map(|part| part.tokens).unwrap_or(0);
            CtxPartRow {
                slot: *slot,
                tokens: tokens.clamp(0, i32::MAX as i64) as i32,
                tokens_text: fmt_k(tokens).into(),
                pct_text: pct(tokens, record.tokens).into(),
            }
        })
        .collect()
}

/// 条目列表：非会话部位按请求顺序在前，工具结果与会话轮次按消息流位置
/// 交错；`tab == 0` 为全部，否则只留该部位。列表不截断、没有展开更多。
pub fn item_rows(record: &CtxRecord, tab: i32) -> Vec<CtxItemRow> {
    let rows = list_rows(record, tab);
    let most = rows
        .iter()
        .map(|(_, _, item)| item.tokens)
        .max()
        .unwrap_or(1)
        .max(1);
    rows.iter()
        .map(|(slot, rank, item)| CtxItemRow {
            slot: *slot,
            tag: item.tag.as_str().into(),
            name: item.name.as_str().into(),
            count: item.count.clamp(0, i32::MAX as i64) as i32,
            tokens_text: fmt_k(item.tokens).into(),
            ratio: (item.tokens as f32 / most as f32).clamp(0.0, 1.0),
            item: (*rank).min(i32::MAX as usize) as i32,
        })
        .collect()
}

/// 某部位详情里列出的条目（按请求顺序）。
pub fn part_detail_rows(record: &CtxRecord, slot: i32) -> Vec<CtxItemRow> {
    let Some(part) = record.part(slot) else {
        return Vec::new();
    };
    let listed = listed_items(part);
    let most = listed
        .iter()
        .map(|item| item.tokens)
        .max()
        .unwrap_or(1)
        .max(1);
    listed
        .iter()
        .enumerate()
        .map(|(rank, item)| CtxItemRow {
            slot: part.slot,
            tag: item.tag.as_str().into(),
            name: item.name.as_str().into(),
            count: item.count.clamp(0, i32::MAX as i64) as i32,
            tokens_text: fmt_k(item.tokens).into(),
            ratio: (item.tokens as f32 / most as f32).clamp(0.0, 1.0),
            item: rank.min(i32::MAX as usize) as i32,
        })
        .collect()
}

/// 按详情入参在缓存里找到条目，返回它的 token 数（用于详情读数）。
pub fn find_item(record: &CtxRecord, slot: i32, tag: &str, name: &str, count: i64) -> Option<i64> {
    let part = record.part(slot)?;
    part.items
        .iter()
        .find(|item| item.tag == tag && item.name == name && item.count == count)
        .map(|item| item.tokens)
}

fn listed_items(part: &PartRecord) -> Vec<&ItemRecord> {
    part.items.iter().filter(|item| item.tokens > 0).collect()
}

fn list_rows<'a>(record: &'a CtxRecord, tab: i32) -> Vec<(i32, usize, &'a ItemRecord)> {
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
    // 旧快照没有消息位置（全 0）：轮次排在一起、结果跟在后面，避免工具结果
    // 抢到所有轮次前面。新快照按记录的 order 交错。
    if talk.iter().any(|(_, _, item)| item.order > 0) {
        talk.sort_by_key(|(_, _, item)| item.order);
    } else {
        talk.sort_by_key(|(slot, _, _)| i32::from(*slot == 4));
    }
    head.extend(talk);
    head
}

fn rows_of_part(part: &PartRecord) -> Vec<(i32, usize, &ItemRecord)> {
    listed_items(part)
        .into_iter()
        .enumerate()
        .map(|(rank, item)| (part.slot, rank, item))
        .collect()
}

/// 400 格方块格：已用份额按部位 token 占比分配，会话轮次每轮至少占一格；
/// 工具结果与轮次按消息流位置交错。空快照返回全空槽位。
pub fn cells(record: &CtxRecord) -> Vec<CtxCell> {
    let mut out = vec![CtxCell { slot: 0, item: -1 }; CELLS];
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
    // 会话每轮至少一格，避免新轮次被部位最小格吞掉。
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
        let target = if part.slot == 4 || part.slot == 5 {
            &mut talk
        } else {
            &mut head
        };
        for item in cell_items(part, got[part_index]) {
            target.push(CtxCell {
                slot: part.slot,
                item,
            });
        }
    }
    talk.sort_by_cached_key(|cell| {
        let order = record
            .parts
            .iter()
            .find(|part| part.slot == cell.slot)
            .and_then(|part| {
                listed_items(part)
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

fn cell_items(part: &PartRecord, cells: usize) -> Vec<i32> {
    if cells == 0 {
        return Vec::new();
    }
    let listed = listed_items(part);
    if listed.is_empty() {
        return vec![-1; cells];
    }
    if part.slot == 5 && cells >= part.items.len() && !part.items.is_empty() {
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
            bonus[n - 1] = extra;
        }
        let mut out = Vec::with_capacity(cells);
        for (index, _) in listed.iter().enumerate() {
            for _ in 0..1 + bonus[index] {
                out.push(index as i32);
            }
        }
        return out;
    }
    let isum: f64 = listed
        .iter()
        .map(|item| item.tokens as f64)
        .sum::<f64>()
        .max(1.0);
    let mut item_at = 0;
    let mut acc = 0.0;
    let mut out = Vec::with_capacity(cells);
    for cell in 0..cells {
        let middle = ((cell as f64 + 0.5) / cells as f64) * isum;
        while item_at + 1 < listed.len() && (acc + listed[item_at].tokens as f64) < middle {
            acc += listed[item_at].tokens as f64;
            item_at += 1;
        }
        out.push(item_at as i32);
    }
    out
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
    tokens: i64,
    kind: TileKind,
    slot: i32,
    tag: String,
    name: String,
    count: i64,
    // 部位排序后的条目序号；部位 -1、聚合小条目 -2、剩余空间 -3。
    item: i32,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn sorted_items(part: &PartRecord) -> Vec<&ItemRecord> {
    let mut items: Vec<&ItemRecord> = part.items.iter().filter(|item| item.tokens > 0).collect();
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    items
}

/// Squarified treemap（Bruls/Huizing/van Wijk）：保持一条带直到加入下一项会
/// 让最差长宽比变差，再沿矩形短边铺出。
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

/// 把每个值抬到至少占 `floor_px` 面积的份额，token 很少的部位仍有一块点得到。
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

/// 两级树图：部位一块、其内条目再切一块，过小条目并成「另外 N 项」，余下为
/// 剩余空间块。坐标归一化到地图盒子；算法与阈值同桌面版。
pub fn tiles(record: &CtxRecord, map_w: f64, map_h: f64) -> Vec<CtxTile> {
    let mut laid = Vec::new();
    if !record.has_data() {
        return laid;
    }
    let w = map_w.max(200.0);
    let h = map_h.max(60.0);
    let scale = record.window.max(record.tokens).max(1) as f64;
    let used = record.tokens as f64;
    let long = w.max(h);
    let across = w >= h;
    // 接近空窗口时把已用带抬高一点（share^0.35，不小于 36px），小部位仍可读。
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
        .map(|part| Entry {
            value: part.tokens as f64,
            tokens: part.tokens,
            kind: TileKind::Part,
            slot: part.slot,
            tag: String::new(),
            name: String::new(),
            count: 0,
            item: -1,
        })
        .collect();
    floor_areas(&mut part_entries, band.w * band.h, TILE_FLOOR);
    let mut part_rects = squarify(&part_entries, band);
    // 夹在大块之间的部位会是一条点不到的细条：把它的份额抬到两边都至少 TILE_SIDE。
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
        emit_tile(&mut laid, rect, entry, record.tokens, record.window.max(record.tokens), w, h);
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
                });
            } else {
                rest.push(rank);
            }
        }
        if !rest.is_empty() {
            // 只剩一条时它单独成块；多条并成「另外 N 项」。
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
                });
            }
        }
        floor_areas(&mut kept, area, TILE_FLOOR);
        let laid_items = squarify(&kept, rect);
        if laid_items.iter().any(|(rect, _)| rect.w.min(rect.h) < 14.0) {
            // 细到读不出独立块：部位保留整块。
            continue;
        }
        // 部位块被它的条目替换。
        if !laid_items.is_empty() {
            laid.pop();
        }
        for (item_rect, item_index) in laid_items {
            let item_entry = &kept[item_index];
            emit_tile(
                &mut laid,
                item_rect,
                item_entry,
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
        };
        emit_tile(&mut laid, free_rect, &entry, record.tokens, record.window.max(record.tokens), w, h);
    }
    laid
}

fn emit_tile(
    laid: &mut Vec<CtxTile>,
    rect: Rect,
    entry: &Entry,
    _prompt_tokens: i64,
    _scale_tokens: i64,
    map_w: f64,
    map_h: f64,
) {
    let bare = rect.w < TILE_BARE_W || rect.h < TILE_BARE_H;
    let low = rect.h < TILE_LOW_H;
    // 块内左右各 9px 内边距；标签/副行估宽后再判定，装不下就整行不写，
    // 绝不把名字截成省略号残片。
    // 块内左右各 9px 内边距，再留 6px 安全余量；标签按 11px、副行按 10px 字号估宽。
    let avail = rect.w - 18.0 - 6.0;
    let name_text = tile_label(entry);
    let value_text = tile_value(entry);
    let name_fits = avail >= 12.0 && est_width(&name_text, 11.0, 6.0) <= avail;
    let value_fits = avail >= 12.0 && est_width(&value_text, 10.0, 5.5) <= avail;
    laid.push(CtxTile {
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
        count: entry.count.clamp(0, i32::MAX as i64) as i32,
        tokens_text: fmt_k(entry.tokens).into(),
        item: entry.item,
        show_name: !bare && name_fits,
        show_value: !bare && !low && value_fits,
    });
}

fn part_label(slot: i32) -> &'static str {
    match slot {
        1 => "系统",
        2 => "工具",
        3 => "图片",
        4 => "结果",
        5 => "会话",
        _ => "剩余",
    }
}

fn item_label(entry: &Entry) -> String {
    match entry.tag.as_str() {
        "tool" if !entry.name.is_empty() => format!("{} · 工具定义", entry.name),
        "result" if !entry.name.is_empty() => format!("{} · 结果", entry.name),
        "turn" => format!("第 {} 轮", entry.count),
        "image" => format!("图片 ×{}", entry.count),
        _ if !entry.name.is_empty() => entry.name.clone(),
        "prompt" => "提示词".into(),
        "reminder" => "提醒".into(),
        _ => "内容".into(),
    }
}

fn tile_label(entry: &Entry) -> String {
    match entry.kind {
        TileKind::Part => part_label(entry.slot).to_string(),
        TileKind::Item => item_label(entry),
        TileKind::More => format!("另外 {} 项", entry.count),
        TileKind::Free => "剩余".into(),
    }
}

fn tile_value(entry: &Entry) -> String {
    let tokens = fmt_k(entry.tokens);
    match entry.kind {
        TileKind::Item | TileKind::More => format!("{tokens} · {}", part_label(entry.slot)),
        TileKind::Free | TileKind::Part => tokens,
    }
}

/// 按字符估宽：非 ASCII 按 CJK 全角，ASCII 按半角。仅用于块内文本是否放得下。
fn est_width(text: &str, cjk: f64, ascii: f64) -> f64 {
    text.chars()
        .map(|c| if c.is_ascii() { ascii } else { cjk })
        .sum()
}

/// 树图盒子默认像素尺寸；卡片回传真实尺寸后按它重排。
pub fn default_map_box() -> (f64, f64) {
    (DEFAULT_MAP_W, DEFAULT_MAP_H)
}

/// magpie 计数口径：992K / 8.3K / 1.25M，单位自带量级。
/// 引擎侧详情读数复用同一套计数口径。
pub fn format_k(value: i64) -> String {
    fmt_k(value)
}

pub fn percent(value: i64, total: i64) -> String {
    pct(value, total)
}

/// 某部位的 token 总数；部位不存在返回 0。
pub fn part_tokens(record: &CtxRecord, slot: i32) -> i64 {
    record.part(slot).map(|part| part.tokens).unwrap_or(0)
}

/// 按列出顺序取某部位的一条目（详情定位用）。
pub fn item_at(record: &CtxRecord, slot: i32, rank: i32) -> Option<(String, String, i64)> {
    let part = record.part(slot)?;
    listed_items(part)
        .get(rank.max(0) as usize)
        .map(|item| (item.tag.clone(), item.name.clone(), item.count))
}

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
