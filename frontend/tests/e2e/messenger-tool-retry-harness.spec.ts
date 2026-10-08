import { expect, test } from '@playwright/test';

test.beforeEach(async ({ page }) => {
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
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.start());
});

test('tool retry is visible with existing reasoning and clears after recovered output', async ({ page }) => {
  const status = page.locator('.messenger-message-stat.is-status').last();
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.retry());
  await expect(status).toContainText(/Invalid tool call|工具调用格式错误/);
  await expect(status).toHaveClass(/is-warning/);
  await page.screenshot({ path: '../temp_dir/messenger-tool-retry.png' });
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.recover());
  await expect(status).toContainText(/Model outputting|模型输出中/);
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.finish('completed'));
  await expect(status).not.toHaveClass(/is-live/);
});

for (const terminal of ['failed', 'cancelled']) {
  test(`${terminal} clears the spinner in the real message panel`, async ({ page }) => {
    const status = page.locator('.messenger-message-stat.is-status').last();
    await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.retry());
    await expect(status).toContainText(/Invalid tool call|工具调用格式错误/);
    await page.evaluate(value => (window as any).__messengerViewPerformanceE2E.toolRetry.finish(value), terminal);
    await expect(status).not.toHaveClass(/is-live/);
    await expect(status).not.toContainText(/retrying|重试中|正在重试|模型输出中/i);
    expect(await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.toolRetry.busy())).toBe(false);
  });
}
