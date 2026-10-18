import { expect, test } from '@playwright/test';

test('remote threads keep workspace identity, render live text and preserve failures', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.route('**/wunder/i18n', route => route.fulfill({ json: {} }));
  await page.goto('/__e2e/composer-b4');
  await page.evaluate(async () => {
    const panelUrl = '/src/views/messenger/interlink/RemoteThreadsPanel.vue';
    const source = await (await fetch(panelUrl)).text();
    const vueUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    if (!vueUrl) throw new Error('Missing fixture runtime');
    const [{ createApp, h }, { default: Panel }, { default: api }] = await Promise.all([
      import(vueUrl), import(panelUrl), import('/src/api/http.ts')
    ]);
    const host = document.createElement('div');
    document.body.append(host);
    const originalAdapter = api.defaults.adapter;
    const OriginalSocket = window.WebSocket;
    const commands: any[] = [];
    const sockets: any[] = [];
    class FixtureSocket {
      onmessage: any; onclose: any; onopen: any;
      constructor(public url: string, public protocols: string[]) {
        sockets.push(this);
        setTimeout(() => this.onopen?.(), 0);
      }
      close() { this.onclose?.(); }
    }
    window.WebSocket = FixtureSocket as any;
    api.defaults.adapter = async (config: any) => {
      const url = String(config.url);
      let data: any = {};
      if (config.method === 'post') {
        const body = typeof config.data === 'string' ? JSON.parse(config.data) : config.data;
        commands.push(body);
        data = { command_id: String(commands.length), status: 'issued' };
      } else {
        const command = commands[Number(url.split('/').at(-1)) - 1] || commands.at(-1);
        const result = command?.kind === 'threads.list'
          ? { threads: [{ local_thread_id: 'fixture_thread', title: 'Fixture thread' }] }
          : { items: [{ role: 'assistant', content: 'Fixture history' }] };
        data = { command_id: String(commands.length), kind: command.kind,
          status: command.kind === 'thread.message' ? 'failed' : 'succeeded',
          error_code: command.kind === 'thread.message' ? 'FIXTURE_REFUSED' : '',
          error_summary: command.kind === 'thread.message' ? 'Fixture refusal' : '', result };
      }
      return { status: 200, statusText: 'OK', headers: {}, config, data: { data } };
    };
    const app = createApp({ setup: () => () => h(Panel, {
      target: 'device:fixture_device', workspaceId: 'fixture_workspace', openThreadId: 'fixture_thread'
    }) });
    app.mount(host);
    (window as any).__remoteFixture = { commands, sockets, dispose() {
      app.unmount(); host.remove(); api.defaults.adapter = originalAdapter; window.WebSocket = OriginalSocket;
    } };
  });
  try {
    const panel = page.locator('.remote-thread-panel');
    await expect(panel).toBeVisible();
    await expect(panel).not.toHaveClass(/el-dialog/);
    await expect.poll(() => page.evaluate(() => (window as any).__remoteFixture.commands.map((item: any) => item.kind))).toContain('threads.get');
    await expect(panel).toContainText('Fixture history');
    await page.evaluate(() => {
      const socket = (window as any).__remoteFixture.sockets.at(-1);
      socket.onmessage({ data: JSON.stringify({ type: 'snapshot', thread_id: 'fixture_thread', payload: {
        cursor: 1, turns: [], blocks: [], items: [{ item_id: 'fixture_item', turn_id: 'fixture_turn', kind: 'assistant_message',
          status: 'completed', revision: 1, payload: { role: 'assistant', content: '<Fixture live>' } }]
      } }) });
    });
    await expect(panel).toContainText('<Fixture live>');
    await expect(panel.locator('fixture')).toHaveCount(0);
    await panel.locator('textarea').fill('Fixture message');
    await panel.getByRole('button', { name: /^发送$|^Send$/ }).click();
    await expect(panel.getByRole('status')).toContainText('Fixture refusal');
    const evidence = await page.evaluate(() => {
      const fixture = (window as any).__remoteFixture;
      return { commands: fixture.commands, url: String(fixture.sockets[0].url), protocols: fixture.sockets[0].protocols };
    });
    expect(evidence.commands.every((command: any) => command.args.workspace_id === 'fixture_workspace')).toBe(true);
    expect(evidence.url).not.toMatch(/[?&](token|access_token|authorization)=/);
    expect(evidence.protocols[0]).toBe('wunder-interlink-remote');
    expect(errors).toEqual([]);
  } finally {
    await page.evaluate(() => (window as any).__remoteFixture.dispose());
  }
});
