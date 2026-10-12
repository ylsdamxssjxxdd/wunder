import test from 'node:test';
import assert from 'node:assert/strict';
import { ref } from 'vue';

import {
  mergeKeyOrderPreservingMissing,
  moveKeyWithinOrder,
  prependFreshKeys,
  usePersistentStableListOrder
} from '../../src/views/messenger/stableListOrder';
import { resolveVisibleTaskRows, taskWindow } from '../../src/views/messenger/taskList';

// --------------------------------------------------------------- helpers

const normalize = (values: string[]): string[] => {
  const output: string[] = [];
  const seen = new Set<string>();
  for (const value of values) {
    const key = String(value || '').trim();
    if (!key || seen.has(key)) continue;
    seen.add(key);
    output.push(key);
  }
  return output;
};

// Reference copy of the historical nested-scan merge. Kept in the test so the
// linear rewrite can be proven equivalent for arbitrary inputs.
function legacyMergeKeyOrderPreservingMissing(previousKeys: string[], incomingKeys: string[]): string[] {
  const normalizedIncoming = normalize(incomingKeys);
  const normalizedPrevious = normalize(previousKeys);
  if (!normalizedIncoming.length) return normalizedPrevious;
  if (!normalizedPrevious.length) return normalizedIncoming;

  const previousSet = new Set(normalizedPrevious);
  const next = normalizedPrevious.filter((key) => normalizedIncoming.includes(key));
  const nextSet = new Set(next);
  normalizedIncoming.forEach((key, index) => {
    if (nextSet.has(key)) return;
    const following = normalizedIncoming.slice(index + 1).find((candidate) => previousSet.has(candidate));
    if (following) {
      next.splice(next.indexOf(following), 0, key);
    } else {
      const preceding = [...normalizedIncoming].slice(0, index).reverse().find((candidate) => nextSet.has(candidate));
      const insertionIndex = preceding ? next.indexOf(preceding) + 1 : next.length;
      next.splice(insertionIndex, 0, key);
    }
    nextSet.add(key);
  });
  return next;
}

function makeFakeStorage() {
  const map = new Map<string, string>();
  const state = { writes: 0 };
  return {
    map,
    state,
    storage: {
      getItem: (key: string): string | null => (map.has(key) ? map.get(key)! : null),
      setItem: (key: string, value: string): void => {
        state.writes += 1;
        map.set(key, value);
      }
    }
  };
}

// ----------------------------------------------------- order persistence

test('sidebar order writes are coalesced off the interaction path', () => {
  const items = ref([{ id: 'a' }, { id: 'b' }, { id: 'c' }]);
  const fake = makeFakeStorage();
  const scheduled: Array<() => void> = [];
  const controller = usePersistentStableListOrder(items, {
    getKey: (item) => item.id,
    storageKey: 'order:test',
    storage: fake.storage,
    scheduleFlush: (task) => {
      scheduled.push(task);
    }
  });

  // The initial list sync is already deferred: no synchronous write.
  assert.equal(fake.state.writes, 0, 'list sync must not write synchronously');
  assert.deepEqual(controller.orderedKeys.value, ['a', 'b', 'c']);

  // Clicking/promoting a thread reorders state only; storage stays untouched
  // until the idle flush. Old write-through behaviour wrote here (1 write).
  controller.prependKey('c');
  assert.deepEqual(controller.orderedKeys.value, ['c', 'a', 'b']);
  assert.equal(fake.state.writes, 0, 'a click must not write the order synchronously');

  // Several interactions within the frame still collapse into one flush.
  controller.prependKey('b');
  assert.equal(scheduled.length, 1, 'writes must coalesce into a single scheduled flush');
  scheduled.shift()!();
  assert.equal(fake.state.writes, 1, 'the flush persists exactly once');
  assert.deepEqual(JSON.parse(fake.map.get('order:test')!), ['b', 'c', 'a']);
});

test('drag reorders are deferred and coalesced', () => {
  const items = ref([{ id: 'a' }, { id: 'b' }, { id: 'c' }, { id: 'd' }]);
  const fake = makeFakeStorage();
  const scheduled: Array<() => void> = [];
  const controller = usePersistentStableListOrder(items, {
    getKey: (item) => item.id,
    storageKey: 'order:drag',
    storage: fake.storage,
    scheduleFlush: (task) => {
      scheduled.push(task);
    }
  });
  controller.moveItem('d', 'a', 'before');
  controller.moveItem('c', 'a', 'before');
  assert.equal(fake.state.writes, 0);
  assert.equal(scheduled.length, 1);
  scheduled.shift()!();
  assert.equal(fake.state.writes, 1);
  assert.deepEqual(JSON.parse(fake.map.get('order:drag')!), ['d', 'c', 'a', 'b']);
});

test('persisted order is restored through the injected storage', () => {
  const fake = makeFakeStorage();
  fake.map.set('order:restore', JSON.stringify(['b', 'a']));
  const items = ref([{ id: 'a' }, { id: 'b' }]);
  const controller = usePersistentStableListOrder(items, {
    getKey: (item) => item.id,
    storageKey: 'order:restore',
    storage: fake.storage,
    scheduleFlush: () => undefined
  });
  assert.deepEqual(controller.orderedKeys.value, ['b', 'a']);
});

// ------------------------------------------------ linear merge equivalence

test('linear merge matches the historical nested-scan merge', () => {
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], ['b', 'x', 'a']), ['x', 'a', 'b']);
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], ['a', 'x', 'b']), ['a', 'x', 'b']);
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], ['x', 'a', 'b']), ['x', 'a', 'b']);
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], ['a', 'b', 'x']), ['a', 'b', 'x']);
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], ['x', 'y']), ['x', 'y']);
  assert.deepEqual(mergeKeyOrderPreservingMissing([], ['a', 'a', 'b']), ['a', 'b']);
  assert.deepEqual(mergeKeyOrderPreservingMissing(['a', 'b'], []), ['a', 'b']);

  let seed = 987654321;
  const random = () => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed / 0x7fffffff;
  };
  const sample = (): string[] => {
    const alphabet = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'];
    const length = 1 + Math.floor(random() * 10);
    const values: string[] = [];
    for (let index = 0; index < length; index += 1) {
      values.push(alphabet[Math.floor(random() * alphabet.length)]);
    }
    return values;
  };
  for (let iteration = 0; iteration < 3000; iteration += 1) {
    const previous = sample();
    const incoming = sample();
    assert.deepEqual(
      mergeKeyOrderPreservingMissing(previous, incoming),
      legacyMergeKeyOrderPreservingMissing(previous, incoming),
      `previous=${previous.join(',')} incoming=${incoming.join(',')}`
    );
  }
});

test('fresh-row promotion and drag reorder semantics are unchanged', () => {
  assert.deepEqual(
    prependFreshKeys(
      ['a', 'n', 'b'],
      ['a', 'b'],
      ['a', 'n', 'b'],
      new Map([
        ['a', 100],
        ['b', 200],
        ['n', 300]
      ])
    ),
    ['n', 'a', 'b']
  );
  assert.deepEqual(moveKeyWithinOrder(['a', 'b', 'c', 'd'], 'd', 'a', 'before'), ['d', 'a', 'b', 'c']);
});

// ------------------------------------------------------- visible window

test('a session bump resolves state only for the visible rows', () => {
  const items = Array.from({ length: 1000 }, (_, index) => ({ id: `thread-${index}` }));
  const range = taskWindow(items.length, 0, 320, 28);
  const visible = range.end - range.start;

  let calls = 0;
  const rows = resolveVisibleTaskRows(items, range, () => {
    calls += 1;
    return 'idle';
  });
  assert.equal(rows.length, visible);
  assert.equal(calls, visible, 'state resolution is bounded by the visible window');
  assert.ok(calls <= visible, 'one bump must never exceed the visible row count');
  // The regression target: the previous implementation scored every row.
  assert.ok(calls < items.length, 'a full-list scan is the regression being guarded');

  // Recomputing after a single-session bump still only touches the window.
  calls = 0;
  resolveVisibleTaskRows(items, range, () => {
    calls += 1;
    return 'running';
  });
  assert.equal(calls, visible);
});
