import { expect, test } from '@playwright/test';

import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('the newest work process ships expanded while older rounds stay folded', async ({ page }) => {
  const service = new ChatMockService();
  service.seedProcessExportFixture(true);
  service.seedProcessExportFixture(true);
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const older = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
    const newest = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-2"]');

    // 打开线程即读到「刚发生了什么」：最新一轮的工作过程已经铺开，条目在 DOM 里。
    const newestHead = newest.locator('.timeline-process-head');
    await expect(newestHead).toHaveAttribute('aria-expanded', 'true');
    await expect(newest.locator('.timeline-process-body .tl-entry').first()).toBeVisible();

    // 更早的轮次保持收起，收起时折叠体不进 DOM。
    const olderHead = older.locator('.timeline-process-head');
    await expect(olderHead).toHaveAttribute('aria-expanded', 'false');
    await expect(older.locator('.timeline-process-body')).toHaveCount(0);

    // 用户点一下仍是收起，默认值不会把它再拉开。
    await newestHead.click();
    await expect(newestHead).toHaveAttribute('aria-expanded', 'false');
    await expect(newest.locator('.timeline-process-body')).toHaveCount(0);
    await expect(olderHead).toHaveAttribute('aria-expanded', 'false');
  } finally {
    await service.dispose();
  }
});
