//! 上下文窗口卡片的壳层投影（wunder 安卓版）。
//!
//! 引擎在请求发出前记下提示词构成，供应商回报真实 input 后再把各部位缩放到
//! 实测总数。本模块只把这份快照转成手机卡片要画的部位行、条目行与树图方块；
//! 不在壳层估算、重算或回填任何 token。口径与桌面版
//! `crates/wunder-desktop/frontend/src/context_window.rs` 一致。

use crate::{ContextTile, CtxItemRow, CtxPartRow};
pub use wunder_desktop::context_map::{fmt_k as format_k, pct as percent};
use wunder_desktop::context_map::{layout, listed_items, Record};

pub const DEFAULT_MAP_BOX: (f64, f64) = (336.0, 520.0);

/// 摘要行文案。`used_text` / `capacity_text` 在没有数据时返回「—」，不拿
/// tokens 当分母。
pub fn summary(record: &Record) -> Summary {
    let has_data = record.has_data();
    let window = record.window.max(0);
    let used = if has_data { record.tokens } else { 0 };
    let free = if window > 0 {
        (window - used).max(0)
    } else {
        0
    };
    Summary {
        used_text: if has_data {
            format_k(used)
        } else {
            "—".into()
        },
        capacity_text: if window > 0 {
            format_k(window)
        } else {
            "—".into()
        },
        free_text: if window > 0 {
            format_k(free)
        } else {
            "—".into()
        },
        pct_text: if window > 0 {
            percent(used, window)
        } else {
            "—".into()
        },
        model_label: record.model_label(),
        turns_text: if record.turns > 0 {
            record.turns.to_string()
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

pub fn part_rows(record: &Record) -> Vec<CtxPartRow> {
    record
        .parts
        .iter()
        .filter(|part| part.tokens > 0)
        .map(|part| CtxPartRow {
            slot: part.slot,
            tokens: part.tokens.min(i32::MAX as i64) as i32,
            tokens_text: format_k(part.tokens).into(),
            pct_text: percent(part.tokens, record.tokens).into(),
        })
        .collect()
}

/// 某部位详情里列出的条目（按请求顺序）。
pub fn part_detail_rows(record: &Record, slot: i32) -> Vec<CtxItemRow> {
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
            tokens_text: format_k(item.tokens).into(),
            ratio: (item.tokens as f32 / most as f32).clamp(0.0, 1.0),
            item: rank.min(i32::MAX as usize) as i32,
        })
        .collect()
}

/// 按详情入参在缓存里找到条目，返回它的 token 数（用于详情读数）。
pub fn find_item(record: &Record, slot: i32, tag: &str, name: &str, count: i64) -> Option<i64> {
    let part = record.part(slot)?;
    part.items
        .iter()
        .find(|item| item.tag == tag && item.name == name && item.count == count)
        .map(|item| item.tokens)
}

pub fn tiles(record: &Record, width: f64, height: f64) -> Vec<ContextTile> {
    layout(record, width, height, false)
        .tiles
        .into_iter()
        .map(|tile| ContextTile {
            x: tile.x,
            y: tile.y,
            w: tile.w,
            h: tile.h,
            slot: tile.slot,
            kind: tile.kind.as_str().into(),
            count: tile.count.clamp(0, i32::MAX as i64) as i32,
            tokens_text: tile.display_value(record).into(),
            tag: tile.tag.into(),
            name: tile.name.into(),
            item: tile.item,
            show_name: tile.show_name,
            show_value: tile.show_value,
        })
        .collect()
}

#[cfg(not(target_os = "android"))]
mod preview;
#[cfg(not(target_os = "android"))]
pub use preview::open as open_preview;
