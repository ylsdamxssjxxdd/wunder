// AI生成
import { test, expect } from '@playwright/test';

/**
 * 设置页 13 分类（方案 §九）真实渲染自检。
 *
 * 与 `shell-layout.spec.ts` 同样的思路：不看 class 是否存在，而是走真实登录、
 * 真实后端，断言**几何与可见内容**，并留一张截图供人工走查。
 *
 * 覆盖点：
 * 1. 左栏底部「设置」进入全屏设置页（左导航 260px + 右内容卡片）
 * 2. 13 个分类导航项齐全，逐一点击后右侧内容区有可见内容且无 pageerror
 * 3. 搜索「模型」后导航只剩「模型设置」
 * 4. 「返回应用」回到聊天区（设置覆盖层消失）
 *
 * 每个分类都是独立异步 chunk，因此逐项断言同时验证了「按需挂载」不会 404。
 */
const CATEGORY_IDS = [
  'general',
  'account',
  'models',
  'tools',
  'agent',
  'companion',
  'cron',
  'memory',
  'channels',
  'runtime',
  'prompts',
  'archived',
  'help'
] as const;

/**
 * 语言固定为 zh-CN：i18n 的初始语言优先取 localStorage，其次取浏览器语言
 * （`resolveInitialLanguage()`）。Playwright 默认 en-US，会让「搜索『模型』」
 * 断言拿到英文标题，因此这里显式声明 locale，让断言与中文文案一致。
 */
test.use({ locale: 'zh-CN' });

test('设置页：13 分类可进入、可搜索、可返回', async ({ page, request }) => {
  const username = `e2e_settings_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post('/wunder/auth/register', {
    data: { username, password }
  });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();

  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/login');
  // 语言固定为中文后，占位符不再是英文：用登录表单的稳定结构选择器。
  const loginForm = page.locator('.auth-form').first();
  await loginForm.locator('input[type="text"]').first().fill(username);
  await loginForm.locator('input[type="password"]').first().fill(password);
  await loginForm.locator('button[type="submit"]').first().click();

  await expect(page.locator('.messenger-view').first()).toBeVisible({ timeout: 30_000 });

  const overlay = page.locator('[data-testid="messenger-settings"]').first();
  const settingsButton = page.locator('.messenger-sidebar-settings').first();

  /**
   * 打开设置页并等待覆盖层出现。
   *
   * 允许重试的原因（已知的产品侧竞态，非本用例引入）：登录后 app 会自动创建/打开
   * 默认智能体线程，若这次会话引导的 route/section 同步晚于「设置」点击落地，
   * 它会把 section 拉回 `messages`，刚打开的覆盖层随即关闭。用例重试点击以消除
   * 时序抖动；该竞态本身记在验收报告里交给 B6 处理。
   */
  const openSettings = async () => {
    await expect(async () => {
      if ((await overlay.count()) === 0) {
        await settingsButton.click();
      }
      await expect(overlay).toBeVisible({ timeout: 4000 });
      // 短窗口复检：确认没有被迟到的会话引导 route/section 同步关掉。
      await page.waitForTimeout(600);
      await expect(overlay).toBeVisible({ timeout: 1000 });
    }).toPass({ timeout: 30_000, intervals: [400, 800, 1600] });
  };

  // 入口：左栏底部「设置」
  await openSettings();

  const nav = page.locator('[data-testid="settings-nav"]').first();
  await expect(nav).toBeVisible();

  // 左导航 260px（方案 §9.1）——宽度退化成 0 或整行说明壳体样式没生效。
  const navBox = await nav.boundingBox();
  expect(navBox, 'settings nav bounding box').not.toBeNull();
  expect(navBox!.width).toBeGreaterThanOrEqual(200);
  expect(navBox!.width).toBeLessThanOrEqual(300);

  // 13 个分类导航项齐全（顺序即方案 §9.2 的编号）
  const navItems = nav.locator('[data-settings-category]');
  await expect(navItems).toHaveCount(CATEGORY_IDS.length);
  for (const id of CATEGORY_IDS) {
    await expect(nav.locator(`[data-settings-category="${id}"]`)).toHaveCount(1);
  }

  // 选中态：浅色主色底 + 左侧主色指示（#C96443 → rgb(201, 100, 67)）
  const activeItem = nav.locator('[data-settings-category="general"]');
  const activeBg = await activeItem.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(activeBg).not.toBe('rgba(0, 0, 0, 0)');
  const indicatorVisible = await activeItem.evaluate((el) => {
    const style = getComputedStyle(el, '::before');
    return style.content !== 'none' && style.width !== 'auto' && style.width !== '0px';
  });
  expect(indicatorVisible, 'active nav item should show a left primary indicator').toBe(true);

  const content = page.locator('[data-testid="settings-content"]').first();
  await expect(content).toBeVisible();

  // 按需挂载：刚打开时只有一个分类实例，其余 12 个分类没有被初始化。
  await expect(page.locator('[data-testid^="settings-category-"]')).toHaveCount(1);
  // 默认分类（常规）留一张截图供人工走查
  await page.screenshot({ path: 'test-results/settings-page-general.png' });

  // 逐一点击 13 个分类：内容区必须有可见内容，且全程无 pageerror
  for (const id of CATEGORY_IDS) {
    await nav.locator(`[data-settings-category="${id}"]`).click();
    const panel = page.locator(`[data-testid="settings-category-${id}"]`).first();
    await expect(panel, `category ${id} should mount`).toBeVisible({ timeout: 20_000 });

    const text = (await panel.innerText()).trim();
    expect(text.length, `category ${id} should render visible content`).toBeGreaterThan(0);

    const box = await panel.boundingBox();
    expect(box, `category ${id} bounding box`).not.toBeNull();
    expect(box!.height, `category ${id} should have real height`).toBeGreaterThan(20);

    expect(pageErrors, `page errors after switching to ${id}: ${pageErrors.join(' | ')}`).toEqual([]);
  }

  // P1：用户侧多智能体已下线（服务端 /wunder/agents/{id} 只剩 GET/PUT），
  // 设置页「智能体」分类下不得再出现可点的「删除」入口。
  await nav.locator('[data-settings-category="agent"]').click();
  const agentPanel = page.locator('[data-testid="settings-category-agent"]').first();
  await expect(agentPanel).toBeVisible({ timeout: 20_000 });
  await expect(
    agentPanel.locator('button', { hasText: /删除|Delete/ }),
    '智能体分类不应再有删除入口'
  ).toHaveCount(0);
  expect(await agentPanel.locator('.messenger-settings-action.danger').count()).toBe(0);

  // 访问过 13 个分类后，KeepAlive 只保留最近 3 个实例，不会堆成 13 份 DOM。
  const mountedCategories = await page.locator('[data-testid^="settings-category-"]').count();
  expect(mountedCategories, 'only the most recent categories should stay mounted').toBeLessThanOrEqual(3);

  // 回到「模型设置」并截图，便于人工走查内容区样式
  await nav.locator('[data-settings-category="models"]').click();
  await expect(page.locator('[data-testid="settings-category-models"]')).toBeVisible();
  await page.screenshot({ path: 'test-results/settings-page.png' });

  // 搜索「模型」→ 只剩「模型设置」
  await page.locator('[data-testid="settings-search"]').first().fill('模型');
  await expect(nav.locator('[data-settings-category]')).toHaveCount(1);
  await expect(nav.locator('[data-settings-category="models"]')).toHaveCount(1);
  const remainingText = (await nav.locator('[data-settings-category="models"]').innerText()).trim();
  expect(remainingText).toContain('模型设置');

  // 清空搜索 → 13 项恢复
  await page.locator('[data-testid="settings-search"]').first().fill('');
  await expect(nav.locator('[data-settings-category]')).toHaveCount(CATEGORY_IDS.length);

  // 返回应用 → 覆盖层消失，聊天区与左栏重新可见
  await page.locator('[data-testid="settings-back"]').first().click();
  await expect(page.locator('[data-testid="messenger-settings"]')).toHaveCount(0);
  await expect(page.locator('.messenger-sidebar').first()).toBeVisible();
  await expect(page.locator('[data-testid="messenger-message-list"]').first()).toBeVisible();

  // 壳体不再有顶栏入口：帮助与资料都只在上面遍历过的 13 个分类里，左栏「设置」是唯一入口。
  await expect(page.locator('.messenger-site-header')).toHaveCount(0);
  await expect(page.locator('.messenger-sidebar-settings')).toHaveCount(1);

  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
