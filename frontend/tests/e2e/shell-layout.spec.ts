import { test, expect } from '@playwright/test';

/**
 * 两栏壳体（方案 §5）真实渲染自检。
 *
 * 背景：B1 交付的 `styles/pages/messenger-shell.css` 曾**从未被 import**，
 * 构建与类型检查都是绿的，但两栏壳体实际没有任何样式。因此这里不看 class 是否存在，
 * 而是直接断言**计算后的几何与颜色**，并留一张截图供人工走查。
 *
 * 需要可用后端（`/wunder/auth/demo` 返回 token）；用 `VITE_ENV_DIR` 指向把
 * `VITE_DEV_PROXY_TARGET` 指到本地隔离服务端的环境目录即可。
 */
test('两栏壳体：左栏宽度/底色/文件区容器真实生效', async ({ page, request }) => {
  // 走真实登录：注册一个每次运行唯一的用户，再用页面表单登录。
  // 不能靠注入 token —— main.ts 会在应用版本变化时 clearAllAccessTokens()，注入的令牌会被清掉。
  const username = `e2e_shell_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post('/wunder/auth/register', {
    data: { username, password }
  });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();

  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/login');
  await page.getByPlaceholder(/username/i).fill(username);
  await page.getByPlaceholder(/password/i).fill(password);
  await page.getByRole('button', { name: /sign in/i }).click();

  const shell = page.locator('.messenger-view').first();
  await expect(shell).toBeVisible({ timeout: 30_000 });

  const sidebar = page.locator('.messenger-sidebar').first();
  await expect(sidebar).toBeVisible();

  // 左栏 240px（方案 §5.2）——宽度退化成 0 或整行即说明壳体样式没生效。
  const box = await sidebar.boundingBox();
  expect(box, 'sidebar bounding box').not.toBeNull();
  expect(box!.width).toBeGreaterThanOrEqual(232);
  expect(box!.width).toBeLessThanOrEqual(248);
  expect(box!.height).toBeGreaterThan(300);

  // 左侧栏底色 token（--mz-sidebar: #F6F5F3）
  const sidebarBg = await sidebar.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(sidebarBg).toBe('rgb(246, 245, 243)');

  // 下半区工作目录容器（B2）+ 聊天主区
  await expect(page.locator('.messenger-sidebar-files-region').first()).toBeVisible();
  await expect(page.locator('.messenger-main').first()).toBeVisible();

  // 主色 token 至少在一个可见元素上生效
  const primarySeen = await page.evaluate(() => {
    const wanted = 'rgb(201, 100, 67)'; // #C96443
    return Array.from(document.querySelectorAll<HTMLElement>('*')).some((el) => {
      const style = getComputedStyle(el);
      return style.backgroundColor === wanted || style.color === wanted;
    });
  });
  expect(primarySeen, 'primary token #C96443 should be applied somewhere visible').toBe(true);

  await page.screenshot({ path: '../temp_dir/screens/shell-desktop.png' });
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});

/**
 * B6 合并后的状态面自检：状态类信息（在线 + 上下文占用）只在壳体底部一处，
 * 输入卡内只留「发送目标工作目录」。用真机断言锁住「不重复」，避免以后又长回来。
 */
test('状态面合并：底部独占在线+占用，输入卡只剩工作目录', async ({ page, request }) => {
  const username = `e2e_status_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post('/wunder/auth/register', { data: { username, password } });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();

  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/login');
  await page.getByPlaceholder(/username/i).fill(username);
  await page.getByPlaceholder(/password/i).fill(password);
  await page.getByRole('button', { name: /sign in/i }).click();
  await expect(page.locator('.messenger-view').first()).toBeVisible({ timeout: 30_000 });

  const statusBar = page.locator('.messenger-status-bar').first();
  await expect(statusBar).toBeVisible();
  await expect(statusBar).toContainText(/在线|重连中|[Oo]nline/);
  // 占用图标/进度条只有一处：底部
  await expect(statusBar.locator('.context-usage-icon')).toHaveCount(1);
  await expect(statusBar.locator('[data-testid="messenger-status-usage"]')).toBeVisible();
  expect(await statusBar.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px');

  // 输入卡内的状态条不得再出现占用图标/百分比：带标签+进度条+百分比的占用面仍只有底部状态栏一处，
  // 输入区工具栏只按桌面补一个同源（sessionContextUsage.ts）的占用图标，不再有第二份数字。
  const composerBar = page.locator('.composer-status-bar').first();
  await expect(composerBar).toBeVisible();
  await expect(composerBar.locator('[data-testid="composer-workspace-name"]')).toBeVisible();
  await expect(composerBar.locator('.context-usage-icon')).toHaveCount(0);
  await expect(statusBar.locator('.messenger-status-usage-text')).toBeVisible();
  await expect(page.locator('.composer-action-row .context-usage-icon')).toHaveCount(1);
  // 百分比文本仍然是底部状态栏独占，工具栏只有一个图标、不含占用数字。
  await expect(composerBar).not.toContainText(/%/);
  await expect(page.locator('.composer-context-usage')).not.toContainText(/%/);

  await page.screenshot({ path: '../temp_dir/screens/status-bar-merged.png' });
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});

/**
 * 窄视口（方案 §15.8）：左栏转为抽屉覆盖，聊天区不被挤压，抽屉可开可关。
 */
test('窄视口 <1024px：左栏转抽屉覆盖且聊天区不被挤压', async ({ page, request }) => {
  const username = `e2e_drawer_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post('/wunder/auth/register', { data: { username, password } });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();

  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/login');
  await page.getByPlaceholder(/username/i).fill(username);
  await page.getByPlaceholder(/password/i).fill(password);
  await page.getByRole('button', { name: /sign in/i }).click();
  await expect(page.locator('.messenger-view').first()).toBeVisible({ timeout: 30_000 });

  const shell = page.locator('.messenger-view').first();
  const sidebar = page.locator('.messenger-sidebar').first();
  const main = page.locator('.messenger-main').first();

  // 聊天区占满可用宽度（左栏是覆盖层，不占布局）
  const mainBox = await main.boundingBox();
  expect(mainBox, 'main bounding box').not.toBeNull();
  expect(mainBox!.width).toBeGreaterThanOrEqual(890);

  // 抽屉关闭：左栏整体移出视口左侧
  const sidebarBox = await sidebar.boundingBox();
  expect(sidebarBox, 'sidebar bounding box').not.toBeNull();
  expect(sidebarBox!.x + sidebarBox!.width).toBeLessThanOrEqual(1);
  await expect(shell).not.toHaveClass(/messenger-view--drawer-open/);
  await page.screenshot({ path: '../temp_dir/screens/narrow-drawer-closed.png' });

  // 打开：切换按钮可见、点击后抽屉滑入且有遮罩
  const toggle = page.locator('.messenger-sidebar-toggle').first();
  await expect(toggle).toBeVisible();
  await toggle.click();
  await expect(shell).toHaveClass(/messenger-view--drawer-open/);
  // 抽屉有 160ms 位移过渡：等 transform 真正落到 0 再量几何。
  await expect.poll(async () => (await sidebar.boundingBox())!.x, { timeout: 5_000 }).toBeGreaterThanOrEqual(-1);
  const openBox = await sidebar.boundingBox();
  expect(openBox!.x).toBeGreaterThanOrEqual(-1);
  expect(openBox!.width).toBeGreaterThanOrEqual(232);
  await expect(page.locator('.messenger-sidebar-backdrop')).toBeVisible();
  // 抽屉覆盖而不是推挤：聊天区几何不变
  const mainBoxAfterOpen = await main.boundingBox();
  expect(Math.abs(mainBoxAfterOpen!.width - mainBox!.width)).toBeLessThanOrEqual(1);
  await page.screenshot({ path: '../temp_dir/screens/narrow-drawer-open.png' });

  // 关闭：点遮罩收回
  await page.locator('.messenger-sidebar-backdrop').click();
  await expect(shell).not.toHaveClass(/messenger-view--drawer-open/);
  await expect
    .poll(async () => (await sidebar.boundingBox())!.x)
    .toBeLessThanOrEqual(-232);

  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
