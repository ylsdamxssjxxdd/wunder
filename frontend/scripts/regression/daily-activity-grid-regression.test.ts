import test from 'node:test';
import assert from 'node:assert/strict';

import {
  ACTIVITY_WEEK_COLUMNS,
  buildActivityGrid,
  formatActivityCount,
  formatDayKey,
  parseDayKey,
  resolveActivityLevel
} from '../../src/utils/dailyActivityGrid';

const weekdayOf = (date: string): number => new Date(parseDayKey(date) ?? 0).getUTCDay();

test('activity grid keeps every column on the same weekday rows', () => {
  // A Monday anchor, so the trailing column is the current partial week.
  const grid = buildActivityGrid([], '2026-10-12', ACTIVITY_WEEK_COLUMNS);
  assert.equal(grid.columns.length, ACTIVITY_WEEK_COLUMNS);
  const cells = grid.columns.flatMap((column) => column.cells);
  assert.equal(cells.length, ACTIVITY_WEEK_COLUMNS * 7);
  assert.equal(weekdayOf(cells[0].date), 0);
  assert.equal(weekdayOf(cells[cells.length - 1].date), 6);
  for (const column of grid.columns) {
    assert.equal(weekdayOf(column.cells[0].date), 0);
    assert.equal(weekdayOf(column.cells[6].date), 6);
  }
  assert.equal(cells[cells.length - 1].date, '2026-10-17');
});

test('days after today stay rendered but marked as future placeholders', () => {
  const grid = buildActivityGrid([], '2026-10-12', ACTIVITY_WEEK_COLUMNS);
  const lastColumn = grid.columns[grid.columns.length - 1];
  assert.equal(lastColumn.cells[0].date, '2026-10-11');
  assert.equal(lastColumn.cells[1].isToday, true);
  assert.equal(lastColumn.cells[1].future, false);
  assert.deepEqual(
    lastColumn.cells.slice(2).map((cell) => cell.future),
    [true, true, true, true, true]
  );
});

test('sparse buckets land on their own cell only', () => {
  const grid = buildActivityGrid(
    [
      { date: '2026-10-12', value: 4 },
      { date: '2026-09-01', value: 2 },
      { date: 'bad-date', value: 9 },
      { date: '2026-10-12', value: 0 },
      { date: '', value: 7 }
    ],
    '2026-10-12',
    ACTIVITY_WEEK_COLUMNS
  );
  const cells = grid.columns.flatMap((column) => column.cells);
  assert.equal(
    cells.filter((cell) => cell.date === '2026-10-12').reduce((sum, cell) => sum + cell.value, 0),
    4
  );
  assert.equal(cells.filter((cell) => cell.date === '2026-09-01')[0].value, 2);
  assert.equal(cells.some((cell) => cell.value === 9), false);
  assert.equal(grid.activeDays, 2);
  assert.equal(grid.total, 6);
  assert.equal(grid.peak, 4);
});

test('levels ramp from the peak so a single busy day is the darkest cell', () => {
  const grid = buildActivityGrid(
    [
      { date: '2026-10-12', value: 100 },
      { date: '2026-10-11', value: 70 },
      { date: '2026-10-10', value: 40 },
      { date: '2026-10-09', value: 1 }
    ],
    '2026-10-12',
    ACTIVITY_WEEK_COLUMNS
  );
  const levels = grid.columns
    .flatMap((column) => column.cells)
    .filter((cell) => cell.value > 0)
    .map((cell) => cell.level);
  assert.deepEqual(levels, [1, 2, 3, 4]);
  assert.equal(resolveActivityLevel(0, 10), 0);
  assert.equal(resolveActivityLevel(5, 0), 0);
});

test('month labels name full weeks only and never crowd each other', () => {
  const grid = buildActivityGrid([], '2026-10-12', ACTIVITY_WEEK_COLUMNS);
  assert.ok(grid.monthLabels.length >= 10, 'a 53-week window spans at least 10 months');
  const MIN_GAP = 4;
  let previousColumn = Number.NEGATIVE_INFINITY;
  for (const label of grid.monthLabels) {
    const column = grid.columns[label.column];
    assert.equal(column.cells[0].month, label.month);
    assert.equal(column.cells[6].month, label.month);
    assert.ok(label.column - previousColumn >= MIN_GAP, 'labels keep their distance');
    assert.ok(label.column <= grid.columns.length - 4, 'labels stay inside the card');
    previousColumn = label.column;
  }
});

test('an unusable anchor yields an empty grid instead of a broken one', () => {
  const unusable = ['', 'slash 2026/10/12', 'impossible 2026-02-30', 'not-a-date'];
  for (const today of unusable) {
    const grid = buildActivityGrid([{ date: '2026-10-12', value: 3 }], today);
    assert.deepEqual(grid.columns, [], today);
    assert.deepEqual(grid.monthLabels, [], today);
  }
  assert.equal(buildActivityGrid([], '2026-10-12', 0).columns.length, 0);
});

test('day keys round trip in UTC and counts compact for credits', () => {
  assert.equal(parseDayKey('2026-10-12'), Date.UTC(2026, 9, 12));
  assert.equal(parseDayKey('2026-13-01'), null);
  assert.equal(parseDayKey('2026-02-30'), null);
  assert.equal(parseDayKey('2026/10/12T00:00:00Z'), null);
  assert.equal(parseDayKey('2026/10/12'), null);
  assert.equal(formatDayKey(Date.UTC(2026, 9, 12)), '2026-10-12');
  assert.equal(formatActivityCount(0), '0');
  assert.equal(formatActivityCount(37), '37');
  assert.equal(formatActivityCount(1234), '1.2k');
  assert.equal(formatActivityCount(45600), '46k');
  assert.equal(formatActivityCount(2_400_000), '2.4M');
});
