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

// 看门狗的活性判定已经换成「持久线程状态优先」：只要该会话有视图状态
// （running/queued），就以它为准；没有持久状态时才回落到持久轮次、运行时状态
// 或旧气泡。漂移不再靠比较远端游标，而是用「空闲超过阈值且没有在途请求」触发重连。
test('watchdog liveness trusts the durable thread status before legacy bubbles', () => {
  assert.ok(watcherSource.includes('threadViewStatus(key)'), '看门狗没有读持久线程状态');
  assert.ok(
    watcherSource.includes("displayStatus!==null ? ['running','queued'].includes(displayStatus)"),
    '持久状态没有优先于旧气泡'
  );
  assert.ok(watcherSource.includes('durableRunning'), '没有使用持久轮次作为回落判据');
  assert.ok(
    watcherSource.includes('WATCHDOG_V2_LIVENESS_IDLE_MS') &&
      watcherSource.includes('!runtime.sendController && !runtime.resumeController'),
    '空闲重连没有按在途请求收敛'
  );
});

test('watchdog no longer resumes from a legacy pending assistant bubble', () => {
  assert.equal(watcherSource.includes('chat_watchdog_resume'), false);
  assert.equal(watcherSource.includes('resumeStream(key, pendingMessage'), false);
  assert.equal(watcherSource.includes('hasPendingMessage'), false);
});
