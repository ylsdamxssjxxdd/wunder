import { expect, test } from '@playwright/test';

test('thinking previews follow live output and preserve a reader scrolling upward', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineThinkingEntry.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, reactive, nextTick, h }, { default: Thinking }, { reasoningPreview }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineThinkingEntry.vue'),
      import('/src/components/chat/timelineActivity.ts')
    ]);
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;inset:0;z-index:10000;padding:20px;background:#fbfaf8';
    document.body.append(host);
    const model = reactive({ open: true, entry: {
      kind: 'reasoning', key: 'fixture', streaming: true,
      text: 'fixture thought\n'.repeat(80), summary: ''
    } });
    model.entry.summary = reasoningPreview(model.entry.text);
    const app = createApp({ setup: () => () => h(Thinking, model) });
    app.mount(host);
    await nextTick();
    model.entry.text += 'fixture latest';
    model.entry.summary = reasoningPreview(model.entry.text);
    await nextTick(); await nextTick();
    const body = host.querySelector<HTMLElement>('.tl-thinking-text')!;
    const summary = host.querySelector<HTMLElement>('.tl-thinking-summary')!;
    const followed = body.scrollTop > 0 && body.scrollHeight - body.clientHeight - body.scrollTop < 2;
    const previewFollowed = summary.scrollLeft > 0 && summary.scrollWidth - summary.clientWidth - summary.scrollLeft < 2;
    body.scrollTop = 0;
    body.dispatchEvent(new Event('scroll'));
    model.entry.text += '\nfixture after scroll';
    model.entry.summary = reasoningPreview(model.entry.text);
    await nextTick(); await nextTick();
    const result = { followed, previewFollowed, preserved: body.scrollTop === 0,
      latest: body.textContent?.endsWith('fixture after scroll') };
    app.unmount(); host.remove();
    return result;
  });
  expect(result).toEqual({ followed: true, previewFollowed: true, preserved: true, latest: true });
});

test('process header shows phase and steps without output, failure counts or entry metrics', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineTurnProcess.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, reactive, nextTick, h }, { default: Process }, { default: Tool }, { buildTimelineToolEntries }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineTurnProcess.vue'),
      import('/src/components/chat/MessageTimelineToolEntry.vue'), import('/src/components/chat/toolTimelineModel.ts')
    ]);
    const host = document.createElement('div'); document.body.append(host);
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 3, status: 'running',
      activity: { kind: 'tool', tool: 'fixture_tool' }, reasoning: 'fixture private output' });
    const entry = buildTimelineToolEntries([{ id: 'fixture-call', eventType: 'tool_call', isTool: true,
      toolName: 'fixture_tool', status: 'completed', detail: '{}', elapsed: 30, tokens: 2000 }], (key: string) => key).entries[0];
    const app = createApp({ setup: () => () => [h(Process, model), h(Tool, { entry })] });
    app.mount(host); await nextTick();
    const before = host.querySelector('.timeline-process-head')?.textContent || '';
    model.activity = { kind: 'thinking', tool: '' };
    await nextTick();
    const after = host.querySelector('.timeline-process-head')?.textContent || '';
    const result = { tool: before.includes('fixture_tool'), output: before.includes('fixture private output'),
      changed: before !== after, connector: Boolean(host.querySelector('.timeline-process-gutter-line')),
      metrics: Boolean(host.querySelector('.tl-entry-metrics')) };
    app.unmount(); host.remove();
    return result;
  });
  expect(result).toEqual({ tool: true, output: false, changed: true, connector: false, metrics: false });
});

test('settling a live thought reloads its complete durable text without changing the step count', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineTurnProcess.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, reactive, nextTick, h }, { default: Process }, { default: api }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineTurnProcess.vue'), import('/src/api/http.ts')
    ]);
    const host = document.createElement('div'); document.body.append(host);
    const originalAdapter = api.defaults.adapter;
    let requests = 0;
    const complete = `${'fixture complete thought\n'.repeat(80)}fixture final tail`;
    api.defaults.adapter = async (config: unknown) => {
      requests++;
      return { status: 200, statusText: 'OK', headers: {}, config, data: { data: { turn: {
        items: [{ item_id: 'fixture-answer', item_index: 1, kind: 'assistant_message', status: 'completed',
          payload: { reasoning: complete, content: '' } }], has_older: false, has_newer: false
      } } } };
    };
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 1, status: 'running',
      activity: { kind: 'thinking' }, reasoning: 'fixture live tail', reasoningItemId: 'fixture-answer',
      reasoningStreaming: true, finalItemId: 'fixture-answer' });
    const app = createApp({ setup: () => () => h(Process, model) });
    const until = async (predicate: () => boolean) => {
      const deadline = Date.now() + 4000;
      while (!predicate()) {
        if (Date.now() > deadline) throw new Error('Fixture did not update');
        await new Promise(resolve => setTimeout(resolve, 25));
      }
    };
    try {
      app.mount(host); await nextTick();
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      await until(() => Boolean(host.querySelector('.tl-thinking-head')));
      host.querySelector<HTMLButtonElement>('.tl-thinking-head')!.click();
      await nextTick();
      const live = host.querySelector('.tl-thinking-text')?.textContent;
      model.reasoningStreaming = false; model.status = 'completed';
      await until(() => requests >= 2 && host.querySelector('.tl-thinking-text')?.textContent === complete);
      return { live, complete: host.querySelector('.tl-thinking-text')?.textContent === complete, steps: model.count };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ live: 'fixture live tail', complete: true, steps: 1 });
});
