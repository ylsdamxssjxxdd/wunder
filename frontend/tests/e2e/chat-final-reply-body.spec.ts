import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('each model round prints as its own body segment in round order', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'reply-turn';
  service.turns.push({ turn_id, user_round: 1, status: 'completed' });
  service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id,
    kind: 'user_message', role: 'user', content: 'Fixture request', status: 'completed', revision: 1, visibility: 'user' });
  for (let round = 1; round <= 5; round++) {
    const item_id = `${turn_id}:text-${round}`;
    service.items.set(item_id, { item_id, turn_id, model_round: round,
      kind: 'assistant_message', role: 'assistant', status: 'completed', revision: 1, visibility: 'user',
      content: round === 5 ? 'Final fixture answer.' : `Intermediate fixture note ${round}.`,
      ...(round < 5 ? { tool_calls: [{ id: `call-${round}`, type: 'function', function: { name: 'ptc', arguments: '{}' } }] } : {}) });
  }
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const reply = page.locator('.messenger-message[data-turn-id="reply-turn"]:not(.mine)');
    const segments = reply.locator('.timeline-body-block[data-turn-slot="body"]');
    for (let attempt = 0; attempt < 2; attempt++) {
      if (attempt) await page.reload();
      await expect(reply).toHaveCount(1);
      // 每一轮模型输出都是时间线上独立的一段，按轮次顺序铺开；
      // 最终答复是最后一段，而不是把前几轮拼接进去。
      await expect(segments).toHaveCount(5);
      for (let round = 1; round <= 4; round++) {
        await expect(segments.nth(round - 1)).toContainText(`Intermediate fixture note ${round}.`);
      }
      await expect(segments.last()).toHaveText('Final fixture answer.');
      await expect(reply).toHaveAttribute('data-message-status', 'final');
    }
  } finally {
    await service.dispose();
  }
});
