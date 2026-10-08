import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('cancelled durable blocks and both workspace images survive reload and page return', async ({ page }) => {
  test.setTimeout(90_000);
  const service = new ChatMockService();
  for (let round = 1; round <= 3; round++) {
    const turn_id = `recovery-turn-${round}`;
    const status = round === 1 ? 'cancelled' : 'completed';
    service.turns.push({ turn_id, user_round: round, status });
    service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id,
      kind: 'user_message', role: 'user', content: `Fixture request ${round}.`,
      status: 'completed', visibility: 'user', revision: 1 });
    const item_id = `${turn_id}:text-1`;
    service.items.set(item_id, { item_id, turn_id, kind: 'assistant_message', role: 'assistant',
      model_round: 1, status, revision: 2, visibility: 'user',
      content: round === 1 ? '' : `![Fixture image ${round}](fixture-${round}.png)` });
    if (round === 1) service.blocks.set(item_id, {
      event: 'thread_item_block', item_id, field: 'content', block_index: 0,
      data: { item_id, turn_id, model_round: 1, field: 'content', block_index: 0,
        content_offset: 0, content: 'Retained partial reply 🐣.' }
    });
  }
  await service.install(page);
  let downloads = 0;
  let downloadGate: Promise<void> | undefined;
  let releaseDownloads: (() => void) | undefined;
  await page.route('**/workspace/download?**', async route => {
    downloads++;
    await downloadGate;
    await route.fulfill({ contentType: 'image/png', body: readFileSync('tests/e2e/fixtures/preview-wide.png') });
  });
  try {
    await page.setViewportSize({ width: 1600, height: 1200 });
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const assertRestored = async () => {
      const cancelled = page.locator('.messenger-message[data-turn-id="recovery-turn-1"]:not(.mine)');
      await expect(cancelled).toContainText('Retained partial reply 🐣.');
      await expect(cancelled).toHaveAttribute('data-message-status', 'cancelled');
      await expect(page.locator('.ai-resource-card[data-workspace-state="ready"]')).toHaveCount(2);
      await expect.poll(() => page.locator('.ai-resource-card img').evaluateAll(images =>
        images.filter(image => (image as HTMLImageElement).naturalWidth > 0).length)).toBe(2);
      await expect(page.locator('.messenger-message[data-turn-id^="recovery-turn-"]:not(.mine)')).toHaveCount(3);
    };
    await assertRestored();
    await page.locator('.ai-resource-card img').first().click();
    const preview = page.locator('.messenger-image-preview-dialog .zoomable-image');
    await expect(preview).toBeVisible();
    const ratio = () => preview.evaluate(image => {
      const rect = image.getBoundingClientRect();
      return rect.width / rect.height;
    });
    await expect.poll(ratio).toBeCloseTo(3, 1);
    await page.locator('.messenger-image-preview-dialog .zoomable-image-btn--label').click();
    await expect.poll(ratio).toBeCloseTo(3, 1);
    await page.locator('.messenger-image-preview-dialog .messenger-dialog-close').click();
    for (let attempt = 0; attempt < 2; attempt++) {
      // 离开聊天区再返回（原「资料页 → 左栏导航返回」在两栏壳体里已不存在；
      // 现在的等价路径是全屏设置覆盖层，覆盖层挂载时聊天面板整体卸载）。
      await page.locator('.messenger-sidebar-settings').first().click();
      await expect(page.locator('[data-testid="messenger-settings"]')).toBeVisible();
      await expect(page.locator('.ai-resource-card')).toHaveCount(0);
      await page.locator('[data-testid="settings-back"]').first().click();
      await expect(page.locator('[data-testid="messenger-settings"]')).toHaveCount(0);
      await assertRestored();
    }
    await page.reload();
    await assertRestored();
    expect(downloads).toBeGreaterThanOrEqual(4);

    // Leave while both downloads are pending, then let abandoned requests settle
    // alongside the replacement page requests. Neither card may get stuck.
    const beforeDelayedReload = downloads;
    downloadGate = new Promise<void>(resolve => { releaseDownloads = resolve; });
    await page.reload();
    await expect.poll(() => downloads).toBeGreaterThanOrEqual(beforeDelayedReload + 2);
    await page.locator('.messenger-sidebar-settings').first().click();
    await expect(page.locator('[data-testid="messenger-settings"]')).toBeVisible();
    await expect(page.locator('.ai-resource-card')).toHaveCount(0);
    await page.locator('[data-testid="settings-back"]').first().click();
    await expect(page.locator('[data-testid="messenger-settings"]')).toHaveCount(0);
    downloadGate = undefined;
    releaseDownloads?.();
    await assertRestored();
  } finally {
    releaseDownloads?.();
    await service.dispose();
  }
});
