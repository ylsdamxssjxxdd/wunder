import { expect, test } from '@playwright/test';

test('reasoning streams update one bounded preview before and after reload', async ({ page }) => {
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
  for (let pass = 0; pass < 2; pass++) {
    if (pass) await page.reload();
    await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
    const result = await page.evaluate(() =>
      (window as any).__messengerViewPerformanceE2E.runReasoningProbe());
    // 时间线里的「有界预览」是思考条目摘要（160 字上限）。
    expect(result.previewLength).toBeLessThanOrEqual(650);
    // 流式推理必须真的进 DOM（展开后的思考正文含最后一帧）。
    expect(result.bodyContainsLastChunk).toBe(true);
    expect(result.panelUpdates).toBeLessThan(10);
    expect(result.contentFlushes).toBe(0);
    expect(result.busy).toBe(false);
  }
});
