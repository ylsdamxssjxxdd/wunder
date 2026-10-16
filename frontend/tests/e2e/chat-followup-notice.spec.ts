import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('a completed thread accepts a new request without an old completion notice and keeps bottom space', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => {
    const notices: string[] = [];
    (window as any).__followupNotices = notices;
    const seen = new WeakSet<Element>();
    new MutationObserver(() => {
      document.querySelectorAll('.el-message--success').forEach(node => {
        const text = node.textContent || '';
        if (seen.has(node) || !/has completed the task|已完成任务/.test(text)) return;
        seen.add(node); notices.push(text);
      });
    }).observe(document, { childList: true, subtree: true });
  });
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const input = page.getByTestId('chat-composer-input');
    const send = async (text: string) => {
      await input.fill(text); await input.press('Control+Enter');
      await expect(input).toHaveValue('');
    };
    const noticeCount = () => page.evaluate(() => (window as any).__followupNotices.length);
    await send('Fixture initial request');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 1.');
    await expect.poll(noticeCount).toBe(1);
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    const padding = await page.getByTestId('messenger-message-list').evaluate(node =>
      Number.parseFloat(getComputedStyle(node).paddingBottom));
    expect(padding).toBeGreaterThanOrEqual(84);

    service.holdNext = true;
    await send('Fixture follow-up request');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 2.');
    await expect.poll(() => service.items.get('fixture-turn-2:tool-2')?.status).toBe('completed');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'stop');
    expect(await noticeCount()).toBe(1);
    service.release();
    await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 2.');
    await expect.poll(noticeCount).toBe(2);
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');

    service.holdNext = true;
    await send('Fixture cancellable request');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 3.');
    await page.getByTestId('chat-composer-send').click();
    await expect.poll(() => service.turns.at(-1)?.status).toBe('cancelled');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    await page.waitForTimeout(1200);
    expect(await noticeCount()).toBe(2);
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});
