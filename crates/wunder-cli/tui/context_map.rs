//! Treemap layout for the `/context` 脑图.
//!
//! The desktop card lays the five parts out as rectangles; the terminal does the
//! same in character cells. A cell is about twice as tall as it is wide, so the
//! squarified pass runs on a half-cell grid (`height * aspect`) and the result is
//! folded back into rows — otherwise every tile would flatten into a ribbon.
//!
//! Pure geometry: no terminal types, no IO, no colour. The panel decides how a
//! tile is painted; this module only decides where it sits.

use crate::tui::context_window::{fmt_k, ItemRow, PartRow};

/// Children are the rows that make a part up: a tool definition, a result, a
/// turn. Only the biggest few earn a tile of their own; the tail is folded into
/// one aggregate so a long session does not turn the map into confetti.
const CHILD_KEEP: usize = 5;
/// Below this the part keeps its single label instead of a nested grid.
const CHILD_MIN_AREA: u32 = 42;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CellRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl CellRect {
    #[cfg(test)]
    pub(crate) fn area(self) -> u32 {
        self.w as u32 * self.h as u32
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ItemTile {
    pub label: String,
    /// The name on its own (`read_file`), for tiles too narrow for the role.
    pub short: String,
    pub tokens_text: String,
    pub rect: CellRect,
}

#[derive(Clone, Debug)]
pub(crate) struct Tile {
    pub slot: i32,
    pub label: String,
    pub pct_text: String,
    pub tokens_text: String,
    pub rect: CellRect,
    /// True when the part carries a header row and its children below it: the
    /// name moves to the top-left instead of sitting centred in the block.
    pub header: bool,
    pub items: Vec<ItemTile>,
}

/// Squarified treemap over the parts that actually carry tokens.
///
/// `aspect` is the measured cell height/width ratio (2.1 on most terminals). It
/// only shapes the geometry: a tile laid out on the half-cell grid is close to
/// square once the terminal stretches it vertically.
pub(crate) fn layout(
    parts: &[PartRow],
    items: &[ItemRow],
    width: u16,
    height: u16,
    aspect: f32,
    is_zh: bool,
) -> Vec<Tile> {
    if width < 4 || height < 2 {
        return Vec::new();
    }
    let rows = aspect.clamp(1.2, 3.0) as f64;
    let mut entries: Vec<(usize, f64)> = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| part.tokens > 0)
        .map(|(index, part)| (index, part.tokens as f64))
        .collect();
    if entries.is_empty() {
        return Vec::new();
    }
    // Largest first: squarify only looks good when the row is fed in this order.
    entries.sort_by(|left, right| right.1.total_cmp(&left.1));

    let canvas_w = width as f64;
    let canvas_h = height as f64 * rows;
    let canvas_area = canvas_w * canvas_h;
    let sum: f64 = entries.iter().map(|(_, value)| *value).sum();
    if !(sum > 0.0) || !(canvas_area > 0.0) {
        return Vec::new();
    }

    // A part with a handful of tokens still earns a tile one can point at: every
    // part is raised to at least 2% of the used tokens (the desk card's
    // `TILE_FLOOR` is the same idea in pixels).
    let floor_value = sum * 0.02;
    let floored: Vec<(usize, f64)> = entries
        .iter()
        .map(|(index, value)| (*index, value.max(floor_value)))
        .collect();
    // Squarify deals in area, not in tokens: rescale so the values add up to the
    // canvas, and the flooring above does not push the row past its edge.
    let total: f64 = floored.iter().map(|(_, value)| *value).sum();
    let values: Vec<(usize, f64)> = floored
        .into_iter()
        .map(|(index, value)| (index, value / total * canvas_area))
        .collect();

    let mut rects = vec![PxRect::default(); values.len()];
    squarify(
        &values,
        PxRect {
            x: 0.0,
            y: 0.0,
            w: canvas_w,
            h: canvas_h,
        },
        &mut rects,
    );

    values
        .iter()
        .zip(rects.iter())
        .filter_map(|((index, _), rect)| {
            to_cell(*rect, rows, width, height).map(|rect| {
                let part = &parts[*index];
                Tile {
                    slot: part.slot,
                    label: part.label.clone(),
                    pct_text: part.pct_text.clone(),
                    tokens_text: part.tokens_text.clone(),
                    rect,
                    header: false,
                    items: Vec::new(),
                }
            })
        })
        .map(|mut tile| {
            nested_children(&mut tile, items, rows, width, height, is_zh);
            tile
        })
        .collect()
}

/// Lay the part's own rows inside it. A part that is tall and wide enough gets a
/// header and a nested grid; anything smaller keeps the single centred label,
/// because a two-cell child tile carries nothing a reader can use.
fn nested_children(
    tile: &mut Tile,
    items: &[ItemRow],
    rows: f64,
    width: u16,
    height: u16,
    is_zh: bool,
) {
    let rect = tile.rect;
    if rect.w < 10 || rect.h < 4 {
        return;
    }
    let body = CellRect {
        x: rect.x,
        y: rect.y + 1,
        w: rect.w,
        h: rect.h - 1,
    };
    let children = child_tiles(items, tile.slot, body, rows, width, height, is_zh);
    if children.is_empty() {
        return;
    }
    tile.header = true;
    tile.items = children;
}

fn child_tiles(
    items: &[ItemRow],
    slot: i32,
    body: CellRect,
    rows: f64,
    width: u16,
    height: u16,
    is_zh: bool,
) -> Vec<ItemTile> {
    if body.w < 8 || body.h < 2 || (body.w as u32 * body.h as u32) < CHILD_MIN_AREA {
        return Vec::new();
    }
    let mut mine: Vec<&ItemRow> = items
        .iter()
        .filter(|item| item.slot == slot && item.tokens > 0)
        .collect();
    if mine.len() < 2 {
        return Vec::new();
    }
    mine.sort_by(|left, right| right.tokens.cmp(&left.tokens));

    let short = |item: &&ItemRow| {
        if item.name.is_empty() {
            item.label.clone()
        } else {
            item.name.clone()
        }
    };
    let mut entries: Vec<(String, String, String, f64)> = mine
        .iter()
        .take(CHILD_KEEP)
        .map(|item| {
            (
                item.label.clone(),
                short(item),
                item.tokens_text.clone(),
                item.tokens as f64,
            )
        })
        .collect();
    if mine.len() > CHILD_KEEP {
        let rest: i64 = mine[CHILD_KEEP..].iter().map(|item| item.tokens).sum();
        let label = if is_zh {
            format!("其余 {} 项", mine.len() - CHILD_KEEP)
        } else {
            format!("{} more", mine.len() - CHILD_KEEP)
        };
        entries.push((label.clone(), label, fmt_k(rest), rest as f64));
    }

    let sum: f64 = entries.iter().map(|(_, _, _, value)| *value).sum();
    if !(sum > 0.0) {
        return Vec::new();
    }
    // Every child keeps a tile big enough to hold a short name.
    let floor = sum * 0.05;
    let total: f64 = entries
        .iter()
        .map(|(_, _, _, value)| value.max(floor))
        .sum();
    let area = body.w as f64 * body.h as f64 * rows;
    let values: Vec<(usize, f64)> = entries
        .iter()
        .enumerate()
        .map(|(index, (_, _, _, value))| (index, value.max(floor) / total * area))
        .collect();
    let canvas = PxRect {
        x: body.x as f64,
        y: body.y as f64 * rows,
        w: body.w as f64,
        h: body.h as f64 * rows,
    };
    let mut rects = vec![PxRect::default(); values.len()];
    squarify(&values, canvas, &mut rects);

    values
        .iter()
        .zip(rects.iter())
        .filter_map(|((index, _), rect)| {
            to_cell(*rect, rows, width, height).map(|rect| ItemTile {
                label: entries[*index].0.clone(),
                short: entries[*index].1.clone(),
                tokens_text: entries[*index].2.clone(),
                rect,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default)]
struct PxRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Fold a rectangle from the half-cell grid into whole terminal cells.
fn to_cell(rect: PxRect, rows: f64, width: u16, height: u16) -> Option<CellRect> {
    if !rect.w.is_finite() || !rect.h.is_finite() || rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let x = rect.x.floor().clamp(0.0, width as f64) as u16;
    let y = (rect.y / rows).floor().clamp(0.0, height as f64) as u16;
    if x >= width || y >= height {
        return None;
    }
    let right = (rect.x + rect.w).floor().clamp(0.0, width as f64) as u16;
    let bottom = ((rect.y + rect.h) / rows).floor().clamp(0.0, height as f64) as u16;
    Some(CellRect {
        x,
        y,
        w: right.saturating_sub(x).clamp(1, width - x),
        h: bottom.saturating_sub(y).clamp(1, height - y),
    })
}

/// Classic squarified layout: keep appending to the running row while the worst
/// aspect ratio improves, then lay the row out along the shorter side left.
fn squarify(values: &[(usize, f64)], canvas: PxRect, out: &mut [PxRect]) {
    let mut rest = canvas;
    let mut index = 0;
    while index < values.len() {
        if rest.w <= 0.0 || rest.h <= 0.0 {
            break;
        }
        let side = rest.w.min(rest.h);
        let mut take = 1;
        let mut best = worst_ratio(&values[index..index + 1], side);
        while index + take < values.len() {
            let ratio = worst_ratio(&values[index..index + take + 1], side);
            if ratio > best {
                break;
            }
            best = ratio;
            take += 1;
        }
        let row = &values[index..index + take];
        let row_area: f64 = row.iter().map(|(_, value)| *value).sum();
        if !(row_area > 0.0) {
            break;
        }
        if rest.w >= rest.h {
            let strip = (row_area / rest.h).clamp(0.0, rest.w);
            let mut y = rest.y;
            for (position, (target, value)) in row.iter().enumerate() {
                let last = position + 1 == row.len();
                let h = if last {
                    rest.y + rest.h - y
                } else {
                    value / row_area * rest.h
                };
                out[*target] = PxRect {
                    x: rest.x,
                    y,
                    w: strip,
                    h,
                };
                y += h;
            }
            rest.x += strip;
            rest.w -= strip;
        } else {
            let strip = (row_area / rest.w).clamp(0.0, rest.h);
            let mut x = rest.x;
            for (position, (target, value)) in row.iter().enumerate() {
                let last = position + 1 == row.len();
                let w = if last {
                    rest.x + rest.w - x
                } else {
                    value / row_area * rest.w
                };
                out[*target] = PxRect {
                    x,
                    y: rest.y,
                    w,
                    h: strip,
                };
                x += w;
            }
            rest.y += strip;
            rest.h -= strip;
        }
        index += take;
    }
}

/// Worst aspect ratio a row would produce against `side`. Lower is squarer.
fn worst_ratio(row: &[(usize, f64)], side: f64) -> f64 {
    let sum: f64 = row.iter().map(|(_, value)| *value).sum();
    let max = row.iter().map(|(_, value)| *value).fold(f64::MIN, f64::max);
    let min = row.iter().map(|(_, value)| *value).fold(f64::MAX, f64::min);
    if !(sum > 0.0) || !(side > 0.0) || !(min > 0.0) {
        return f64::MAX;
    }
    let sum_squared = sum * sum;
    let side_squared = side * side;
    (side_squared * max / sum_squared).max(sum_squared / (side_squared * min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(slot: i32, tokens: i64) -> PartRow {
        PartRow {
            slot,
            kind: "system".to_string(),
            label: format!("p{slot}"),
            tokens_text: tokens.to_string(),
            pct_text: "0%".to_string(),
            tokens,
        }
    }

    fn item(slot: i32, tag: &str, label: &str, tokens: i64) -> ItemRow {
        ItemRow {
            slot,
            tag: tag.to_string(),
            name: String::new(),
            count: 0,
            order: 0,
            label: label.to_string(),
            tokens_text: fmt_k(tokens),
            tokens,
        }
    }

    fn occupied(tiles: &[Tile]) -> Vec<(u16, u16)> {
        let mut cells = Vec::new();
        for tile in tiles {
            for y in tile.rect.y..tile.rect.y + tile.rect.h {
                for x in tile.rect.x..tile.rect.x + tile.rect.w {
                    cells.push((x, y));
                }
            }
        }
        cells
    }

    #[test]
    fn every_tile_sits_inside_the_map_and_none_overlap() {
        let parts = vec![
            part(1, 3_100),
            part(2, 1_400),
            part(3, 260),
            part(4, 90),
            part(5, 40),
        ];
        let (width, height) = (60, 9);
        let tiles = layout(&parts, &[], width, height, 2.1, false);
        assert_eq!(tiles.len(), 5);
        let mut seen = std::collections::HashSet::new();
        for tile in &tiles {
            assert!(tile.rect.x + tile.rect.w <= width, "{tile:?}");
            assert!(tile.rect.y + tile.rect.h <= height, "{tile:?}");
            assert!(tile.rect.w >= 1 && tile.rect.h >= 1, "{tile:?}");
            for cell in occupied(std::slice::from_ref(tile)) {
                assert!(seen.insert(cell), "cell {cell:?} painted twice");
            }
        }
        // The map is nearly fully used: gaps only come from the floored edges.
        assert!(seen.len() as f32 >= 0.75 * (width as f32 * height as f32));
    }

    #[test]
    fn the_largest_part_gets_the_largest_tile_and_tiny_parts_stay_pointable() {
        let parts = vec![part(1, 9_000), part(2, 80), part(3, 10), part(4, 1)];
        let tiles = layout(&parts, &[], 80, 10, 2.0, false);
        assert_eq!(tiles.len(), 4);
        let system = tiles.iter().find(|tile| tile.slot == 1).expect("system");
        for tile in tiles.iter().filter(|tile| tile.slot != 1) {
            assert!(
                system.rect.area() >= tile.rect.area(),
                "{system:?} {tile:?}"
            );
        }
        // Even the 1-token part keeps a visible, non-zero tile.
        let tiny = tiles.iter().find(|tile| tile.slot == 4).expect("tiny");
        assert!(tiny.rect.area() >= 1);
    }

    #[test]
    fn a_single_part_takes_the_whole_map() {
        let tiles = layout(&[part(1, 500)], &[], 40, 6, 2.0, false);
        assert_eq!(tiles.len(), 1);
        assert_eq!(
            tiles[0].rect,
            CellRect {
                x: 0,
                y: 0,
                w: 40,
                h: 6
            }
        );
    }

    #[test]
    fn empty_or_absent_parts_draw_nothing() {
        assert!(layout(&[], &[], 40, 6, 2.0, false).is_empty());
        assert!(layout(&[part(1, 0), part(2, 0)], &[], 40, 6, 2.0, false).is_empty());
        assert!(layout(&[part(1, 100)], &[], 2, 1, 2.0, false).is_empty());
    }

    #[test]
    fn a_big_part_nests_the_rows_that_make_it_up() {
        let parts = vec![part(1, 5_000)];
        let items = vec![
            item(1, "prompt", "提示词", 3_000),
            item(1, "turn", "第 1 轮", 1_200),
            item(1, "turn", "第 2 轮", 800),
        ];
        let tiles = layout(&parts, &items, 50, 8, 2.0, true);
        assert_eq!(tiles.len(), 1);
        let tile = &tiles[0];
        assert!(tile.header, "a part with rows carries a header: {tile:?}");
        assert_eq!(tile.items.len(), 3);
        for child in &tile.items {
            assert!(child.rect.x >= tile.rect.x, "{child:?}");
            assert!(child.rect.y > tile.rect.y, "the header keeps its row: {child:?}");
            assert!(
                child.rect.x + child.rect.w <= tile.rect.x + tile.rect.w,
                "{child:?}"
            );
            assert!(
                child.rect.y + child.rect.h <= tile.rect.y + tile.rect.h,
                "{child:?}"
            );
        }
    }

    #[test]
    fn a_part_keeps_its_label_when_the_rows_would_be_too_small() {
        let parts = vec![part(1, 5_000)];
        let items = vec![
            item(1, "turn", "第 1 轮", 3_000),
            item(1, "turn", "第 2 轮", 2_000),
        ];
        let tiles = layout(&parts, &items, 20, 3, 2.0, true);
        assert_eq!(tiles.len(), 1);
        assert!(!tiles[0].header);
        assert!(tiles[0].items.is_empty());
    }

    #[test]
    fn a_long_tail_folds_into_one_aggregate_tile() {
        let items: Vec<ItemRow> = (0..8)
            .map(|index| item(1, "turn", &format!("第 {index} 轮"), 500))
            .collect();
        let parts = vec![part(1, 4_000)];
        let tiles = layout(&parts, &items, 60, 10, 2.0, true);
        let children = &tiles[0].items;
        assert_eq!(children.len(), CHILD_KEEP + 1);
        assert!(children.last().expect("aggregate").label.contains("其余"));
    }
}
