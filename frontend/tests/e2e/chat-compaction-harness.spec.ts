import { expect, test } from '@playwright/test';

test.describe.configure({ mode: 'serial' });

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

test('failed compaction remains failed and shows the failure details', async ({ page }) => {
  await page.getByTestId('scenario-failed').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveAttribute('data-compaction-status', 'failed');
  await expect(detail).toHaveAttribute('data-compaction-detail-status', 'failed');
  await expect(detail).toContainText('CONTEXT_WINDOW_EXCEEDED');
  await expect(detail).toContainText('still exceeds the context limit');
});

test('cancelled compaction remains terminal while a later turn is busy', async ({ page }) => {
  await page.getByTestId('scenario-cancelled').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveAttribute('data-compaction-status', 'cancelled');
  await expect(detail).toHaveAttribute('data-compaction-detail-status', 'cancelled');

  await page.getByTestId('append-next-turn-busy').click();
  await expect(divider).toHaveAttribute('data-compaction-status', 'cancelled');
});

test('legacy events display their persisted injected summary', async ({ page }) => {
  await page.getByTestId('scenario-legacy-summary').click();

  const divider = page.getByTestId('chat-compaction-divider');
  const detail = page.getByTestId('chat-compaction-detail');
  await expect(divider).toHaveAttribute('data-compaction-status', 'completed');
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
