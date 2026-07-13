import { expect, test, type Locator, type Page, type TestInfo } from '@playwright/test';

type RenderedMessage = {
  key: string;
  tone: string;
  sender: string;
  body: string;
};

const accessToken = String(process.env.BEEROOM_REAL_ACCESS_TOKEN || '').trim();
const firstGroupName = String(process.env.BEEROOM_REAL_GROUP_A || '').trim();
const secondGroupName = String(process.env.BEEROOM_REAL_GROUP_B || '').trim();

const requiredEnvironmentReady = Boolean(accessToken && firstGroupName && secondGroupName);

const groupRow = (page: Page, name: string) =>
  page.locator('.messenger-swarm-item').filter({ hasText: name }).first();

const chatStream = (page: Page) => page.getByTestId('beeroom-chat-stream');

const readMessages = async (stream: Locator): Promise<RenderedMessage[]> =>
  stream.locator('[data-message-key]').evaluateAll((elements) =>
    elements.map((element) => {
      const item = element as HTMLElement;
      return {
        key: item.dataset.messageKey || '',
        tone: item.dataset.messageTone || '',
        sender: item.dataset.senderName || '',
        body: item.querySelector('.beeroom-canvas-chat-bubble')?.textContent?.trim() || ''
      };
    })
  );

const attachRuntimeEvidence = async (
  testInfo: TestInfo,
  page: Page,
  evidence: Record<string, unknown>
) => {
  const messages = await readMessages(chatStream(page)).catch(() => []);
  await testInfo.attach('beeroom-real-service-evidence.json', {
    body: JSON.stringify({ ...evidence, url: page.url(), messages }, null, 2),
    contentType: 'application/json'
  });
};

const selectGroup = async (page: Page, name: string) => {
  const row = groupRow(page, name);
  await expect(row).toBeVisible();
  await row.click();
  await expect(row).toHaveClass(/\bactive\b/);
  await expect(page.getByTestId('beeroom-chat-textarea')).toBeVisible();
};

const sendMessage = async (page: Page, content: string) => {
  await page.getByTestId('beeroom-chat-textarea').fill(content);
  await page.getByTestId('beeroom-chat-send').click();
  await expect(
    chatStream(page).locator('[data-message-tone="user"] .beeroom-canvas-chat-bubble', {
      hasText: content
    })
  ).toHaveCount(1);
};

const waitForComposerIdle = async (page: Page) => {
  await expect(page.getByTestId('beeroom-chat-send')).not.toHaveClass(/\bis-stop\b/, {
    timeout: 120_000
  });
};

const messageIndex = (messages: RenderedMessage[], text: string) =>
  messages.findIndex((message) => message.body.includes(text));

const waitForVisibleMessage = async (page: Page, content: string) => {
  await expect
    .poll(
      async () => (await readMessages(chatStream(page))).filter((message) => message.body.includes(content)).length,
      { timeout: 30_000 }
    )
    .toBe(1);
};

const waitForVisibleMotherReply = async (page: Page, userContent: string) => {
  await expect
    .poll(async () => {
      const messages = await readMessages(chatStream(page));
      const userIndex = messageIndex(messages, userContent);
      return userIndex < 0 ? 0 : messages.slice(userIndex + 1).filter((item) => item.tone === 'mother').length;
    }, { timeout: 30_000 })
    .toBeGreaterThanOrEqual(1);
};

const createFreshMotherSession = async (page: Page, groupName: string) =>
  page.evaluate(async ({ token, expectedGroupName }) => {
    const headers = {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json'
    };
    const groupsResponse = await fetch('/wunder/beeroom/groups', { headers });
    const groupsPayload = await groupsResponse.json();
    const group = Array.isArray(groupsPayload?.data?.items)
      ? groupsPayload.data.items.find((item: Record<string, unknown>) => item?.name === expectedGroupName)
      : null;
    const agentId = String(group?.mother_agent_id || '').trim();
    if (!agentId) throw new Error('Mother agent is unavailable for the test group.');

    const createResponse = await fetch('/wunder/chat/sessions', {
      method: 'POST',
      headers,
      body: JSON.stringify({ agent_id: agentId })
    });
    const createPayload = await createResponse.json();
    const sessionId = String(createPayload?.data?.id || '').trim();
    if (!sessionId) throw new Error('Fresh mother session was not created.');
    return sessionId;
  }, { token: accessToken, expectedGroupName: groupName });

test.skip(!requiredEnvironmentReady, 'real-service credentials and two isolated swarm groups are required');

test('real service keeps rapid multi-round sends ordered and isolated by swarm', async ({ page }, testInfo) => {
  const runId = `${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`;
  const firstTurn = `e2e-first-${runId}`;
  const secondTurn = `e2e-second-${runId}`;
  const otherGroupTurn = `e2e-other-${runId}`;
  const browserErrors: string[] = [];
  const motherSessionResponses: Array<Record<string, unknown>> = [];

  page.on('pageerror', (error) => browserErrors.push(`pageerror: ${error.message}`));
  page.on('console', (message) => {
    if (message.type() === 'error') browserErrors.push(`console: ${message.text()}`);
  });
  page.on('response', async (response) => {
    if (!response.url().includes('/mother-session')) return;
    const payload = await response.json().catch(() => null);
    motherSessionResponses.push({
      url: response.url(),
      status: response.status(),
      payload
    });
  });

  await page.addInitScript((token) => localStorage.setItem('access_token', token), accessToken);

  try {
    // Let the version migration finish before the authenticated navigation; a fresh browser profile
    // intentionally clears tokens once when it first observes a new application version.
    await page.goto('/login');
    await page.goto('/app/beeroom');
    await expect(page.getByTestId('messenger-view')).toBeVisible();

    await selectGroup(page, firstGroupName);
    await sendMessage(page, firstTurn);
    await waitForComposerIdle(page);

    let firstGroupMessages = await readMessages(chatStream(page));
    const firstUserIndex = messageIndex(firstGroupMessages, firstTurn);
    expect(firstUserIndex).toBeGreaterThanOrEqual(0);
    await waitForVisibleMotherReply(page, firstTurn);

    await sendMessage(page, secondTurn);

    // Switch while the second stream is active, then dispatch independently in the other swarm.
    await selectGroup(page, secondGroupName);
    await sendMessage(page, otherGroupTurn);
    await selectGroup(page, firstGroupName);
    await selectGroup(page, secondGroupName);
    // Re-entering a group first restores its cache, then fetches the authoritative session history.
    await waitForVisibleMessage(page, otherGroupTurn);
    await waitForVisibleMotherReply(page, otherGroupTurn);

    const secondGroupMessages = await readMessages(chatStream(page));
    expect(secondGroupMessages.filter((message) => message.body.includes(otherGroupTurn))).toHaveLength(1);
    expect(secondGroupMessages.some((message) => message.body.includes(firstTurn))).toBe(false);
    expect(secondGroupMessages.some((message) => message.body.includes(secondTurn))).toBe(false);

    await selectGroup(page, firstGroupName);
    await waitForVisibleMessage(page, secondTurn);
    await waitForVisibleMotherReply(page, secondTurn);

    firstGroupMessages = await readMessages(chatStream(page));
    expect(firstGroupMessages.filter((message) => message.body.includes(firstTurn))).toHaveLength(1);
    expect(firstGroupMessages.filter((message) => message.body.includes(secondTurn))).toHaveLength(1);
    expect(firstGroupMessages.some((message) => message.body.includes(otherGroupTurn))).toBe(false);

    const firstIndex = messageIndex(firstGroupMessages, firstTurn);
    const secondIndex = messageIndex(firstGroupMessages, secondTurn);
    expect(firstIndex).toBeGreaterThanOrEqual(0);
    expect(secondIndex).toBeGreaterThan(firstIndex);

    const repliesAfterFirst = firstGroupMessages
      .slice(firstIndex + 1, secondIndex)
      .filter((message) => message.tone === 'mother');
    const repliesAfterSecond = firstGroupMessages
      .slice(secondIndex + 1)
      .filter((message) => message.tone === 'mother');
    expect(repliesAfterFirst).toHaveLength(1);
    expect(repliesAfterSecond).toHaveLength(1);

    const boundSessions = motherSessionResponses
      .map((item) => {
        const payload = item.payload as { data?: { id?: unknown } } | null;
        return String(payload?.data?.id || '').trim();
      })
      .filter(Boolean);
    expect(new Set(boundSessions).size).toBeGreaterThanOrEqual(2);
  } finally {
    await attachRuntimeEvidence(testInfo, page, { browserErrors, motherSessionResponses, runId });
  }
});

test('fresh mother main session is not replaced by a stale swarm summary during send', async ({ page }, testInfo) => {
  const runId = `${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`;
  const message = `e2e-fresh-main-${runId}`;
  const motherSessionResponses: Array<Record<string, unknown>> = [];

  page.on('response', async (response) => {
    if (!response.url().includes('/mother-session')) return;
    motherSessionResponses.push({
      status: response.status(),
      payload: await response.json().catch(() => null)
    });
  });
  await page.addInitScript((token) => localStorage.setItem('access_token', token), accessToken);

  try {
    await page.goto('/login');
    await page.goto('/app/beeroom');
    await expect(page.getByTestId('messenger-view')).toBeVisible();

    // The page holds the old group summary before this creates a new main thread.
    const freshSessionId = await createFreshMotherSession(page, firstGroupName);
    await selectGroup(page, firstGroupName);
    await sendMessage(page, message);
    await expect
      .poll(() =>
        motherSessionResponses.some((item) => {
          const payload = item.payload as { data?: { id?: unknown } } | null;
          return String(payload?.data?.id || '').trim() === freshSessionId;
        })
      )
      .toBe(true);

    // The old implementation cleared this optimistic message within milliseconds.
    await page.waitForTimeout(250);
    await waitForVisibleMessage(page, message);
    const messages = await readMessages(chatStream(page));
    expect(messages.filter((item) => item.body.includes(message))).toHaveLength(1);
  } finally {
    await attachRuntimeEvidence(testInfo, page, { runId, motherSessionResponses });
  }
});
