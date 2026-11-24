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

test('a barely overflowing transcript stays at the top after upward scrolling', async ({ page }) => {
  const service = new ChatMockService();
  const turn = service.scheduledTurn('completed');
  service.items.get(`${turn.turn_id}:text-0`)!.content = Array.from(
    { length: 17 }, (_, index) => `Fixture reply line ${index + 1}`
  ).join('\n');
  await service.install(page);
  await page.setViewportSize({ width: 1280, height: 720 });
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const list = page.getByTestId('messenger-message-list');
    await expect(list).toBeVisible();
    await expect.poll(() => list.evaluate(el => el.scrollHeight - el.clientHeight)).toBeGreaterThan(0);
    const overflow = await list.evaluate(el => el.scrollHeight - el.clientHeight);
    expect(overflow).toBeLessThan(72);
    await expect.poll(() => list.evaluate(el => el.scrollHeight - el.clientHeight - el.scrollTop)).toBeLessThan(3);

    await list.hover();
    await page.mouse.wheel(0, -1000);
    await expect.poll(() => list.evaluate(el => el.scrollTop)).toBe(0);
    await page.setViewportSize({ width: 1280, height: 721 });
    await page.waitForTimeout(250);
    expect(await list.evaluate(el => el.scrollTop)).toBe(0);
  } finally {
    await service.dispose();
  }
});

test('dropped files appear above the draft, can be removed, and serialize only on send', async ({ page }) => {
  await page.routeWebSocket(/\/wunder\//, socket => socket.close());
  await page.route('**/*', route => new URL(route.request().url()).pathname.startsWith('/wunder/')
    ? route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data: { items: [] } }) })
    : route.continue());
  let uploadCount = 0;
  const uploadPayloads: string[] = [];
  await page.route('**/wunder/workspace/upload*', async route => {
    uploadCount++;
    const payload = route.request().postDataBuffer()?.toString('latin1') || '';
    uploadPayloads.push(payload);
    const files = payload.includes('selected-image.png')
      ? ['selected-image.png', 'selected-report.pdf']
      : payload.includes('pasted-image.png')
        ? ['pasted-image.png', 'pasted-report.docx']
        : ['fixtures/notes.txt', 'fixtures/data.csv'];
    await route.fulfill({ status: 200, contentType: 'application/json',
      body: JSON.stringify({ files }) });
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
  await expect(input).toHaveValue('fixtures/existing.txt');
  await expect(cards).toHaveCount(0);
  expect(uploadCount).toBe(3);
  await send.click();
  await expect(input).toHaveValue('');
  await expect(log).toContainText('fixtures/existing.txt');

  // 桌面「+」选择图片和文档后同样先落工作目录，再在发送内容中提示路径。
  await page.getByTestId('chat-composer-plus').click();
  await page.getByTestId('chat-composer-upload-files').click();
  await page.getByTestId('chat-composer-file-input').setInputFiles([
    { name: 'selected-image.png', mimeType: 'image/png', buffer: Buffer.from('fixture image') },
    { name: 'selected-report.pdf', mimeType: 'application/pdf', buffer: Buffer.from('fixture document') }
  ]);
  await expect(cards.locator('.upload-preview-item')).toHaveCount(2);
  await expect(cards).toContainText('selected-image.png');
  await expect(cards).toContainText('selected-report.pdf');
  expect(uploadCount).toBe(4);
  expect(uploadPayloads[3]).toContain('selected-image.png');
  expect(uploadPayloads[3]).toContain('selected-report.pdf');
  await send.click();
  await expect(log).toContainText(/已附加 2 个文件到当前工作目录|Attached 2 file\(s\) to the current workspace/);
  await expect(log).toContainText('selected-image.png');

  // 剪贴板里的图片/文档不插入文件名文本，而是复用工作目录上传链路。
  await input.evaluate((element) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File(['pasted image'], 'pasted-image.png', { type: 'image/png' }));
    transfer.items.add(new File(['pasted document'], 'pasted-report.docx', {
      type: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document'
    }));
    element.dispatchEvent(new ClipboardEvent('paste', {
      clipboardData: transfer,
      bubbles: true,
      cancelable: true
    }));
  });
  await expect(cards.locator('.upload-preview-item')).toHaveCount(2);
  await expect(cards).toContainText('pasted-image.png');
  await expect(cards).toContainText('pasted-report.docx');
  await expect(input).toHaveValue('');
  expect(uploadCount).toBe(5);
  expect(uploadPayloads[4]).toContain('pasted-image.png');
  expect(uploadPayloads[4]).toContain('pasted-report.docx');
  await send.click();
  await expect(cards).toHaveCount(0);
  await expect(log).toContainText('pasted-report.docx');
});
