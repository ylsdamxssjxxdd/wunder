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

test('projection timeline paints one thought when a live handoff repeats its item in two activity blocks', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineBlocks.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, h, nextTick }, { default: Blocks }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineBlocks.vue')
    ]);
    const host = document.createElement('div');
    document.body.append(host);
    const itemId = 'fixture-turn:text-1';
    const blocks = [
      { kind: 'activity', id: 'act-1', seq: 1, rows: [{ type: 'reasoning', itemId, text: 'durable thought', streaming: false }] },
      { kind: 'body', id: 'body-1', seq: 2, round: 1, text: '' },
      { kind: 'activity', id: 'act-2', seq: 3, rows: [{ type: 'reasoning', itemId, text: 'durable thought', streaming: true }] },
      { kind: 'body', id: 'body-2', seq: 4, round: 2, text: 'final answer' }
    ];
    const app = createApp({ setup: () => () => h(Blocks, {
      blocks, workflowItems: [], identityKey: 'fixture-turn', defaultOpen: true, streaming: true
    }) });
    try {
      app.mount(host);
      await nextTick(); await nextTick();
      return {
        entries: host.querySelectorAll('.tl-thinking-head').length,
        text: host.querySelector('.tl-thinking-summary')?.textContent?.includes('durable thought') ?? false
      };
    } finally {
      app.unmount(); host.remove();
    }
  });
  expect(result).toEqual({ entries: 1, text: true });
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
    let settled = false;
    const complete = `${'fixture complete thought\n'.repeat(80)}fixture final tail`;
    api.defaults.adapter = async (config: unknown) => {
      requests++;
      return { status: 200, statusText: 'OK', headers: {}, config, data: { data: { turn: {
        items: [{ item_id: 'fixture-answer', item_index: 1, kind: 'assistant_message', status: settled ? 'completed' : 'running',
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
      settled = true; model.reasoningStreaming = false; model.status = 'completed';
      await until(() => requests >= 2 && host.querySelector('.tl-thinking-text')?.textContent === complete);
      return { live, complete: host.querySelector('.tl-thinking-text')?.textContent === complete, steps: model.count };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ live: 'fixture live tail', complete: true, steps: 1 });
});

test('a durable thought and a live preview share one canonical process identity', async ({ page }) => {
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
    const durable = 'fixture durable thought';
    api.defaults.adapter = async (config: unknown) => ({
      status: 200, statusText: 'OK', headers: {}, config,
      data: { data: { turn: { items: [{
        item_id: 'fixture-turn:text-1', item_index: 1, kind: 'assistant_message', status: 'running',
        payload: { reasoning: durable, content: '' }
      }], has_older: false, has_newer: false } } }
    });
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 1, status: 'running',
      activity: { kind: 'thinking' }, reasoning: durable, reasoningItemId: 'fixture-turn:text-1',
      reasoningStreaming: true, finalItemId: 'fixture-turn:text-1' });
    const app = createApp({ setup: () => () => h(Process, model) });
    const until = async (predicate: () => boolean) => {
      const deadline = Date.now() + 4000;
      while (!predicate()) {
        if (Date.now() > deadline) throw new Error(`Fixture did not update: ${host.textContent}`);
        await new Promise(resolve => setTimeout(resolve, 25));
      }
    };
    try {
      app.mount(host); await nextTick();
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      await until(() => host.querySelectorAll('.tl-thinking-head').length === 1);
      model.reasoning = `${durable} extended`;
      await until(() => host.querySelector('.tl-thinking-summary')?.textContent?.includes('extended') ?? false);
      return {
        entries: host.querySelectorAll('.tl-thinking-head').length,
        updated: host.querySelector('.tl-thinking-summary')?.textContent?.includes('extended') ?? false
      };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ entries: 1, updated: true });
});

test('continuous work events refresh within a bounded window and never overwrite a completed prefix thought', async ({ page }) => {
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const source = await (await fetch('/src/components/chat/MessageTimelineTurnProcess.vue')).text();
    const runtimeUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!runtimeUrl) throw new Error('Missing fixture Vue runtime');
    const [{ createApp, reactive, nextTick, h }, { default: Process }, { default: api }] = await Promise.all([
      import(runtimeUrl), import('/src/components/chat/MessageTimelineTurnProcess.vue'), import('/src/api/http.ts')
    ]);
    const host = document.createElement('div');
    document.body.append(host);
    const originalAdapter = api.defaults.adapter;
    const thought = (round: number, reasoning: string) => ({ item_id: 'fixture-turn:text-'+round, turn_id: 'fixture-turn',
      kind: 'assistant_message', status: 'completed', item_index: round, payload: { model_round: round, reasoning } });
    let requests = 0;
    let firstRefresh = 0;
    let started = 0;
    api.defaults.adapter = async (config: unknown) => {
      requests++;
      if (requests === 2) firstRefresh = performance.now() - started;
      return { status: 200, statusText: 'OK', headers: {}, config, data: { data: { turn: {
        items: [thought(1, 'Fixture shared completed')], has_older: false, has_newer: false
      } } } };
    };
    const model = reactive({ sessionId: 'fixture-session', turnId: 'fixture-turn', count: 1, status: 'running',
      reasoning: '', reasoningItemId: '', reasoningStreaming: false, processItems: [] as unknown[], processRevision: 0 });
    const app = createApp({ render: () => h(Process, model) });
    try {
      app.mount(host);
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      while (!host.querySelector('.tl-thinking-head')) await new Promise(resolve => setTimeout(resolve, 10));
      model.reasoning = 'Fixture shared'; model.reasoningItemId = 'fixture-turn:text-2'; model.reasoningStreaming = true;
      await nextTick();
      const tool = { item_id: 'fixture-turn:tool-first', turn_id: 'fixture-turn', kind: 'tool_call', status: 'running',
        payload: { model_round: 2, process_seq: 3, tool: 'read_file', tool_call_id: 'fixture-first', args: { path: 'fixture.txt' } } };
      model.processItems = [tool]; model.processRevision++;
      const toolStarted = performance.now();
      await nextTick(); await nextTick();
      const onlineToolVisible = Boolean(host.querySelector('.tl-entry-head'));
      const onlineLatencyMs = performance.now() - toolStarted;
      started = performance.now();
      for (let index = 2; index <= 15; index++) {
        model.count = index;
        await new Promise(resolve => setTimeout(resolve, 30));
      }
      const entries = host.querySelectorAll('.tl-thinking');
      return { entries: entries.length, oldUntouched: entries[0]?.textContent?.includes('Fixture shared completed') ?? false,
        liveAtEnd: entries[1]?.textContent?.includes('Fixture shared') ?? false, requests, firstRefresh, onlineToolVisible, onlineLatencyMs };
    } finally { app.unmount(); host.remove(); api.defaults.adapter = originalAdapter; }
  });
  expect(result.entries).toBe(2);
  expect(result.oldUntouched).toBe(true);
  expect(result.liveAtEnd).toBe(true);
  expect(result.onlineToolVisible).toBe(true);
  expect(result.onlineLatencyMs).toBeLessThan(200);
  expect(result.requests).toBeGreaterThan(1);
  expect(result.firstRefresh).toBeGreaterThan(0);
  expect(result.firstRefresh).toBeLessThan(400);
  console.log(JSON.stringify({ processDelivery: result }));
});

test('a bounded live thought binds to the same model round as its full durable process row', async ({ page }) => {
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
    const durable = 'full durable thought '.repeat(80) + 'durable tail';
    api.defaults.adapter = async (config: unknown) => ({
      status: 200, statusText: 'OK', headers: {}, config,
      data: { data: { turn: { items: [{
        item_id: 'fixture-turn:text-2', item_index: 2, kind: 'assistant_message', status: 'running',
        payload: { model_round: 2, reasoning: durable, content: '' }
      }], has_older: false, has_newer: false } } }
    });
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 1, status: 'running',
      activity: { kind: 'thinking' }, reasoning: durable.slice(-512), reasoningItemId: 'fixture-turn:text-2',
      reasoningStreaming: true, finalItemId: 'fixture-turn:text-2' });
    const app = createApp({ setup: () => () => h(Process, model) });
    const until = async (predicate: () => boolean) => {
      const deadline = Date.now() + 4000;
      while (!predicate()) {
        if (Date.now() > deadline) throw new Error(`Fixture did not update: ${host.textContent}`);
        await new Promise(resolve => setTimeout(resolve, 25));
      }
    };
    try {
      app.mount(host); await nextTick();
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      await until(() => host.querySelectorAll('.tl-thinking-head').length === 1);
      return {
        entries: host.querySelectorAll('.tl-thinking-head').length,
        text: host.querySelector('.tl-thinking-summary')?.textContent?.includes('durable tail') ?? false
      };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ entries: 1, text: true });
});

test('a corrected final identity immediately restores intermediate prose to the process timeline', async ({ page }) => {
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
    api.defaults.adapter = async (config: unknown) => ({
      status: 200, statusText: 'OK', headers: {}, config,
      data: { data: { turn: { items: [
        { item_id: 'fixture-intermediate', item_index: 1, kind: 'assistant_message', status: 'completed',
          payload: { content: 'fixture intermediate prose' } }
      ], has_older: false, has_newer: false } } }
    });
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 1, status: 'running',
      activity: { kind: 'tool', tool: 'fixture_tool' }, finalItemId: 'fixture-intermediate' });
    const app = createApp({ setup: () => () => h(Process, model) });
    try {
      app.mount(host); await nextTick();
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      const until = async (predicate: () => boolean) => {
        const deadline = Date.now() + 4000;
        while (!predicate()) {
          if (Date.now() > deadline) throw new Error('Fixture did not update');
          await new Promise(resolve => setTimeout(resolve, 25));
        }
      };
      await until(() => Boolean(host.querySelector('.timeline-process-body')));
      const before = host.textContent?.includes('fixture intermediate prose') ?? false;
      model.finalItemId = 'fixture-final';
      await until(() => host.textContent?.includes('fixture intermediate prose') ?? false);
      return { before, after: host.textContent?.includes('fixture intermediate prose') ?? false };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ before: false, after: true });
});

test('automatic compaction system records render as one process entry', async ({ page }) => {
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
    api.defaults.adapter = async (config: unknown) => ({
      status: 200, statusText: 'OK', headers: {}, config,
      data: { data: { turn: { items: [{
        item_id: 'fixture-compaction', item_index: 3, kind: 'system_message', status: 'completed',
        payload: {
          content: 'Retained compacted history.',
          meta: { type: 'compaction_summary', compaction_id: 'cmp-fixture' }
        }
      }], has_older: false, has_newer: false } } }
    });
    const model = reactive({ sessionId: 'fixture-thread', turnId: 'fixture-turn', count: 1, status: 'completed' });
    const app = createApp({ setup: () => () => h(Process, model) });
    try {
      app.mount(host); await nextTick();
      host.querySelector<HTMLButtonElement>('.timeline-process-head')!.click();
      const until = async (predicate: () => boolean) => {
        const deadline = Date.now() + 4000;
        while (!predicate()) {
          if (Date.now() > deadline) throw new Error(`Fixture did not render: ${host.textContent}`);
          await new Promise(resolve => setTimeout(resolve, 25));
        }
      };
      await until(() => (host.textContent?.includes('上下文压缩') ?? false));
      host.querySelector<HTMLElement>('.tl-entry-head')?.click();
      await until(() => host.textContent?.includes('Retained compacted history.') ?? false);
      return {
        compaction: host.textContent?.includes('上下文压缩') ?? false,
        summary: host.textContent?.includes('Retained compacted history.') ?? false
      };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result).toEqual({ compaction: true, summary: true });
});
