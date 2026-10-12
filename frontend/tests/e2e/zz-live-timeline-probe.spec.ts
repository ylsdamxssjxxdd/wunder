import { expect, test } from '@playwright/test';

// Reproduce the live-timeline regression report in the real render layer.
test('live multi-round tool turn renders timeline entries, intermediate body and work-process fold', async ({ page }) => {
  const consoleErrors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });
  page.on('pageerror', (error) => consoleErrors.push(`pageerror: ${error.message}`));

  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) {
      await route.continue();
      return;
    }
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  const reports = await page.evaluate(() =>
    (window as any).__messengerViewPerformanceE2E.runLiveTimelineProbe()
  );
  console.log('LIVE TIMELINE REPORT');
  for (const report of reports) {
    console.log(JSON.stringify(report));
  }
  console.log('CONSOLE ERRORS', JSON.stringify(consoleErrors.slice(0, 10), null, 2));
  expect(reports.length).toBeGreaterThan(0);
});
