import {expect,test} from '@playwright/test';
import {ChatMockService,MOCK_SESSION} from '../support/chatMockService';
test('production chat uses bounded bootstrap and retains conversation during recovery',async({page})=>{
 const service=new ChatMockService();await service.install(page);const errors:string[]=[];page.on('pageerror',e=>errors.push(e.message));
 try {
  await page.goto('/login');await page.locator('input[autocomplete="username"]').fill('fixture-user');await page.locator('input[autocomplete="current-password"]').fill('fixture-password');await page.locator('button[type="submit"]').click();await page.waitForURL('**/app/**');
  await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);await expect(page.getByTestId('chat-composer-input')).toBeVisible();
  await expect.poll(()=>service.requests.filter(r=>r.path?.endsWith('/view/bootstrap')).length).toBeGreaterThan(0);
  await page.getByTestId('chat-composer-input').fill('Fixture request');await page.getByTestId('chat-composer-input').press('Control+Enter');
  await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 1.');
  await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode','send');
  await expect(page.locator('.messenger-turn')).toHaveCount(1);
  await page.reload();await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 1.');
  service.disconnect();await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 1.');
  await page.waitForTimeout(1800);await expect(page.locator('.messenger-turn')).toHaveCount(1);
  expect(service.requests.filter(r=>r.path?.includes('/thread-log/snapshot')||r.path?.endsWith('/events')||r.path?.endsWith('/history'))).toEqual([]);
  await page.goto(`/app/trajectory?session=${MOCK_SESSION}`);await expect(page.locator('.tt-root')).toBeVisible();await expect(page.locator('.tt-ledger')).toContainText('Completed reply 1.');
  await page.locator('.tt-row').last().click();await expect(page.locator('.tt-details')).toBeVisible();await expect.poll(()=>service.requests.filter(r=>r.path?.includes('/bodies/')).length).toBeGreaterThan(0);
  expect(errors).toEqual([]);
 }finally{await service.dispose();}
});
