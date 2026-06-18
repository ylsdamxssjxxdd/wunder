import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildExistingHistoryIdSet,
  collectDedupedHistoryBackfillPage,
  prependHistoryBackfillPage,
  readHistoryBackfillPage
} from '../../src/stores/chatHistoryBackfill';

test('history backfill continues past duplicate pages and preserves chronological order', () => {
  const existingIds = buildExistingHistoryIdSet([
    { role: 'user', content: 'recent user', history_id: 30 },
    { role: 'assistant', content: 'recent assistant', history_id: 31 }
  ]);
  let accumulated: Record<string, unknown>[] = [];

  const duplicatePage = readHistoryBackfillPage({
    transcript: [{ role: 'user', content: 'recent user', history_id: 30 }],
    history_has_more: true,
    history_before_id: 20
  });
  const duplicateDeduped = collectDedupedHistoryBackfillPage(
    duplicatePage.transcript,
    existingIds
  );
  accumulated = prependHistoryBackfillPage(accumulated, duplicateDeduped);

  assert.equal(duplicateDeduped.length, 0);
  assert.equal(duplicatePage.hasMore, true);
  assert.equal(duplicatePage.beforeId, 20);

  const olderPage = readHistoryBackfillPage({
    transcript: [
      { role: 'user', content: 'older user', history_id: 10 },
      { role: 'assistant', content: 'older assistant', history_id: 11 }
    ],
    history_has_more: false,
    history_before_id: 10
  });
  const olderDeduped = collectDedupedHistoryBackfillPage(olderPage.transcript, existingIds);
  accumulated = prependHistoryBackfillPage(accumulated, olderDeduped);

  assert.deepEqual(
    accumulated.map((message) => message.content),
    ['older user', 'older assistant']
  );
});

test('history backfill prepends later-discovered older pages before accumulated newer pages', () => {
  const existingIds = buildExistingHistoryIdSet([]);
  let accumulated: Record<string, unknown>[] = [];

  const newerPage = collectDedupedHistoryBackfillPage(
    [
      { role: 'user', content: 'middle user', history_id: 20 },
      { role: 'assistant', content: 'middle assistant', history_id: 21 }
    ],
    existingIds
  );
  accumulated = prependHistoryBackfillPage(accumulated, newerPage);

  const olderPage = collectDedupedHistoryBackfillPage(
    [
      { role: 'user', content: 'oldest user', history_id: 10 },
      { role: 'assistant', content: 'oldest assistant', history_id: 11 }
    ],
    existingIds
  );
  accumulated = prependHistoryBackfillPage(accumulated, olderPage);

  assert.deepEqual(
    accumulated.map((message) => message.history_id),
    [10, 11, 20, 21]
  );
});
