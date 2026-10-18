import { expect, test, type Page } from '@playwright/test';
import { createServer, type Server } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';

const webRoot = path.resolve(process.cwd(), '../web');
let staticServer: Server;
let bridgeUrl: string;

test.use({
  locale: 'zh-CN',
  viewport: { width: 1440, height: 900 },
  launchOptions: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] }
});

test.beforeAll(async () => {
  staticServer = createServer(async (request, response) => {
    const pathname = decodeURIComponent(new URL(request.url || '/', 'http://fixture.local').pathname);
    const target = path.resolve(webRoot, `.${pathname === '/' ? '/index.html' : pathname}`);
    if (!target.startsWith(`${webRoot}${path.sep}`)) { response.writeHead(403).end(); return; }
    try {
      const bytes = await readFile(target);
      const type = ({ '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css',
        '.glb': 'model/gltf-binary', '.svg': 'image/svg+xml' } as Record<string, string>)[path.extname(target)];
      response.writeHead(200, { 'Content-Type': `${type || 'application/octet-stream'}; charset=utf-8` });
      response.end(bytes);
    } catch { response.writeHead(404).end(); }
  });
  await new Promise<void>(resolve => staticServer.listen(0, '127.0.0.1', resolve));
  bridgeUrl = `http://127.0.0.1:${(staticServer.address() as { port: number }).port}`;
});

test.afterAll(async () => {
  staticServer.closeAllConnections();
  await new Promise<void>((resolve, reject) => staticServer.close(error => error ? reject(error) : resolve()));
});

async function prepareBridge(page: Page, scope = 'admin') {
  await page.addInitScript(scope => {
    localStorage.setItem('wunder_admin_auth', JSON.stringify({ token: 'fixture-token', scope }));
    // Legacy preferences must not override the shared landing page.
    localStorage.setItem('wunder_app_config', JSON.stringify({ defaultPanel: 'monitor' }));
  }, scope);
  await page.route(url => url.pathname.startsWith('/wunder/'), route => route.fulfill({ json:
    route.request().url().endsWith('/auth/me')
      ? { data: { user_id: 'fixture-user', roles: [scope] } }
      : { data: {}, tools: [], entries: [], sessions: [], supported_languages: ['zh-CN', 'en-US'], default_language: 'zh-CN' }
  }));
  await page.goto(bridgeUrl);
  await expect(page.locator('#adminLoginModal')).toHaveAttribute('aria-hidden', 'true');
}

test('bridge opens on the full model page, rotates, and releases the context on navigation', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await prepareBridge(page);
  await expect(page.locator('#wunderModelPanel .wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  await expect(page.locator('#sidebarCollapse')).toHaveCount(0);
  await expect(page.locator('.nav-group.is-collapsed')).toHaveCount(4);
  const canvas = page.locator('#wunderModelPanel canvas');
  await expect(page.locator('#wunderModelPanel h1')).toHaveCount(0);
  await expect(page.locator('[data-model-coordinates]')).toContainText('X');
  await expect(page.locator('[data-model-coordinates]')).toContainText('Y -4.88');
  await expect(page.locator('.wunder-model-brand')).toHaveText('AAA wunder 心舰');
  await expect(page.locator('.wunder-model-hint')).toHaveCount(0);
  await expect(page.locator('.wunder-model-credit')).toHaveCount(0);
  await expect(page.locator('[data-action="reset"]')).toHaveCount(0);
  await expect(page.locator('[data-action="rotate"]')).toHaveCount(0);
  const coordinates = page.locator('[data-model-coordinates]');
  const initialCoordinates = await coordinates.textContent();
  await expect(async () => expect(await coordinates.textContent()).not.toBe(initialCoordinates)).toPass();
  const initial = await canvas.screenshot();
  await expect(async () => expect(Buffer.compare(initial, await canvas.screenshot())).not.toBe(0)).toPass();
  const box = (await canvas.boundingBox())!;
  const content = (await page.locator('.content').boundingBox())!;
  expect(box.x).toBeCloseTo(content.x, 0);
  expect(box.y).toBe(0);
  expect(box.width).toBeCloseTo(content.width, 0);
  expect(box.height).toBe(900);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 140, box.y + box.height / 2 + 45, { steps: 8 });
  await page.mouse.up();
  const rotated = await canvas.screenshot();
  await page.mouse.wheel(0, -120);
  expect(Buffer.compare(rotated, await canvas.screenshot())).not.toBe(0);
  await canvas.screenshot({ path: 'test-results/wunder-model-bridge.png' });
  await expect(page.locator('#wunderModelPanel')).toBeVisible();
  await page.locator('.nav-group[data-group="system"] .nav-group-btn').click();
  const context = await canvas.evaluateHandle(element => (element as HTMLCanvasElement).getContext('webgl2'));
  await page.locator('#navMonitor').click();
  await expect(canvas).toHaveCount(0);
  expect(await context.evaluate(value => value?.isContextLost())).toBe(true);
  await page.locator('#sidebarTitle').click();
  await expect(page.locator('.wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  await expect(page.locator('[data-action="rotate"]')).toHaveCount(0);
  await page.reload();
  await expect(page.locator('#wunderModelPanel .wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  expect(errors).toEqual([]);
});

test('bridge model load failure can be retried', async ({ page }) => {
  let fail = true;
  await page.route('**/third/models/wunder.glb**', async route => {
    if (fail) await route.fulfill({ status: 404, body: '' });
    else await route.continue();
  });
  await prepareBridge(page);
  await expect(page.locator('.wunder-model-viewer')).toHaveAttribute('data-state', 'error');
  await expect(page.locator('.wunder-model-status')).toContainText('模型加载失败');
  fail = false;
  await page.getByRole('button', { name: '重试', exact: true }).click();
  await expect(page.locator('.wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
});

test('leader scope also lands on the model while retaining its navigation permissions', async ({ page }) => {
  await prepareBridge(page, 'leader');
  await expect(page.locator('#wunderModelPanel .wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  await expect(page.locator('#navMonitor')).toBeHidden();
  await page.locator('.nav-group[data-group="system"] .nav-group-btn').click();
  await expect(page.locator('#navOrgUnits')).toBeVisible();
});

test('hive settings opens the shared viewer and destroys it on close', async ({ page }) => {
  const errors: string[] = [];
  const requests: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => requests.push(request.url()));
  // Keep assets on the same origin, as the production /third proxy does.
  await page.route('**/third/**', async route => {
    const url = new URL(route.request().url());
    const response = await page.request.get(`${bridgeUrl}${url.pathname}`);
    await route.fulfill({ response });
  });
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route(url => url.pathname.startsWith('/wunder/'), route => route.fulfill({ json: { data: { items: [] } } }));
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(async () => {
    const routerModule = performance.getEntriesByType('resource')
      .map(entry => entry.name).find(url => new URL(url).pathname === '/src/router/index.ts') || '/src/router/index.ts';
    const { default: router } = await import(routerModule);
    const replace = router.replace.bind(router);
    router.replace = (location: any) => replace(typeof location === 'object'
      ? { ...location, path: '/__e2e/messenger-view-performance' } : location);
  });
  const open = page.getByTestId('settings-open-wunder-model');
  // The fixture's initial thread route can finish after the first settings click.
  await expect(async () => {
    if (!(await open.count())) await page.locator('.messenger-sidebar-settings').click();
    await expect(open).toBeVisible({ timeout: 1500 });
  }).toPass({ timeout: 15000 });
  expect(requests.some(url => url.includes('/models/wunder.glb'))).toBe(false);
  await open.click();
  await expect(page.locator('.wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  await expect(page.locator('[data-action="reset"]')).toHaveCount(0);
  await expect(page.locator('[data-action="rotate"]')).toHaveCount(0);
  await expect(page.locator('.wunder-model-dialog')).toHaveCount(0);
  await expect(page.locator('[data-testid="wunder-model-inline"]')).toBeVisible();
  const box = (await page.locator('.wunder-model-viewer canvas').boundingBox())!;
  expect(box.x).toBeGreaterThan(0);
  expect(box.y).toBeGreaterThan(0);
  expect(box.width).toBeLessThan(1440);
  expect(box.height).toBeLessThan(900);
  await page.screenshot({ path: 'test-results/wunder-model-hive.png' });
  await page.keyboard.press('Escape');
  await expect(page.locator('.wunder-model-viewer')).toHaveCount(0);
  await expect(page.getByTestId('messenger-settings')).toBeVisible();
  await open.click();
  await expect(page.locator('.wunder-model-viewer')).toHaveAttribute('data-state', 'ready', { timeout: 25000 });
  expect(errors).toEqual([]);
});
