import { expect, test } from '@playwright/test';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';

test('busy composer sends, drains queued rows and interrupts through the queue action', async ({ page }) => {
  const service = new ChatMockService();
  await service.install(page);
  const errors: string[] = [];
  page.on('pageerror', error => errors.push(error.message));
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const input = page.getByTestId('chat-composer-input');
    const button = page.getByTestId('chat-composer-send');
    const messages = page.getByTestId('messenger-message-list');
    const queue = page.getByTestId('chat-queue-strip');
    service.holdNext = true;
    await input.fill('First fixture input');
    await button.click();
    await expect(messages).toContainText('Partial reply 1.');
    await expect(button).toHaveAttribute('data-mode', 'stop');
    await input.fill('Queued fixture input');
    await expect(button).toHaveAttribute('data-mode', 'send');
    await input.fill('');
    await expect(button).toHaveAttribute('data-mode', 'stop');
    await input.fill('Queued fixture input');
    await button.click();
    await expect(queue).toContainText('Queued fixture input');
    await expect(page.locator('.mz-thread-row.is-active .mz-thread-status')).toHaveClass(/is-running/);
    await expect(messages).not.toContainText('Queued fixture input');
    service.release();
    await expect(messages).toContainText('Completed reply 2.');
    await expect(queue).toHaveCount(0);
    service.holdNext = true;
    await input.fill('Third fixture input');
    await button.click();
    await expect(messages).toContainText('Partial reply 3.');
    await input.fill('Interrupting fixture input');
    await button.click();
    await expect(queue).toContainText('Interrupting fixture input');
    await page.getByTestId('chat-queue-interject').click();
    await expect.poll(() => service.turns[2]?.status).toBe('cancelled');
    await expect(messages).toContainText('Completed reply 4.');
    await expect(queue).toHaveCount(0);
    expect(errors).toEqual([]);
  } finally { await service.dispose(); }
});

test('an accepted idle-thread submission stays mounted across admission and execution', async ({ page }) => {
  const service = new ChatMockService();
  for (let index = 0; index < 18; index++) service.scheduledTurn('completed');
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    const input = page.getByTestId('chat-composer-input');
    await expect(input).toBeVisible();
    await input.fill(['Fixture admission continuity', ...Array(8).fill('Fixture multiline draft')].join('\n'));
    const list = page.getByTestId('messenger-message-list');
    await expect(list).toContainText('Scheduled fixture result');
    await list.evaluate(element => { element.scrollTop = element.scrollHeight; });
    await page.waitForTimeout(150);
    await page.evaluate(() => {
      const list = document.querySelector('[data-testid="messenger-message-list"]')!;
      const samples: boolean[] = [];
      (window as any).__admissionSamples = samples;
      const observer = new MutationObserver(() => samples.push(list.textContent!.includes('Fixture admission continuity')));
      observer.observe(list, { childList: true, subtree: true, characterData: true });
      (window as any).__admissionObserver = observer;
      const gaps: number[] = [];
      (window as any).__bottomGaps = gaps;
      const sample = () => {
        // ResizeObserver applies bottom alignment after rAF and before paint.
        setTimeout(() => {
          gaps.push(list.scrollHeight - list.clientHeight - list.scrollTop);
          if (gaps.length < 60) requestAnimationFrame(sample);
        }, 0);
      };
      requestAnimationFrame(sample);
    });
    await page.getByTestId('chat-composer-send').click();
    await expect(list).toContainText('Completed reply 19.');
    const samples = await page.evaluate(() => {
      (window as any).__admissionObserver.disconnect();
      return (window as any).__admissionSamples as boolean[];
    });
    expect(samples.some(Boolean)).toBe(true);
    expect(samples.slice(samples.indexOf(true)).every(Boolean)).toBe(true);
    await expect.poll(() => list.evaluate(element => element.scrollHeight - element.clientHeight - element.scrollTop)).toBeLessThan(4);
    const gaps = await page.evaluate(() => (window as any).__bottomGaps as number[]);
    expect(Math.max(...gaps)).toBeLessThan(8);
    await list.evaluate(element => { element.scrollTop -= 300; });
    await page.waitForTimeout(150);
    const historyTop = await list.evaluate(element => element.scrollTop);
    await input.fill('Fixture draft while reading history');
    await page.waitForTimeout(150);
    expect(Math.abs(await list.evaluate(element => element.scrollTop) - historyTop)).toBeLessThan(4);
  } finally { await service.dispose(); }
});


test('capacity waiting survives reconnect and hands off without a stale queue row', async ({page})=>{
  const service=new ChatMockService();
  await service.install(page);
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    service.queueNext=true;
    service.duplicateNext=true;
    await page.getByTestId('chat-composer-input').fill('Fixture capacity input');
    await page.getByTestId('chat-composer-send').click();
    await expect(page.getByTestId('chat-queue-strip')).toContainText('Fixture capacity input');
    await expect(page.getByTestId('messenger-message-list')).not.toContainText('Fixture capacity input');
    await page.reload();
    await expect(page.getByTestId('chat-queue-strip')).toContainText('Fixture capacity input');
    const connections=service.connections;
    service.disconnect();
    service.release();
    await expect.poll(()=>service.connections).toBeGreaterThan(connections);
    await expect(page.getByTestId('messenger-message-list')).toContainText('Completed reply 1.');
    await expect(page.getByTestId('chat-queue-strip')).toHaveCount(0);
    await expect(page.locator('.messenger-turn[data-root-turn-id="fixture-turn-1"]')).toHaveCount(1);
  } finally {await service.dispose();}
});
