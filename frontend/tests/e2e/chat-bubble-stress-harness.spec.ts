import { expect, test } from '@playwright/test';

const readState = async (page) =>
  JSON.parse(await page.getByTestId('chat-bubble-stress-state').textContent() || '{}');

const runHarnessScenario = async (page, count: number, intensity: number) => {
  await page.evaluate(
    async ({ count, intensity }) => {
      await window.__chatBubbleStressE2E.loadScenario(count, intensity);
      await window.__chatBubbleStressE2E.runScrollProbe();
    },
    { count, intensity }
  );
};

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/chat-bubble-stress');
  await expect(page.getByTestId('chat-bubble-stress-harness')).toBeVisible();
});

test('large markdown bubbles still render and scroll without catastrophic stalls', async ({ page }) => {
  await page.getByTestId('scenario-load-120-huge').click();

  await expect(page.getByTestId('chat-bubble-item:stress-119')).toBeVisible();
  const loaded = await readState(page);
  expect(loaded.perf.messageCount).toBe(120);
  expect(loaded.perf.renderDurationMs).toBeLessThan(5000);
  expect(loaded.perf.scrollHeight).toBeGreaterThan(loaded.perf.clientHeight);

  await page.getByTestId('probe-scroll-jank').click();
  const scrolled = await readState(page);
  expect(scrolled.perf.scrollProbeDurationMs).toBeLessThan(4000);
  expect(scrolled.perf.scrollProbeMaxFrameGapMs).toBeLessThan(250);
});

test('500 huge bubbles still stay within the non-catastrophic interaction envelope', async ({ page }) => {
  await runHarnessScenario(page, 500, 12);

  await expect(page.getByTestId('chat-bubble-item:stress-499')).toBeVisible();
  const state = await readState(page);
  expect(state.perf.messageCount).toBe(500);
  expect(state.perf.renderDurationMs).toBeLessThan(4500);
  expect(state.perf.scrollProbeDurationMs).toBeLessThan(2000);
  expect(state.perf.scrollProbeMaxFrameGapMs).toBeLessThan(180);
  expect(state.perf.scrollHeight).toBeGreaterThan(3_000_000);
});

test('appending more huge bubbles keeps the page interactive', async ({ page }) => {
  await page.getByTestId('scenario-load-120-huge').click();
  await page.getByTestId('append-20-huge').click();

  await expect(page.getByTestId('chat-bubble-item:stress-139')).toBeVisible();
  const appended = await readState(page);
  expect(appended.perf.messageCount).toBe(140);
  expect(appended.perf.appendDurationMs).toBeLessThan(3000);
  expect(appended.sample[0].contentLength).toBeGreaterThan(4000);
  expect(appended.sample[0].htmlLength).toBeGreaterThan(5000);
});
