import { expect, test } from '@playwright/test';

import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('new live reasoning is appended after durable process entries', async ({ page }) => {
  const service = new ChatMockService();
  service.seedDeferredReasoningTurn();
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const bubble = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]');
    const process = bubble.locator('.timeline-process-head');
    await expect(process).toBeVisible();
    await process.click();

    const thoughts = bubble.locator('.tl-thinking');
    await expect(thoughts).toHaveCount(2);
    await expect(thoughts.nth(0)).toContainText('Earlier completed thought.');
    await expect(thoughts.nth(0)).not.toContainText('New live thought.');
    await expect(thoughts.nth(1)).toContainText('New live thought.');
    await expect(thoughts.nth(1)).toHaveClass(/is-streaming/);
  } finally {
    await service.dispose();
  }
});
