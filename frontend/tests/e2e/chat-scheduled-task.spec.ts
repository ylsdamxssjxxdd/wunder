import { expect, test } from '@playwright/test';
import { resolve } from 'node:path';
import { mkdir } from 'node:fs/promises';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';
import { installChatTurnDomAudit } from '../support/chatTurnDomAudit';
import { captureConversation, saveChatEvidence } from '../support/chatEvidenceCapture';

test('background scheduled turns preserve active Stop, queue and independent terminal bubbles', async ({ page }, info) => {
  const service = new ChatMockService();
  const directory = resolve(process.cwd(), '../temp_dir/chat-scheduled-review');
  await mkdir(directory, { recursive: true });
  await service.install(page);
  await page.addInitScript(installChatTurnDomAudit);
  const browserErrors: string[] = [];
  page.on('pageerror', error => browserErrors.push(error.message));
  await page.setViewportSize({ width: 1600, height: 1200 });
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();
    await page.evaluate(() => (window as any).wunderPerf?.start());
    service.holdNext = true;
    await page.getByTestId('chat-composer-input').fill('Start fixture work');
    await page.getByTestId('chat-composer-input').press('Control+Enter');
    const assistant = (round: number) => page.locator(`.messenger-turn[data-root-turn-id="fixture-turn-${round}"] [data-turn-slot="assistant"] .messenger-message`);
    await expect(assistant(1)).toContainText('Partial reply 1.');
    service.scheduledTurn('rejected');
    await expect(assistant(2)).toHaveAttribute('data-message-status', 'failed');
    await expect(assistant(2)).toContainText('Fixture admission rejected');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'stop');
    await expect(assistant(1)).toContainText('Partial reply 1.');
    await page.screenshot({ path: resolve(directory, 'rejected-while-running.png'), fullPage: true });
    await page.getByTestId('chat-composer-send').click();
    await expect(assistant(1)).toHaveAttribute('data-message-status', 'cancelled');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    const queued = service.scheduledTurn('queued');
    await expect(assistant(3)).toHaveAttribute('data-message-status', 'queued');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'stop');
    await page.screenshot({ path: resolve(directory, 'scheduled-queue.png'), fullPage: true });
    service.settleScheduled(queued, 'cancelled');
    service.scheduledTurn('completed');
    await expect(assistant(4)).toContainText('Scheduled fixture result');
    await expect(assistant(4)).toHaveAttribute('data-message-status', 'final');
    await page.evaluate(() => (window as any).wunderPerf?.stop());
    const performance = await page.evaluate(() => (window as any).wunderPerf?.snapshot());
    const audit = await page.evaluate(() => (window as any).__chatRenderAudit);
    const before = await captureConversation(page, resolve(directory, 'conversation'));
    await page.reload();
    await expect(assistant(4)).toContainText('Scheduled fixture result');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    const after = await captureConversation(page, resolve(directory, 'reloaded'));
    expect(after.map(row => [row.turnId, row.role, row.content])).toEqual(before.map(row => [row.turnId, row.role, row.content]));
    const analysis = await saveChatEvidence(directory, { mode: 'mock-service', snapshot: service.snapshot(),
      changes: service.changes, performance, expectedUserTurns: 4, collectionErrors: service.failures,
      browserErrors: browserErrors.length, transientRenderingErrors: audit.duplicate + audit.structure, coverage: { scheduled: true }, requiredCoverage: ['scheduled'] }, service.export());
    await info.attach('analysis', { body: JSON.stringify(analysis), contentType: 'application/json' });
    expect(analysis.findings.filter(row => row.severity === 'error')).toEqual([]);
  } finally { await service.dispose(); }
});
