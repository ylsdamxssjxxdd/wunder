import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

for (const action of ['answer', 'free reply', 'stop'] as const) {
  test(`durable inquiry opens before process hydration and supports ${action}`, async ({ page }) => {
    const service = new ChatMockService();
    service.seedInquiryTurn();
    await page.addInitScript(() => {
      (window as any).__inquiryTaskNotices = [];
      const seen = new WeakSet<Element>();
      new MutationObserver(() => document.querySelectorAll('.el-message--success').forEach(node => {
        if (!seen.has(node) && /has completed the task|已完成任务/.test(node.textContent || '')) {
          seen.add(node); (window as any).__inquiryTaskNotices.push(node.textContent);
        }
      })).observe(document, { childList: true, subtree: true, characterData: true });
    });
    await service.install(page);
    const errors: string[] = [];
    page.on('pageerror', error => errors.push(error.message));
    try {
      await page.goto('/login');
      await page.locator('input[autocomplete="username"]').fill('fixture-user');
      await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
      await page.locator('button[type="submit"]').click();
      await page.waitForURL('**/app/**');
      await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
      const panel = page.locator('.inquiry-panel');
      await expect(panel).toBeVisible();
      await expect(panel).toContainText('Fixture choice?');
      await page.reload();
      await expect(panel).toBeVisible();
      expect(await page.evaluate(() => (window as any).__inquiryTaskNotices)).toEqual([]);
      const send = page.getByTestId('chat-composer-send');
      if (action === 'stop') {
        await expect(send).toHaveAttribute('data-mode', 'stop');
        await send.click();
        const confirm = page.locator('.mz-dialog-foot .is-primary');
        if (await confirm.isVisible()) await confirm.click();
        await expect(send).toHaveAttribute('data-mode', 'send');
        await expect.poll(() => service.turns[0].status).toBe('cancelled');
      } else {
        service.holdNext = true;
        if (action === 'answer') await panel.locator('.inquiry-option').first().click();
        else await page.getByTestId('chat-composer-input').fill('Fixture free reply');
        await send.click();
        await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 2.');
        expect(await page.evaluate(() => (window as any).__inquiryTaskNotices)).toEqual([]);
        service.release();
        await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 2.');
        await expect(send).toHaveAttribute('data-mode', 'send');
        expect(service.turns[0].status).toBe('completed');
        expect(service.turns[1].content).toContain(action === 'answer' ? 'First fixture option' : 'Fixture free reply');
      }
      await expect(panel).toHaveCount(0);
      await page.reload();
      await expect(panel).toHaveCount(0);
      await expect(send).toHaveAttribute('data-mode', 'send');
      expect(errors).toEqual([]);
      expect(service.failures).toEqual([]);
    } finally { await service.dispose(); }
  });
}
