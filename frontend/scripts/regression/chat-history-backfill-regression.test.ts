import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildExistingHistoryItemSeqSet,
  collectDedupedHistoryBackfillPage,
  prependHistoryBackfillPage,
  readHistoryBackfillPage
} from '../../src/stores/chatHistoryBackfill';

test('history backfill continues past duplicate pages and preserves chronological order', () => {
  const existingIds = buildExistingHistoryItemSeqSet([
    { role: 'user', content: 'recent user', created_seq: 30 },
    { role: 'assistant', content: 'recent assistant', created_seq: 31 }
  ]);
  let accumulated: Record<string, unknown>[] = [];

  const duplicatePage = readHistoryBackfillPage({
    transcript: [{ role: 'user', content: 'recent user', created_seq: 30 }],
    has_more: true,
    before_seq: 20
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
      { role: 'user', content: 'older user', created_seq: 10 },
      { role: 'assistant', content: 'older assistant', created_seq: 11 }
    ],
    has_more: false,
    before_seq: 10
  });
  const olderDeduped = collectDedupedHistoryBackfillPage(olderPage.transcript, existingIds);
  accumulated = prependHistoryBackfillPage(accumulated, olderDeduped);

  assert.deepEqual(
    accumulated.map((message) => message.content),
    ['older user', 'older assistant']
  );
});

test('history backfill prepends later-discovered older pages before accumulated newer pages', () => {
  const existingIds = buildExistingHistoryItemSeqSet([]);
  let accumulated: Record<string, unknown>[] = [];

  const newerPage = collectDedupedHistoryBackfillPage(
    [
      { role: 'user', content: 'middle user', created_seq: 20 },
      { role: 'assistant', content: 'middle assistant', created_seq: 21 }
    ],
    existingIds
  );
  accumulated = prependHistoryBackfillPage(accumulated, newerPage);

  const olderPage = collectDedupedHistoryBackfillPage(
    [
      { role: 'user', content: 'oldest user', created_seq: 10 },
      { role: 'assistant', content: 'oldest assistant', created_seq: 11 }
    ],
    existingIds
  );
  accumulated = prependHistoryBackfillPage(accumulated, olderPage);

  assert.deepEqual(
    accumulated.map((message) => message.created_seq),
    [10, 11, 20, 21]
  );
});
