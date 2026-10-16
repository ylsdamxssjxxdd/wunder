import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('goal command shows the goal bar, streams a reply, and follows goal lifecycle controls', async ({ page }) => {
  const service = new ChatMockService();
  service.holdNext = true;
  await page.addInitScript(() => {
    (window as any).__goalTaskNotices = [];
    const seen = new WeakSet<Element>();
    new MutationObserver(() => document.querySelectorAll('.el-message--success').forEach(node => {
      if (!seen.has(node) && /has completed the task|已完成任务/.test(node.textContent || '')) {
        seen.add(node); (window as any).__goalTaskNotices.push(node.textContent);
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
    const input = page.getByTestId('chat-composer-input');
    await expect(input).toBeVisible();
    const sent = page.waitForRequest(request => request.method() === 'PUT' && request.url().endsWith('/goal'));
    await input.fill('/goal Complete the example task');
    await input.press('Control+Enter');
    expect((await sent).postDataJSON()).toEqual({ objective: 'Complete the example task' });
    const bar = page.locator('.goal-bar');
    await expect(bar).toBeVisible();
    await expect(bar).toContainText('Complete the example task');
    await expect(bar).toContainText('0/8');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 1.');
    expect(await page.evaluate(() => (window as any).__goalTaskNotices)).toEqual([]);
    service.holdNext = true;
    await bar.locator('button[title="Edit goal"]').click();
    await page.locator('.el-message-box__input input').fill('Revise the example task');
    await page.locator('.el-message-box__btns .el-button--primary').click();
    await expect(bar).toContainText('Revise the example task');
    const original = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
    const revised = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-2"]');
    await expect(original).toContainText('Partial reply 1.');
    await expect(revised).toContainText('/goal edit Revise the example task');
    await expect(revised).toContainText('Partial reply 2.');
    await expect(original).not.toContainText('Partial reply 2.');
    expect(await page.evaluate(() => (window as any).__goalTaskNotices)).toEqual([]);
    await bar.locator('button[title="Pause goal"]').click();
    await expect(bar).toContainText('Paused goal');
    await expect(input).toBeEnabled();
    await expect.poll(() => service.turns.filter(turn => turn.goalRun).at(-1)?.status).toBe('cancelled');
    service.holdNext = true;
    await bar.locator('button[title="Resume"]').click();
    await expect(bar.locator('button[title="Pause goal"]')).toBeVisible();
    await page.reload();
    await expect(bar).toContainText('Revise the example task');
    await expect.poll(() => service.subscriptions.length).toBeGreaterThan(0);
    service.release();
    await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 4.');
    await expect(bar).toHaveCount(0);
    expect(errors).toEqual([]);
  } finally {
    await service.dispose();
  }
});
