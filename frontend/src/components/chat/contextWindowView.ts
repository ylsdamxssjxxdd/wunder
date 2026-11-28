// Pure projection of the engine's recorded context window onto the cloud
// card. No HTTP, no store, no Vue: the snapshot shape, palette, labels,
// legend, contents order, grid cells, two-level treemap tiles, hover tips and
// detail rows are all functions here so they can be unit-tested against the
// desktop reference projection (crates/wunder-desktop/frontend/src/context_window.rs).
//
// Token numbers come straight from the engine record; this module never
// estimates, re-derives or backfills a count.

export type ContextWindowItemRow = {
  slot: number;
  tag: string;
  name: string;
  count: number;
  tokens: number;
  order: number;
};

export type ContextWindowPartRow = {
  slot: number;
  kind: string;
  tokens: number;
  items: ContextWindowItemRow[];
};

export type ContextWindowRecord = {
  model: string;
  provider: string;
  window: number;
  tokens: number;
  measured: boolean;
  turns: number;
  parts: ContextWindowPartRow[];
};

export const PART_KINDS = ['system', 'tools', 'files', 'results', 'chat'] as const;

export const PART_SLOTS: Array<{ kind: string; slot: number }> = PART_KINDS.map(
  (kind, index) => ({ kind, slot: index + 1 })
);

import { paletteFor } from './contextWindowPalette';

export { paletteFor };
export type { Palette } from './contextWindowPalette';

const asNumber = (value: unknown): number => {
  if (typeof value === 'number' && Number.isFinite(value) && value >= 0) return Math.round(value);
  if (typeof value === 'string') {
    const parsed = Number(value.trim());
    if (Number.isFinite(parsed) && parsed >= 0) return Math.round(parsed);
  }
  return 0;
};

const asString = (value: unknown): string => String(value ?? '').trim();

const normalizeItems = (
  source: unknown,
  slot: number
): ContextWindowItemRow[] => {
  if (!Array.isArray(source)) return [];
  return source
    .map((entry) => {
      const record = entry && typeof entry === 'object' ? (entry as Record<string, unknown>) : {};
      return {
        slot,
        tag: asString(record.tag),
        name: asString(record.name),
        count: asNumber(record.count),
        tokens: asNumber(record.tokens),
        order: asNumber(record.order)
      };
    })
    .filter((item) => item.tokens > 0);
};

/**
 * Normalize the persisted snapshot from the endpoint. The engine only records
 * non-empty parts, so the five stable legend parts are filled here with zero
 * tokens and an empty item list; readings still come from the engine record.
 */
export function normalizeContextWindow(raw: unknown): ContextWindowRecord {
  const source = raw && typeof raw === 'object' ? (raw as Record<string, unknown>) : {};
  const sourceParts = Array.isArray(source.parts) ? source.parts : [];
  const byKind = new Map<string, Record<string, unknown>>();
  sourceParts.forEach((entry) => {
    if (entry && typeof entry === 'object') {
      const part = entry as Record<string, unknown>;
      byKind.set(asString(part.kind), part);
    }
  });

  const parts: ContextWindowPartRow[] = PART_KINDS.map((kind, index) => {
    const slot = index + 1;
    const partSource = byKind.get(kind);
    const items = normalizeItems(partSource?.items, slot);
    const tokens = partSource ? asNumber(partSource.tokens) : items.reduce((sum, item) => sum + item.tokens, 0);
    return { slot, kind, tokens, items };
  });

  return {
    model: asString(source.model),
    provider: asString(source.provider),
    window: asNumber(source.window),
    tokens: parts.reduce((sum, part) => sum + part.tokens, 0),
    measured: source.measured === true,
    turns: asNumber(source.turns),
    parts
  };
}

/** A thread that never sent a request has no composition to draw. */
export const hasContextData = (record: ContextWindowRecord): boolean =>
  record.parts.some((part) => part.tokens > 0) && record.tokens > 0;

/** Strip trailing zeros from a decimal fraction; integer parts keep their zeros. */
const trimZeros = (text: string): string => text.replace(/0+$/, '').replace(/\.$/, '');

export function fmtK(value: number): string {
  const amount = Math.max(0, Math.round(value));
  if (amount >= 1_000_000) {
    const digits = amount >= 10_000_000 ? 1 : 2;
    const text = (amount / 1_000_000).toFixed(digits);
    return `${digits > 0 ? trimZeros(text) : text}M`;
  }
  if (amount >= 1_000) {
    const digits = amount >= 100_000 ? 0 : 1;
    const text = (amount / 1_000).toFixed(digits);
    return `${digits > 0 ? trimZeros(text) : text}K`;
  }
  return String(amount);
}

export const pctValue = (value: number, total: number): number => {
  const denominator = Math.max(1, total);
  if (value <= 0) return 0;
  return Math.min(100, Math.round((value / denominator) * 100));
};

export const pctText = (value: number, total: number): string => `${pctValue(value, total)}%`;

type TranslateFn = (key: string, params?: Record<string, unknown>) => string;

export function partLabel(slot: number, t: TranslateFn): string {
  const kind = PART_SLOTS.find((entry) => entry.slot === slot)?.kind;
  const keyMap: Record<string, string> = {
    system: 'chat.contextWindow.partSystem',
    tools: 'chat.contextWindow.partTools',
    files: 'chat.contextWindow.partFiles',
    results: 'chat.contextWindow.partResults',
    chat: 'chat.contextWindow.partChat'
  };
  return kind ? t(keyMap[kind]) : t('chat.contextWindow.free');
}

/**
 * Item label per the desktop `ContextText.item(tag, name, count)` rule.
 * `count` is overloaded by tag: turn = turn ordinal, result = aggregated
 * count, image = image count, otherwise 1.
 */
export function contextItemLabel(
  item: Pick<ContextWindowItemRow, 'tag' | 'name' | 'count'>,
  t: TranslateFn
): string {
  const { tag, name, count } = item;
  if (tag === 'tool' && name) return `${name} · ${t('chat.contextWindow.itemTool')}`;
  if (tag === 'result' && name) return `${name} · ${t('chat.contextWindow.itemResult')}`;
  if (tag === 'turn') return t('chat.contextWindow.turn', { count });
  if (name) return name;
  if (tag === 'prompt') return t('chat.contextWindow.itemPrompt');
  if (tag === 'reminder') return t('chat.contextWindow.itemReminder');
  if (tag === 'image') return t('chat.contextWindow.image', { count });
  return t('chat.contextWindow.itemOther');
}

export type ContextListRow = {
  slot: number;
  kind: string;
  tag: string;
  name: string;
  count: number;
  tokens: number;
  order: number;
  rank: number;
  ratio: number;
  label: string;
  tokensText: string;
};

const sizeRanks = (items: ContextWindowItemRow[]): Map<ContextWindowItemRow, number> => {
  const sorted = [...items].sort((a, b) => b.tokens - a.tokens);
  const ranks = new Map<ContextWindowItemRow, number>();
  sorted.forEach((item, index) => ranks.set(item, index));
  return ranks;
};

const rowsOfPart = (part: ContextWindowPartRow, t: TranslateFn): ContextListRow[] => {
  const ranks = sizeRanks(part.items);
  return part.items.map((item) => ({
    slot: part.slot,
    kind: part.kind,
    tag: item.tag,
    name: item.name,
    count: item.count,
    tokens: item.tokens,
    order: item.order,
    rank: ranks.get(item) ?? 0,
    // Ratio is filled once on the whole filtered list (see listRows); a part
    // alone must not normalize its own max to full width.
    ratio: 0,
    label: contextItemLabel(item, t),
    tokensText: fmtK(item.tokens)
  }));
};

/**
 * The rows shown by the contents list. Non-conversation parts keep the head
 * order; tool results and turns interleave by the message position the engine
 * recorded, so a result stays after the turn that called it. Older snapshots
 * have order 0 everywhere: results then stay after the turns instead of
 * jumping ahead. The mini bar's share is relative to the largest row in this
 * same filtered list — not to the window.
 */
export function listRows(
  record: ContextWindowRecord,
  filterSlot: number,
  t: TranslateFn
): ContextListRow[] {
  const rows: ContextListRow[] = [];
  if (filterSlot !== 0) {
    const part = record.parts.find((entry) => entry.slot === filterSlot);
    rows.push(...(part ? rowsOfPart(part, t) : []));
  } else {
    const head: ContextListRow[] = [];
    const talk: ContextListRow[] = [];
    record.parts.forEach((part) => {
      const rows = rowsOfPart(part, t);
      if (part.slot === 4 || part.slot === 5) talk.push(...rows);
      else head.push(...rows);
    });
    if (talk.some((row) => row.order > 0)) {
      talk.sort((a, b) => a.order - b.order);
    } else {
      talk.sort((a, b) => Number(a.slot === 4) - Number(b.slot === 4));
    }
    rows.push(...head, ...talk);
  }
  const most = Math.max(1, ...rows.map((row) => row.tokens));
  rows.forEach((row) => {
    row.ratio = Math.min(1, row.tokens / most);
  });
  return rows;
}

export type ContextLegendRow = {
  slot: number;
  kind: string;
  label: string;
  tokens: number;
  tokensText: string;
};

export function legendRows(record: ContextWindowRecord, t: TranslateFn): ContextLegendRow[] {
  return record.parts.map((part) => ({
    slot: part.slot,
    kind: part.kind,
    label: partLabel(part.slot, t),
    tokens: part.tokens,
    tokensText: fmtK(part.tokens)
  }));
}

export const modelLabel = (record: ContextWindowRecord): string => {
  const provider = record.provider.trim();
  const model = record.model.trim();
  if (!provider) return model;
  if (!model) return provider;
  return `${provider} › ${model}`;
};

// ---- Size-sorted helpers shared by the map, grid and detail ---------------

/** Items with tokens, sorted largest-first; the map and grid light this rank. */
export const sortedItems = (part: ContextWindowPartRow): ContextWindowItemRow[] =>
  part.items.filter((item) => item.tokens > 0).sort((a, b) => b.tokens - a.tokens);

/** Items as the request carried them; the contents list and detail keep this order. */
export const listedItems = (part: ContextWindowPartRow): ContextWindowItemRow[] =>
  part.items.filter((item) => item.tokens > 0);

export const findListedItem = (
  part: ContextWindowPartRow,
  tag: string,
  name: string,
  count: number
): ContextWindowItemRow | undefined =>
  listedItems(part).find((item) => item.tag === tag && item.name === name && item.count === count);

// ---- 400-cell grid: one cell per quarter percent of the window ------------

export const GRID_CELLS = 400;

export type ContextCell = {
  slot: number;
  /** Rank among the part's size-sorted items; -1 means the part as a whole. */
  item: number;
};

/** Wide card: 50 columns × 8 rows; narrow card: 25 × 16. */
export const gridLayout = (cardWidth: number): { columns: number; rows: number } => {
  const columns = cardWidth < 520 ? 25 : 50;
  const rows = columns === 50 ? 8 : 16;
  return { columns, rows };
};

const cellItems = (
  part: ContextWindowPartRow,
  sorted: ContextWindowItemRow[],
  cells: number
): number[] => {
  if (cells === 0) return [];
  if (sorted.length === 0) return Array<number>(cells).fill(-1);
  if (part.slot === 5 && cells >= part.items.length && part.items.length > 0) {
    const listed = listedItems(part);
    const count = listed.length;
    const extra = cells - count;
    const sum = listed.reduce((acc, item) => acc + Math.max(0, item.tokens), 0);
    const bonus = Array<number>(count).fill(0);
    if (extra > 0 && sum > 0) {
      const remainders: Array<[number, number]> = [];
      listed.forEach((item, index) => {
        const exact = (Math.max(0, item.tokens) / sum) * extra;
        bonus[index] = Math.floor(exact);
        remainders.push([exact - Math.floor(exact), index]);
      });
      let assigned = bonus.reduce((acc, value) => acc + value, 0);
      remainders.sort((a, b) => b[0] - a[0] || b[1] - a[1]);
      let at = 0;
      while (assigned < extra && remainders.length > 0) {
        bonus[remainders[at % remainders.length][1]] += 1;
        assigned += 1;
        at += 1;
      }
    } else if (extra > 0) {
      bonus[count - 1] = extra;
    }
    const ranks = sizeRanks(listed);
    const out: number[] = [];
    listed.forEach((item, index) => {
      const rank = ranks.get(item) ?? 0;
      for (let repeat = 0; repeat < 1 + bonus[index]; repeat += 1) out.push(rank);
    });
    return out;
  }
  const itemSum = Math.max(1, sorted.reduce((acc, item) => acc + item.tokens, 0));
  let itemAt = 0;
  let acc = 0;
  const out: number[] = [];
  for (let cell = 0; cell < cells; cell += 1) {
    const middle = ((cell + 0.5) / cells) * itemSum;
    while (itemAt + 1 < sorted.length && acc + sorted[itemAt].tokens < middle) {
      acc += sorted[itemAt].tokens;
      itemAt += 1;
    }
    out.push(itemAt);
  }
  return out;
};

/** The window as 400 cells; free cells carry slot 0 / item -1. */
export function gridCells(record: ContextWindowRecord): ContextCell[] {
  const out: ContextCell[] = Array<ContextCell>(GRID_CELLS)
    .fill(null)
    .map(() => ({ slot: 0, item: -1 }));
  if (!hasContextData(record)) return out;
  const scale = Math.max(1, Math.max(record.window, record.tokens));
  const used = Math.round((record.tokens / scale) * GRID_CELLS);
  const usedClamped = Math.min(GRID_CELLS, Math.max(0, used));
  if (usedClamped === 0) return out;

  const parts = record.parts.filter((part) => part.tokens > 0);
  const sum = Math.max(1, parts.reduce((acc, part) => acc + part.tokens, 0));
  const want = parts.map((part) => (part.tokens / sum) * usedClamped);
  const minimum = parts.map(() => 1);
  const chatIndex = parts.findIndex((part) => part.slot === 5);
  if (chatIndex >= 0) {
    const room = Math.max(1, usedClamped - (parts.length - 1));
    minimum[chatIndex] = Math.min(parts[chatIndex].items.length, room);
  }
  const got = want.map((value, index) => Math.max(Math.floor(value), minimum[index]));
  let left = usedClamped - got.reduce((acc, value) => acc + value, 0);
  const order = want
    .map((value, index) => [value - Math.floor(value), index] as [number, number])
    .sort((a, b) => b[0] - a[0]);
  let at = 0;
  while (left > 0 && order.length > 0) {
    got[order[at % order.length][1]] += 1;
    at += 1;
    left -= 1;
  }
  let fromEnd = got.length;
  while (left < 0 && fromEnd > 0) {
    fromEnd -= 1;
    while (left < 0 && got[fromEnd] > minimum[fromEnd]) {
      got[fromEnd] -= 1;
      left += 1;
    }
  }

  const head: ContextCell[] = [];
  const talk: ContextCell[] = [];
  parts.forEach((part, partIndex) => {
    const sorted = sortedItems(part);
    for (const itemRank of cellItems(part, sorted, got[partIndex])) {
      const cell: ContextCell = { slot: part.slot, item: itemRank };
      if (part.slot === 4 || part.slot === 5) talk.push(cell);
      else head.push(cell);
    }
  });
  talk.sort((a, b) => {
    const orderOf = (cell: ContextCell): number => {
      const part = parts.find((entry) => entry.slot === cell.slot);
      const sorted = part ? sortedItems(part) : [];
      const item = cell.item >= 0 ? sorted[cell.item] : undefined;
      return item?.order ?? 0;
    };
    const ao = orderOf(a);
    const bo = orderOf(b);
    if (ao > 0 || bo > 0) return ao - bo;
    return Number(a.slot === 4) - Number(b.slot === 4);
  });

  let fill = 0;
  for (const cell of [...head, ...talk]) {
    if (fill < out.length) {
      out[fill] = cell;
      fill += 1;
    }
  }
  return out;
}

// ---- Two-level treemap: part blocks, item blocks, free remainder ---------

const TILE_BARE_W = 54;
const TILE_BARE_H = 30;
const TILE_LOW_H = 46;
const TILE_FLOOR = 700;
const TILE_SIDE = 16;
const TILE_MIN = 1500;
const TILE_MAX_ITEMS = 12;
const TIP_ROWS = 5;
const GRID_TIP_ROWS = 4;

type Rect = { x: number; y: number; w: number; h: number };

type TileKind = 'part' | 'item' | 'more' | 'free';

type Entry = {
  value: number;
  tokens: number;
  kind: TileKind;
  slot: number;
  tag: string;
  name: string;
  count: number;
  item: number;
  its: number[];
};

export type ContextTile = {
  x: number;
  y: number;
  w: number;
  h: number;
  slot: number;
  kind: TileKind;
  tag: string;
  name: string;
  count: number;
  tokens: number;
  tokensText: string;
  /** Rank among the part's sorted items; -1 part, -2 gathered small, -3 free. */
  item: number;
  showName: boolean;
  showValue: boolean;
  its: number[];
};

export type ContextTipRow = {
  tag: string;
  name: string;
  count: number;
  value: string;
  on: boolean;
};

export type ContextTip = {
  kind: TileKind;
  slot: number;
  tag: string;
  name: string;
  count: number;
  value: string;
  pct: string;
  rows: ContextTipRow[];
  more: number;
};

const worstRatio = (
  entries: Entry[],
  row: number[],
  side: number,
  scale: number
): number => {
  let sum = 0;
  let hi = 0;
  let lo = Number.POSITIVE_INFINITY;
  for (const index of row) {
    const scaled = entries[index].value * scale;
    sum += scaled;
    hi = Math.max(hi, scaled);
    lo = Math.min(lo, scaled);
  }
  if (sum <= 0 || lo <= 0) return Number.POSITIVE_INFINITY;
  return Math.max(
    (side * side * hi) / (sum * sum),
    (sum * sum) / (side * side * lo)
  );
};

const layRow = (
  entries: Entry[],
  row: number[],
  scale: number,
  rect: Rect
): Array<[Rect, number]> => {
  const sum = row.reduce((acc, index) => acc + entries[index].value * scale, 0);
  const out: Array<[Rect, number]> = [];
  if (rect.w >= rect.h) {
    const columnWidth = sum / rect.h;
    let cy = rect.y;
    for (const index of row) {
      const height = (entries[index].value * scale) / columnWidth;
      out.push([{ x: rect.x, y: cy, w: columnWidth, h: height }, index]);
      cy += height;
    }
    rect.x += columnWidth;
    rect.w -= columnWidth;
  } else {
    const rowHeight = sum / rect.w;
    let cx = rect.x;
    for (const index of row) {
      const width = (entries[index].value * scale) / rowHeight;
      out.push([{ x: cx, y: rect.y, w: width, h: rowHeight }, index]);
      cx += width;
    }
    rect.y += rowHeight;
    rect.h -= rowHeight;
  }
  return out;
};

const squarify = (entries: Entry[], rect: Rect): Array<[Rect, number]> => {
  const out: Array<[Rect, number]> = [];
  const live = entries
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => entry.value > 0)
    .map(({ index }) => index);
  const total = live.reduce((acc, index) => acc + entries[index].value, 0);
  if (total <= 0 || rect.w <= 0 || rect.h <= 0) return out;
  const scale = (rect.w * rect.h) / total;
  let box = { ...rect };
  let row: number[] = [];
  for (const index of live) {
    const side = Math.min(box.w, box.h);
    if (row.length === 0) {
      row.push(index);
      continue;
    }
    const candidate = [...row, index];
    if (worstRatio(entries, candidate, side, scale) <= worstRatio(entries, row, side, scale)) {
      row = candidate;
      continue;
    }
    out.push(...layRow(entries, row, scale, box));
    row = [index];
  }
  if (row.length > 0) out.push(...layRow(entries, row, scale, box));
  return out;
};

/** Raise every value to at least the area share of `floorPx`, so a part with a
 *  handful of tokens still owns a tile one can point at. */
const floorAreas = (entries: Entry[], area: number, floorPx: number): void => {
  const sum = entries.reduce((acc, entry) => acc + entry.value, 0);
  if (sum <= 0 || area <= 0) return;
  const lo = (sum * floorPx) / area;
  entries.forEach((entry) => {
    entry.value = Math.max(entry.value, lo);
  });
};

const tipOf = (
  entry: Entry,
  sorted: ContextWindowItemRow[],
  promptTokens: number,
  scaleTokens: number
): ContextTip => {
  const kind: TileKind =
    entry.kind === 'part' || entry.kind === 'more' ? 'part' : entry.kind;
  const tip: ContextTip = {
    kind,
    slot: entry.slot,
    tag: entry.tag,
    name: entry.name,
    count: entry.count,
    value: fmtK(entry.tokens),
    pct: pctText(entry.tokens, entry.kind === 'free' ? scaleTokens : promptTokens),
    rows: [],
    more: 0
  };
  if (entry.kind === 'part' || entry.kind === 'more') {
    const take = Math.min(entry.its.length, TIP_ROWS);
    for (const rank of entry.its.slice(0, take)) {
      const item = sorted[rank];
      if (item) {
        tip.rows.push({
          tag: item.tag,
          name: item.name,
          count: item.count,
          value: fmtK(item.tokens),
          on: false
        });
      }
    }
    tip.more = entry.its.length - take;
  }
  return tip;
};

const emitTile = (
  laid: { tiles: ContextTile[]; tips: ContextTip[] },
  rect: Rect,
  entry: Entry,
  sorted: ContextWindowItemRow[],
  promptTokens: number,
  scaleTokens: number,
  mapW: number,
  mapH: number
): void => {
  const bare = rect.w < TILE_BARE_W || rect.h < TILE_BARE_H;
  const low = rect.h < TILE_LOW_H;
  laid.tiles.push({
    x: rect.x / mapW,
    y: rect.y / mapH,
    w: rect.w / mapW,
    h: rect.h / mapH,
    slot: entry.slot,
    kind: entry.kind,
    tag: entry.tag,
    name: entry.name,
    count: entry.count,
    tokens: entry.tokens,
    tokensText: fmtK(entry.tokens),
    item: entry.item,
    showName: !bare,
    showValue: !bare && !low,
    its: entry.its
  });
  laid.tips.push(tipOf(entry, sorted, promptTokens, scaleTokens));
};

/**
 * Two-level treemap laid out for the map's real pixel size. The used share
 * takes a band along the long side (eased by share^0.35 so an almost empty
 * window still keeps its parts readable); free space fills the remainder.
 * A part lays its own items inside its rectangle; items too small to read
 * gather into one "more" tile.
 */
export function layoutTiles(
  record: ContextWindowRecord,
  width: number,
  height: number
): { tiles: ContextTile[]; tips: ContextTip[] } {
  const laid: { tiles: ContextTile[]; tips: ContextTip[] } = { tiles: [], tips: [] };
  if (!hasContextData(record)) return laid;
  const w = Math.max(200, width);
  const h = Math.max(60, height);
  const scale = Math.max(1, Math.max(record.window, record.tokens));
  const used = record.tokens;
  const long = Math.max(w, h);
  const across = w >= h;
  const usedLength =
    used < scale ? Math.min(long, Math.max(36, long * Math.pow(used / scale, 0.35))) : long;
  const band: Rect = across
    ? { x: 0, y: 0, w: usedLength, h }
    : { x: 0, y: 0, w, h: usedLength };
  const freeRect: Rect = across
    ? { x: usedLength, y: 0, w: w - usedLength, h }
    : { x: 0, y: usedLength, w, h: h - usedLength };

  const partEntries: Entry[] = record.parts.map((part) => {
    const sorted = sortedItems(part);
    return {
      value: part.tokens,
      tokens: part.tokens,
      kind: 'part' as TileKind,
      slot: part.slot,
      tag: '',
      name: '',
      count: 0,
      item: -1,
      its: sorted.map((_, rank) => rank)
    };
  });
  floorAreas(partEntries, band.w * band.h, TILE_FLOOR);
  let partRects = squarify(partEntries, band);
  // A part between large ones would be a strip too thin to point at: raise its
  // value until it is at least TILE_SIDE across both ways.
  for (let round = 0; round < 8; round += 1) {
    const thin = partRects
      .filter(([rect]) => Math.min(rect.w, rect.h) < TILE_SIDE)
      .map(([, index]) => index);
    if (thin.length === 0) break;
    for (const index of thin) {
      const rect = partRects.find(([, at]) => at === index)?.[0] ?? { x: 0, y: 0, w: 1, h: 1 };
      const side = Math.max(1, Math.min(rect.w, rect.h));
      partEntries[index].value *= (TILE_SIDE / side) * 1.05;
    }
    partRects = squarify(partEntries, band);
  }

  for (const [rect, index] of partRects) {
    const part = record.parts[index];
    if (!part) continue;
    const entry = partEntries[index];
    const sorted = sortedItems(part);
    emitTile(laid, rect, entry, sorted, record.tokens, scale, w, h);
    if (sorted.length === 0) continue;
    const area = rect.w * rect.h;
    if (area < TILE_MIN * 2) continue;
    const itemSum = sorted.reduce((acc, item) => acc + item.tokens, 0);
    const kept: Entry[] = [];
    const rest: number[] = [];
    sorted.forEach((item, rank) => {
      if (kept.length < TILE_MAX_ITEMS && (item.tokens / itemSum) * area >= TILE_MIN) {
        kept.push({
          value: item.tokens,
          tokens: item.tokens,
          kind: 'item',
          slot: part.slot,
          tag: item.tag,
          name: item.name,
          count: item.count,
          item: rank,
          its: [rank]
        });
      } else {
        rest.push(rank);
      }
    });
    if (rest.length > 0) {
      if (kept.length === 0 && rest.length === 1) {
        const rank = rest[0];
        const item = sorted[rank];
        if (item) {
          kept.push({
            value: item.tokens,
            tokens: item.tokens,
            kind: 'item',
            slot: part.slot,
            tag: item.tag,
            name: item.name,
            count: item.count,
            item: rank,
            its: [rank]
          });
        }
      } else {
        const tokens = rest.reduce((sum, rank) => sum + (sorted[rank]?.tokens ?? 0), 0);
        kept.push({
          value: tokens,
          tokens,
          kind: 'more',
          slot: part.slot,
          tag: '',
          name: '',
          count: rest.length,
          item: -2,
          its: rest
        });
      }
    }
    floorAreas(kept, area, TILE_FLOOR);
    const laidItems = squarify(kept, rect);
    if (laidItems.some(([itemRect]) => Math.min(itemRect.w, itemRect.h) < 14)) continue;
    if (laidItems.length > 0) {
      laid.tiles.pop();
      laid.tips.pop();
    }
    for (const [itemRect, itemIndex] of laidItems) {
      const itemEntry = kept[itemIndex];
      if (!itemEntry) continue;
      emitTile(laid, itemRect, itemEntry, sorted, record.tokens, scale, w, h);
    }
  }

  const free = Math.max(0, scale - record.tokens);
  if (freeRect.w > 0 && freeRect.h > 0) {
    const entry: Entry = {
      value: free,
      tokens: free,
      kind: 'free',
      slot: 0,
      tag: '',
      name: '',
      count: 0,
      item: -3,
      its: []
    };
    emitTile(laid, freeRect, entry, [], record.tokens, scale, w, h);
  }
  return laid;
}

/** Hover tip for a grid cell: part rows top-listed, the hovered item kept on. */
export function partTip(
  record: ContextWindowRecord,
  part: ContextWindowPartRow,
  itemRank: number,
  rows: number = GRID_TIP_ROWS
): ContextTip {
  const sorted = sortedItems(part);
  const tip: ContextTip = {
    kind: 'part',
    slot: part.slot,
    tag: '',
    name: '',
    count: 0,
    value: fmtK(part.tokens),
    pct: pctText(part.tokens, record.tokens),
    rows: [],
    more: 0
  };
  const picked: number[] = sorted.map((_, rank) => rank).slice(0, rows);
  if (itemRank >= 0) {
    if (!picked.includes(itemRank)) {
      if (picked.length === rows && rows > 0) picked[rows - 1] = itemRank;
      else picked.push(itemRank);
    }
  }
  // A new turn is often far smaller than the older turns; keep the latest
  // turn in the grid tip even when it did not make the size-ranked top rows.
  if (part.slot === 5 && rows > 0) {
    const listed = listedItems(part);
    const latest = listed[listed.length - 1];
    if (latest) {
      const at = sorted.findIndex((candidate) => candidate === latest);
      if (at >= 0 && !picked.includes(at)) picked[rows - 1] = at;
    }
  }
  for (const rank of picked) {
    const item = sorted[rank];
    if (item) {
      tip.rows.push({
        tag: item.tag,
        name: item.name,
        count: item.count,
        value: fmtK(item.tokens),
        on: itemRank >= 0 && rank === itemRank
      });
    }
  }
  tip.more = sorted.length - picked.length;
  return tip;
}

// ---- Detail projections ---------------------------------------------------

export type ContextDetailRow = {
  slot: number;
  tag: string;
  name: string;
  count: number;
  tokens: number;
  tokensText: string;
  ratio: number;
  /** Index in the part's listed (not size-sorted) order. */
  item: number;
  label: string;
};

/** A part's detail rows: listed order, each with its share of the largest row. */
export function partDetailRows(
  record: ContextWindowRecord,
  slot: number,
  t: TranslateFn
): ContextDetailRow[] {
  const part = record.parts.find((entry) => entry.slot === slot);
  if (!part) return [];
  const listed = listedItems(part);
  const most = Math.max(1, ...listed.map((item) => item.tokens));
  return listed.map((item, index) => ({
    slot: part.slot,
    tag: item.tag,
    name: item.name,
    count: item.count,
    tokens: item.tokens,
    tokensText: fmtK(item.tokens),
    ratio: Math.min(1, item.tokens / most),
    item: index,
    label: contextItemLabel(item, t)
  }));
}

// ---- Click target classification ----------------------------------------

export type ContextTarget =
  | { kind: 'item'; slot: number; tag: string; name: string; count: number; itemRank: number }
  | { kind: 'part'; slot: number };

/** A tile opens its item, its part, or nothing for the free remainder. */
export function tileTarget(record: ContextWindowRecord, tile: ContextTile): ContextTarget | null {
  if (tile.kind === 'free') return null;
  if (tile.kind === 'item') {
    return {
      kind: 'item',
      slot: tile.slot,
      tag: tile.tag,
      name: tile.name,
      count: tile.count,
      itemRank: tile.item
    };
  }
  return { kind: 'part', slot: tile.slot };
}

/** State held for the item detail overlay; text comes only from the memory endpoint. */
export type ItemDetailState = {
  kind: 'item';
  slot: number;
  tag: string;
  name: string;
  count: number;
  label: string;
  tokensText: string;
  pctText: string;
  text: string;
  loading: boolean;
  unavailable: boolean;
};

/** State held for the part detail overlay; one row expands its own text. */
export type PartDetailState = {
  kind: 'part';
  slot: number;
  label: string;
  tokensText: string;
  pctText: string;
  rows: ContextDetailRow[];
  openRow: number;
  rowText: string;
  rowLoading: boolean;
};

export type DetailState = ItemDetailState | PartDetailState;

/** A cell with item >= 0 opens that item; a part-only cell opens the part. */
export function cellTarget(record: ContextWindowRecord, cell: ContextCell): ContextTarget | null {
  if (cell.slot === 0) return null;
  const part = record.parts.find((entry) => entry.slot === cell.slot);
  if (!part) return null;
  if (cell.item < 0) return { kind: 'part', slot: cell.slot };
  const sorted = sortedItems(part);
  const item = sorted[cell.item];
  if (!item) return { kind: 'part', slot: cell.slot };
  return {
    kind: 'item',
    slot: part.slot,
    tag: item.tag,
    name: item.name,
    count: item.count,
    itemRank: cell.item
  };
}
