import { expect, test } from '@playwright/test';
import { readFile } from 'node:fs/promises';

test('performance capture starts in settings, downloads sanitized JSON, survives reload and stops', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  await expect(page.getByTestId('settings-category-general').getByText(/^(测试|Testing)$/)).toBeVisible();
  await expect(page.getByTestId('perf-start')).toBeVisible();
  await expect(page.getByTestId('perf-download')).toBeDisabled();
  await page.getByTestId('perf-start').click();
  await expect(page.getByTestId('perf-start')).toBeDisabled();
  await page.evaluate(() => {
    const capture = (window as any).wunderPerf;
    capture.recordDuration('chat_workflow_entries_build', 100, { content: 'private-value', sessionId: 'private-value', itemCount: 25 });
    capture.count('chat_watch_event', 10, { sessionId: 'private-value' });
  });
  // A real main-thread stall must appear independently of app timing instrumentation.
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.evaluate(() => { const end = performance.now() + 90; while (performance.now() < end) { /* deliberate stall */ } });
  await expect.poll(() => page.evaluate(() => (window as any).wunderPerf.snapshot().summary.responsiveness.gapsOver50Ms)).toBeGreaterThan(0);
  const downloadEvent = page.waitForEvent('download');
  await page.getByTestId('perf-download').click();
  const download = await downloadEvent;
  expect(download.suggestedFilename()).toMatch(/^wunder-performance-.*\.json$/);
  const text = await readFile((await download.path())!, 'utf8');
  const report = JSON.parse(text);
  expect(report.enabled).toBe(true);
  expect(report.counters.chat_watch_event).toBe(10);
  expect(report.summary.rendering.workflowBuild.maxMs).toBe(100);
  expect(text).not.toContain('private-value');
  expect(text.length).toBeLessThan(64000);
  await page.reload();
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  await expect(page.getByTestId('perf-start')).toBeDisabled();
  expect(await page.evaluate(() => (window as any).wunderPerf.snapshot().capture.pages)).toBe(2);
  await page.getByTestId('perf-stop').click();
  await expect(page.getByTestId('perf-stop')).toBeDisabled();
  await expect(page.getByTestId('perf-download')).toBeEnabled();
  const frozen = await page.evaluate(() => (window as any).wunderPerf.snapshot());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  expect(await page.evaluate(() => (window as any).wunderPerf.snapshot())).toEqual(frozen);
  await page.getByTestId('perf-start').click();
  expect(await page.evaluate(() => (window as any).wunderPerf.snapshot().counters.chat_watch_event || 0)).toBe(0);
  await page.getByTestId('perf-stop').click();
});

test('frontend debug switch records across settings navigation and exports fresh reports after stopping', async ({ page }, testInfo) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(String(error)));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', async route => {
    if (!new URL(route.request().url()).pathname.startsWith('/wunder/')) return route.continue();
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  const toggle = page.getByTestId('settings-debug-enabled');
  await expect(toggle).not.toBeChecked();
  await toggle.locator('..').click();
  await expect(toggle).toBeChecked();
  await expect(page.getByTestId('perf-start')).toBeDisabled();

  const log = async (event: string) => page.evaluate(async name => {
    const modulePath = '/src/utils/chatDebug.ts';
    const { chatDebugLog } = await import(modulePath);
    chatDebugLog('messenger.viewport', name, {
      status: 'running', messageCount: 4, contentLength: 128,
      content: 'fixture-private-value', session_id: 'fixture-private-value',
      tool: { args: { text: 'fixture-private-value' } }
    });
  }, event);
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('messages'));
  await log('fixture-first');
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  await expect(toggle).toBeChecked();

  const downloadReport = async () => {
    const downloaded = page.waitForEvent('download');
    await page.getByTestId('settings-debug-download').click();
    const download = await downloaded;
    expect(download.suggestedFilename()).toMatch(/^wunder-debug-.*\.json$/);
    const json = await readFile((await download.path())!, 'utf8');
    expect(json).not.toContain('fixture-private-value');
    return JSON.parse(json);
  };
  const first = await downloadReport();
  expect(first.performance.capture.startedAt).toBeTruthy();
  expect(first.debug.events.some((entry: any) => entry.event === 'fixture-first')).toBe(true);
  await page.reload();
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(() => (window as any).__messengerViewPerformanceE2E.setSection('more'));
  await expect(toggle).toBeChecked();
  await expect(page.getByTestId('perf-start')).toBeDisabled();
  await log('fixture-second');
  await toggle.locator('..').click();
  await expect(toggle).not.toBeChecked();
  await expect(page.getByTestId('perf-stop')).toBeDisabled();
  await log('fixture-disabled');
  const second = await downloadReport();
  expect(second.debug.enabled).toBe(false);
  expect(second.performance.enabled).toBe(false);
  expect(second.debug.events.some((entry: any) => entry.event === 'fixture-second')).toBe(true);
  expect(second.debug.events.some((entry: any) => entry.event === 'fixture-disabled')).toBe(false);
  await page.getByTestId('settings-debug-clear').click();
  expect(await page.evaluate(() => JSON.parse((window as any).__wunderChatDebugDump()))).toEqual([]);
  await page.getByTestId('messenger-test-tools').scrollIntoViewIfNeeded();
  await page.screenshot({ path: testInfo.outputPath('testing-controls.png') });
  expect(errors).toEqual([]);
});
