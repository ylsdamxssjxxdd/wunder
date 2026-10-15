import { expect, test } from '@playwright/test';

test('thread lifecycle updates root and expanded directories without a manual refresh', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await page.goto('/__e2e/composer-b4');
  const result = await page.evaluate(async () => {
    const panelUrl = '/src/views/messenger/workspace/WorkspaceFilesPanel.vue';
    const source = await (await fetch(panelUrl)).text();
    const vueUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    const targetSource = await (await fetch('/src/views/messenger/interlink/interlinkNodeTarget.ts')).text();
    const routerUrl = targetSource.match(/from\s+["']([^"']*\/vue-router\.js[^"']*)["']/)?.[1];
    if (!vueUrl || !routerUrl) throw new Error('Missing fixture runtime');
    const [{ createApp, h, nextTick }, { createRouter, createMemoryHistory }, { default: Panel },
      { default: api }, { applyChatThreadServerEvent }, { applyThreadViewEffects }] = await Promise.all([
      import(vueUrl), import(routerUrl), import(panelUrl), import('/src/api/http.ts'),
      import('/src/realtime/chat/chatThreadRuntime.ts'), import('/src/stores/chatThreadEffects.ts')
    ]);
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;inset:0;width:400px;z-index:10000;background:white';
    document.body.append(host);
    const originalAdapter = api.defaults.adapter;
    let rootFiles: string[] = [];
    let nestedFiles: string[] = [];
    let scans = 0;
    const readPaths: string[] = [];
    const makeEntry = (path: string, type = 'file') => ({ path, name: path.split('/').at(-1), type, size: 0 });
    api.defaults.adapter = async (config: any) => {
      const path = String(config.params?.path || '');
      let data: unknown = {};
      if (String(config.url).includes('/workspace/stats')) {
        data = { files: rootFiles.length + nestedFiles.length, used_bytes: 0 };
      } else if (String(config.url).includes('/workspace')) {
        readPaths.push(path);
        if (config.params?.refresh_tree) scans++;
        const entries = path ? nestedFiles.map(name => makeEntry(`reports/${name}`))
          : [makeEntry('reports', 'dir'), ...rootFiles.map(name => makeEntry(name))];
        data = { entries, path, total: entries.length, offset: 0, limit: 100,
          workspace_id: 'fixture-workspace', tree_version: 7 };
      } else if (String(config.url).includes('/interlink/nodes')) {
        data = { data: { nodes: [], total: 0 } };
      }
      return { status: 200, statusText: 'OK', headers: {}, config, data };
    };
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: {} }] });
    await router.push('/'); await router.isReady();
    const app = createApp({ setup: () => () => h(Panel) });
    app.use(router);
    const store = { activeSessionId: '', sessions: [{ id: 'fixture-lifecycle' }], loadingBySession: {} };
    let revision = 0;
    const applyStatus = (status: string) => {
      revision++;
      applyChatThreadServerEvent(store, 'fixture-lifecycle', 'thread_view_change', {
        epoch: 1, view_seq: revision, rows: [{ root_turn_id: 'fixture-turn', user_round: 1, revision, status,
          user: { item_id: 'fixture-user', content: 'question' },
          assistant: { item_id: 'fixture-answer', content: 'output' }, workflow_count: 0, failed_count: 0 }]
      }, { onViewApplied: prior => applyThreadViewEffects(store, 'fixture-lifecycle', prior) });
    };
    const waitFor = async (check: () => boolean) => {
      const start = performance.now();
      while (!check()) {
        if (performance.now() - start > 5000) throw new Error('Workspace fixture timed out');
        await new Promise(requestAnimationFrame);
      }
      await nextTick();
    };
    const names = () => [...host.querySelectorAll('.workspace-file-name')].map(node => node.textContent);
    try {
      app.mount(host);
      await waitFor(() => names().includes('reports'));
      host.querySelector<HTMLElement>('.workspace-file-twisty')!.click();
      await waitFor(() => readPaths.includes('reports'));
      applyStatus('running');
      rootFiles = ['completed.txt']; nestedFiles = ['completed-child.txt'];
      applyStatus('completed');
      await waitFor(() => names().includes('completed-child.txt'));
      const completed = names();
      const scansAfterCompletion = scans;
      applyStatus('completed');
      await new Promise(requestAnimationFrame);
      const replayScans = scans - scansAfterCompletion;
      applyStatus('running');
      rootFiles.push('failed.txt'); nestedFiles.push('failed-child.txt');
      applyStatus('failed');
      await waitFor(() => names().includes('failed-child.txt'));
      applyStatus('running');
      rootFiles.push('waiting.txt'); nestedFiles.push('waiting-child.txt');
      applyStatus('waiting_input');
      await waitFor(() => names().includes('waiting-child.txt'));
      applyStatus('running');
      rootFiles.push('stopped.txt'); nestedFiles.push('stopped-child.txt');
      applyStatus('cancelled');
      await waitFor(() => names().includes('stopped-child.txt'));
      return { completed, final: names(), replayScans,
        expanded: host.querySelector('[role="treeitem"]')?.getAttribute('aria-expanded'), scans };
    } finally {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter;
    }
  });
  expect(result.completed).toContain('completed.txt');
  expect(result.completed).toContain('completed-child.txt');
  expect(result.final).toEqual(expect.arrayContaining(['failed.txt', 'failed-child.txt',
    'waiting.txt', 'waiting-child.txt', 'stopped.txt', 'stopped-child.txt']));
  expect(result.replayScans).toBe(0);
  expect(result.scans).toBe(4);
  expect(result.expanded).toBe('true');
  expect(errors).toEqual([]);
});
