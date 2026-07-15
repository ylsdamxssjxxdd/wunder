import { expect, test } from '@playwright/test';

type HarnessApi = {
  runScrollProbe: () => Promise<void>;
  switchSessionAndReturn: () => Promise<void>;
  prependHistory: () => Promise<void>;
  streamLatestMessage: () => Promise<void>;
  streamToolOutputWhileTyping: () => Promise<void>;
  expandToolDetails: () => Promise<void>;
  showEarlierToolEntries: () => Promise<void>;
};

const readMetrics = async (page) =>
  JSON.parse(await page.getByTestId('messenger-view-performance-state').textContent() || '{}');

test('real MessengerView keeps a bounded DOM through long history, scroll and session return', async ({ page }) => {
  await page.route('**/*', async (route) => {
    const pathname = new URL(route.request().url()).pathname;
    if (!pathname.startsWith('/wunder/')) {
      await route.continue();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ data: { items: [] } })
    });
  });
  await page.goto('/__e2e/messenger-view-performance');
  await expect(page.getByTestId('messenger-view-performance-harness')).toBeVisible();
  await expect(page.getByTestId('messenger-view')).toBeVisible();
  await page.waitForFunction(() => Boolean((window as unknown as { __messengerViewPerformanceE2E?: HarnessApi }).__messengerViewPerformanceE2E));

  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.runScrollProbe());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.prependHistory());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.streamLatestMessage());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.streamToolOutputWhileTyping());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.expandToolDetails());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.showEarlierToolEntries());
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.switchSessionAndReturn());

  const metrics = await readMetrics(page);
  expect(metrics.firstInteractiveMs).toBeLessThan(8000);
  expect(metrics.maxFrameGapMs).toBeLessThan(250);
  expect(metrics.mountedMessageCount).toBeLessThan(40);
  expect(metrics.expandedToolCount).toBeLessThanOrEqual(3);
  expect(metrics.maxExpandedToolCount).toBeLessThanOrEqual(3);
  expect(metrics.availableToolSummaryCount).toBeGreaterThanOrEqual(0);
  expect(metrics.initialToolSummaryCount).toBeGreaterThan(0);
  // The active row remains mounted alongside the 40-entry virtual page.
  expect(metrics.initialToolSummaryCount).toBeLessThanOrEqual(41);
  expect(metrics.earlierToolSummaryCount).toBeGreaterThan(metrics.initialToolSummaryCount);
  expect(metrics.earlierToolSummaryCount).toBeLessThanOrEqual(81);
  expect(metrics.domNodeCount).toBeLessThan(5000);
  expect(metrics.requestCount).toBeLessThan(80);
  expect(metrics.historyBackfillCount).toBe(40);
  expect(metrics.streamedCharacters).toBeGreaterThan(0);
  expect(metrics.toolStreamUpdates).toBe(24);
  expect(metrics.streamingWorkflowShellVisible).toBe(true);
  expect(metrics.toolStreamFrameGapMs).toBeLessThan(250);
  expect(metrics.composerInputLatencyMs).toBeLessThan(5000);
});
