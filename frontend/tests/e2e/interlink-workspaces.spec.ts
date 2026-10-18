import { expect, test, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';

const workspaces = [
  { workspace_id: '', name: '', file: 'default.txt', thread: 'Default thread' },
  { workspace_id: 'fixture_one', name: 'Project one', file: 'one.txt', thread: 'Thread one' },
  { workspace_id: 'fixture_two', name: 'Project two', file: 'two.txt', thread: 'Thread two' }
];
const threads = workspaces.map((workspace, index) => ({
  local_thread_id: `fixture_thread_${index}`, workspace_id: workspace.workspace_id,
  title: workspace.thread, status: 'ready'
}));

async function prepare(page: Page) {
  const commands = new Map<string, any>();
  await page.setViewportSize({ width: 1600, height: 1000 });
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route(url => url.pathname.startsWith('/wunder/'), async route => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    let data: any = { data: { items: [] } };
    if (path === '/wunder/interlink/nodes') {
      data = { data: { nodes: [{
        node_id: 'device:fixture_device', node_type: 'desktop', label: 'Fixture device',
        status: 'online', connected: true, capabilities: ['query.basic', 'workspace.write'],
        meta: { workspaces }
      }], total: 1 } };
    } else if (path.endsWith('/shadow')) {
      data = { data: { device_id: 'fixture_device', revision: 1, threads,
        workspace: { workspaces: workspaces.map(workspace => ({ ...workspace,
          tree: [{ path: workspace.file, kind: 'file', size: 13 }]
        })) } } };
    } else if (path === '/wunder/interlink/commands' && request.method() === 'POST') {
      const command = request.postDataJSON();
      commands.set(command.command_id, command);
      data = { data: { command_id: command.command_id, status: 'issued' } };
    } else if (path.startsWith('/wunder/interlink/commands/')) {
      const command = commands.get(path.split('/').at(-1)!);
      if (command) {
        const workspace = workspaces.find(item => item.workspace_id === (command.args.workspace_id || ''))!;
        const result = command.kind === 'threads.list' ? { threads: threads.filter(thread => thread.workspace_id === command.args.workspace_id) }
          : command.kind === 'workspace.list' ? { entries: [{ path: workspace.file, name: workspace.file, kind: 'file', size: 13 }], total: 1 }
          : command.kind === 'workspace.read' ? { inline: Buffer.from('Fixture bytes').toString('base64'), size: 13, truncated: false }
          : { items: [{ role: 'assistant', content: 'Fixture history' }] };
        data = { data: { command_id: command.command_id, kind: command.kind, status: 'succeeded', result } };
      }
    } else if (path === '/wunder/workspace') {
      data = { entries: [{ name: 'cloud.txt', path: 'cloud.txt', type: 'file', size: 5 }], total: 1, path: '' };
    } else if (path === '/wunder/workspace/stats') {
      data = { path: '', files: 1, dirs: 0, used_bytes: 5, quota_bytes: null, recent: [] };
    }
    await route.fulfill({ json: data });
  });
  await page.goto('/__e2e/messenger-view-performance?session_id=perf-session-a');
  await page.waitForFunction(() => Boolean((window as any).__messengerViewPerformanceE2E));
  await page.evaluate(async () => {
    const routerPath = '/src/router/index.ts';
    const { default: router } = await import(routerPath);
    const replace = router.replace.bind(router);
    // Exercise real query changes while retaining the unauthenticated shell.
    router.replace = location => replace(typeof location === 'object'
      ? { ...location, path: '/__e2e/messenger-view-performance' } : location);
  });
  return commands;
}

test('local workspace rows own their threads and files, and cloud remains expandable', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await prepare(page);
  const rows = page.locator('.messenger-sidebar-workspace-row');
  const cloud = rows.filter({ has: page.locator('.fa-cloud') });
  await expect(rows).toHaveCount(4);
  await expect(page.locator('.workspace-file-name')).toHaveText(['cloud.txt']);
  await expect(page.locator('.mz-thread-select')).toHaveCount(2);
  for (let pass = 0; pass < 2; pass += 1) {
    for (const workspace of workspaces) {
      const label = workspace.name || /本地工作区|Local workspace/;
      await rows.filter({ hasText: label }).click();
      await expect(page.locator('.interlink-thread-row')).toHaveText([workspace.thread]);
      await expect(page.locator('.workspace-file-name')).toHaveText([workspace.file]);
      await expect(page.locator('.remote-thread-controls')).toHaveCount(0);
      await expect(cloud.locator('.fa-cloud')).toHaveCount(1);
      await cloud.click();
      await expect(page).not.toHaveURL(/[?&](node|ws)=/);
      await expect(page.locator('.mz-thread-select')).toHaveCount(2);
      await expect(page.locator('.workspace-file-name')).toHaveText(['cloud.txt']);
    }
  }
  expect(errors).toEqual([]);
});

test('local file menu downloads exact bytes within the selected workspace', async ({ page }) => {
  const commands = await prepare(page);
  await page.locator('.messenger-sidebar-workspace-row').filter({ hasText: 'Project two' }).click();
  const file = page.locator('.workspace-file-name', { hasText: 'two.txt' });
  await expect(file).toBeVisible();
  await file.click({ button: 'right' });
  const menu = page.locator('.workspace-files-menu-item');
  await expect(menu.filter({ hasText: /预览|Preview/ })).toBeVisible();
  await expect(menu.filter({ hasText: /重命名|Rename/ })).toBeVisible();
  await expect(menu.filter({ hasText: /删除|Delete/ })).toBeVisible();
  const downloadReady = page.waitForEvent('download');
  await menu.filter({ hasText: /下载|Download/ }).click();
  const download = await downloadReady;
  expect(download.suggestedFilename()).toBe('two.txt');
  expect(await readFile((await download.path())!, 'utf8')).toBe('Fixture bytes');
  const read = [...commands.values()].find(command => command.kind === 'workspace.read');
  expect(read?.args).toMatchObject({ workspace_id: 'fixture_two', path: 'two.txt' });
});

test('late local listing cannot overwrite cloud files after a workspace switch', async ({ page }) => {
  const commands = await prepare(page);
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  let finish!: () => void;
  const finished = new Promise<void>(resolve => { finish = resolve; });
  await page.route('**/wunder/interlink/commands/*', async route => {
    const id = new URL(route.request().url()).pathname.split('/').at(-1)!;
    const command = commands.get(id);
    if (command?.kind !== 'workspace.list') return route.fallback();
    await gate;
    try {
      await route.fulfill({ json: { data: { command_id: id, status: 'succeeded', result: {
        entries: [{ name: 'stale.txt', path: 'stale.txt', kind: 'file' }], total: 1
      } } } });
    } finally { finish(); }
  });
  try {
    await page.locator('.messenger-sidebar-workspace-row').filter({ hasText: 'Project one' }).click();
    await expect.poll(() => [...commands.values()].some(command => command.kind === 'workspace.list')).toBe(true);
    await page.locator('.messenger-sidebar-workspace-row').filter({ has: page.locator('.fa-cloud') }).click();
    await expect(page.locator('.workspace-file-name')).toHaveText(['cloud.txt']);
    release();
    await finished;
    await expect(page.locator('.workspace-file-name', { hasText: 'stale.txt' })).toHaveCount(0);
    await expect(page.locator('.mz-thread-select')).toHaveCount(2);
    await expect(page.locator('.el-message--error')).toHaveCount(0);
  } finally { release(); }
});

test('local threads load bounded pages in the selected workspace', async ({ page }) => {
  const commands = await prepare(page);
  const catalog = Array.from({ length: 105 }, (_, index) => ({
    local_thread_id: `paged_${index}`, title: `Paged thread ${index}`,
    workspace_id: 'fixture_one', status: 'ready'
  }));
  await page.route('**/wunder/interlink/commands/*', route => {
    const command = commands.get(new URL(route.request().url()).pathname.split('/').at(-1)!);
    if (command?.kind !== 'threads.list') return route.fallback();
    expect(command.args.workspace_id).toBe('fixture_one');
    expect(command.args.limit).toBe(100);
    return route.fulfill({ json: { data: { command_id: command.command_id, status: 'succeeded', result: {
      threads: catalog.slice(command.args.offset, command.args.offset + command.args.limit), total: catalog.length
    } } } });
  });
  await page.locator('.messenger-sidebar-workspace-row').filter({ hasText: 'Project one' }).click();
  await expect(page.locator('.interlink-thread-row')).toHaveCount(100);
  await page.locator('.interlink-thread-tree').getByRole('button', { name: /加载更多|Load more/ }).click();
  await expect(page.locator('.interlink-thread-row')).toHaveCount(105);
  await expect(page.locator('.interlink-thread-tree').getByRole('button', { name: /加载更多|Load more/ })).toHaveCount(0);
});

test('opening a local thread uses the main chat area', async ({ page }) => {
  const commands = await prepare(page);
  await page.locator('.messenger-sidebar-workspace-row').filter({ hasText: 'Project one' }).click();
  await page.locator('.interlink-thread-row', { hasText: 'Thread one' }).click();
  const panel = page.locator('.remote-thread-panel');
  await expect(panel).toBeVisible();
  await expect(page.getByRole('dialog')).toHaveCount(0);
  await expect(panel).toContainText('Fixture history');
  await expect(page.getByTestId('chat-composer-input')).toBeVisible();
  await page.getByTestId('chat-composer-input').fill('Local composer message');
  await page.getByTestId('chat-composer-send').click();
  await expect.poll(() => [...commands.values()].some(command => command.kind === 'thread.message')).toBe(true);
  const message = [...commands.values()].find(command => command.kind === 'thread.message');
  expect(message?.args).toMatchObject({ workspace_id: 'fixture_one', local_thread_id: 'fixture_thread_1', message: 'Local composer message' });
  expect(message?.args.attachments).toBeUndefined();
});

test('a pending rename cannot mutate the workspace selected after the prompt opened', async ({ page }) => {
  const commands = await prepare(page);
  await page.locator('.messenger-sidebar-workspace-row').filter({ hasText: 'Project one' }).click();
  const file = page.locator('.workspace-file-name', { hasText: 'one.txt' });
  await expect(file).toBeVisible();
  await file.click({ button: 'right' });
  await page.locator('.workspace-files-menu-item').filter({ hasText: /重命名|Rename/ }).click();
  await expect(page.locator('.mz-dialog-input')).toBeVisible();
  await page.locator('.mz-dialog-input').fill('renamed.txt');
  // Browser back / another navigation can change the target behind a modal.
  await page.evaluate(async () => {
    const path = '/src/router/index.ts';
    const { default: router } = await import(path);
    await router.replace({ query: { ...router.currentRoute.value.query, ws: 'fixture_two' } });
  });
  await expect(page.locator('.workspace-file-name')).toHaveText(['two.txt']);
  await page.locator('.mz-dialog-btn.is-primary').click();
  await expect(page.locator('.mz-dialog')).toHaveCount(0);
  expect([...commands.values()].some(command => command.kind === 'workspace.move')).toBe(false);
  await expect(page.locator('.workspace-file-name')).toHaveText(['two.txt']);
});
