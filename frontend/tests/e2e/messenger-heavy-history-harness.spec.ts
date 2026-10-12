import { expect, test } from '@playwright/test';

const readState = async (page) =>
  JSON.parse(await page.getByTestId('messenger-heavy-state').textContent() || '{}');

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/messenger-heavy-history');
  await expect(page.getByTestId('messenger-heavy-history-harness')).toBeVisible();
});

test('messenger-like heavy history keeps scroll runtime healthy under 400 large messages', async ({ page }) => {
  await page.getByTestId('load-400-history').click();
  await expect(page.getByTestId('messenger-heavy-item:heavy-399')).toBeVisible();

  const loaded = await readState(page);
  expect(loaded.perf.messageCount).toBe(400);
  expect(loaded.perf.loadDurationMs).toBeLessThan(6000);
  expect(loaded.flags.autoStickToBottom).toBe(true);
  expect(loaded.flags.showScrollBottomButton).toBe(false);

  await page.getByTestId('probe-scroll-runtime').click();
  const scrolled = await readState(page);
  expect(scrolled.perf.scrollProbeDurationMs).toBeLessThan(4000);
  expect(scrolled.perf.scrollProbeMaxFrameGapMs).toBeLessThan(220);
  expect(scrolled.flags.showScrollTopButton).toBe(true);
  expect(scrolled.flags.showScrollBottomButton).toBe(true);
});

test('reading old history and appending more messages does not force jump back to bottom', async ({ page }) => {
  await page.getByTestId('load-400-history').click();
  await page.getByTestId('jump-top').click();

  const beforeAppend = await readState(page);
  expect(beforeAppend.perf.scrollTop).toBeLessThanOrEqual(8);
  expect(beforeAppend.flags.autoStickToBottom).toBe(false);
  expect(beforeAppend.flags.showScrollBottomButton).toBe(true);

  await page.getByTestId('append-40-history').click();
  await expect(page.getByTestId('messenger-heavy-item:heavy-439')).toBeVisible();

  const afterAppend = await readState(page);
  expect(afterAppend.perf.messageCount).toBe(440);
  expect(afterAppend.perf.appendDurationMs).toBeLessThan(2500);
  expect(afterAppend.perf.scrollTop).toBeLessThanOrEqual(24);
  expect(afterAppend.flags.autoStickToBottom).toBe(false);
  expect(afterAppend.flags.showScrollBottomButton).toBe(true);
});

// W4-L gate: the perf capture surface records every registered heavy-history metric and still
// drops unknown names. Removing the chatPerfMetrics.ts whitelist registration turns this red.
test('heavy-history perf gate records registered metrics and drops unknown ones', async ({ page }) => {
  const report = await page.evaluate(async () => {
    const perf = await (async () => {
      const globalPerf = (window as Window & { wunderPerf?: unknown }).wunderPerf;
      if (globalPerf) return globalPerf as any;
      const module_ = await import('/src/utils/chatPerf.ts');
      return (module_ as any).chatPerf;
    })();
    perf.start();
    perf.recordSnapshotApply(184.5, { itemCount: 4800 });
    perf.recordThreadProjection(233.1, { roundCount: 480 });
    perf.recordVirtualWindowRecompute(12.4, { messageCount: 980 });
    perf.recordTrajectoryLayout(96.7, { itemCount: 1000 });
    perf.countSnapshotAbort(2);
    perf.countWorkflowEventsItems(25000);
    perf.recordDuration('heavy_history_projection_probe_ms', 5);
    perf.count('heavy_history_probe_counter', 5);
    const snapshot = perf.snapshot();
    perf.stop();
    return snapshot as {
      durations: Record<string, { count: number; maxMs: number } | undefined>;
      counters: Record<string, number | undefined>;
      summary: { heavyHistory: { snapshotAborts: number; workflowEventItems: number } };
    };
  });

  expect(report.durations.snapshot_apply_ms?.count).toBe(1);
  expect(report.durations.thread_projection_ms?.count).toBe(1);
  expect(report.durations.virtual_window_recompute_ms?.count).toBe(1);
  expect(report.durations.trajectory_layout_ms?.count).toBe(1);
  expect(report.counters.snapshot_abort).toBe(2);
  expect(report.counters.workflow_events_items).toBe(25000);
  expect(report.durations.heavy_history_projection_probe_ms).toBeUndefined();
  expect(report.counters.heavy_history_probe_counter).toBeUndefined();
  expect(report.summary.heavyHistory.snapshotAborts).toBe(2);
  expect(report.summary.heavyHistory.workflowEventItems).toBe(25000);
});
