import { expect, test } from '@playwright/test';
import { ChatMockService } from '../support/chatMockService';

/**
 * 渠道运行期未启用的状态自检。
 *
 * 入口改到全屏设置页（方案 §九）：左栏底部「设置」→ 分类 `channels`。
 * 原先的顶层 `Agents` 导航与 `.messenger-inline-actions--agent-settings` 内联动作
 * 在两栏壳体里已不存在，因此本用例只保留仍然有效的断言对象：
 * 渠道面板本身、它的状态文案、以及「运行期未启用时仍可打开创建卡片且不弹错误」。
 */
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

    // 设置覆盖层按需挂载分类；登录后的会话引导可能把 section 拉回 messages，
    // 因此允许重试点击（与 settings-page.spec.ts 同一竞态）。
    const overlay = page.locator('[data-testid="messenger-settings"]');
    await expect(async () => {
      if ((await overlay.count()) === 0) {
        await page.locator('.messenger-sidebar-settings').first().click();
      }
      await expect(overlay).toBeVisible({ timeout: 4000 });
      await page.waitForTimeout(400);
      await expect(overlay).toBeVisible({ timeout: 1000 });
    }).toPass({ timeout: 30_000, intervals: [400, 800, 1600] });

    await page.locator('[data-settings-category="channels"]').click();
    const panel = page.locator('.channel-manager-page');
    await expect(panel).toBeVisible({ timeout: 20_000 });
    await expect(panel.getByRole('status')).toContainText('The channel service is disabled');
    await expect(panel.locator('.channel-sidebar-actions button').first()).toBeEnabled();
    await panel.locator('.channel-sidebar-actions button').first().click();
    await expect(panel.locator('.channel-create-card')).toBeVisible();
    await expect(page.locator('.el-message--error')).toHaveCount(0);
    await panel.locator('.channel-sidebar-actions button').last().click();
    await expect(panel.getByRole('status')).toContainText('The channel service is disabled');
  } finally { await service.dispose(); }
});
