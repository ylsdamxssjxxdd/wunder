import { expect, test } from '@playwright/test';

test.describe.configure({ mode: 'serial' });

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/chat-compaction-harness');
  await expect(page.getByTestId('chat-compaction-e2e-harness')).toBeVisible();
});

test('manual compaction uses an assistant bubble and never renders a divider', async ({ page }) => {
  await page.getByTestId('scenario-manual-running').click();

  const divider = page.getByTestId('chat-compaction-divider');
  await expect(divider).toHaveCount(0);
  await expect(page.locator('[data-role="assistant"]').last()).toBeVisible();

  await page.getByTestId('hydrate-manual-terminal').click();
  await expect(divider).toHaveCount(0);
  await expect(page.locator('[data-role="assistant"]').last()).toContainText('cmp-terminal');

  await page.getByTestId('append-next-turn-busy').click();
  await expect(divider).toHaveCount(0);
});

test('rehydration after a new turn keeps manual compaction out of divider layout', async ({ page }) => {
  await page.getByTestId('scenario-manual-running').click();
  await page.getByTestId('hydrate-manual-terminal').click();
  await page.getByTestId('append-next-turn-busy').click();
  await page.getByTestId('rehydrate-after-next-turn').click();

  const dividers = page.getByTestId('chat-compaction-divider');
  await expect(dividers).toHaveCount(0);
});

test('failed manual compaction remains terminal and shows failure details', async ({ page }) => {
  await page.getByTestId('scenario-failed').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveCount(0);
  await expect(detail).toHaveAttribute('data-compaction-detail-status', 'failed');
  await expect(detail).toContainText('CONTEXT_WINDOW_EXCEEDED');
  await expect(detail).toContainText('still exceeds the context limit');
});

test('cancelled manual compaction remains terminal while a later turn is busy', async ({ page }) => {
  await page.getByTestId('scenario-cancelled').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveCount(0);
  await expect(detail).toHaveAttribute('data-compaction-detail-status', 'cancelled');

  await page.getByTestId('append-next-turn-busy').click();
  await expect(divider).toHaveCount(0);
});

test('legacy manual compaction displays its persisted injected summary without a divider', async ({ page }) => {
  await page.getByTestId('scenario-legacy-summary').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveCount(0);
  await expect(detail).toHaveAttribute('data-compaction-detail-status', 'completed');
  await expect(detail).toContainText('Persisted summary text from an older event.');
});

test('bounded patch preview shows exact totals and marks omitted lines', async ({ page }) => {
  const preview = page.getByTestId('developer-tool-preview');
  const workflow = preview.locator('details.message-tool-workflow');
  if (!(await workflow.evaluate((node) => (node as HTMLDetailsElement).open))) {
    await workflow.locator(':scope > summary').click();
  }
  const entry = preview.locator('details.tool-workflow-entry').first();
  if (!(await entry.evaluate((node) => (node as HTMLDetailsElement).open))) {
    await entry.locator(':scope > summary').click();
  }
  await expect(preview.locator('.tool-workflow-patch-metrics')).toContainText('10000');
  await expect(preview.locator('.tool-workflow-patch-preview-limit')).toContainText(/no files were written|未写入文件/);
  await expect(preview.locator('.tool-workflow-patch-line.is-note').last()).toContainText('9920');
  expect(await preview.locator('.tool-workflow-patch-line').count()).toBeLessThanOrEqual(82);
  await preview.screenshot({ path: '../temp_dir/developer-tool-preview.png' });
});
