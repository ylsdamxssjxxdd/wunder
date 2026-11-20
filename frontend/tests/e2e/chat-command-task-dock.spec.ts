import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('background task dock expands output and terminates one command', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'fixture-command-turn';
  service.turns.push({ turn_id, user_round: 1, status: 'running', content: 'Fixture command' });
  service.commandTasks.push({
    command_session_id: 'fixture-command', session_id: MOCK_SESSION,
    command: 'fixture-command --wait', cwd: '/fixture', status: 'running',
    started_at: new Date(Date.now() - 5000).toISOString(), updated_at: new Date().toISOString(),
    stdout_tail: 'fixture output', stderr_tail: '', pty_tail: ''
  });
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    const trigger = page.getByTestId('composer-background-tasks');
    await expect(trigger).toHaveText('1');
    const geometry = await page.locator('.composer-status-trigger').evaluateAll(buttons =>
      buttons.map(button => {
        const bounds = button.getBoundingClientRect();
        const icon = button.querySelector('.composer-status-icon')!.getBoundingClientRect();
        const count = button.querySelector('span')!.getBoundingClientRect();
        return {
          iconX: icon.left - bounds.left, iconY: icon.top - bounds.top,
          iconWidth: icon.width, iconHeight: icon.height, countX: count.left - bounds.left
        };
      })
    );
    expect(geometry).toEqual([
      { iconX: 5, iconY: 5, iconWidth: 16, iconHeight: 16, countX: 26 },
      { iconX: 5, iconY: 5, iconWidth: 16, iconHeight: 16, countX: 26 }
    ]);
    await trigger.hover();
    const panel = page.getByTestId('composer-background-task-panel');
    await expect(panel).toBeVisible();
    const open = panel.locator('.composer-task-row__open');
    await expect(open).toContainText('fixture-command --wait');
    await open.click();
    await expect(panel.locator('.composer-task-detail')).toContainText('fixture output');
    await panel.locator('.composer-task-stop').click();
    await expect(trigger).toHaveText('0');
    await expect(panel).toHaveCount(0);
    expect(service.requests.some(request =>
      request.method === 'DELETE' && String(request.path).endsWith('/command-sessions/fixture-command')
    )).toBe(true);
    expect(service.commandTasks[0].status).toBe('exited');
  } finally {
    await service.dispose();
  }
});

test('background task output box scrolls inside the panel with a visible thumb', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'fixture-long-output-turn';
  service.turns.push({ turn_id, user_round: 1, status: 'running', content: 'Fixture long output' });
  const longTail = Array.from({ length: 80 }, (_, index) => `line ${index + 1}: compile chunk ${index}`).join('\n');
  service.commandTasks.push({
    command_session_id: 'fixture-long-output', session_id: MOCK_SESSION,
    command: 'fixture-build --watch', cwd: '/fixture', status: 'running',
    started_at: new Date(Date.now() - 5000).toISOString(), updated_at: new Date().toISOString(),
    stdout_tail: longTail, stderr_tail: '', pty_tail: ''
  });
  // 再塞满任务行：浮层必须自己限高，任务列表单独滚动。
  for (let index = 0; index < 6; index += 1) {
    service.commandTasks.push({
      command_session_id: `fixture-extra-${index}`, session_id: MOCK_SESSION,
      command: `fixture-extra-${index} --watch`, cwd: '/fixture', status: 'running',
      started_at: new Date(Date.now() - 4000).toISOString(), updated_at: new Date().toISOString(),
      stdout_tail: 'ready', stderr_tail: '', pty_tail: ''
    });
  }
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    await page.getByTestId('composer-background-tasks').hover();
    const panel = page.getByTestId('composer-background-task-panel');
    await expect(panel).toBeVisible();
    await panel.locator('.composer-task-row__open').first().click();

    const output = panel.locator('pre[data-task-output]');
    await expect(output).toBeVisible();
    const metrics = await output.evaluate(element => {
      const node = element as HTMLElement;
      const thumb = node.parentElement!.querySelector<HTMLElement>('[data-scroll-thumb]')!;
      return {
        overflowing: node.scrollHeight > node.clientHeight + 1,
        atBottom: node.scrollHeight - node.scrollTop - node.clientHeight <= 8,
        thumbOpacity: getComputedStyle(thumb).opacity,
        thumbHeight: Math.round(thumb.getBoundingClientRect().height),
        viewportHeight: node.clientHeight
      };
    });
    expect(metrics.overflowing).toBe(true);
    expect(metrics.atBottom).toBe(true);
    expect(metrics.thumbOpacity).toBe('1');
    expect(metrics.thumbHeight).toBeGreaterThan(16);
    expect(metrics.thumbHeight).toBeLessThan(metrics.viewportHeight);

    // 条目多时任务列表自己滚动，滚条同样可见。
    const listThumb = panel.locator('.composer-task-body > .composer-scroll-track [data-scroll-thumb]');
    expect(await listThumb.evaluate(element => getComputedStyle(element).opacity)).toBe('1');

    // 面板高度封顶在视口内，不会因为展开长输出而顶出屏幕。
    const panelBox = (await panel.boundingBox())!;
    const viewport = page.viewportSize()!;
    expect(panelBox.y).toBeGreaterThanOrEqual(0);
    expect(panelBox.height).toBeLessThanOrEqual(Math.min(viewport.height * 0.58, 420) + 1);

    // 上滚查看历史后不再强行跟随最新输出。
    await output.evaluate(element => { (element as HTMLElement).scrollTop = 0; });
    await page.waitForTimeout(120);
    const scrolled = await output.evaluate(element => (element as HTMLElement).scrollTop);
    expect(scrolled).toBe(0);
  } finally {
    await service.dispose();
  }
});
