import { test, expect } from '@playwright/test';

/**
 * 舰桥（`web/`）真实加载自检：由隔离服务端在 `http://127.0.0.1:18000/` 直接托管静态站点。
 * 目的：C0~C7 连续删改（蜂群面板、预设绑定、容器化、473 个 i18n 键、大量 id/死分支）之后，
 * 确认管理端仍能正常引导、无 JS 报错、无资源 404。
 */
const ADMIN_ORIGIN = process.env.ADMIN_E2E_ORIGIN || 'http://127.0.0.1:18000';

test('舰桥首页可加载：登录门可见且无脚本错误', async ({ page }) => {
  const pageErrors: string[] = [];
  const failedRequests: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));
  page.on('requestfailed', (request) => failedRequests.push(request.url()));

  const response = await page.goto(ADMIN_ORIGIN + '/');
  expect(response?.status(), 'admin index status').toBeLessThan(400);

  await expect(page.locator('#adminLoginModal')).toBeVisible({ timeout: 20_000 });

  // 关键模块确实加载（app.js 是模块入口，elements.js/i18n.js 是基础设施）
  const moduleLoaded = await page.evaluate(
    () => Boolean(document.querySelector('script[type="module"][src*="app.js"]'))
  );
  expect(moduleLoaded, 'app.js module script tag present').toBe(true);

  await page.screenshot({ path: 'test-results/admin-login.png' });

  const localFailures = failedRequests.filter((url) => url.startsWith(ADMIN_ORIGIN));
  expect(localFailures, `failed requests: ${localFailures.join(' | ')}`).toEqual([]);
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
