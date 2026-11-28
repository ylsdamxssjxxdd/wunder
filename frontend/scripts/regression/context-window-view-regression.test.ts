import test from 'node:test';
import assert from 'node:assert/strict';

import {
  cellTarget,
  contextItemLabel,
  findListedItem,
  fmtK,
  gridCells,
  gridLayout,
  hasContextData,
  layoutTiles,
  legendRows,
  listRows,
  normalizeContextWindow,
  partDetailRows,
  partTip,
  tileTarget,
  type ContextWindowRecord
} from '../../src/components/chat/contextWindowView';

const t = (key: string, params: Record<string, unknown> = {}): string => {
  const table: Record<string, string> = {
    'chat.contextWindow.partSystem': '系统',
    'chat.contextWindow.partTools': '工具',
    'chat.contextWindow.partFiles': '图片',
    'chat.contextWindow.partResults': '结果',
    'chat.contextWindow.partChat': '会话',
    'chat.contextWindow.itemTool': '工具定义',
    'chat.contextWindow.itemResult': '结果',
    'chat.contextWindow.itemPrompt': '提示词',
    'chat.contextWindow.itemReminder': '提醒',
    'chat.contextWindow.itemOther': '内容',
    'chat.contextWindow.turn': '第 {count} 轮',
    'chat.contextWindow.image': '图片 ×{count}',
    'chat.contextWindow.free': '剩余',
    'chat.contextWindow.moreItems': '另外 {count} 项'
  };
  return (table[key] ?? key).replace(/\{count\}/g, String(params.count ?? ''));
};

const buildRecord = (parts: Array<Record<string, unknown>>): ContextWindowRecord =>
  normalizeContextWindow({
    model: 'test-model',
    provider: 'test-provider',
    window: 262_144,
    tokens: 8_621,
    measured: true,
    turns: 2,
    parts
  });

const sampleParts = (): Array<Record<string, unknown>> => [
  {
    kind: 'system',
    tokens: 3_422,
    items: [{ tag: 'prompt', name: '', count: 1, tokens: 3_422, order: 0 }]
  },
  {
    kind: 'tools',
    tokens: 5_003,
    items: Array.from({ length: 19 }, (_, index) => ({
      tag: 'tool',
      name: `tool${index}`,
      count: 1,
      tokens: 263,
      order: 0
    }))
  },
  {
    kind: 'results',
    tokens: 40,
    items: [{ tag: 'result', name: 'read_file', count: 1, tokens: 40, order: 2 }]
  },
  {
    kind: 'chat',
    tokens: 196,
    items: [
      { tag: 'turn', name: '', count: 1, tokens: 188, order: 1 },
      { tag: 'turn', name: '', count: 2, tokens: 8, order: 3 }
    ]
  }
];

test('legend fills all five parts; missing parts read 0 instead of disappearing', () => {
  const record = buildRecord([
    { kind: 'system', tokens: 3_422, items: [] },
    { kind: 'chat', tokens: 196, items: [] }
  ]);
  const legend = legendRows(record, t);
  assert.equal(legend.length, 5);
  assert.deepEqual(
    legend.map((entry) => entry.kind),
    ['system', 'tools', 'files', 'results', 'chat']
  );
  const tools = legend.find((entry) => entry.slot === 2);
  assert.equal(tools?.tokens, 0);
});

test('contents list keeps head order and interleaves results by message position', () => {
  const record = buildRecord(sampleParts());
  const rows = listRows(record, 0, t);
  const conversation = rows
    .filter((row) => row.slot === 4 || row.slot === 5)
    .map((row) => [row.slot, row.tag, row.count]);
  assert.deepEqual(conversation, [
    [5, 'turn', 1],
    [4, 'result', 1],
    [5, 'turn', 2]
  ]);
  const firstChat = rows.findIndex((row) => row.slot === 5);
  assert.equal(rows[firstChat - 1].slot, 2, 'all tools belong before the first turn');
  assert.ok(rows.slice(firstChat).every((row) => row.slot === 5 || row.slot === 4));

  const chatOnly = listRows(record, 5, t);
  assert.equal(chatOnly[0].count, 1);
  assert.equal(chatOnly[1].count, 2);
});

test('mini bar normalizes against the largest row in the current list only', () => {
  const record = buildRecord(sampleParts());
  const all = listRows(record, 0, t);
  const max = Math.max(...all.map((row) => row.tokens));
  const full = all.filter((row) => row.tokens === max);
  assert.equal(full.length, 1, 'only the global largest row fills its bar');
  assert.equal(full[0].ratio, 1);
  assert.ok(all.every((row) => row.tokens === max || row.ratio < 1));

  // A part filter degrades to its own max, the same as the desktop projection.
  const tools = listRows(record, 2, t);
  const toolMax = Math.max(...tools.map((row) => row.tokens));
  assert.equal(tools.find((row) => row.tokens === toolMax)?.ratio, 1);
  assert.ok(
    tools.every((row) => Math.abs(row.ratio - Math.min(1, row.tokens / toolMax)) < 1e-9)
  );
});

test('item labels follow the desktop tag rule; turn ordinal comes from count', () => {
  assert.equal(
    contextItemLabel({ tag: 'turn', name: '', count: 3 }, t),
    '第 3 轮'
  );
  assert.equal(
    contextItemLabel({ tag: 'result', name: 'read_file', count: 1 }, t),
    'read_file · 结果'
  );
  assert.equal(
    contextItemLabel({ tag: 'tool', name: 'read_file', count: 1 }, t),
    'read_file · 工具定义'
  );
  assert.equal(
    contextItemLabel({ tag: 'image', name: '', count: 4 }, t),
    '图片 ×4'
  );
  assert.equal(contextItemLabel({ tag: 'prompt', name: '', count: 1 }, t), '提示词');
  assert.equal(contextItemLabel({ tag: 'reminder', name: '', count: 1 }, t), '提醒');
  assert.equal(contextItemLabel({ tag: 'reply', name: 'custom-note', count: 1 }, t), 'custom-note');
  assert.equal(contextItemLabel({ tag: 'unknown', name: '', count: 1 }, t), '内容');
});

test('token counts read like the reference fmt_k', () => {
  assert.equal(fmtK(8_300), '8.3K');
  assert.equal(fmtK(992_000), '992K');
  assert.equal(fmtK(1_000_000), '1M');
  assert.equal(fmtK(1_250_000), '1.25M');
  assert.equal(fmtK(0), '0');
  // Whole-hundred K values keep their trailing zero: 260K must not render 26K.
  assert.equal(fmtK(260_000), '260K');
  assert.equal(fmtK(100_000), '100K');
  assert.equal(fmtK(128_000), '128K');
  assert.equal(fmtK(262_144), '262K');
  assert.equal(fmtK(251_000), '251K');
});

test('grid keeps one cell per turn and places the result between surrounding turns', () => {
  const record = buildRecord(sampleParts());
  const cells = gridCells(record);
  assert.equal(cells.length, 400);
  const chat = cells.filter((cell) => cell.slot === 5);
  assert.ok(chat.length >= 2, 'every turn keeps a cell');
  const resultAt = cells.findIndex((cell) => cell.slot === 4);
  const lastChat = cells.map((cell) => cell.slot).lastIndexOf(5);
  assert.ok(resultAt >= 0 && resultAt < lastChat);
});

test('empty snapshot has no data and an empty grid', () => {
  const record = normalizeContextWindow({ parts: [] });
  assert.equal(hasContextData(record), false);
  const cells = gridCells(record);
  assert.ok(cells.every((cell) => cell.slot === 0));
});

test('grid layout splits 50×8 on wide cards and 25×16 on narrow ones', () => {
  assert.deepEqual(gridLayout(920), { columns: 50, rows: 8 });
  assert.deepEqual(gridLayout(390), { columns: 25, rows: 16 });
});

test('two-level treemap lays out part/item/more tiles and a free remainder', () => {
  const record = buildRecord(sampleParts());
  const { tiles, tips } = layoutTiles(record, 880, 232);
  assert.ok(tiles.length >= 5);
  assert.equal(tips.length, tiles.length);
  assert.ok(tiles.some((tile) => tile.kind === 'free'));
  assert.ok(tiles.some((tile) => tile.kind === 'more'), 'small tools gather into a more tile');
  assert.ok(tiles.some((tile) => tile.kind === 'item'));
  for (const tile of tiles) {
    assert.ok(tile.x >= -0.001 && tile.y >= -0.001);
    assert.ok(tile.x + tile.w <= 1.001 && tile.y + tile.h <= 1.001);
    assert.equal(typeof tile.showName, 'boolean');
    assert.equal(typeof tile.showValue, 'boolean');
  }
  for (let index = 0; index < tiles.length; index += 1) {
    for (let other = index + 1; other < tiles.length; other += 1) {
      const a = tiles[index];
      const b = tiles[other];
      const overlapX = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const overlapY = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      assert.ok(overlapX <= 0.001 || overlapY <= 0.001);
    }
  }
});

test('part tips list at most five top items; item tips carry their own subline', () => {
  const record = buildRecord(sampleParts());
  const { tiles, tips } = layoutTiles(record, 880, 232);
  const moreTile = tiles.find((tile) => tile.kind === 'more');
  assert.ok(moreTile);
  const moreTip = tips[tiles.indexOf(moreTile!)];
  assert.ok(moreTip);
  assert.equal(moreTip.kind, 'part');
  assert.ok(moreTip.rows.length <= 5);
  assert.ok(moreTip.more > 0);

  const itemTile = tiles.find((tile) => tile.kind === 'item');
  assert.ok(itemTile);
  const itemTip = tips[tiles.indexOf(itemTile!)];
  assert.equal(itemTip.kind, 'item');
  assert.ok(itemTip.pct.length > 0);

  const tools = record.parts.find((part) => part.slot === 2);
  assert.ok(tools);
  const gridTip = partTip(record, tools, 0);
  assert.equal(gridTip.rows.length, 4);
  assert.ok(gridTip.rows[0]?.on === true);
});

test('click targets classify tiles and cells into item, part or none', () => {
  const record = buildRecord(sampleParts());
  const { tiles } = layoutTiles(record, 880, 232);
  const freeTile = tiles.find((tile) => tile.kind === 'free');
  assert.equal(tileTarget(record, freeTile!), null);
  const itemTile = tiles.find((tile) => tile.kind === 'item');
  const itemTarget = tileTarget(record, itemTile!);
  assert.equal(itemTarget?.kind, 'item');
  assert.equal(itemTarget?.slot, itemTile!.slot);
  const partTile = tiles.find((tile) => tile.kind === 'part');
  if (partTile) {
    assert.equal(tileTarget(record, partTile)?.kind, 'part');
  }

  const cells = gridCells(record);
  const freeCell = cells.find((cell) => cell.slot === 0);
  assert.equal(cellTarget(record, freeCell!), null);
  const partCell = cells.find((cell) => cell.item < 0 && cell.slot > 0);
  if (partCell) assert.equal(cellTarget(record, partCell)?.kind, 'part');
  const itemCell = cells.find((cell) => cell.item >= 0);
  if (itemCell) assert.equal(cellTarget(record, itemCell)?.kind, 'item');
});

test('part detail rows keep listed order and resolve their own item', () => {
  const record = buildRecord(sampleParts());
  const rows = partDetailRows(record, 2, t);
  assert.equal(rows.length, 19);
  assert.ok(rows.every((row) => row.slot === 2));
  const first = rows[0];
  const part = record.parts.find((entry) => entry.slot === 2);
  const item = part ? findListedItem(part, first.tag, first.name, first.count) : undefined;
  assert.ok(item);
  assert.equal(first.tokensText, fmtK(item!.tokens));
});
