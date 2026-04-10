import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/chat-compaction-harness');
  await expect(page.getByTestId('chat-compaction-e2e-harness')).toBeVisible();
});

test('manual compaction divider stays completed during the next busy turn', async ({ page }) => {
  await page.getByTestId('scenario-manual-running').click();

  const divider = page.getByTestId('chat-compaction-divider');
  await expect(divider).toHaveCount(1);
  await expect(divider).toHaveAttribute('data-compaction-status', 'running');

  await page.getByTestId('hydrate-manual-terminal').click();
  await expect(divider).toHaveCount(1);
  await expect(divider).toHaveAttribute('data-compaction-status', 'completed');
  await expect(divider).toContainText('16,249');
  await expect(divider).toContainText('5,670');

  await page.getByTestId('append-next-turn-busy').click();
  await expect(divider).toHaveCount(1);
  await expect(divider).toHaveAttribute('data-compaction-status', 'completed');
});

test('rehydration after a new turn does not create a duplicate compaction divider', async ({ page }) => {
  await page.getByTestId('scenario-manual-running').click();
  await page.getByTestId('hydrate-manual-terminal').click();
  await page.getByTestId('append-next-turn-busy').click();
  await page.getByTestId('rehydrate-after-next-turn').click();

  const dividers = page.getByTestId('chat-compaction-divider');
  await expect(dividers).toHaveCount(1);
  await expect(dividers.first()).toHaveAttribute('data-compaction-status', 'completed');
});
