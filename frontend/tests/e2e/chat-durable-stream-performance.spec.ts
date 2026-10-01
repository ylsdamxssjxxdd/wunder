import { expect, test } from '@playwright/test';

type DurableProbe = {
  tokens: number;
  p95FrameLatencyMs: number;
  maxFrameLatencyMs: number;
  reconnectRecoveryMs: number;
  longTasksOver50Ms: number;
  inputApplied: boolean;
  scrollApplied: boolean;
  copyObserved: boolean;
  rendered: boolean;
};

test('durable 1000-token chat stream renders, resumes, and stays interactive', async ({ page }) => {
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
  const result = await page.evaluate<[], DurableProbe>(() =>
    (window as any).__messengerViewPerformanceE2E.runDurableStreamProbe());
  console.log('durable stream probe', JSON.stringify(result));

  expect(result.tokens).toBe(1000);
  expect(result.p95FrameLatencyMs).toBeLessThan(100);
  expect(result.reconnectRecoveryMs).toBeLessThan(1000);
  expect(result.longTasksOver50Ms).toBe(0);
  expect(result.inputApplied).toBe(true);
  expect(result.scrollApplied).toBe(true);
  expect(result.copyObserved).toBe(true);
  expect(result.rendered).toBe(true);
});
