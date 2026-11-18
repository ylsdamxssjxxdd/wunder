// AI生成
// The work process prints each model round exactly once. A live tail that
// appended its own block reprinted the round's prose inside the fold and left
// the fold's text leaking into the transcript, so the live text must land in the
// row that already owns that item and never add one.
import assert from 'node:assert/strict';

import { placeLivePrint, type LivePrintRow } from '@/components/chat/livePrintRow';

const rows: LivePrintRow[] = [
  { kind: 'reasoning', key: 'reasoning:turn:text-1', text: 'thought' },
  { kind: 'print', key: 'print:turn:text-1', text: 'intermediate' },
  { kind: 'tool', key: 'tool:call-1', text: '' }
];

const live = placeLivePrint(rows, 'intermediate continues', 'print:turn:text-1', true);
assert.equal(live.placed, true, 'the live text lands in the durable row');
assert.equal(live.rows.length, rows.length, 'the live text never appends a row');
const printed = live.rows.filter(row => String(row.text || '').includes('intermediate'));
assert.equal(printed.length, 1, 'one round prints exactly once');
assert.equal(printed[0].key, 'print:turn:text-1', 'it prints in that row own place');
assert.equal(printed[0].live, true, 'the row keeps its streaming caret');
assert.equal(printed[0].text, 'intermediate continues');
assert.deepEqual(live.rows.map(row => row.key), rows.map(row => row.key));

// Nothing to print into: the caller must not invent a row of its own either — the
// reply area is streaming that same text, so a stand-in block here would double it.
const absent = placeLivePrint(rows, 'intermediate continues', 'print:turn:text-2', true);
assert.equal(absent.placed, false);
assert.deepEqual(absent.rows, rows);

// A reader browsing history keeps seeing what they were reading.
const browsing = placeLivePrint(rows, 'intermediate continues', 'print:turn:text-1', false);
assert.equal(browsing.placed, false);
assert.deepEqual(browsing.rows, rows);

// No live text (a settled turn): the durable row renders its markdown again.
const settled = placeLivePrint(rows, '', '', false);
assert.equal(settled.placed, false);
assert.deepEqual(settled.rows, rows);

console.log('live print: one row per model round, placed in order, never appended pass');
