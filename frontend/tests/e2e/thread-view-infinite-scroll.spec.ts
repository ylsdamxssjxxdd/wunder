import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('chat and trajectory fetch older pages from scroll edges without a load button', async ({ page }) => {
  test.setTimeout(120_000);
  const service = new ChatMockService();
  await service.install(page);
  for (let index = 0; index < 360; index += 1) service.scheduledTurn('completed');
  const pageErrors: string[] = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  const pages: Array<{ kind: string; query: URLSearchParams; data: any }> = [];
  page.on('response', async response => {
    const url = new URL(response.url());
    if (!/\/view\/(chat|trajectory)$/.test(url.pathname)) return;
    try { pages.push({ kind: url.pathname.endsWith('/chat') ? 'chat' : 'trajectory', query: url.searchParams, data: (await response.json()).data.page }); } catch {}
  });
  const scrollPage = async (selector: string, kind: string, direction: 'before' | 'after') => {
    const count = pages.filter(p => p.kind === kind && p.query.has(direction)).length;
    await page.locator(selector).evaluate((element: HTMLElement, direction) => {
      element.scrollTop = direction === 'before' ? 0 : element.scrollHeight;
      element.dispatchEvent(new Event('scroll'));
    }, direction);
    await expect.poll(() => pages.filter(p => p.kind === kind && p.query.has(direction)).length).toBeGreaterThan(count);
    await page.waitForTimeout(100);
    return pages.filter(p => p.kind === kind && p.query.has(direction)).at(-1)!.data;
  };
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');

    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const chat = page.getByTestId('messenger-message-list');
    await expect(chat).toContainText('Scheduled fixture result');
    let history: any;
    for (let index = 0; index < 30; index++) {
      history = await scrollPage('[data-testid="messenger-message-list"]', 'chat', 'before');
      expect(await page.locator('.messenger-turn').count()).toBeLessThanOrEqual(100);
      if (!history.has_more) break;
    }
    expect(history.chat[0].user_round).toBe(1);
    for (let index = 0; index < 30; index++) {
      history = await scrollPage('[data-testid="messenger-message-list"]', 'chat', 'after');
      if (!history.has_newer) break;
    }
    expect(history.chat.at(-1).user_round).toBe(360);
    history = await scrollPage('[data-testid="messenger-message-list"]', 'chat', 'before');
    expect(history.chat.at(-1).user_round).toBe(260);
    await expect(page.locator('.timeline-process-more')).toHaveCount(0);

    await page.goto(`/app/trajectory?session=${MOCK_SESSION}`);
    const ledger = page.locator('.tt-ledger');
    await expect(ledger).toBeVisible();
    await expect(page.locator('.tt-row').first()).toBeVisible();
    for (let index = 0; index < 15; index++) {
      history = await scrollPage('.tt-ledger', 'trajectory', 'before');
      expect(await page.locator('.tt-row').count()).toBeLessThan(100);
      if (!history.has_more) break;
    }
    expect(history.trajectory[0].user_round).toBe(1);
    for (let index = 0; index < 15; index++) {
      history = await scrollPage('.tt-ledger', 'trajectory', 'after');
      if (!history.has_newer) break;
    }
    expect(history.trajectory.at(-1).user_round).toBe(360);
    await expect(page.locator('.tt-truncated-hint button')).toHaveCount(0);
    expect(pageErrors).toEqual([]);
  } finally {
    await service.dispose();
  }
});

test('work process crosses the retained window in both directions and keeps its anchor', async ({ page }) => {
  test.setTimeout(90_000);
  const service = new ChatMockService();
  await service.install(page);
  const turn = service.scheduledTurn('completed');
  for (let index = 1; index <= 1300; index++) {
    const item_id = `fixture-process-${index}`;
    service.items.set(item_id, {
      item_id, turn_id: turn.turn_id, root_turn_id: turn.turn_id, visibility: 'user', revision: 1,
      kind: index % 2 ? 'assistant_message' : 'tool_call', role: index % 2 ? 'assistant' : 'tool',
      status: 'completed', model_round: index, created_seq: index + 10,
      ...(index % 2 ? { content: `Fixture print ${index}`, reasoning: `Fixture thought ${index}` }
        : { tool: 'read_file', tool_call_id: `fixture-call-${index}`, args: { path: 'fixture.txt' }, result: { content: 'fixture result' } })
    });
  }
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    await page.locator('.timeline-turn-process .timeline-process-head').click();
    const body = page.locator('.timeline-turn-process .timeline-process-body');
    await expect(body.locator('[data-process-key]').first()).toBeVisible();
    const readEdge = async (direction: 'item_before' | 'item_after') => {
      const response = page.waitForResponse(r => r.url().includes('/thread-log/turns/') && new URL(r.url()).searchParams.has(direction));
      const anchor = await body.evaluate((element, direction) => {
        element.scrollTop = direction === 'item_before' ? 0 : element.scrollHeight;
        const top = element.getBoundingClientRect().top;
        const row = [...element.querySelectorAll<HTMLElement>('[data-process-key]')].find(row => row.getBoundingClientRect().bottom > top);
        element.dispatchEvent(new Event('scroll'));
        return row ? { key: row.dataset.processKey, offset: row.getBoundingClientRect().top - top } : null;
      }, direction);
      const data = (await (await response).json()).data.turn;
      const visibleItem = data.items.find((item: any) => item.payload.reasoning);
      if (visibleItem) await expect(body.locator(`[data-process-key$="${visibleItem.item_id}"]`).first()).toBeAttached();
      else await expect(body.locator('.timeline-process-page-state-top')).toHaveCount(0);
      await page.waitForTimeout(80);
      if (anchor) {
        const offset = await body.evaluate((element, key) => {
          const row = [...element.querySelectorAll<HTMLElement>('[data-process-key]')].find(row => row.dataset.processKey === key);
          return row ? row.getBoundingClientRect().top - element.getBoundingClientRect().top : null;
        }, anchor.key);
        expect(offset).not.toBeNull();
        // The final page can contain only the filtered user/final-answer
        // records. Removing its top hint clamps scrollTop at zero, so the
        // first process row naturally aligns with the very start instead.
        const atStart = !data.has_older && await body.evaluate(element => element.scrollTop === 0);
        expect(atStart ? Math.abs(offset!) : Math.abs(offset! - anchor.offset)).toBeLessThan(3);
      }
      expect(await body.locator('[data-process-key]').count()).toBeLessThanOrEqual(800);
      return data;
    };
    let result: any;
    for (let index = 0; index < 20; index++) {
      result = await readEdge('item_before');
      if (!result.has_older) break;
    }
    expect(result.next_before).toBe(0);
    for (let index = 0; index < 20; index++) {
      result = await readEdge('item_after');
      if (!result.has_newer) break;
    }
    expect(result.next_after).toBe(1301);
    await readEdge('item_before');
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});
