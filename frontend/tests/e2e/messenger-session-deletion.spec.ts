import { expect, test, type Page } from '@playwright/test';

async function prepare(page: Page) {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route(url => url.pathname.startsWith('/wunder/'), route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: { items: [] } }) }));
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(async () => {
    const storePath = '/src/stores/chat.ts';
    const actionsPath = '/src/stores/chatSessionOpenLoadActions.ts';
    const { useChatStore } = await import(storePath);
    const { chatSessionOpenLoadActions } = await import(actionsPath);
    const routerPath = '/src/router/index.ts';
    const { default: router } = await import(routerPath);
    const replace = router.replace.bind(router);
    // Preserve real navigation/query behavior inside the unauthenticated harness.
    router.replace = location => replace(typeof location === 'object'
      ? { ...location, path: '/__e2e/messenger-view-performance' } : location);
    const store = useChatStore();
    store.loadSessions = (...args) => chatSessionOpenLoadActions.loadSessions.apply(store, args);
    (window as any).__catalogStore = store;
  });
}

test('complete catalog removes deleted rows from the actual work thread component', async ({ page }) => {
  await prepare(page);
  await page.evaluate(() => {
    const store = (window as any).__catalogStore;
    store.sessions.push(...Array.from({ length: 130 }, (_, i) => ({ id: `removed-${i}`, agent_id: 'perf-agent', title: `Removed ${i}` })));
  });
  await expect(page.locator('.messenger-right-panel--tasks .messenger-right-section-count')).toHaveText('132');
  await page.route('**/wunder/chat/sessions?**', route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: { total: 2, items: [
      { id: 'perf-session-a', agent_id: 'perf-agent', title: 'Session A' },
      { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B' }
    ] } }) }));
  await page.evaluate(() => (window as any).__catalogStore.loadSessions({ force: true }));
  await expect(page.locator('.messenger-right-panel--tasks .messenger-right-section-count')).toHaveText('2');
  await expect(page.locator('.messenger-task-select')).toHaveCount(2);
  await expect(page.locator('.messenger-task-select[title^="Removed"]')).toHaveCount(0);
});

test('clicking a deleted row removes it, explains the failure, and clears its route', async ({ page }) => {
  await prepare(page);
  await page.evaluate(() => (window as any).__catalogStore.sessions.push({ id: 'removed-thread', agent_id: 'perf-agent', title: 'Removed thread' }));
  await page.route('**/wunder/chat/sessions/removed-thread**', route => route.fulfill({ status: 404,
    contentType: 'application/json', body: JSON.stringify({ error: { message: 'Not found' } }) }));
  await page.locator('.messenger-task-select[title="Removed thread"]').click();
  await expect(page.locator('.messenger-task-select[title="Removed thread"]')).toHaveCount(0);
  await expect(page.locator('.el-message--warning')).toBeVisible();
  await expect(page).not.toHaveURL(/session_id=removed-thread/);
  await page.evaluate(async () => {
    const path = '/src/stores/chatSessionCatalog.ts';
    const { mergeSessionCatalogPage } = await import(path);
    const store = (window as any).__catalogStore;
    const old = { id: 'removed-thread', agent_id: 'perf-agent', title: 'Removed thread' };
    store.syncSessionSummary(old);
    mergeSessionCatalogPage(store, { items: [old] });
  });
  await expect(page.locator('.messenger-task-select[title="Removed thread"]')).toHaveCount(0);
  await page.route('**/wunder/chat/sessions/perf-session-b**', route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: route.request().url().includes('/events') ? { events: [], rounds: [], running: false } :
      { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B', transcript: [] } }) }));
  await page.locator('.messenger-task-select[title="Session B"]').click();
  await expect(page.locator('.messenger-task-select[title="Session B"]')).toHaveAttribute('aria-current', 'true');
});
