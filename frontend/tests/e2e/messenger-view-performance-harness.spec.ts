import { expect, test } from '@playwright/test';

type HarnessApi = {
  runTwoTurnProbe: () => Promise<{ turns: number; toolCalls: number; maxFrameGapMs: number; maxNodes: number; retainedWorkflowCounts: number[] }>;
  installWorkflowHistory: () => Promise<void>;
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
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
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
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
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

test('two tool-heavy turns keep streaming, typing and refreshed scrolling responsive', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) {
      await route.continue();
      return;
    }
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  const metrics = await page.evaluate(() =>
    (window as unknown as { __messengerViewPerformanceE2E: HarnessApi }).__messengerViewPerformanceE2E.runTwoTurnProbe());
  console.log('two-turn probe', JSON.stringify(metrics));
  expect(metrics.turns).toBe(2);
  expect(metrics.toolCalls).toBe(160);
  expect(metrics.retainedWorkflowCounts).toEqual([1, 2]);
  expect(metrics.maxNodes).toBeLessThan(5000);
  expect(metrics.maxFrameGapMs).toBeLessThan(150);
  await page.reload();
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi })
    .__messengerViewPerformanceE2E.runScrollProbe());
  const refreshed = await readMetrics(page);
  expect(refreshed.maxFrameGapMs).toBeLessThan(150);
  expect(refreshed.domNodeCount).toBeLessThan(5000);
});


test('historical workflow shells survive reload and collapsed calls expose arguments directly', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) {
      await route.continue();
      return;
    }
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as unknown as { __messengerViewPerformanceE2E: HarnessApi })
    .__messengerViewPerformanceE2E.installWorkflowHistory());
  for (let pass = 0; pass < 2; pass++) {
    if (pass) {
      await page.reload();
      await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
    }
    await expect(page.locator('.message-tool-workflow')).toHaveCount(2);
    await expect(page.locator('.tool-workflow-entry')).toHaveCount(0);
    const history = page.locator('[data-virtual-key="runtime:assistant:workflow-message-1"] .message-tool-workflow');
    await history.locator(':scope > summary').click();
    await expect(history.locator('.tool-workflow-entry')).toHaveCount(40);
    const entry = history.locator('.tool-workflow-entry').last();
    await expect(entry).not.toHaveAttribute('open');
    await entry.locator('summary').click({ button: 'right' });
    await expect(page.locator('.tool-workflow-debug-floating')).toContainText('259');
    await expect(entry).not.toHaveAttribute('open');
    await expect(history.locator('.tool-workflow-entry-body')).toHaveCount(0);
    await page.keyboard.press('Escape');
    await entry.locator('summary').click();
    await expect(entry.locator('.tool-workflow-entry-body')).toContainText('worker-preview-text');
    expect((await entry.locator('.tool-workflow-entry-body').innerText()).length).toBeLessThan(5000);
    await entry.locator('summary').click();
    // Every earlier call remains recoverable across the bounded source pages.
    for (let step = 0; step < 12 && await history.locator('.tool-workflow-load-earlier').count(); step++) {
      await history.locator('.tool-workflow-load-earlier').click();
    }
    await expect(history.locator('.tool-workflow-entry')).toHaveCount(260);
    const first = history.locator('.tool-workflow-entry').first();
    await first.locator('summary').click({ button: 'right' });
    await expect(page.locator('.tool-workflow-debug-floating')).toContainText(/"item":\s*0/);
    await page.keyboard.press('Escape');
    await first.locator('summary').click();
    await expect(first.locator('.tool-workflow-entry-body')).toHaveCount(1);
    await history.locator(':scope > summary').click();
    await expect(history.locator('.tool-workflow-entry')).toHaveCount(0);
    await history.locator(':scope > summary').click();
    await expect(first.locator('.tool-workflow-entry-body')).toHaveCount(1);
    await history.locator(':scope > summary').click();
  }
});


test('leaving messages unmounts chat rendering while the background reply survives return', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await expect(page.locator('.message-tool-workflow').first()).toBeAttached();
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  await expect(page.locator('.messenger-message-panel')).toHaveCount(0);
  await expect(page.locator('.message-tool-workflow')).toHaveCount(0);
  const busy = await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.streamInBackground());
  expect(busy).toBe(true);
  await expect(page.locator('.messenger-message')).toHaveCount(0);
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('messages'));
  await expect(page.locator('.messenger-message-panel')).toContainText('background-23');
  await expect(page.locator('.message-tool-workflow').first()).toBeAttached();
  expect(await page.locator('.messenger-message').count()).toBeLessThan(40);
});


test('shared render worker preserves rich content, bounds large previews and recovers from cancellation', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  const result = await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.runChatWorkerProbe());
  expect(result.detailChars).toBeGreaterThan(2_000_000);
  expect(result.frames).toBeGreaterThan(0);
  expect(result.previewChars).toBeLessThanOrEqual(24000);
  expect(result.clonedChars).toBeLessThan(40000);
  // Steady text goes to the body subscriber, without re-rendering the page shell.
  await page.evaluate(() => { (window as any).wunderPerf.start(); });
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.streamLatestMessage());
  const counters = await page.evaluate(() => (window as any).wunderPerf.snapshot().counters);
  expect(counters.chat_shell_render || 0).toBeLessThan(15);
});
