import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('channel input and direct replies render from durable events and survive reload', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  const publish = (change_type: string, payload: Record<string, any>) => {
    const change = { change_seq: service.changes.length + 1, cursor: service.changes.length + 1,
      change_type, turn_id: payload.turn_id, item_id: payload.item_id, revision: payload.revision, payload };
    service.changes.push(change);
    for (const sub of service.subscriptions) sub.socket.send(JSON.stringify({ type: 'event', request_id: sub.request,
      payload: { event: 'thread_change', data: { session_id: MOCK_SESSION, ...change } } }));
  };
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    await expect.poll(() => service.subscriptions.length).toBeGreaterThan(0);
    for (const [index, content] of ['Fixture channel input', '/help', 'Fixture file upload', 'Fixture busy input'].entries()) {
      const turn = { turn_id: `channel-turn-${index}`, user_round: index + 1, content, status: 'queued' };
      service.turns.push(turn);
      publish('turn_upsert', { ...turn });
      const user = { item_id: `${turn.turn_id}:user`, turn_id: turn.turn_id, user_round: turn.user_round,
        kind: 'user_message', role: 'user', content, status: 'completed', visibility: 'user', revision: 1 };
      service.items.set(user.item_id, user);
      publish('item_upsert', user);
      const pair = page.locator(`.messenger-turn[data-root-turn-id="${turn.turn_id}"]`);
      await expect(pair.locator('[data-turn-slot="user"]')).toContainText(content);
      await expect(pair.locator('[data-turn-slot="assistant"]')).toHaveCount(1);
      const reply = { item_id: `${turn.turn_id}:text-0`, turn_id: turn.turn_id, model_round: 0,
        kind: 'assistant_message', role: 'assistant', content: `Fixture reply ${index}`,
        status: 'completed', visibility: 'user', revision: 1, source: 'channel' };
      service.items.set(reply.item_id, reply);
      publish('item_upsert', reply);
      turn.status = 'completed';
      publish('turn_upsert', { ...turn });
      await expect(pair.locator('[data-turn-slot="assistant"]')).toContainText(reply.content);
    }
    await page.reload();
    await expect(page.locator('.messenger-turn')).toHaveCount(4);
    for (let index = 0; index < 4; index++) {
      const pair = page.locator(`.messenger-turn[data-root-turn-id="channel-turn-${index}"]`);
      await expect(pair.locator('[data-turn-slot="assistant"]')).toContainText(`Fixture reply ${index}`);
      await expect(pair.locator('[data-message-status="final"]:not(.mine)')).toHaveCount(1);
    }
  } finally { await service.dispose(); }
});
