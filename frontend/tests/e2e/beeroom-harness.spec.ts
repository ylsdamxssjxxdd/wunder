import { expect, test, type Page } from '@playwright/test';

const getScrollSnapshot = async (page: Page) =>
  page.getByTestId('beeroom-chat-stream').evaluate((element) => {
    const target = element as HTMLElement;
    return {
      scrollTop: Math.round(target.scrollTop || 0),
      scrollHeight: Math.round(target.scrollHeight || 0),
      clientHeight: Math.round(target.clientHeight || 0),
      remaining: Math.round((target.scrollHeight || 0) - (target.clientHeight || 0) - (target.scrollTop || 0))
    };
  });

const getHarnessState = async (page: Page) => JSON.parse(await page.getByTestId('beeroom-e2e-state').innerText());

test.beforeEach(async ({ page }) => {
  await page.goto('/__e2e/beeroom-harness');
  await expect(page.getByTestId('beeroom-e2e-harness')).toBeVisible();
});

test('swarm worker keeps its own node and main session while a real subagent is projected separately', async ({
  page
}) => {
  await page.getByTestId('scenario-real-subagent-running').click();

  const workerNode = page.locator('[data-node-id="agent:worker-agent-1"]');
  const subagentNode = page.locator('[data-node-id="subagent:sess_subagent_real"]');

  await expect(workerNode).toBeVisible();
  await expect(workerNode).toHaveAttribute('data-node-role', 'worker');
  await expect(subagentNode).toBeVisible();
  await expect(subagentNode).toHaveAttribute('data-node-role', 'subagent');

  const state = await getHarnessState(page);
  expect(state.dispatchPreview?.sessionId).toBe('sess_worker_main');
  expect(state.dispatchPreview?.targetAgentId).toBe('worker-agent-1');
  expect(state.dispatchPreview?.subagents).toEqual([
    {
      sessionId: 'sess_subagent_real',
      status: 'running'
    }
  ]);
});

test('chat stream keeps manual scroll position when new messages arrive', async ({ page }) => {
  await page.getByTestId('scenario-long-thread').click();
  const stream = page.getByTestId('beeroom-chat-stream');
  await stream.evaluate((element) => {
    (element as HTMLElement).scrollTop = 0;
  });
  const before = await getScrollSnapshot(page);
  expect(before.remaining).toBeGreaterThan(200);

  await page.getByTestId('append-tail-message').click();

  const after = await getScrollSnapshot(page);
  expect(after.scrollTop).toBeLessThanOrEqual(16);
  expect(after.remaining).toBeGreaterThan(200);
});

test('chat stream returns to bottom after collapse and expand', async ({ page }) => {
  await page.getByTestId('scenario-long-thread').click();
  const stream = page.getByTestId('beeroom-chat-stream');
  await stream.evaluate((element) => {
    (element as HTMLElement).scrollTop = 0;
  });

  await page.getByTestId('collapse-chat').click();
  await page.getByTestId('expand-chat').click();

  const snapshot = await getScrollSnapshot(page);
  expect(snapshot.remaining).toBeLessThanOrEqual(12);
});

test('chat side panel shows default actions and composer when optional props are omitted', async ({ page }) => {
  await page.getByTestId('scenario-idle').click();

  await expect(page.getByTestId('beeroom-chat-textarea')).toBeVisible();
  await expect(page.getByTestId('beeroom-chat-send')).toBeVisible();
  await expect(page.locator('.beeroom-canvas-icon-btn')).toHaveCount(2);
});

test('canvas ignores swarm worker shadow sessions and only projects real subagents', async ({ page }) => {
  await page.getByTestId('scenario-worker-shadow').click();
  await expect(page.locator('[data-node-role="subagent"]')).toHaveCount(0);

  await page.getByTestId('scenario-real-subagent-running').click();
  const subagentNode = page.locator('[data-node-id="subagent:sess_subagent_real"]');
  await expect(subagentNode).toBeVisible();
  await expect(subagentNode).toHaveAttribute('data-node-status', 'running');
  await expect(subagentNode).toHaveAttribute('data-node-emphasis', 'active');

  await page.getByTestId('scenario-real-subagent-dormant').click();
  await expect(subagentNode).toBeVisible();
  await expect(subagentNode).toHaveAttribute('data-node-status', 'completed');
  await expect(subagentNode).toHaveAttribute('data-node-emphasis', 'dormant');
});

test('subagent flow keeps the rendered sender order stable', async ({ page }) => {
  await page.getByTestId('scenario-real-subagent-dormant').click();
  const senders = await page.locator('[data-message-key]').evaluateAll((elements) =>
    elements.map((element) => ({
      key: (element as HTMLElement).dataset.messageKey || '',
      sender: (element as HTMLElement).dataset.senderName || '',
      tone: (element as HTMLElement).dataset.messageTone || ''
    }))
  );
  expect(senders.map((item) => item.key)).toEqual([
    'real:user',
    'real:mother',
    'real:subagent-request',
    'real:subagent-reply',
    'real:mother-final'
  ]);
  expect(senders.map((item) => item.sender)).toEqual([
    '用户',
    '默认智能体',
    '工蜂一号',
    '子智能体',
    '默认智能体'
  ]);
});
