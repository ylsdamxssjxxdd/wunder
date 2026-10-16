import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('composer shows durable plans before process hydration and keeps updates across reload', async ({ page }) => {
  const service = new ChatMockService();
  const turn = service.seedPlanTurn();
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
    const dock = page.locator('.status-dock');
    const first = dock.locator('.status-dock-step').nth(0);
    const second = dock.locator('.status-dock-step').nth(1);
    await expect(dock).toBeVisible();
    await expect(dock).toContainText('Fixture plan progress');
    await expect(first).toHaveClass(/is-in_progress/);
    await expect(second).toHaveClass(/is-pending/);
    await expect.poll(() => service.subscriptions.filter(sub => sub.display).length).toBeGreaterThan(0);
    service.updatePlanTurn(turn, true);
    await expect(first).toHaveClass(/is-completed/);
    await expect(second).toHaveClass(/is-in_progress/);
    await page.reload();
    await expect(dock).toBeVisible();
    await expect(first).toHaveClass(/is-completed/);
    await expect(second).toHaveClass(/is-in_progress/);
    await dock.locator('.status-dock-pop').hover();
    await dock.locator('.status-dock-pop-close').click();
    await expect(dock).toHaveCount(0);
    service.updatePlanTurn(turn);
    await expect(dock).toBeVisible();
    await expect(first).toHaveClass(/is-in_progress/);
    expect(errors).toEqual([]);
  } finally {
    await service.dispose();
  }
});
