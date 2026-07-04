import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const watcherSource = readFileSync(
  resolve(process.cwd(), 'src/stores/chatWatcher.ts'),
  'utf8'
);

test('watchdog no longer depends on a content-level recovery helper', () => {
  assert.equal(watcherSource.includes("from './chatWatchdogRecovery'"), false);
  assert.equal(watcherSource.includes('shouldWatchdogReconcileDrift'), false);
});

test('watchdog drift is based on projection cursor versus remote tail', () => {
  assert.ok(
    watcherSource.includes(
      'Number.isFinite(remoteLastEventId) && remoteLastEventId > localLastEventId'
    )
  );
});

test('watchdog no longer resumes from a legacy pending assistant bubble', () => {
  assert.equal(watcherSource.includes('chat_watchdog_resume'), false);
  assert.equal(watcherSource.includes('resumeStream(key, pendingMessage'), false);
  assert.equal(watcherSource.includes('hasPendingMessage'), false);
});
