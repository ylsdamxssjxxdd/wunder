import { test, expect } from '@playwright/test';

/**
 * G2 联调：舰桥「预设智能体」面板 ↔ 真实后端契约。
 *
 * 前置：隔离服务端在跑（`python scripts/check-web.py --keep-server`），且其配置里存在
 * `preset_e2e_a` / `preset_e2e_b` 两个预设（`check-web.py` 生成的隔离配置已内置）。
 *
 * 登录方式：舰桥首页是一道「气闸门」，按 [5, 4, 1, 10] 依次点击节点即完成管理员登录
 * （`web/modules/admin-auth.js` 的 `TREE_UNLOCK_SEQUENCE`），没有用户名/密码表单。
 *
 * 断言分两层：
 * - 面板与绑定区块必须渲染（这是 C1/C2 的交付）；
 * - 契约就绪时 `bound_users` 必须是数字；未就绪时必须显示「未就绪」徽标而不是崩掉或伪造 0。
 */
const ADMIN_ORIGIN = process.env.ADMIN_E2E_ORIGIN || 'http://127.0.0.1:18000';
const UNLOCK_SEQUENCE = [5, 4, 1, 10];

test('舰桥预设智能体：绑定区块渲染且契约状态自洽', async ({ page }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.goto(ADMIN_ORIGIN + '/');
  await expect(page.locator('#adminLoginModal')).toBeVisible({ timeout: 20_000 });

  for (const node of UNLOCK_SEQUENCE) {
    // 监听挂在 <g class="airlock-node"> 上，可点区域是其内部的圆；直接派发冒泡 click
    // 比 Playwright 的可点性等待更稳（气闸动画期间几何会变，elementFromPoint 常被判为不可点）。
    await page.evaluate((nodeId: number) => {
      const target = document.querySelector(`.airlock-node[data-node="${nodeId}"]`);
      target?.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    }, node);
    await page.waitForTimeout(250);
  }

  // 登录完成后侧栏出现；气闸门会留在 DOM 里做开合动画，必须等它真正隐藏再点导航，
  // 否则 `airlock-hull` 会拦截指针事件（Playwright 会一直重试到超时）。
  await expect(page.locator('#navPresetAgents')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#adminLoginModal')).toBeHidden({ timeout: 30_000 });
  await page.evaluate(() => {
    document.querySelector<HTMLElement>('#navPresetAgents')?.click();
  });

  const panel = page.locator('#presetAgentsPanel');
  await expect(panel).toBeVisible({ timeout: 20_000 });

  // 预设列表应有内容（隔离配置内置两个 e2e 预设）
  const list = page.locator('#presetAgentList');
  await expect(list).toBeVisible();
  await expect
    .poll(async () => (await list.locator('.preset-agent-item, .preset-agent-list-item, li, button').count()), {
      timeout: 20_000,
      message: '预设列表应至少渲染一个条目'
    })
    .toBeGreaterThan(0);

  // 选中第一个预设 → 详情区出现
  await list.locator('.preset-agent-item, .preset-agent-list-item, li, button').first().click();
  await expect(page.locator('#presetBindingSummary')).toBeVisible({ timeout: 20_000 });

  // 契约状态自洽：要么显示未就绪徽标，要么给出绑定用户数（不得既不显示也不报错）
  const badge = page.locator('#presetBindingContractBadge');
  const badgeVisible = await badge.isVisible().catch(() => false);
  const summaryText = (await page.locator('#presetBindingSummary').innerText().catch(() => '')) || '';
  const tableRows = await page.locator('#presetBindingTableBody tr').count();
  const emptyVisible = await page.locator('#presetBindingEmpty').isVisible().catch(() => false);

  expect(
    badgeVisible || tableRows > 0 || emptyVisible || summaryText.trim().length > 0,
    `绑定区块必须给出明确状态（badge=${badgeVisible} rows=${tableRows} empty=${emptyVisible} summary="${summaryText.trim()}")`
  ).toBe(true);

  // 契约就绪时，绑定概览必须给出**真实计数**，且同步预览不能停在「正在统计…」
  // （停在 loading 说明 preview 请求没落地或 syncLoading 未被清掉——都是必须发现的缺陷）。
  await expect
    .poll(
      async () => {
        const text = (await page.locator('#presetBindingSummary').innerText().catch(() => '')) || '';
        // 实际文案：「共 {total} 个绑定用户 · 已选 {selected} 个」
        return /共\s*\d+\s*个绑定用户/.test(text);
      },
      { timeout: 20_000, message: '绑定概览应出现真实绑定用户数' }
    )
    .toBe(true);

  await expect
    .poll(
      async () => {
        const text = (await page.locator('#presetAgentSyncSummary').innerText().catch(() => '')) || '';
        return /正在统计/.test(text) ? 'loading' : 'settled';
      },
      { timeout: 20_000, message: '同步影响面预览不应长期停留在加载态' }
    )
    .toBe('settled');

  // 证据截图写到 temp_dir/screens（Playwright 在成功运行结束时会清空 test-results，
  // 手写截图会被一起删掉，不利于人工走查与留档）。
  await page.screenshot({ path: '../temp_dir/screens/admin-preset-bindings.png', fullPage: false });
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
