import { expect, test } from '@playwright/test';
import { ChatMockService } from '../support/chatMockService';

test('disabled channel runtime opens settings with a clear status', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  await page.route('**/wunder/channels/**', async (route) => {
    const path = new URL(route.request().url()).pathname;
    const data = path.endsWith('/accounts')
      ? { items: [], supported_channels: [{ channel: 'qqbot', display_name: 'Fixture channel' }], runtime_enabled: false }
      : { items: [], total: 0, status: {}, runtime_enabled: false };
    await route.fulfill({ json: { data } });
  });
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.getByRole('button', { name: 'Agents', exact: true }).click();
    await page.locator('.messenger-inline-actions--agent-settings').getByRole('button', { name: 'Channels', exact: true }).click();
    const panel = page.locator('.channel-manager-page');
    await expect(panel.getByRole('status')).toContainText('The channel service is disabled');
    await expect(panel.locator('.channel-sidebar-actions button').first()).toBeEnabled();
    await panel.locator('.channel-sidebar-actions button').first().click();
    await expect(panel.locator('.channel-create-card')).toBeVisible();
    await expect(page.locator('.el-message--error')).toHaveCount(0);
    await panel.locator('.channel-sidebar-actions button').last().click();
    await expect(panel.getByRole('status')).toContainText('The channel service is disabled');
  } finally { await service.dispose(); }
});
