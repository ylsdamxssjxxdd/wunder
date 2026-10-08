import { test, expect } from '@playwright/test';

/**
 * G3 全链路真机验收（用户侧）：登录 → 单智能体工作台 → 工作目录上传 → 发消息 → 时间线出现回复。
 *
 * 前置（由主智能体搭建）：
 * - 隔离服务端（含 A2/A3 的云端契约）跑在 `ADMIN_E2E_ORIGIN`（默认 127.0.0.1:18020）；
 * - 模型指向 `scripts/mock-llm-server.py`（OpenAI 兼容流式 mock），因此**不需要真实大模型**；
 * - 该服务端的配置里 `preset_e2e_a` 放开了 `model_name`/`approval_mode` 自定义。
 *
 * 断言刻意覆盖方案 §13.1 的功能条目：单智能体（无创建/切换入口）、工作目录可用、
 * 聊天条目化（用户气泡 + 助手正文）、无 pageerror。
 */
const ORIGIN = process.env.ADMIN_E2E_ORIGIN || 'http://127.0.0.1:18020';
const REPLY = process.env.MOCK_REPLY || '这是 mock 模型回复：收到你的消息了。';

test('G3 全链路：登录 → 工作目录上传 → 发消息 → 时间线回复', async ({ page, request }) => {
  // 这条用例是**第一个**打开蜂巢主视图的用例时，Vite dev server 要按需编译整棵聊天模块树；
  // 默认 60s 的单测超时会被冷编译吃掉（实测：单独跑 8–12s 通过，排在舰桥用例之后跑 46s 被超时打断）。
  // 放宽的是**环境等待**，不是断言口径：回复仍必须在时间线里实时渲染出来才会通过。
  test.setTimeout(180_000);
  const username = `e2e_g3_${Date.now().toString(36)}`;
  const password = 'Passw0rd!23';
  const registered = await request.post(ORIGIN + '/wunder/auth/register', {
    data: { username, password }
  });
  expect(registered.ok(), `register failed: ${registered.status()}`).toBeTruthy();

  const pageErrors: string[] = [];
  const consoleLogs: string[] = [];
  page.on('pageerror', (error) => pageErrors.push(String(error)));
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      consoleLogs.push(`[${message.type()}] ${message.text().slice(0, 200)}`);
    }
  });

  await page.setViewportSize({ width: 1440, height: 900 });
  // 发送要求聊天 WebSocket 已就绪（服务端对只走 HTTP 的发送返回 CHAT_WS_REQUIRED）。
  // 冷启动的 dev server 下 WS 握手会晚于视图渲染，先按住这个信号再发送，
  // 否则会得到一个「乐观气泡 + Requesting 卡住」的假失败。
  const chatSocketOpened = page.waitForEvent('websocket', {
    predicate: (socket) => socket.url().includes('/wunder/chat/ws'),
    timeout: 90_000
  });
  await page.goto('/login');
  await page.getByPlaceholder(/username/i).fill(username);
  await page.getByPlaceholder(/password/i).fill(password);
  await page.getByRole('button', { name: /sign in/i }).click();

  // 1) 两栏工作台 + 单一工作区
  await expect(page.locator('.messenger-view').first()).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('.messenger-sidebar-files-region').first()).toBeVisible();
  await chatSocketOpened;
  await page.waitForTimeout(800);

  // 2) 工作目录区可用：上传一个文件并出现在文件树里
  const fileName = `e2e-note-${Date.now().toString(36)}.md`;
  const uploadInput = page.locator('.messenger-sidebar-files-region input[type="file"]').first();
  await uploadInput.setInputFiles({
    name: fileName,
    mimeType: 'text/markdown',
    buffer: Buffer.from('# e2e\n\nhello from playwright\n', 'utf-8')
  });
  await expect
    .poll(async () => page.locator('.messenger-sidebar-files-region').innerText(), { timeout: 30_000 })
    .toContain(fileName);

  // 3) 发一条消息，时间线应出现用户气泡 + mock 模型回复
  const composer = page.locator('.messenger-agent-composer textarea, .messenger-agent-composer [contenteditable="true"]').first();
  await composer.click();
  await composer.fill('你好，这是一条 E2E 消息');
  await page.keyboard.press('Enter');

  await expect
    .poll(async () => page.locator('.messenger-chat-body').innerText(), { timeout: 60_000 })
    .toContain(REPLY.replace(/^(.*)$/, '$1'))
    .catch(() => {
      throw new Error(
        `未出现 mock 回复。浏览器 console（error/warning）：\n${consoleLogs.slice(0, 20).join('\n') || '(无)'}\n` +
          `页面 pageerror：\n${pageErrors.slice(0, 10).join('\n') || '(无)'}`
      );
    });

  // 4) 单智能体形态：不应存在「新建智能体」这类入口
  const createAgentEntry = await page
    .locator('button:has-text("新建智能体"), button:has-text("创建智能体"), [data-testid="agent-create"]')
    .count();
  expect(createAgentEntry, '单智能体形态下不应存在创建智能体入口').toBe(0);

  await page.screenshot({ path: '../temp_dir/screens/g3-full-chain.png', fullPage: false });
  expect(pageErrors, `page errors: ${pageErrors.join(' | ')}`).toEqual([]);
});
