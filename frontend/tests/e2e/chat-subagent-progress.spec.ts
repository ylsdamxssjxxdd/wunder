import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('subagent cards update below the owning bubble and survive reload', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'fixture-parent';
  service.turns.push({ turn_id, user_round: 1, status: 'completed', content: 'Fixture task' });
  service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id, kind: 'user_message',
    role: 'user', content: 'Fixture task', status: 'completed', revision: 1, visibility: 'user' });
  const item = { item_id: `${turn_id}:subagent-run-a`, turn_id, kind: 'subagent_run',
    status: 'completed', visibility: 'user', revision: 1,
    runtime: { session_id: 'fixture-child', run_id: 'run-a', title: 'Fixture worker',
      status: 'running', terminal: false, can_terminate: true, latest_message: 'Working on fixture',
      metrics: { tool_calls: 2, account_credits_consumed: 0, model_request_count: 3, context_tokens: 2048, max_context: 8192 } } };
  service.items.set(item.item_id, item);
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const card = page.locator('.subagent-panel__item');
    await expect(card).toHaveCount(1);
    await expect(card).toContainText('Working on fixture');
    await expect(card).toContainText('工具 2 次');
    await expect(card).toContainText('额度 0');
    await expect(card).toContainText('25%');
    await expect(card).toContainText('运行中');
    await expect.poll(() => service.subscriptions.length).toBeGreaterThan(0);
    item.revision = 2;
    item.runtime.latest_message = 'More fixture progress';
    item.runtime.metrics.tool_calls = 3;
    const progress = { change_seq: 1, cursor: 1, change_type: 'item_upsert',
      turn_id, item_id: item.item_id, revision: 2, payload: structuredClone(item) };
    service.changes.push(progress);
    for (const sub of service.subscriptions) sub.socket.send(JSON.stringify({ type: 'event',
      request_id: sub.request, payload: { event: 'thread_change', data: { session_id: MOCK_SESSION, ...progress } } }));
    await expect(card).toContainText('More fixture progress');
    await expect(card).toContainText('工具 3 次');
    await expect(card).toContainText('运行中');
    item.revision = 3;
    item.runtime.latest_message = 'Fixture completed';
    item.runtime.status = 'success';
    item.runtime.terminal = true;
    item.runtime.can_terminate = false;
    item.runtime.metrics.tool_calls = 4;
    const change = { change_seq: 2, cursor: 2, change_type: 'item_upsert',
      turn_id, item_id: item.item_id, revision: 3, payload: item };
    service.changes.push(change);
    for (const sub of service.subscriptions) sub.socket.send(JSON.stringify({ type: 'event',
      request_id: sub.request, payload: { event: 'thread_change', data: { session_id: MOCK_SESSION, ...change } } }));
    for (let attempt = 0; attempt < 2; attempt++) {
      if (attempt) await page.reload();
      await expect(card).toHaveCount(1);
      await expect(card).toContainText('Fixture completed');
      await expect(card).toContainText('工具 4 次');
      await expect(card).toContainText('已完成');
      await expect(card.locator('.subagent-panel__stop')).toHaveCount(0);
    }
  } finally { await service.dispose(); }
});
