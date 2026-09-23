import { expect, test, type Locator, type Page } from '@playwright/test';

const sessions = Array.from({ length: 80 }, (_, index) => ({
  id: `thread-${index}`, title: `Thread ${index}`, status: 'active',
  runtime_status: ['idle', 'running', 'completed', 'queued', 'failed'][index % 5],
  created_at: 100 - index, consumed_tokens: index * 1500, tool_calls: index, quota_used: index === 2 ? null : index * 1200
}));
const scrollTop = (list: Locator) => list.evaluate(element => element.scrollTop);
const setScroll = (list: Locator, value: number) => list.evaluate((element, top) => {
  element.scrollTop = top;
  element.dispatchEvent(new Event('scroll'));
}, value);
const dragAt = async (list: Locator, fraction: number, type = 'dragover') => {
  const rect = await list.boundingBox();
  if (!rect) throw new Error('Missing list viewport');
  await list.dispatchEvent(type, { clientX: rect.x + rect.width / 2, clientY: rect.y + rect.height * fraction });
};
const startDrag = async (page: Page, title: string) => {
  const transfer = await page.evaluateHandle(() => new DataTransfer());
  await page.locator('.messenger-task-item').filter({ hasText: title }).first().dispatchEvent('dragstart', { dataTransfer: transfer });
  await transfer.dispose();
};

test.beforeEach(async ({ page }) => {
  await page.route('**/chat/sessions?*', route => route.fulfill({ json: { data: { items: sessions, total: sessions.length } } }));
  await page.goto('/tests/e2e/fixtures/task-list.html');
  await expect(page.locator('.messenger-task-item').first()).toContainText('Thread 0');
});

test('thread icons share status shapes, animation and terminal precedence', async ({ page }) => {
  const rows = page.locator('.messenger-task-item');
  for (const [index, state] of ['idle', 'running', 'done', 'pending', 'error'].entries()) {
    const icon = rows.nth(index).locator('.messenger-agent-avatar');
    await expect(icon).toHaveClass(new RegExp(`state-${state}`));
    const badge = icon.locator('.messenger-agent-avatar-status');
    await expect(badge).toHaveCSS('border-radius', '999px');
    const reference = page.getByTestId('reference-icons').locator(`.state-${state} .messenger-agent-avatar-status`);
    expect(await badge.evaluate(el => getComputedStyle(el).backgroundColor)).toBe(await reference.evaluate(el => getComputedStyle(el).backgroundColor));
  }
  await expect(page.locator('.messenger-right-panel--tasks')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await expect(rows.nth(1).locator('.messenger-agent-avatar-status-dot-spinner')).toHaveCSS('animation-name', 'messenger-agent-avatar-spin');
  await page.evaluate(() => (window as any).taskListFixture.setStatus('thread-1', 'idle'));
  await expect(rows.nth(1).locator('.messenger-agent-avatar')).toHaveClass(/state-idle/);
  await page.evaluate(() => (window as any).taskListFixture.setStatus('thread-1', 'completed', true));
  await expect(rows.nth(1).locator('.messenger-agent-avatar')).toHaveClass(/state-done/);
  await page.screenshot({ path: '../temp_dir/task-list-browser.png' });
});

test('thread credits show zero, compact totals and live updates without replay inflation', async ({ page }) => {
  const rows = page.locator('.messenger-task-item');
  const credit = rows.first().locator('.messenger-task-item-meta > span').nth(2);
  await expect(credit.locator('.fa-coins')).toHaveCount(1);
  await expect(credit).toHaveText('0');
  await expect(rows.nth(1).locator('.messenger-task-item-meta > span').nth(2)).toHaveText('1.2k');
  await expect(rows.nth(2).locator('.messenger-task-item-meta > span').nth(2)).toHaveText('--');
  await page.evaluate(() => {
    (window as any).taskListFixture.quota(3, 1);
    (window as any).taskListFixture.quota(3, 1);
    (window as any).taskListFixture.quota(2, 2);
  });
  await expect(credit).toHaveText('3');
  await page.evaluate(items => (window as any).taskListFixture.replayPage(items), sessions);
  await expect(credit).toHaveText('3');
  await page.screenshot({ path: '../temp_dir/thread-quota-browser.png' });
});

test('virtual drag follows both scroll directions and stops on center, exit and cancellation', async ({ page }) => {
  const list = page.locator('.messenger-task-list');
  await startDrag(page, 'Thread 0');
  await dragAt(list, .99);
  await expect.poll(() => scrollTop(list)).toBeGreaterThan(700);
  await dragAt(list, .5);
  const middle = await scrollTop(list);
  await page.waitForTimeout(120);
  expect(await scrollTop(list)).toBe(middle);
  await dragAt(list, .99);
  await expect.poll(() => scrollTop(list), { timeout: 10_000 }).toBeGreaterThan(3900);
  await dragAt(list, .99, 'drop');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('messenger:threads:guest:default') || '[]').at(-1))).toBe('thread-0');
  await expect(page.locator('.is-drop-before, .is-drop-after')).toHaveCount(0);

  await startDrag(page, 'Thread 0');
  await dragAt(list, .01);
  await expect.poll(() => scrollTop(list), { timeout: 10_000 }).toBe(0);
  await dragAt(list, .01, 'drop');
  await expect(page.locator('.messenger-task-item').first()).toContainText('Thread 0');
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('messenger:threads:guest:default') || '[]')[0])).toBe('thread-0');

  await startDrag(page, 'Thread 0');
  await dragAt(list, .99);
  await expect.poll(() => scrollTop(list)).toBeGreaterThan(700);
  await list.dispatchEvent('dragleave', { relatedTarget: null });
  const left = await scrollTop(list);
  await page.waitForTimeout(120);
  expect(await scrollTop(list)).toBe(left);
  await page.evaluate(() => window.dispatchEvent(new DragEvent('dragend')));
  await dragAt(list, .99);
  await page.waitForTimeout(120);
  expect(await scrollTop(list)).toBe(left);
  await setScroll(list, 0);
  await expect(page.locator('.is-dragging, .is-drop-before, .is-drop-after')).toHaveCount(0);
});

test('archive removes a row immediately and a late catalog page cannot restore it', async ({ page }) => {
  let archived = false;
  await page.route('**/chat/sessions/thread-0/archive', route => {
    archived = true;
    return route.fulfill({ json: { data: { ...sessions[0], status: 'archived' } } });
  });
  await page.locator('.messenger-task-item').first().locator('.messenger-task-menu').click();
  await page.getByRole('menuitem', { name: '归档线程' }).click();
  await expect.poll(() => archived).toBe(true);
  await expect(page.locator('.el-message-box')).toHaveCount(0);
  await expect(page.locator('.messenger-task-item-title').filter({ hasText: /^Thread 0$/ })).toHaveCount(0);
  await page.evaluate(items => (window as any).taskListFixture.replayPage(items), sessions);
  await expect(page.locator('.messenger-task-item-title').filter({ hasText: /^Thread 0$/ })).toHaveCount(0);
});

test('native mouse drag survives virtual source removal', async ({ page }) => {
  const list = page.locator('.messenger-task-list');
  const row = await page.locator('.messenger-task-item').first().boundingBox();
  const rect = await list.boundingBox();
  if (!row || !rect) throw new Error('Missing drag target');
  await page.mouse.move(row.x + 50, row.y + row.height / 2);
  await page.mouse.down();
  await page.mouse.move(row.x + 55, row.y + row.height / 2 + 10, { steps: 4 });
  await page.mouse.move(rect.x + 100, rect.y + rect.height - 3, { steps: 8 });
  await expect.poll(() => scrollTop(list), { timeout: 10_000 }).toBeGreaterThan(3900);
  await page.mouse.move(rect.x + 101, rect.y + rect.height - 3);
  await page.mouse.up();
  await expect.poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('messenger:threads:guest:default') || '[]').at(-1))).toBe('thread-0');
});
