import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

/** 真实会话链路：命令状态不改变开合，成功终态立即替换转圈。 */
test('execute_command stays folded across running and settled frames in the live timeline', async ({ page }) => {
  const service = new ChatMockService();
  const turn = service.seedRunningCommandTurn();
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const bubble = page.locator(
      `.messenger-turn[data-root-turn-id="${turn.turn_id}"] [data-turn-slot="assistant"]`
    );
    const entry = bubble.locator('.tl-entry.is-command').first();
    const head = entry.locator('.tl-entry-head');

    await expect(head).toBeVisible();
    await expect(head).toHaveAttribute('aria-expanded', 'false');
    await expect(entry.locator('.tl-entry-output')).toHaveCount(0);

    service.settleCommandTurn(turn);
    await expect(head).toHaveAttribute('aria-expanded', 'false');
    await expect(entry.locator('.tl-entry-output')).toHaveCount(0);
    await expect(entry.locator('.tl-entry-status .fa-check')).toHaveCount(1);
    await expect(entry.locator('.tl-entry-status .fa-xmark')).toHaveCount(0);
    await expect(entry.locator('.tl-entry-status .fa-spin')).toHaveCount(0);

    await head.click();
    await expect(head).toHaveAttribute('aria-expanded', 'true');
    service.settleCommandTurn(turn, 'Fixture output after review');
    await expect(head).toHaveAttribute('aria-expanded', 'true');
    await expect(entry.locator('.tl-entry-output')).toContainText('Fixture output after review');
    await expect(entry.locator('.tl-entry-status .fa-check')).toHaveCount(1);
  } finally {
    await service.dispose();
  }
});
