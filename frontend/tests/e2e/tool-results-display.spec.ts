import { expect, test } from '@playwright/test';

test('expanded production tools retain content and hide internal payloads', async ({ page }) => {
  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    return route.fulfill({ contentType: 'application/json', body: '{"data":{"items":[]}}' });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.installToolResultsProbe());
  const probe = page.locator('#tool-results-probe');
  const panel = probe.locator('.message-tool-workflow');
  if (!await panel.evaluate(node => (node as HTMLDetailsElement).open)) await panel.locator(':scope > summary').click();
  const rows = probe.locator('.tool-workflow-entry');
  await expect(rows).toHaveCount(12);
  for (let index = 7; index < 10; index++) {
    const row = rows.nth(index);
    await expect(row.locator(':scope > summary')).toContainText('上下文压缩');
    await expect(row.locator(':scope > summary')).not.toContainText('tool_call');
    if (!await row.evaluate(node => (node as HTMLDetailsElement).open)) await row.locator(':scope > summary').click();
    await expect(row.locator(':scope > summary')).toContainText('上下文压缩');
    if (index === 8) await expect(row.locator('.tool-workflow-compaction')).toContainText('Retained compaction summary');
  }
  for (const [index, content] of [[10, 'Computed fixture result'], [11, 'Fixture preference']] as const) {
    const row = rows.nth(index);
    if (!await row.evaluate(node => (node as HTMLDetailsElement).open)) await row.locator(':scope > summary').click();
    await expect(row.locator('.tool-workflow-entry-body')).toContainText(content);
  }
  const expected = ['retained file content', 'written sample content', 'sample output', 'after', 'Useful custom result', 'Write denied', /running|运行/i];
  for (let index = 0; index < 7; index++) {
    const row = rows.nth(index);
    if (!await row.evaluate(node => (node as HTMLDetailsElement).open)) await row.locator(':scope > summary').click();
    const body = row.locator('.tool-workflow-entry-body');
    await expect(body).toBeVisible();
    await expect(body).toContainText(expected[index]);
    await expect(body).not.toContainText('hidden-handle');
    await expect(body).not.toContainText('must not show');
    if (index === 2) await expect(body).toContainText('echo sample');
    if (index === 3) {
      await expect(body.locator('.is-add')).toContainText('after');
      await expect(body.locator('.is-delete')).toContainText('before');
    }
    if (index === 3) await page.screenshot({ path: '../temp_dir/tool-results-web.png' });
  }
});
