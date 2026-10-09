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

  // §8.1 卡片几何：圆角 22px、1px 边框、白底、10x14 内边距
  const cardStyle = await card.evaluate((el) => {
    const style = getComputedStyle(el);
    return {
      radius: style.borderTopLeftRadius,
      borderWidth: style.borderTopWidth,
      background: style.backgroundColor,
      padding: `${style.paddingTop} ${style.paddingRight}`
    };
  });
  expect(cardStyle.radius).toBe('22px');
  expect(cardStyle.borderWidth).toBe('1px');
  expect(cardStyle.background).toBe('rgb(255, 255, 255)');
  expect(cardStyle.padding).toBe('10px 14px');

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

  // §8.2 发送按钮：与桌面一致 40×30 主色按钮（radius 8px）
  const sendButton = page.locator('[data-testid="chat-composer-send"]').first();
  const sendGeometry = await sendButton.evaluate((el) => {
    const style = getComputedStyle(el);
    return { width: style.width, height: style.height, radius: style.borderTopLeftRadius };
  });
  expect(sendGeometry.width).toBe('40px');
  expect(sendGeometry.height).toBe('30px');
  expect(sendGeometry.radius).toBe('8px');
  await expect(sendButton).toBeDisabled();
  await textarea.fill('你好');
  await expect(sendButton).toBeEnabled();
  await textarea.fill('');

  // §8.5 工具栏左组第一项：输入卡只留「发送目标工作目录」，占用统计归到右组的模型触发器。
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

  // §8.3 模型浮层：300px、12px 圆角、贴卡片上方、上下文占用明细 + 推理强度 + 当前模型打勾。
  // 触发器本身就是占用面（大脑按占用填充 + 百分比），浮层里再给容量与进度。
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
  expect(popoverBox!.y + popoverBox!.height).toBeLessThanOrEqual(cardBox!.y + 2);
  await expect(popover).toContainText(/思考等级|Reasoning effort/);
  await expect(popover.locator('.composer-model-context')).toBeVisible();
  await expect(popover.locator('.composer-model-context-track')).toBeVisible();
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
  // `+` 更多 / 回形针附件 / 历史 三个入口已随桌面对齐移除。
  await expect(page.locator('.composer-action-group [aria-label="更多"], .composer-action-group [aria-label="More"]')).toHaveCount(0);
  await expect(page.locator('.composer-action-group [aria-label="添加附件"], .composer-action-group [aria-label="Add attachment"]')).toHaveCount(0);
  await expect(page.locator('.composer-action-group [aria-label="历史"], .composer-action-group [aria-label="History"]')).toHaveCount(0);
  // `.composer-action-group` 里第一个图标按钮就是命令按钮（终端图标）。
  const commandButton = page.locator('.composer-action-group .composer-icon-btn').first();
  await expect(commandButton).toHaveAttribute('aria-label', /命令|Commands/);
  await commandButton.click();
  const commandPanel = page.locator('.command-menu').first();
  await expect(commandPanel).toBeVisible();
  await expect(commandPanel).toContainText('/help');
  await expect(commandPanel).toContainText('/compact');
  // 点选只填入草稿（不直接发送），与输入 `/` 的建议链路同一份 DOM。
  await commandPanel.locator('.command-menu-item').first().click();
  await expect(textarea).toHaveValue(/^\/(help|new|stop|goal|compact)\s/);
  await textarea.fill('');
  // 工具栏右组：模型（大脑按占用填充 + 百分比）→ 发送；整屏占用面只有这一处。
  expect(await page.locator('.context-usage-icon').count()).toBe(1);

  // 预设问题：不再常驻一行，收在工具栏的魔法棒里，点开才列、点击只填入输入框
  const presetButton = page
    .locator('.composer-action-group [aria-label="预设问题"], .composer-action-group [aria-label="Preset Questions"]')
    .first();
  await expect(presetButton).toBeVisible();
  await expect(page.locator('.composer-preset-panel')).toHaveCount(0);
  await presetButton.click();
  const presetPanel = page.locator('.composer-preset-panel').first();
  await expect(presetPanel).toBeVisible();
  await presetPanel.locator('.composer-preset-panel-item').first().click();
  await expect(textarea).toHaveValue(/总结当前线程/);
  await expect(page.locator('.composer-preset-panel')).toHaveCount(0);

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
