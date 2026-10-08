import { expect, test, type Page } from '@playwright/test';

/**
 * 线程目录（左栏线程树）与「已删除线程」交互自检。
 *
 * 原用例断言的是右 Dock 的 `.messenger-right-panel--tasks` / `.messenger-task-select`
 * （四区骨架的一部分，两栏壳体里已不存在）。此处按新形态改写为左栏
 * `MessengerThreadTree`：`.mz-thread-row` / `.mz-thread-select`（`title` 为线程名、
 * `aria-current` 表示选中）。断言的**行为**保持不变：
 * 目录刷新后已删除的行从组件里消失；打开空线程不报错；点击已删除的行会被移除、
 * 给出提示并清掉指向它的路由。
 *
 * 线程树是虚拟窗口，因此「行存在/不存在」的判定前先滚到底，确保窗口覆盖全部行。
 */
async function prepare(page: Page) {
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route(url => url.pathname.startsWith('/wunder/'), route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: { items: [] } }) }));
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  // 左栏线程树由会话目录接口驱动（壳体的 fixtureSessions 会被真实的 loadSessions
  // 覆盖），因此这里给出默认的两条线程；需要改口径的用例可再覆盖一次。
  await page.route('**/wunder/chat/sessions?**', route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: { total: 2, items: [
      { id: 'perf-session-a', agent_id: 'perf-agent', title: 'Session A' },
      { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B' }
    ] } }) }));
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

/** 线程树虚拟窗口：滚到底再断言，避免把窗口外当成「不存在」。 */
async function scrollThreadTreeToEnd(page: Page, passes = 3) {
  for (let pass = 0; pass < passes; pass += 1) {
    await page.evaluate(() => {
      const scroller = document.querySelector<HTMLElement>('.mz-thread-scroll');
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
    });
    await page.waitForTimeout(120);
  }
}

test('complete catalog removes deleted rows from the actual work thread component', async ({ page }) => {
  await prepare(page);
  await page.evaluate(() => {
    const store = (window as any).__catalogStore;
    store.sessions.push(...Array.from({ length: 130 }, (_, i) => ({ id: `removed-${i}`, agent_id: 'perf-agent', title: `Removed ${i}` })));
  });
  await page.waitForTimeout(200);
  await scrollThreadTreeToEnd(page);
  await expect(page.locator('.mz-thread-title').filter({ hasText: /^Removed / })).not.toHaveCount(0);

  await page.route('**/wunder/chat/sessions?**', route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: { total: 2, items: [
      { id: 'perf-session-a', agent_id: 'perf-agent', title: 'Session A' },
      { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B' }
    ] } }) }));
  await page.evaluate(() => (window as any).__catalogStore.loadSessions({ force: true }));
  await scrollThreadTreeToEnd(page);
  await expect(page.locator('.mz-thread-title').filter({ hasText: /^Removed / })).toHaveCount(0);
  await expect(page.locator('.mz-thread-select')).toHaveCount(2);
  await expect(page.locator('.mz-thread-select[title="Session A"]')).toHaveCount(1);
});

test('opening an empty thread completes without an error notification', async ({ page }) => {
  await prepare(page);
  await page.route('**/wunder/chat/sessions/perf-session-b**', route => {
    const path = new URL(route.request().url()).pathname;
    const data = path.endsWith('/thread-log/snapshot') ? { cursor: 0, turns: [], items: [] }
      : path.endsWith('/events') ? { events: [], rounds: [], running: false, runtime: { status: 'idle' } }
      : { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B', transcript: [] };
    return route.fulfill({ contentType: 'application/json', body: JSON.stringify({ data }) });
  });
  await page.locator('.mz-thread-select[title="Session B"]').click();
  await expect(page.locator('.mz-thread-select[title="Session B"]')).toHaveAttribute('aria-current', 'true');
  await page.waitForTimeout(600);
  await expect(page.locator('.el-message--error')).toHaveCount(0);
});

test('clicking a deleted row removes it, explains the failure, and clears its route', async ({ page }) => {
  await prepare(page);
  await page.evaluate(() => (window as any).__catalogStore.sessions.push({ id: 'removed-thread', agent_id: 'perf-agent', title: 'Removed thread' }));
  await page.route('**/wunder/chat/sessions/removed-thread**', route => route.fulfill({ status: 404,
    contentType: 'application/json', body: JSON.stringify({ error: { message: 'Not found' } }) }));
  await scrollThreadTreeToEnd(page);
  await page.locator('.mz-thread-select[title="Removed thread"]').click();
  await expect(page.locator('.mz-thread-select[title="Removed thread"]')).toHaveCount(0);
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
  await scrollThreadTreeToEnd(page);
  await expect(page.locator('.mz-thread-select[title="Removed thread"]')).toHaveCount(0);
  await page.route('**/wunder/chat/sessions/perf-session-b**', route => route.fulfill({ status: 200, contentType: 'application/json',
    body: JSON.stringify({ data: route.request().url().includes('/events') ? { events: [], rounds: [], running: false } :
      { id: 'perf-session-b', agent_id: 'perf-agent', title: 'Session B', transcript: [] } }) }));
  await page.locator('.mz-thread-select[title="Session B"]').click();
  await expect(page.locator('.mz-thread-select[title="Session B"]')).toHaveAttribute('aria-current', 'true');
});
