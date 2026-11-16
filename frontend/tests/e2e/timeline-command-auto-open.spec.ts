import { expect, test } from '@playwright/test';

test('command entries stay manually folded and refresh terminal status through the content clock', async ({ page }) => {
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
    const state = reactive({
      version: 0,
      items: [{
        id: 'fixture-call', eventType: 'tool_call', toolName: 'execute_command',
        toolCallId: 'fixture-call', status: 'running',
        detail: JSON.stringify({ args: { description: 'fixture title', content: 'fixture-command --wait' } })
      }] as Array<Record<string, unknown>>
    });
    const blocks = [
      { kind: 'activity', id: 'act-1', seq: 1, rows: [{ type: 'tool', itemId: 'fixture-call' }] },
      { kind: 'body', id: 'body-1', seq: 2, round: 1, text: 'final answer' }
    ];
    const app = createApp({
      setup: () => () => h(Blocks, {
        blocks,
        workflowItems: state.items,
        contentVersion: state.version,
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
      head!.click();
      await nextTick();
      const readerOpened = head!.getAttribute('aria-expanded');

      // Mutate the existing projection in place, just like the real runtime.
      // Only the content clock tells the computed tool model to rebuild.
      state.items[0].status = 'completed';
      state.items.push({
        id: 'fixture-result', eventType: 'tool_result', toolName: 'execute_command',
        toolCallId: 'fixture-call', status: 'completed',
        detail: JSON.stringify({ data: {
          status: 'exited', exit_code: 0, stdout: 'Fixture output\n', stderr: 'Fixture warning\n', error: null
        } })
      });
      state.version += 1;
      await nextTick(); await nextTick(); await nextTick();
      const settled = {
        expanded: head!.getAttribute('aria-expanded'),
        output: host.querySelector('.tl-entry-output-body')?.textContent ?? '',
        check: host.querySelectorAll('.tl-entry-status .fa-check').length,
        cross: host.querySelectorAll('.tl-entry-status .fa-xmark').length,
        spinning: host.querySelectorAll('.tl-entry-status .fa-spin').length
      };
      return { running, readerOpened, settled };
    } finally {
      app.unmount();
      host.remove();
    }
  });
  expect(result).toEqual({
    running: { head: true, expanded: 'false', output: false },
    readerOpened: 'true',
    settled: {
      expanded: 'true', output: 'Fixture output\n\nFixture warning\n', check: 1, cross: 0, spinning: 0
    }
  });
});
