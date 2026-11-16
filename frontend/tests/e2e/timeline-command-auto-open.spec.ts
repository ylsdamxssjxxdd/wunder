import { expect, test } from '@playwright/test';

test('a running command entry opens itself and folds once it settles', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineBlocks.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, h, nextTick, reactive }, { default: Blocks }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineBlocks.vue')
    ]);
    const host = document.createElement('div');
    document.body.append(host);
    const run = reactive({ status: 'running' });
    const workflowItems = [{
      id: 'fixture-call', eventType: 'tool_call', toolName: 'execute_command',
      toolCallId: 'fixture-call', status: 'running',
      detail: JSON.stringify({ args: { description: 'fixture title', content: 'fixture-command --wait' } })
    }];
    const blocks = [
      { kind: 'activity', id: 'act-1', seq: 1, rows: [{ type: 'tool', itemId: 'fixture-call' }] },
      { kind: 'body', id: 'body-1', seq: 2, round: 1, text: 'final answer' }
    ];
    const app = createApp({
      setup: () => () => h(Blocks, {
        blocks,
        workflowItems: workflowItems.map((item) => ({ ...item, status: run.status })),
        identityKey: 'fixture-turn', defaultOpen: true, streaming: true
      })
    });
    try {
      app.mount(host);
      await nextTick(); await nextTick();
      const head = host.querySelector('.tl-entry-head') as HTMLButtonElement | null;
      const running = {
        head: Boolean(head),
        expanded: head?.getAttribute('aria-expanded'),
        output: host.querySelector('.tl-entry-output') !== null
      };
      // One command session frame arrives while it runs: still open, no click.
      run.status = 'running';
      await nextTick(); await nextTick();
      const stillOpen = host.querySelector('.tl-entry-head')?.getAttribute('aria-expanded');
      // The command settles: the entry folds itself again.
      run.status = 'completed';
      await nextTick(); await nextTick(); await nextTick();
      const settled = {
        expanded: host.querySelector('.tl-entry-head')?.getAttribute('aria-expanded'),
        output: host.querySelector('.tl-entry-output') !== null
      };
      // A reader who opened it keeps it open through the next settle frame.
      host.querySelector<HTMLButtonElement>('.tl-entry-head')!.click();
      await nextTick();
      const readerOpened = host.querySelector('.tl-entry-head')?.getAttribute('aria-expanded');
      return { running, stillOpen, settled, readerOpened };
    } finally {
      app.unmount();
      host.remove();
    }
  });
  expect(result).toEqual({
    running: { head: true, expanded: 'true', output: true },
    stillOpen: 'true',
    settled: { expanded: 'false', output: false },
    readerOpened: 'true'
  });
});
