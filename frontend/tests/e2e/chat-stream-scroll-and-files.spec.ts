import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('long replies retain every line across durable updates and keep scroll intent', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  const turn = service.scheduledTurn('completed');
  turn.status = 'running';
  const itemId = `${turn.turn_id}:text-0`;
  service.items.get(itemId)!.content = '';
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  let content = '';
  let revision = 100;
  let bodyReads = 0;
  await page.route('**/wunder/chat/sessions/*/bodies/*', async route => {
    bodyReads++;
    await new Promise(resolve => setTimeout(resolve, 100));
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: {
      body_id: `${itemId}|stream`, revision: 0, chunks: [content], has_more: false
    } }) });
  });
  const send = (event: string, data: Record<string, unknown>) => {
    for (const sub of service.subscriptions.filter(sub => sub.display)) {
      sub.socket.send(JSON.stringify({ type: 'event', request_id: sub.request,
        payload: { event, data: { session_id: MOCK_SESSION, ...data } } }));
    }
  };
  const append = async (index: number) => {
    const offset = content.length;
    const delta = `Line ${index}: ${'示例文字😀 '.repeat(35)}\n`;
    content += delta;
    // A durable flush can overtake its ephemeral delta. Both carry offsets,
    // and the projection only retains a small head and the newest suffix.
    const row = service.viewRows()[0];
    send('thread_view_change', { epoch: 1, view_seq: ++revision, runtime_status: 'running', rows: [{
      ...row, revision, assistant: { ...row.assistant, content: content.slice(0, 600),
        content_offset: content.length, durable_seq: revision,
        tail: content.slice(-1024), tail_offset: content.length,
        body: { body_id: `${itemId}|stream`, revision: 0, bytes: new TextEncoder().encode(content).length } }
    }] });
    send('thread_item_tail', { item_id: itemId, field: 'content', offset, text: delta, base_seq: revision });
    await expect(page.locator('.message-markdown-body--streaming-text').last()).toHaveText(content);
  };
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const list = page.getByTestId('messenger-message-list');
    await expect(list).toBeVisible();
    await expect.poll(() => service.subscriptions.filter(sub => sub.display).length).toBeGreaterThan(0);
    for (let index = 0; index < 32; index++) await append(index);
    const before = await list.evaluate(el => ({ height: el.scrollHeight, top: el.scrollTop }));
    expect(before.top).toBeGreaterThan(0);
    for (let index = 32; index < 40; index++) await append(index);
    await expect.poll(() => list.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThan(3);
    expect(await list.evaluate(el => el.scrollHeight)).toBeGreaterThan(before.height);

    await list.hover();
    await page.mouse.wheel(0, -600);
    await expect.poll(() => list.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeGreaterThan(300);
    // Wait for the wheel animation to finish before recording its anchor.
    await page.waitForTimeout(200);
    const readingTop = await list.evaluate(el => el.scrollTop);
    for (let index = 40; index < 44; index++) await append(index);
    expect(Math.abs(await list.evaluate(el => el.scrollTop) - readingTop)).toBeLessThan(3);
    expect(bodyReads).toBe(0);

    // A lost window is healed from the durable body. Repeated pushes while
    // that read is pending share one request and never overwrite old lines.
    content += 'Recovered fixture line😀\n'.repeat(140);
    for (let index = 0; index < 4; index++) {
      const row = service.viewRows()[0];
      send('thread_view_change', { epoch: 1, view_seq: ++revision, runtime_status: 'running', rows: [{
        ...row, revision, assistant: { ...row.assistant, content: content.slice(0, 600),
          content_offset: content.length, tail: content.slice(-1024), tail_offset: content.length,
          body: { body_id: `${itemId}|stream`, revision: 0, bytes: new TextEncoder().encode(content).length } }
      }] });
    }
    await expect(page.locator('.message-markdown-body--streaming-text').last()).toHaveText(content);
    expect(bodyReads).toBe(1);
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});

test('dropped files appear above the draft, can be removed, and serialize only on send', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', route => new URL(route.request().url()).pathname.startsWith('/wunder/')
    ? route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) })
    : route.continue());
  let uploadCount = 0;
  await page.route('**/wunder/workspace/upload*', async route => {
    uploadCount++;
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: JSON.stringify({ files: ['fixtures/notes.txt', 'fixtures/data.csv'] }) });
  });
  await page.goto('/__e2e/composer-b4');
  const input = page.getByTestId('chat-composer-input');
  const send = page.getByTestId('chat-composer-send');
  const drop = async () => {
    const transfer = await page.evaluateHandle(() => {
      const transfer = new DataTransfer();
      transfer.items.add(new File(['fixture notes'], 'notes.txt', { type: 'text/plain' }));
      transfer.items.add(new File(['value\n1'], 'data.csv', { type: 'text/csv' }));
      return transfer;
    });
    await input.dispatchEvent('drop', { dataTransfer: transfer });
    await transfer.dispose();
  };
  await input.fill('Fixture draft');
  await drop();
  const cards = page.getByTestId('chat-composer-attachments');
  await expect(cards.locator('.upload-preview-item')).toHaveCount(2);
  await expect(input).toHaveValue('Fixture draft');
  expect((await cards.boundingBox())!.y).toBeLessThan((await input.boundingBox())!.y);
  await cards.locator('.upload-preview-item').filter({ hasText: 'data.csv' }).getByRole('button').click();
  await expect(cards.locator('.upload-preview-item')).toHaveCount(1);
  await send.click();
  const log = page.getByTestId('composer-b4-log');
  await expect(log).toContainText('Fixture draft');
  await expect(log).toContainText('fixtures/notes.txt');
  await expect(log).not.toContainText('fixtures/data.csv');
  await expect(input).toHaveValue('');
  await expect(cards).toHaveCount(0);

  // An attachment alone is sendable. Repeated drops reuse the same cards.
  await drop();
  await drop();
  await expect(cards.locator('.upload-preview-item')).toHaveCount(2);
  await expect(input).toHaveValue('');
  await expect(send).toBeEnabled();
  expect(uploadCount).toBe(3);
  await send.click();
  await expect(cards).toHaveCount(0);
  await expect(log).toContainText('fixtures/data.csv');

  // A workspace file drag references the existing file without uploading it.
  const reference = await page.evaluateHandle(() => {
    const transfer = new DataTransfer();
    transfer.setData('application/x-wunder-workspace-entry', JSON.stringify(['fixtures/existing.txt']));
    return transfer;
  });
  await input.dispatchEvent('drop', { dataTransfer: reference });
  await reference.dispose();
  await expect(cards).toContainText('existing.txt');
  await expect(input).toHaveValue('');
  expect(uploadCount).toBe(3);
  await send.click();
  await expect(log).toContainText('fixtures/existing.txt');
});
