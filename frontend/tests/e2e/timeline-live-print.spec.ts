// AI生成
import { expect, test } from '@playwright/test';

test('a streaming tool round prints once inside the work process and stops when it settles', async ({ page }) => {
  // The round that is still printing already owns a durable print row in the
  // work process. Its live text must land in that row: an appended second block
  // printed the same prose twice, and the fold's text also leaked into the reply
  // area below.
  await page.route('**/thread-log/turns/**', async (route) => {
    await route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        data: {
          turn: {
            has_older: false,
            has_newer: false,
            items: [{
              item_id: 'turn-1:text-1', turn_id: 'turn-1', kind: 'assistant_message',
              status: 'running', revision: 1, item_index: 1, model_round: 1,
              payload: {
                model_round: 1, role: 'assistant',
                content: '中间打印', reasoning: 'thought',
                tool_calls: [{ id: 'call-1', name: 'exec_command' }]
              }
            }]
          }
        }
      })
    });
  });

  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineTurnProcess.vue')).text();
    const runtimeUrl = source.match(/from\s+[\"']([^\"']*\/vue\.js[^\"']*)[\"']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, h, nextTick, reactive }, { default: Process }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineTurnProcess.vue')
    ]);
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;inset:0;z-index:10000;background:#fbfaf8';
    document.body.append(host);
    const model = reactive({
      sessionId: 'fixture-session', turnId: 'turn-1', count: 1,
      status: 'running', liveTail: '中间打印，继续输出', defaultOpen: true,
      // The view row owns the identity of the round that is printing; without it
      // the live text cannot be placed into that round's own row.
      finalItemId: 'turn-1:text-1'
    });
    const app = createApp({ setup: () => () => h(Process, model) });
    const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    try {
      app.mount(host);
      await frame(); await nextTick(); await frame();
      const liveNodes = [...host.querySelectorAll('.timeline-live-print')];
      const liveTexts = liveNodes.map((node) => (node.textContent || '').trim());
      // Every leaf that shows the round's prose: exactly one, and it is the live
      // row. A second leaf was the duplicate that also leaked into the transcript.
      const occurrences = [...host.querySelectorAll('*')]
        .filter((node) => node.children.length === 0 && (node.textContent || '').includes('中间打印'))
        .length;
      const settledBefore = liveNodes.length;
      model.status = 'completed';
      model.liveTail = '';
      await frame(); await nextTick(); await frame();
      const afterSettle = host.querySelectorAll('.timeline-live-print').length;
      const durablePrint = [...host.querySelectorAll('[data-process-key]')]
        .some((node) => (node.textContent || '').includes('中间打印'));
      return { settledBefore, liveTexts, occurrences, afterSettle, durablePrint };
    } finally {
      app.unmount(); host.remove();
    }
  });

  expect(result.liveTexts).toEqual(['中间打印，继续输出']);
  // One leaf carries the text: the live row is the durable row, not a second copy.
  expect(result.occurrences).toBe(1);
  expect(result.settledBefore).toBe(1);
  expect(result.afterSettle).toBe(0);
  expect(result.durablePrint).toBe(true);
});
