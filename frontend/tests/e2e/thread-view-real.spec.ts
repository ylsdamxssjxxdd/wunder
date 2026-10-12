import {expect,test} from '@playwright/test';

test('real SQLite, auth, display API and WebSocket retain the published chat',async({page,request})=>{
  const errors:string[]=[];const paths:string[]=[];
  page.on('pageerror',e=>errors.push(e.message));
  page.on('request',r=>{const path=new URL(r.url()).pathname;if(path.startsWith('/wunder/'))paths.push(path)});
  await page.goto('/login');
  await page.locator('input[autocomplete="username"]').fill('fixture-user');
  await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
  await page.locator('button[type="submit"]').click();await page.waitForURL('**/app/**');
  const started=Date.now();await page.goto('/app/chat?session_id=fixture-thread');
  await expect(page.getByTestId('messenger-message-list')).toContainText('Fixture answer 20');
  console.log(JSON.stringify({fixture_items:2000,cold_navigation_visible_ms:Date.now()-started}));
  await expect(page.locator('.messenger-turn')).toHaveCount(12);
  expect((await request.post('/fixture/append')).ok()).toBeTruthy();
  // The fixture endpoint mutates the real store directly; reload exercises the
  // same recovery path used after a missed WS wakeup.
  await page.reload();
  await expect(page.getByTestId('messenger-message-list')).toContainText('Fixture live answer');
  await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode','send');
  await page.reload();await expect(page.getByTestId('messenger-message-list')).toContainText('Fixture live answer');
  await page.waitForTimeout(61000);
  await expect(page.getByTestId('messenger-message-list')).toContainText('Fixture live answer');
  await page.goto('/app/trajectory?session=fixture-thread');
  await expect(page.locator('.tt-ledger')).toContainText('Fixture live answer');
  await page.locator('.tt-row').last().click();await expect(page.locator('.tt-details')).toBeVisible();
  await expect(page.locator('.thread-body-viewport')).toBeVisible();
  await page.locator('.thread-body-viewport button').first().click();
  await expect(page.locator('.thread-body-scroll')).toContainText('Fixture live answer');
  await page.locator('.tt-search-input').fill('Fixture');
  await expect(page.locator('.tt-search-results button').first()).toBeVisible();
  expect(paths.filter(p=>p.includes('/thread-log/snapshot')||p.endsWith('/events')||p.endsWith('/history'))).toEqual([]);
  expect(errors).toEqual([]);
});
