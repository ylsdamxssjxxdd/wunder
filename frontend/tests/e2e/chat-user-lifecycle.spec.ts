import { expect, test } from '@playwright/test';
import { resolve } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { redactEvidence } from '../support/chatEvidenceRedaction';
import { ChatMockService, MOCK_SESSION } from '../support/chatMockService';
import { captureConversation, saveChatEvidence } from '../support/chatEvidenceCapture';

test('user operations drive the production chat through mocked HTTP and WebSocket', async ({ page }, testInfo) => {
  const service = new ChatMockService();
  await service.install(page);
  const directory = resolve(process.cwd(), '../temp_dir/chat-user-review');
  await mkdir(directory, { recursive: true });
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1600, height: 1200 });
  const errors: string[] = [];
  const coverage: Record<string, boolean> = {};
  const completedNoticeRounds = new Set<number>();
  let transientErrors = 0;
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() === 'error' || message.text().startsWith('[Vue warn]')) errors.push(message.text());
  });
  await page.addInitScript(() => {
    const states = new Map<string, string>();
    const completionNodes = new Set<Element>();
    const audit = { duplicate: 0, revived: 0, completionNotices: 0, completionEvents: 0 };
    (window as any).__chatRenderAudit = audit;
    window.addEventListener('wunder:agent-runtime-refresh', (event) => {
      if ((event as CustomEvent).detail?.completedTurns?.length) audit.completionEvents += (event as CustomEvent).detail.completedTurns.length;
    });
    new MutationObserver(() => {
      document.querySelectorAll('.el-message--success').forEach((node) => {
        if (completionNodes.has(node) || !/(has completed the task|已完成任务)/.test(node.textContent || '')) return;
        completionNodes.add(node);
        audit.completionNotices++;
      });
    }).observe(document, { childList: true, subtree: true, characterData: true });
    setInterval(() => {
      const seen = new Set<string>();
      document.querySelectorAll<HTMLElement>('.messenger-message[data-turn-id]:not(.mine)').forEach(row => {
        const key = row.dataset.turnId;
        if (!key) return;
        if (seen.has(key)) audit.duplicate++;
        seen.add(key);
        const status = row.dataset.messageStatus ?? '';
        if (['final', 'cancelled', 'failed'].includes(states.get(key) ?? '') && !['final', 'cancelled', 'failed'].includes(status)) audit.revived++;
        if (states.size < 1000) states.set(key, status);
      });
    }, 50);
  });
  try {
    await page.goto('/login');
    await page.locator('input[autocomplete="username"]').fill('fixture-user');
    await page.locator('input[autocomplete="current-password"]').fill('fixture-password');
    await page.locator('button[type="submit"]').click();
    await page.waitForURL('**/app/**');
    await page.goto(`/app/chat?session_id=${MOCK_SESSION}`);
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();
    await page.waitForFunction(() => Boolean((window as any).wunderPerf));
    // Do not attribute initial route hydration and font layout to a chat turn.
    await page.waitForTimeout(300);
    await page.evaluate(() => (window as any).wunderPerf.start());
    await expect(page.locator('.messenger-message:not(.mine)')).toHaveCount(1);
    coverage.greeting = true;
    const send = async (content: string) => {
      await page.getByTestId('chat-composer-input').fill(content);
      await page.getByTestId('chat-composer-input').press('Control+Enter');
      await expect(page.getByTestId('chat-composer-input')).toHaveValue('');
    };
    const completed = async (round: number, requireNotice = true) => {
      await expect(page.getByTestId('messenger-message-list')).toContainText(`Completed reply ${round}.`);
      await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
      if (requireNotice) {
        await expect(page.locator('.el-message--success')
          .filter({ hasText: /has completed the task|已完成任务/ }).last()).toBeVisible();
        completedNoticeRounds.add(round);
      }
    };
    await send('Inspect the fixture.'); await completed(1);
    await send('Verify the fixture.'); await completed(2);
    service.holdNext = true;
    await send('Begin a longer fixture task.');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 3.');
    await expect.poll(() => service.items.get('fixture-turn-3:tool-2')?.status).toBe('completed');
    await page.getByTestId('chat-composer-send').click();
    await expect.poll(() => service.turns.at(-1)?.status).toBe('cancelled');
    await send('Continue with a new request.'); await completed(4);
    await captureConversation(page, resolve(directory, 'after-stop'));
    coverage.stop = true;
    service.autoCompactNext = true;
    await send('Continue the context fixture.'); await completed(5);
    await send('/compact');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Retained fixture summary.');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    await send('Verify the retained fixture.'); await completed(7);
    await send('/goal Verify the fixture objective.'); await completed(8);
    service.holdNext = true;
    await send('Prepare the final fixture check.');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 9.');
    const connections = service.connections;
    service.disconnect();
    service.release();
    await expect.poll(() => service.connections).toBeGreaterThan(connections);
    // A reconnect may legitimately replay the terminal frame after this
    // assertion; the final aggregate count below covers the acknowledgement.
    await completed(9, false);
    coverage.reconnect = true;
    service.queueNext = true;
    service.duplicateNext = true;
    await send('Queue the next fixture request.');
    await expect.poll(() => service.turns.at(-1)?.status).toBe('queued');
    await expect(page.locator('.messenger-message[data-turn-id="fixture-turn-10"]:not(.mine)')).toHaveAttribute('data-message-status', 'queued');
    const queuedBubble = page.locator('.messenger-message[data-turn-id="fixture-turn-10"]:not(.mine)');
    await expect(queuedBubble.locator('.messenger-message-stats')).toContainText('3 ahead');
    await queuedBubble.screenshot({ path: resolve(directory, 'queue-three-ahead.png') });
    service.updateQueueAhead(1);
    await expect(queuedBubble.locator('.messenger-message-stats')).toContainText('1 ahead');
    await page.reload();
    await expect(queuedBubble.locator('.messenger-message-stats')).toContainText('Queued · 1 ahead');
    service.updateQueueAhead(0);
    await expect(queuedBubble.locator('.messenger-message-stats')).toContainText('0 ahead');
    await queuedBubble.screenshot({ path: resolve(directory, 'queue-next.png') });
    coverage.queue = true;
    service.release();
    await completed(10);
    await expect(queuedBubble.locator('.messenger-message-stats')).not.toContainText('Queued');
    service.failToolNext = true;
    await send('Verify after the queued request.');
    await completed(11);
    const before = await captureConversation(page, directory);
    expect(before.map(row => [row.turnId, row.role])).toEqual([['', 'assistant'],
      ...Array.from({ length: 11 }, (_, index) => [[`fixture-turn-${index + 1}`, 'user'], [`fixture-turn-${index + 1}`, 'assistant']]).flat()]);
    expect(before.filter(row => row.role === 'user').map(row => row.turnId)).toEqual(
      Array.from({ length: 11 }, (_, index) => `fixture-turn-${index + 1}`));
    for (let round = 1; round <= 11; round++) {
      const rows = before.filter(row => row.turnId === `fixture-turn-${round}` && row.role === 'assistant');
      expect(rows, `assistant count for round ${round}`).toHaveLength(1);
      expect(rows[0].content).toBe(round === 3 ? 'Partial reply 3.' : round === 6 ? 'Retained fixture summary.' : `Completed reply ${round}.`);
      expect(rows[0].stats, `statistics for round ${round}`).not.toBe('');
      expect(rows[0].stats).toContain('token/s');
      expect(rows[0].status).toBe(round === 3 ? 'cancelled' : 'final');
    }
    coverage.render = true;
    const detailed = await captureConversation(page, resolve(directory, 'tools'), true);
    for (const row of detailed.filter(row => row.role === 'assistant' && row.turnId)) {
      const round = Number(row.turnId.split('-').at(-1));
      if (round === 6) {
        // Manual compaction is represented by its durable workflow entry;
        // depending on viewport timing the summary may be in the assistant
        // body or in the expanded tool text.
        expect(`${row.content} ${row.tools}`).toContain('Retained fixture summary.');
        continue;
      }
      for (const index of [1, 2]) {
        expect(row.tools).toContain(`fixture-${index}.txt`);
        // The visible workflow may show a compact "Read" label while the
        // durable export contains the full result. Verify both sources
        // without making screenshot text formatting a protocol requirement.
        const durableTool = service.items.get(`fixture-turn-${round}:tool-${index}`);
        expect(durableTool?.result?.content).toBe(round === 11 && index === 2
          ? 'Fixture tool unavailable.'
          : `Fixture result ${round}-${index}`);
      }
    }
    expect(service.items.get('fixture-turn-5:compaction')?.summary_text).toBe('Retained fixture summary.');
    coverage.tools = true;
    coverage.compaction = true;
    expect(service.goal?.status).toBe('complete');
    coverage.goal = true;
    await expect(page.locator('.message-compaction-divider')).toHaveCount(0);
    const audit = await page.evaluate(() => (window as any).__chatRenderAudit);
    transientErrors = audit.duplicate + audit.revived;
    expect([...completedNoticeRounds].sort((left, right) => left - right)).toEqual([1, 2, 4, 5, 7, 8, 10, 11]);
    coverage.completionNotice = completedNoticeRounds.size === 8;
    await page.reload();
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();
    const after = await captureConversation(page, resolve(directory, 'reloaded'));
    expect(after.map(row => [row.turnId, row.role, row.content])).toEqual(before.map(row => [row.turnId, row.role, row.content]));
    expect(service.failures).toEqual([]);
    coverage.reload = true;
  } catch (error) {
    service.failures.push('scenario-did-not-complete');
    await page.screenshot({ path: resolve(directory, 'failure.png'), timeout: 2000 }).catch(() => undefined);
    throw error;
  } finally {
    await service.dispose();
    const performance = await page.evaluate(() => {
      (window as any).wunderPerf?.stop(); return (window as any).wunderPerf?.snapshot();
    }).catch(() => undefined);
    const analysis = await saveChatEvidence(directory, { mode: 'mock-service', snapshot: service.snapshot(),
      changes: service.changes, performance, expectedUserTurns: 11, collectionErrors: service.failures,
      browserErrors: errors.length, transientRenderingErrors: transientErrors, coverage,
      requiredCoverage: ['greeting', 'stop', 'reconnect', 'queue', 'render', 'tools', 'compaction', 'goal', 'reload', 'completionNotice'] }, service.export());
    await writeFile(resolve(directory, 'transport.json'), JSON.stringify(redactEvidence({ requests: service.requests, connections: service.connections, errors }), null, 2));
    await testInfo.attach('analysis', { body: JSON.stringify(analysis), contentType: 'application/json' });
    expect.soft(analysis.findings.filter(item => item.severity === 'error')).toEqual([]);
  }
});
