// Run: node tests/quota-admin-smoke.mjs. Uses real UI modules with isolated API fixtures.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { chromium } from 'playwright';

const root = path.resolve('web');
const server = createServer(async (request, response) => {
  const name = decodeURIComponent(new URL(request.url, 'http://localhost').pathname);
  const file = path.resolve(root, `.${name === '/' ? '/index.html' : name}`);
  if (!file.startsWith(`${root}${path.sep}`)) { response.writeHead(403).end(); return; }
  try {
    let body = await readFile(file);
    if (file.endsWith('index.html')) body = Buffer.from(body.toString().replace(/<script type="module" src="\.\/app\.js[^>]*><\/script>/, ''));
    const type = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' }[path.extname(file)] || 'application/octet-stream';
    response.writeHead(200, { 'Content-Type': `${type}; charset=utf-8` }).end(body);
  } catch { response.writeHead(404).end(); }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 1080 } });
  const errors = [];
  page.on('pageerror', (error) => errors.push(error.message));
  const user = { id: 'user_1', username: 'user_1', roles: ['user'], status: 'active',
    quota_balance: 1000, quota_granted_total: 1000, quota_used_total: 0, daily_quota_grant: 1000 };
  const admin = { ...user, id: 'user_2', username: 'user_2', roles: ['admin'] };
  const mutations = [];
  await page.route('**/wunder/**', async (route) => {
    const request = route.request();
    const endpoint = new URL(request.url()).pathname;
    let data = {};
    if (endpoint.endsWith('/user_accounts')) data = { items: [user, admin], total: 2 };
    else if (endpoint.endsWith('/org_units')) data = { items: [] };
    else if (endpoint.endsWith('/tool_access')) data = { allowed_tools: null };
    if (request.method() === 'PATCH') {
      const payload = request.postDataJSON(); mutations.push(payload);
      user.quota_balance = payload.quota_balance;
      data = user;
    }
    if (request.method() === 'POST' && endpoint.endsWith('/quota_adjustment')) {
      const payload = request.postDataJSON(); mutations.push(payload);
      if (payload.action === 'grant') {
        user.quota_balance += payload.amount; user.quota_granted_total += payload.amount;
      } else {
        user.quota_balance -= payload.amount; user.quota_used_total += payload.amount;
      }
      data = user;
    }
    await route.fulfill({ json: { data } });
  });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.evaluate(async () => {
    document.querySelectorAll('.panel.active').forEach((node) => node.classList.remove('active'));
    document.getElementById('userAccountsPanel').classList.add('active');
    const { setLanguage } = await import('/modules/i18n.js?v=20260516-01');
    setLanguage('zh-CN', { force: true });
    const accounts = await import('/modules/user-accounts.js');
    accounts.initUserAccountsPanel();
    await accounts.loadUserAccounts();
  });
  const settings = page.locator('#userAccountTableBody button').filter({ hasText: '设置' });
  await settings.first().click();
  await page.locator('#userAccountSettingsModal').waitFor({ state: 'visible' });
  assert.match(await page.locator('#userAccountSettingsModal').innerText(), /直接设置额度余额/);
  assert.equal(await page.locator('#userAccountQuotaInput').inputValue(), '1000');
  assert.match(await page.locator('#userAccountQuotaMeta').innerText(), /每日发放 1,000/);
  await page.locator('#userAccountQuotaInput').fill('');
  await page.locator('#userAccountQuotaSave').click();
  assert.deepEqual(mutations, []);
  await page.locator('#userAccountQuotaInput').fill('0');
  await page.locator('#userAccountQuotaSave').click();
  await page.waitForFunction(() => document.querySelector('#userAccountQuotaMeta').textContent.includes('持有 0 /'));
  assert.deepEqual(mutations, [{ quota_balance: 0 }]);
  await page.locator('#userAccountQuotaAdjustInput').fill('0.5');
  await page.locator('#userAccountQuotaGrantBtn').click();
  assert.equal(mutations.length, 1);
  await page.locator('#userAccountQuotaAdjustInput').fill('12');
  await page.locator('#userAccountQuotaGrantBtn').click();
  await page.waitForFunction(() => document.querySelector('#userAccountQuotaInput').value === '12');
  await page.locator('#userAccountQuotaAdjustInput').fill('2');
  await page.locator('#userAccountQuotaDeductBtn').click();
  await page.waitForFunction(() => document.querySelector('#userAccountQuotaInput').value === '10');
  assert.deepEqual(mutations, [{ quota_balance: 0 }, { action: 'grant', amount: 12 }, { action: 'deduct', amount: 2 }]);
  await page.locator('#userAccountSettingsCancel').click();
  await settings.last().click();
  assert.equal(await page.locator('#userAccountQuotaInput').isDisabled(), true);
  assert.equal(await page.locator('#userAccountQuotaGrantBtn').isDisabled(), true);
  assert.equal(await page.locator('#userAccountQuotaDeductBtn').isDisabled(), true);
  assert.deepEqual(errors, []);
  console.log('Quota admin browser smoke passed: labels, zero balance, grant/deduct, invalid input, admin exemption.');
} finally {
  await browser?.close();
  await new Promise((resolve) => server.close(resolve));
}
