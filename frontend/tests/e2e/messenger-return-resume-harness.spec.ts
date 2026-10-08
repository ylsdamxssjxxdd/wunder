import { expect, test } from '@playwright/test';

const readState = async (page) =>
  JSON.parse(await page.getByTestId('messenger-return-resume-state').textContent() || '{}');

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/messenger-return-resume');
  await expect(page.getByTestId('messenger-return-resume-harness')).toBeVisible();
});

test('assistant pending bubble survives section switch away and back during send foreground lock', async ({ page }) => {
  await page.getByTestId('simulate-return-resume').click();

  await expect(page.getByTestId('return-resume-item:assistant-pending')).toBeVisible();
  await expect(page.getByTestId('return-resume-empty')).toHaveCount(0);

  const state = await readState(page);
  expect(state.activeSection).toBe('messages');
  expect(state.hasRetainedMessageConversationContext).toBe(true);
  expect(state.messageConversationKind).toBe('agent');
  expect(state.foregroundLock).toBe(false);
  expect(state.messageCount).toBe(2);
  expect(state.keys).toContain('assistant-pending');
});
