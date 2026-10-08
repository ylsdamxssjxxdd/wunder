import { expect, test } from '@playwright/test';

const readState = async (page) =>
  JSON.parse(await page.getByTestId('messenger-send-guard-state').textContent() || '{}');

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/messenger-send-guard');
  await expect(page.getByTestId('messenger-send-guard-harness')).toBeVisible();
});

test('assistant pending bubble remains visible while send foreground lock bridges a session identity glitch', async ({ page }) => {
  await page.getByTestId('simulate-send-glitch').click();

  await expect(page.getByTestId('send-guard-item:assistant-pending')).toBeVisible();
  await expect(page.getByTestId('send-guard-empty')).toHaveCount(0);

  const state = await readState(page);
  expect(state.hasRetainedMessageConversationContext).toBe(true);
  expect(state.isAgentConversationActive).toBe(true);
  expect(state.foregroundLock).toBe(false);
  expect(state.messageCount).toBe(2);
  expect(state.keys).toContain('assistant-pending');
});
