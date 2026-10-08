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
