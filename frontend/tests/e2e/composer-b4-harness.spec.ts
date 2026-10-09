import { test, expect } from '@playwright/test';

/**
 * B4 输入区真实渲染自检（走 `/__e2e/composer-b4` dev harness）。
 *
 * 用例断言的是**计算后的几何/可见性/事件回传**，不看 class 是否存在：
 * 悬浮卡片几何（§8.1）、控件（§8.2）、模型切换浮层链路（§8.3）、
 * 审批模式三档（§8.4）、状态栏（§8.5）、引用 chip 与发送注入（§八交接点）。
 */
test('B4 输入区：悬浮卡片 + 模型浮层 + 审批模式 + 引用注入 + 状态栏', async ({ page, request }) => {
  const pageErrors: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));

  // 先走真实登录，模型清单才会走通鉴权（harness 路由本身不需要登录）。
  const username = `e2e_b4_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post('/wunder/auth/register', {
    data: { username, password }
  });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/login');
  await page.getByPlaceholder(/username/i).fill(username);
  await page.getByPlaceholder(/password/i).fill(password);
  await page.getByRole('button', { name: /sign in/i }).click();
  // 登录成功后路由会跳到 /app/*；壳体自身是否渲染不属于本用例（controller 由专项修复负责）
  await page.waitForURL(/\/app\//, { timeout: 30_000 });

  // 独立挂载真实输入区（不经过 controller 的壳体装配）
  await page.goto('/__e2e/composer-b4');
  await expect(page.locator('[data-testid="composer-b4-harness"]')).toBeVisible();

  const card = page.locator('.input-box--world').first();
  await expect(card).toBeVisible({ timeout: 20_000 });

  // §8.1 输入卡只包输入框：圆角 16px、1px 边框、白底、12x14 内边距；
  // 工具行是卡的兄弟节点，直接落在页面底色上（footer 不再铺白底）。
  const cardStyle = await card.evaluate((el) => {
    const style = getComputedStyle(el);
    return {
      radius: style.borderTopLeftRadius,
      borderWidth: style.borderTopWidth,
      background: style.backgroundColor,
      padding: `${style.paddingTop} ${style.paddingRight}`,
      parentHasRow: Array.from(el.parentElement?.children || []).some((child) =>
        child.classList.contains('composer-action-row'))
    };
  });
  expect(cardStyle.radius).toBe('16px');
  expect(cardStyle.borderWidth).toBe('1px');
  expect(cardStyle.background).toBe('rgb(255, 255, 255)');
  expect(cardStyle.padding).toBe('12px 14px');
  expect(cardStyle.parentHasRow).toBe(true);
  expect(await page.locator('.input-box--world .composer-action-row').count()).toBe(0);
  expect(await page.locator('.messenger-chat-footer').first().evaluate((el) =>
    getComputedStyle(el).backgroundColor)).toBe('rgba(0, 0, 0, 0)');

  // §8.1 容器左右 16px、底部 12px
  const footerPadding = await page.locator('.messenger-chat-footer').first().evaluate((el) => {
    const style = getComputedStyle(el);
    return { left: style.paddingLeft, bottom: style.paddingBottom };
  });
  expect(footerPadding.left).toBe('16px');
  expect(footerPadding.bottom).toBe('12px');

  // §8.1 字号 14px + 无占位文字（发送目标由工具栏的工作目录 chip 说明）+ 多行自适应（1-8 行后内部滚动）
  const textarea = page.locator('[data-testid="chat-composer-input"]').first();
  await expect(textarea).toBeVisible();
  const textareaStyle = await textarea.evaluate((el) => getComputedStyle(el).fontSize);
  expect(textareaStyle).toBe('14px');
  await expect(textarea).toHaveAttribute('placeholder', '');
  await textarea.fill(Array.from({ length: 12 }, (_, index) => `line ${index}`).join('\n'));
  const overflow = await textarea.evaluate((el) => ({
    height: el.getBoundingClientRect().height,
    scrollHeight: el.scrollHeight,
    overflowY: getComputedStyle(el).overflowY
  }));
  expect(overflow.height).toBeLessThanOrEqual(168);
  expect(overflow.scrollHeight).toBeGreaterThan(overflow.height);
  expect(overflow.overflowY).toBe('auto');
  await textarea.fill('');

  // §8.2 发送按钮：34×34 圆角方形主色按钮（radius 10px，与「+」同档，不是正圆）
  const sendButton = page.locator('[data-testid="chat-composer-send"]').first();
  const sendGeometry = await sendButton.evaluate((el) => {
    const style = getComputedStyle(el);
    return { width: style.width, height: style.height, radius: style.borderTopLeftRadius };
  });
  expect(sendGeometry.width).toBe('34px');
  expect(sendGeometry.height).toBe('34px');
  expect(sendGeometry.radius).toBe('10px');
  await expect(sendButton).toBeDisabled();
  await textarea.fill('你好');
  await expect(sendButton).toBeEnabled();
  await textarea.fill('');

  // §8.5 工具栏左组：「+」菜单 → 工作目录 chip → 审批；占用统计归到右组的模型触发器。
  const statusBar = page.locator('.composer-status-bar').first();
  await expect(statusBar).toBeVisible();
  expect(await statusBar.evaluate((el) => getComputedStyle(el).fontSize)).toBe('12px');
  await expect(statusBar.locator('[data-testid="composer-workspace-name"]')).toBeVisible();
  // 工作目录 chip 内不得有占用图标/百分比。
  await expect(statusBar.locator('.context-usage-icon')).toHaveCount(0);
  await expect(statusBar).not.toContainText(/%/);

  // §8.2 审批模式三档 + 即时回传
  const approvalTrigger = page.locator('.composer-approval-trigger').first();
  await expect(approvalTrigger).toContainText(/自动执行|Run automatically/);
  await approvalTrigger.click();
  const approvalPanel = page.locator('.composer-panel--approval').first();
  await expect(approvalPanel).toBeVisible();
  await expect(approvalPanel).toContainText(/工具需确认|Confirm tools/);
  await expect(approvalPanel).toContainText(/全部需确认|Confirm everything/);
  await approvalPanel.locator('.composer-panel-item--radio').nth(1).click();
  await expect(approvalTrigger).toContainText(/工具需确认|Confirm tools/);
  await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText('approval-mode:auto_edit');

  // §8.3 模型浮层：300px、12px 圆角、锚在触发器上方、上下文占用明细 + 推理强度 + 当前模型打勾。
  // 触发器本身就是占用面（星形按占用填充 + 百分比），浮层里再给容量数字。
  const modelTrigger = page.locator('.composer-model-trigger').first();
  await expect(modelTrigger).toContainText('harness-model-a');
  await expect(modelTrigger.locator('.context-usage-icon')).toHaveCount(1);
  await expect(modelTrigger.locator('[data-testid="composer-context-percent"]')).toBeVisible();
  await modelTrigger.click();
  const popover = page.locator('.composer-model-popover').first();
  await expect(popover).toBeVisible();
  const popoverBox = await popover.boundingBox();
  expect(Math.round(popoverBox!.width)).toBe(300);
  const popoverStyle = await popover.evaluate((el) => {
    const style = getComputedStyle(el);
    return { radius: style.borderTopLeftRadius, borderWidth: style.borderTopWidth };
  });
  expect(popoverStyle.radius).toBe('12px');
  expect(popoverStyle.borderWidth).toBe('1px');
  const cardBox = await card.boundingBox();
  expect(popoverBox!.y + popoverBox!.height).toBeLessThanOrEqual(cardBox!.y + cardBox!.height);
  // 浮层锚在**触发器**上方并与之右对齐（此前锚到整张输入卡，才会浮到输入区之上）。
  const triggerBox = await modelTrigger.boundingBox();
  expect(popoverBox!.y + popoverBox!.height).toBeLessThanOrEqual(triggerBox!.y + 2);
  expect(Math.abs(popoverBox!.x + popoverBox!.width - (triggerBox!.x + triggerBox!.width))).toBeLessThanOrEqual(2);
  await expect(popover).toContainText(/思考等级|Reasoning effort/);
  // 上下文占用在浮层**顶部**，只有数字，没有进度条。
  const contextBlock = popover.locator('.composer-model-context');
  await expect(contextBlock).toBeVisible();
  await expect(contextBlock).toHaveText(/占用|Occupancy/);
  expect(await popover.evaluate((el) => el.firstElementChild?.className)).toContain('composer-model-context');
  await expect(popover.locator('.composer-model-context-track, .composer-model-context-fill')).toHaveCount(0);
  // 模型清单不再有搜索框，也没有「设为我的默认」（默认模型归设置页管理）。
  await expect(popover.locator('.composer-model-search-input')).toHaveCount(0);
  await expect(popover.locator('.composer-model-default-row')).toHaveCount(0);

  const modelRows = page.locator('.composer-model-row');
  const rowCount = await modelRows.count();
  if (rowCount > 0) {
    // 当前模型在列表中打勾（仅当目录里确实含该模型时校验，避免依赖后端模型目录内容）
    const harnessRow = page.locator('.composer-model-row').filter({ hasText: 'harness-model-a' });
    if (await harnessRow.count()) {
      await expect(page.locator('.composer-model-row.is-active').first()).toContainText('harness-model-a');
    }
    // 选择另一个模型 -> 立即切换 + 事件回传
    const target = modelRows.filter({ hasNotText: 'harness-model-a' }).first();
    if (await target.count()) {
      const targetName = (await target.locator('.composer-model-row-name').innerText()).trim();
      await target.click();
      await expect(modelTrigger).toContainText(targetName);
      await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText(`apply-model:${targetName}`);
    }
  } else {
    // 无可用模型：提示联系管理员（用户侧不提供建模型入口）
    await expect(popover).toContainText(/联系管理员|administrator/);
  }

  // 底部推理强度档位即时回传（档位由当前模型的契约能力决定，未交付时不渲染）
  const effortItems = popover.locator('.composer-model-effort-item');
  if (await effortItems.count()) {
    await expect(effortItems.first()).toBeVisible();
    await effortItems.nth(4).click();
    await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText('reasoning-effort:');
  }
  await page.keyboard.press('Escape');

  // §8.2 工具栏左组（对齐桌面）：工作目录 → 审批 → 命令 → 语音；
  // 命令 / 预设问题 / 录音 都收在「+」里：行上不再常驻图标按钮。
  expect(await page.locator('.composer-action-group .composer-icon-btn').count()).toBe(0);
  const plusButton = page.locator('[data-testid="chat-composer-plus"]');
  await expect(plusButton).toHaveAttribute('aria-label', /更多操作|More actions/);
  await expect(plusButton).toHaveCSS('border-radius', '10px');
  await expect(page.locator('.composer-panel--plus')).toHaveCount(0);
  await plusButton.click();
  const plusPanel = page.locator('.composer-panel--plus');
  await expect(plusPanel).toBeVisible();
  // 面板贴在「+」上方并与之左对齐（锚在触发器，不再浮到输入区之上）。
  const plusBox = await plusButton.boundingBox();
  const plusPanelBox = await plusPanel.boundingBox();
  expect(plusPanelBox!.y + plusPanelBox!.height).toBeLessThanOrEqual(plusBox!.y + 2);
  expect(Math.abs(plusPanelBox!.x - plusBox!.x)).toBeLessThanOrEqual(2);
  // 命令 + 预设问题恒在，录音行取决于运行环境是否支持 MediaRecorder。
  expect(await plusPanel.locator('.composer-plus-item').count()).toBeGreaterThanOrEqual(2);

  // 命令：悬停行就从右侧呼出子面板（级联形态，不向下挤开菜单），点选只填草稿不发送。
  const commandRow = plusPanel.locator('.composer-plus-row').filter({ hasText: /命令|Commands/ }).first();
  await commandRow.hover();
  const commandFlyout = commandRow.locator('.composer-plus-flyout');
  await expect(commandFlyout).toBeVisible();
  await expect(commandFlyout).toContainText('/help');
  await expect(commandFlyout).toContainText('/compact');
  const commandRowBox = await commandRow.boundingBox();
  const commandFlyoutBox = await commandFlyout.boundingBox();
  expect(commandFlyoutBox!.x).toBeGreaterThan(commandRowBox!.x + commandRowBox!.width - 2);
  await commandFlyout.locator('.command-menu-item').first().click();
  await expect(textarea).toHaveValue(/^\/(help|new|stop|goal|compact)\s/);
  await textarea.fill('');
  // 工具栏右组：模型（星形按占用填充 + 百分比）→ 发送；整屏占用面只有这一处。
  expect(await page.locator('.context-usage-icon').count()).toBe(1);

  // 预设问题：换成它自己的右侧子面板，命令子面板同时收起；点击填入输入框并收起整个面板。
  const presetRow = plusPanel
    .locator('.composer-plus-row')
    .filter({ hasText: /预设问题|Preset/ })
    .first();
  await presetRow.hover();
  const presetPanel = page.locator('.composer-preset-panel').first();
  await expect(presetPanel).toBeVisible();
  await expect(commandRow.locator('.composer-plus-flyout')).toHaveCount(0);
  const presetRowBox = await presetRow.boundingBox();
  expect((await presetPanel.boundingBox())!.x).toBeGreaterThan(presetRowBox!.x + presetRowBox!.width - 2);
  await presetPanel.locator('.composer-preset-panel-item').first().click();
  await expect(textarea).toHaveValue(/总结当前线程/);
  await expect(page.locator('.composer-panel--plus')).toHaveCount(0);

  // 引用 chip（B2 通道）可移除 + 发送时以明确形式注入
  await page.locator('[data-testid="composer-b4-queue-reference"]').click();
  const referenceChip = page.locator('.workspace-quote-item').first();
  await expect(referenceChip).toContainText('harness-notes.md');
  await textarea.fill('看看这个文件');
  await sendButton.click();
  await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText('@docs/harness-notes.md');
  await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText('send:');
  // 发送后引用 chip 清空
  await expect(page.locator('.workspace-quote-item')).toHaveCount(0);

  // 执行中发送按钮变「停止」
  await page.locator('[data-testid="composer-b4-loading-toggle"]').click();
  await expect(sendButton).toHaveAttribute('data-mode', 'stop');
  await sendButton.click();
  await expect(page.locator('[data-testid="composer-b4-log"]')).toContainText('stop');

  await page.screenshot({ path: 'test-results/b4-composer.png' });
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
