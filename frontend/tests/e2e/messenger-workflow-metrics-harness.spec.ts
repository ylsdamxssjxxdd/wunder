import { expect, test } from '@playwright/test';

test('real workflow row and composer render request metrics, zero reset, and subsequent stream observations', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify({data:{items:[]}})});
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.workflowMetrics.start());
  const workflow = page.locator('details.message-tool-workflow').last();
  await expect(workflow).toBeVisible();
  if (!(await workflow.evaluate(node => (node as HTMLDetailsElement).open))) {
    await workflow.locator(':scope > summary').click();
  }
  const row = page.locator('details.tool-workflow-entry').filter({has:page.locator('.tool-workflow-entry-duration', {hasText:'0ms'})});
  await expect(row).toHaveCount(1);
  await expect(row.locator('.tool-workflow-entry-context')).toHaveText('500 token');
  await row.scrollIntoViewIfNeeded();
  await page.screenshot({path:'../temp_dir/workflow-tool-metrics.png'});
  const context = page.locator('.chat-composer-world-context-usage').last();
  await expect(context).toHaveText('500/1000');
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.workflowMetrics.compact());
  await expect(context).toHaveText('0/1000');
  await expect(page.locator('details.tool-workflow-entry').last()
    .locator('.tool-workflow-entry-context')).toHaveText('0 token');
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.workflowMetrics.observe());
  await expect(context).toHaveText('120/1000');
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.workflowMetrics.delta());
  await expect(context).toHaveText('120/1000');
  await expect(row.locator('.tool-workflow-entry-context')).toHaveText('500 token');
  await page.screenshot({path:'../temp_dir/workflow-metrics.png'});
});
