import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('subagent dock updates from the composer and survives reload', async ({ page }) => {
  const service = new ChatMockService();
  const turn_id = 'fixture-parent';
  service.turns.push({ turn_id, user_round: 1, status: 'completed', content: 'Fixture task' });
  service.items.set(`${turn_id}:user`, { item_id: `${turn_id}:user`, turn_id, kind: 'user_message',
    role: 'user', content: 'Fixture task', status: 'completed', revision: 1, visibility: 'user' });
  const item = { item_id: `${turn_id}:subagent-run-a`, turn_id, kind: 'subagent_run',
    status: 'completed', visibility: 'user', revision: 1,
    runtime: { session_id: 'fixture-child', run_id: 'run-a', title: 'Fixture worker',
      status: 'running', terminal: false, can_terminate: true, latest_message: 'Working on fixture',
      started_time: Date.now() / 1000 - 65,
      metrics: { child_turn_id: 'fixture-child-turn', tool_calls: 2, account_credits_consumed: 7,
        model_request_count: 3, context_tokens: 2048, max_context: 8192 } } };
  service.items.set(item.item_id, item);
  await service.install(page);
  await page.route('**/chat/sessions/fixture-child/thread-log/snapshot*', route => route.fulfill({
    json: { data: { cursor: 0, turns: [{ turn_id: 'fixture-child-turn', status: 'completed',
      content: 'Fixture child task' }],
      items: [{ item_id: 'fixture-child-tool', turn_id: 'fixture-child-turn', kind: 'tool_call',
        status: 'completed', visibility: 'user', revision: 1, payload: {
          tool: 'read_file', tool_call_id: 'fixture-call', event_type: 'tool_result',
          args: { path: 'fixture.txt' }, result: { ok: true, content: 'Fixture result' }
        } }], blocks: [] } }
  }));
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const trigger = page.getByTestId('composer-subagents');
    await expect(trigger).toHaveText('1');
    // 有子智能体在跑时图标点亮。
    await expect(trigger).toHaveClass(/is-active/);
    await trigger.hover();
    const card = page.locator('.subagent-panel__item');
    await expect(card).toHaveCount(1);
    await expect(card).toContainText('Working on fixture');
    // 统计行只留图标与数字：时长 / 额度消耗 / 上下文占比，细节在 title 里。
    await expect(card.locator('.subagent-panel__stat')).toHaveCount(3);
    await expect(card).not.toContainText('工具');
    await expect(card).not.toContainText('额度 ');
    await expect(card.locator('.subagent-panel__stat').first()).toContainText(/1m \d+s/);
    await expect(card.locator('.subagent-panel__stat').nth(1)).toContainText('7');
    await expect(card).toContainText('25%');
    await expect(card).toContainText('运行中');
    const statsHint = await card.locator('.subagent-panel__stat').nth(1).getAttribute('title');
    expect(statsHint).toContain('模型请求 3');
    await expect.poll(() => service.subscriptions.length).toBeGreaterThan(0);
    item.revision = 2;
    item.runtime.latest_message = 'More fixture progress';
    item.runtime.metrics.tool_calls = 3;
    item.runtime.metrics.model_request_count = 4;
    const progress = { change_seq: 1, cursor: 1, change_type: 'item_upsert',
      turn_id, item_id: item.item_id, revision: 2, payload: structuredClone(item) };
    service.changes.push(progress);
    for (const sub of service.subscriptions) sub.socket.send(JSON.stringify({ type: 'event',
      request_id: sub.request, payload: { event: 'thread_change', data: { session_id: MOCK_SESSION, ...progress } } }));
    await expect.poll(() => service.requests.filter(request =>
      request.method === 'GET' && String(request.path).endsWith('/subagents')
    ).length).toBeGreaterThan(1);
    await expect(card).toContainText('More fixture progress');
    await expect(card).toContainText('运行中');
    item.revision = 3;
    item.runtime.latest_message = 'Fixture completed';
    item.runtime.status = 'success';
    item.runtime.terminal = true;
    item.runtime.can_terminate = false;
    item.runtime.metrics.tool_calls = 4;
    item.runtime.metrics.model_request_count = 5;
    const change = { change_seq: 2, cursor: 2, change_type: 'item_upsert',
      turn_id, item_id: item.item_id, revision: 3, payload: item };
    service.changes.push(change);
    for (const sub of service.subscriptions) sub.socket.send(JSON.stringify({ type: 'event',
      request_id: sub.request, payload: { event: 'thread_change', data: { session_id: MOCK_SESSION, ...change } } }));
    for (let attempt = 0; attempt < 2; attempt++) {
      if (attempt) await page.reload();
      // 完成后数字仍在（可回看结果），但图标不再点亮。
      await expect(trigger).toHaveText('1');
      await expect(trigger).not.toHaveClass(/is-active/);
      await trigger.hover();
      await expect(card).toHaveCount(1);
      await expect(card).toContainText('Fixture completed');
      await expect(card).toContainText('已完成');
      await expect(card.locator('.subagent-panel__stat')).toHaveCount(3);
      await expect(card.locator('.subagent-panel__stop')).toHaveCount(0);
    }
    await card.click();
    const dialog = page.locator('.subagent-panel__dialog');
    await expect(dialog).toBeVisible();
    await expect(dialog.locator('.subagent-panel__stat')).toHaveCount(3);
    // 子线程运行过程与主时间线同一套块渲染：工具批次折叠条 + 条目行。
    const title = dialog.locator('.timeline-group-title').first();
    await expect(title).toBeVisible();
    // Check the real teleported component against its dialog background, not a CSS literal.
    const contrast = await title.evaluate(node => {
      const luminance = (color: string) => {
        const channels = color.match(/[\d.]+/g)!.slice(0, 3).map(value => {
          const channel = Number(value) / 255;
          return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
        });
        return channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
      };
      const foreground = luminance(getComputedStyle(node).color);
      const background = luminance(getComputedStyle(node.closest('.el-dialog')!).backgroundColor);
      return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05);
    });
    expect(contrast).toBeGreaterThanOrEqual(4.5);
    // 最后一批默认展开：条目行直接可见；折叠条可收起再展开。
    const entry = dialog.locator('.tl-entry').first();
    await expect(entry).toBeVisible();
    const head = dialog.locator('.timeline-group-head').first();
    await head.click();
    await expect(entry).toBeHidden();
    await head.click();
    await expect(entry).toBeVisible();
  } finally { await service.dispose(); }
});
