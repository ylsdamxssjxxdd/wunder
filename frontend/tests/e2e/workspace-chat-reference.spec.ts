import { expect, test, type Page } from '@playwright/test';

const mountWorkspace = async (page: Page) => {
  await page.route('**/wunder/i18n', route => route.fulfill({ json: {} }));
  await page.goto('/__e2e/composer-b4');
  await page.evaluate(async () => {
    const panelUrl = '/src/views/messenger/workspace/WorkspaceFilesPanel.vue';
    const source = await (await fetch(panelUrl)).text();
    const vueUrl = source.match(/from\s+["']([^"']*\/vue\.js[^"']*)["']/)?.[1];
    const targetSource = await (await fetch('/src/views/messenger/interlink/interlinkNodeTarget.ts')).text();
    const routerUrl = targetSource.match(/from\s+["']([^"']*\/vue-router\.js[^"']*)["']/)?.[1];
    if (!vueUrl || !routerUrl) throw new Error('Missing fixture runtime');
    const [{ createApp, h }, { createRouter, createMemoryHistory }, { default: Panel }, { default: api },
      { installElementPlus }] = await Promise.all([
      import(vueUrl), import(routerUrl), import(panelUrl), import('/src/api/http.ts'), import('/src/plugins/elementPlus.ts')
    ]);
    const original = api.defaults.adapter;
    const host = document.createElement('div');
    host.style.cssText = 'position:fixed;right:0;top:0;width:340px;height:460px;z-index:1000;background:white';
    document.body.append(host);
    const outside = document.createElement('button');
    outside.dataset.testid = 'workspace-outside';
    outside.style.cssText = 'position:fixed;right:360px;top:16px;z-index:1000';
    outside.textContent = 'Outside';
    outside.addEventListener('pointerdown', event => event.stopPropagation());
    outside.addEventListener('click', event => event.stopPropagation());
    document.body.append(outside);
    api.defaults.adapter = async (config: any) => {
      let data: unknown = {};
      const path = String(config.params?.path || '');
      if (String(config.url).includes('/workspace/stats')) data = { files: 2, used_bytes: 0 };
      else if (String(config.url).includes('/workspace')) {
        const entries = path === 'reports'
          ? [{ name: 'notes draft.md', path: 'reports/notes draft.md', type: 'file', size: 0 }]
          : [{ name: 'reports', path: 'reports', type: 'dir', size: 0 },
            { name: 'other.txt', path: 'other.txt', type: 'file', size: 0 }];
        data = { entries, path, total: entries.length, offset: 0, limit: 200 };
      } else if (String(config.url).includes('/interlink/nodes')) data = { data: { nodes: [], total: 0 } };
      return { status: 200, statusText: 'OK', headers: {}, config, data };
    };
    const router = createRouter({ history: createMemoryHistory(), routes: [{ path: '/', component: {} }] });
    await router.push('/'); await router.isReady();
    const app = createApp({ setup: () => () => h(Panel) });
    app.use(router); installElementPlus(app); app.mount(host);
    (window as any).__disposeWorkspaceFixture = () => {
      app.unmount(); host.remove(); outside.remove(); api.defaults.adapter = original;
    };
  });
  await expect(page.locator('.workspace-file-name', { hasText: /^reports$/ })).toBeVisible();
};

const row = (page: Page, name: string) => page.locator('.workspace-file-row')
  .filter({ has: page.locator('.workspace-file-name', { hasText: name }) });
const menu = (page: Page) => page.locator('.workspace-files-menu');
const quote = async (page: Page, name: string) => {
  await row(page, name).click({ button: 'right' });
  await menu(page).getByRole('menuitem', { name: /引用到聊天|Quote in chat/ }).click();
  await expect(menu(page)).toHaveCount(0);
};

test.afterEach(async ({ page }) => {
  await page.evaluate(() => (window as any).__disposeWorkspaceFixture?.());
});

test('quoting a workspace entry inserts only editable relative path text and preserves the draft', async ({ page }) => {
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  await mountWorkspace(page);
  const input = page.getByTestId('chat-composer-input');
  await input.fill('Fixture draft');
  await row(page, 'reports').click();
  await quote(page, 'notes draft.md');
  await expect(input).toHaveValue('Fixture draft\nreports/notes draft.md');
  await expect(input).toBeFocused();
  await expect(page.locator('.workspace-quote-item, .upload-preview-item')).toHaveCount(0);
  const send = page.locator('.composer-send-btn');
  await send.click();
  await expect(page.getByTestId('composer-b4-log')).toContainText(
    '"content":"Fixture draft\\nreports/notes draft.md"'
  );
  await expect(page.getByTestId('composer-b4-log')).toContainText('"attachmentCount":0');
  await expect(input).toHaveValue('');
  // Repeating the same action after send must still insert the path.
  await quote(page, 'notes draft.md');
  await expect(input).toHaveValue('reports/notes draft.md');
  // The path can be edited or deleted without leaving hidden send metadata.
  await input.fill('Fixture edited'); await send.click();
  const entries = (await page.getByTestId('composer-b4-log').innerText()).split('\n');
  expect(entries.at(-1)).toContain('"content":"Fixture edited"');
  // Directory references use the same plain relative path behavior.
  await quote(page, 'reports');
  await expect(input).toHaveValue('reports');
  expect(errors).toEqual([]);
});

test('dragging a workspace file icon inserts only its relative path', async ({ page }) => {
  await mountWorkspace(page);
  const input = page.getByTestId('chat-composer-input');
  await input.fill('Fixture draft');
  await expect(row(page, 'other.txt').locator('.workspace-file-icon-img')).toBeVisible();
  const dragResult = await page.evaluate(() => {
    const row = Array.from(document.querySelectorAll<HTMLElement>('.workspace-file-row'))
      .find((element) => element.querySelector('.workspace-file-name')?.textContent?.trim() === 'other.txt');
    const icon = row?.querySelector<HTMLImageElement>('.workspace-file-icon-img');
    const composer = document.querySelector<HTMLElement>('.input-container--world');
    if (!row || !icon || !composer) throw new Error('Missing workspace drag fixture');
    const transfer = new DataTransfer();
    icon.dispatchEvent(new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    composer.dispatchEvent(new DragEvent('dragover', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    composer.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
    return {
      iconDraggable: icon.draggable,
      hasWorkspacePayload: Array.from(transfer.types).includes('application/x-wunder-workspace-entry')
    };
  });
  expect(dragResult).toEqual({ iconDraggable: false, hasWorkspacePayload: true });
  await expect(input).toHaveValue('Fixture draft\nother.txt');
  await expect(page.locator('.workspace-quote-item, .upload-preview-item')).toHaveCount(0);
});

test('workspace menus dismiss outside stopped events and retain menu actions', async ({ page }) => {
  await mountWorkspace(page);
  await row(page, 'other.txt').click({ button: 'right' });
  await expect(menu(page)).toBeVisible();
  await page.getByTestId('workspace-outside').click();
  await expect(menu(page)).toHaveCount(0);

  await row(page, 'other.txt').click({ button: 'right' });
  await row(page, 'reports').click();
  await expect(menu(page)).toHaveCount(0);
  await expect(row(page, 'notes draft.md')).toBeVisible();

  await row(page, 'other.txt').click({ button: 'right' });
  await page.getByTestId('chat-composer-input').click();
  await expect(menu(page)).toHaveCount(0);

  await row(page, 'other.txt').locator('.workspace-file-menu').click();
  await expect(menu(page)).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu(page)).toHaveCount(0);

  await quote(page, 'other.txt');
  await expect(page.getByTestId('chat-composer-input')).toHaveValue('other.txt');
});
