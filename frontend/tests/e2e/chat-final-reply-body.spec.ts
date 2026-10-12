import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('model rounds fold into the agent work process, only the final reply stays open', async ({ page }) => {
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
    // 中间轮各留下一次工具调用：过程区展开后它们以批次条目出现。
    if (round < 5) {
      const tool_id = `${turn_id}:tool-${round}`;
      service.items.set(tool_id, { item_id: tool_id, turn_id, model_round: round,
        kind: 'tool_call', status: 'completed', revision: 1, visibility: 'user',
        tool: 'read_file', tool_call_id: `call-${round}`, event_type: 'tool_call',
        args: { path: 'fixture.txt' } });
    }
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
    const processHead = reply.locator('[data-turn-slot="process"]');
    for (let attempt = 0; attempt < 2; attempt++) {
      if (attempt) await page.reload();
      await expect(reply).toHaveCount(1);
      // 默认形态只有两条可见内容：一个「工作过程」条目 + 最终回复。
      // 中间各轮的打印与工具批次都在折叠体里，收起时整体不进 DOM。
      await expect(processHead).toHaveCount(1);
      await expect(processHead).toHaveAttribute('aria-expanded', 'false');
      await expect(segments).toHaveCount(1);
      await expect(segments).toHaveText('Final fixture answer.');
      await expect(reply).toHaveAttribute('data-message-status', 'final');
      // 展开过程条目：每一轮模型输出都是时间线上独立的一段，按轮次顺序铺开；
      // 最终答复仍是最后一段，而不是把前几轮拼接进去。
      await processHead.click();
      await expect(processHead).toHaveAttribute('aria-expanded', 'true');
      await expect(segments).toHaveCount(5);
      for (let round = 1; round <= 4; round++) {
        await expect(segments.nth(round - 1)).toContainText(`Intermediate fixture note ${round}.`);
      }
      await expect(segments.last()).toHaveText('Final fixture answer.');
      // 批次栏跟随过程区打开：中间轮的工具调用条目直接可见，不用再点第二层。
      await expect(reply.locator('.timeline-group').first()).toBeAttached();
      await expect(reply.locator('.tl-entry')).toHaveCount(4);
      // 再收起：折叠体整体卸载。
      await processHead.click();
      await expect(segments).toHaveCount(1);
    }
  } finally {
    await service.dispose();
  }
});
