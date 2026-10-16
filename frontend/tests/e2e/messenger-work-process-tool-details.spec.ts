import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('work process shows panel, media, plan and fallback details after durable hydration', async ({ page }) => {
  const service = new ChatMockService();
  service.seedDeferredReasoningTurn();
  const turn = service.turns[0];
  turn.status = 'completed';
  service.items.clear();
  const questions = [{ question: 'Fixture choice?', multiple: true,
    options: [{ label: 'Option A', description: 'Fixture description', recommended: true }, 'Option B'] }];
  const fixtures = [
    { tool: '问询面板', args: { questions }, result: { action: 'question_panel', state: 'awaiting_input', data: { questions } } },
    { tool: '读图工具', args: { path: 'fixtures/sample.png' }, result: { action: 'read_image', data: {
      path: 'fixtures/sample.png', resolved_path: 'hidden-source', media_kind: 'image', size_bytes: 128,
      result: { kind: 'image', frame_count: 1, attachments: [{ public_path: 'hidden-resource' }] }
    } } },
    { tool: '计划面板', args: { plan: [{ step: 'Fixture plan step', status: 'in_progress' }] }, result: { data: { status: 'ok' } } },
    { tool: 'fixture_custom', args: {}, result: { data: { count: 2, query_handle: 'hidden-handle' } } },
    { tool: 'fixture_empty', args: { operation: 'fixture operation' }, result: {} }
  ];
  for (const [index, fixture] of fixtures.entries()) {
    const id = `fixture-tool-${index}`;
    service.items.set(id, { item_id: id, turn_id: turn.turn_id, kind: 'tool_call', status: 'completed',
      revision: 1, tool: fixture.tool, tool_call_id: id, args: fixture.args,
      result: { ok: true, data: fixture.result } });
  }
  const answerId = `${turn.turn_id}:text-1`;
  service.items.set(answerId, { item_id: answerId, turn_id: turn.turn_id, kind: 'assistant_message',
    status: 'completed', revision: 1, content: 'Fixture completed response.' });
  await service.install(page);
  await page.addInitScript(() => localStorage.setItem('wunder_language', 'zh-CN'));
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);

    for (let pass = 0; pass < 2; pass++) {
      if (pass) await page.reload();
      const process = page.locator('.messenger-turn .timeline-turn-process');
      await process.locator('.timeline-process-head').click();
      const entries = process.locator('.tl-entry');
      await expect(entries).toHaveCount(5);
      for (const [index, expected] of [
        ['Fixture choice?', 'Option A', 'Fixture description', '推荐', '多选'],
        ['fixtures/sample.png', '媒体类型: 图片', '字节数: 128', '帧数: 1'],
        ['Fixture plan step', '运行中'],
        ['count', '2'],
        ['已完成', '调用参数', 'fixture operation']
      ].entries()) {
        const entry = entries.nth(index);
        await entry.locator('.tl-entry-head').click();
        const body = entry.locator('.tl-entry-body');
        for (const text of expected) await expect(body).toContainText(text);
        await expect(body).not.toContainText(/hidden-source|hidden-resource|hidden-handle|该工具没有可展示的输出/);
      }
    }
    expect(errors).toEqual([]);
  } finally {
    await service.dispose();
  }
});
