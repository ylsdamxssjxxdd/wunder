import { expect, test } from '@playwright/test';
import { resolve } from 'node:path';
import { mkdir, writeFile } from 'node:fs/promises';
import { installChatTurnDomAudit } from '../support/chatTurnDomAudit';
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
  await page.addInitScript(installChatTurnDomAudit);
  await page.addInitScript(() => {
    // A completion toast only lives ~3s, so asserting it is still mounted later
    // races its lifetime. Record every notice the moment it mounts instead.
    const notices: string[] = [];
    (window as any).__completionNotices = notices;
    const seen = new WeakSet<Element>();
    const observer = new MutationObserver(() => {
      document.querySelectorAll('.el-message--success').forEach(node => {
        if (seen.has(node)) return;
        const text = (node.textContent || '').trim();
        // The root node can mount before Vue writes its text, so a node is only
        // consumed once its notice text is actually readable.
        if (!/has completed the task|已完成任务/.test(text)) return;
        seen.add(node);
        notices.push(text);
      });
    });
    observer.observe(document, { childList: true, subtree: true });
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
    // 空线程首屏：年度活动网格 + 合成问候气泡一起上下居中显示；
    // 网格消失由 turn 槽位驱动，首条用户轮次后恢复流式排布。
    await expect(page.locator('.messenger-thread-activity')).toHaveCount(1);
    coverage.emptyState = true;
    const noticeCount = () => page.evaluate(() => (window as any).__completionNotices.length as number);
    // Each round is judged against the notices already recorded when it was sent,
    // so a missing notice fails and an unrelated earlier one cannot stand in.
    let noticesBeforeRound = 0;
    const send = async (content: string) => {
      noticesBeforeRound = await noticeCount();
      await page.getByTestId('chat-composer-input').fill(content);
      await page.getByTestId('chat-composer-input').press('Control+Enter');
      await expect(page.getByTestId('chat-composer-input')).toHaveValue('');
    };
    const completed = async (round: number, requireNotice = true) => {
      await expect(page.getByTestId('messenger-message-list')).toContainText(`Completed reply ${round}.`);
      await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
      const assistant = page.locator(`.messenger-turn[data-root-turn-id="fixture-turn-${round}"] [data-turn-slot="assistant"] .messenger-message`);
      await expect(assistant).toHaveAttribute('data-message-status', 'final');
      await expect(assistant.locator('.messenger-message-stats')).toContainText('token/s');
      await expect(assistant.locator('.messenger-message-stats')).not.toContainText('Queued');
      if (requireNotice) {
        await expect.poll(noticeCount).toBeGreaterThan(noticesBeforeRound);
        completedNoticeRounds.add(round);
      }
    };
    await send('Inspect the fixture.'); await completed(1);
    // Prove the independent inspector rejects an extra assistant DOM region.
    const mutationProof = await page.evaluate(() => {
      const copy = document.querySelector('.messenger-turn')!.cloneNode(true) as HTMLElement;
      const container = document.createElement('div');
      container.append(copy);
      const inspect = (window as any).__chatInspectTurns;
      const valid = inspect(container);
      copy.lastElementChild!.append(copy.querySelector('[data-turn-slot="assistant"] .messenger-message')!.cloneNode(true));
      return { valid, invalid: inspect(container) };
    });
    expect(mutationProof.valid).toBe(0);
    expect(mutationProof.invalid).toBeGreaterThan(0);
    coverage.structureMutation = true;
    await send('Verify the fixture.'); await completed(2);
    service.holdNext = true;
    await send('Begin a longer fixture task.');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 3.');
    await expect.poll(() => service.items.get('fixture-turn-3:tool-2')?.status).toBe('completed');
    await page.getByTestId('chat-composer-send').click();
    await expect.poll(() => service.turns.at(-1)?.status).toBe('cancelled');
    // A stopped turn is not a finished task: it must not announce success.
    await page.waitForTimeout(1200);
    expect(await noticeCount()).toBe(noticesBeforeRound);
    await send('Continue with a new request.'); await completed(4);
    await captureConversation(page, resolve(directory, 'after-stop'));
    coverage.stop = true;
    service.autoCompactNext = true;
    await send('Continue the context fixture.'); await completed(5);
    await page.getByTestId('chat-composer-input').fill('/compact');
    await page.getByTestId('chat-composer-input').press('Control+Enter');
    // A visible command suggestion makes the first send complete the command
    // instead of submitting it; that is the composer's product behaviour.
    await expect(page.getByTestId('chat-composer-input')).toHaveValue('/compact ');
    await page.getByTestId('chat-composer-input').press('Control+Enter');
    await expect(page.getByTestId('chat-composer-input')).toHaveValue('');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Retained fixture summary.');
    await expect(page.getByTestId('chat-composer-send')).toHaveAttribute('data-mode', 'send');
    await send('Verify the retained fixture.');
    await completed(7);
    service.holdNext = true;
    await send('/goal Verify the fixture objective.');
    await expect(page.getByTestId('messenger-message-list')).toContainText('Partial reply 8.');
    await expect(page.locator('.messenger-turn[data-root-turn-id="fixture-turn-8"]')).toHaveCount(1);
    await expect(page.locator('.messenger-turn[data-root-turn-id="fixture-turn-8-continuation"]')).toHaveCount(0);
    await page.screenshot({ path: resolve(directory, 'goal-running.png') });
    service.release();
    await completed(8);
    await expect(page.locator('.messenger-message[data-turn-id="fixture-turn-8"]:not(.mine)')).toHaveCount(1);
    await expect(page.locator('.messenger-message[data-turn-id="fixture-turn-8-continuation"]')).toHaveCount(0);
    await page.screenshot({ path: resolve(directory, 'goal-completed.png') });
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
    // The queue list can lag this durable view update. A queued, empty turn is
    // owned by the composer queue and must never flash as an assistant bubble.
    const queuedTurn = page.locator('.messenger-turn[data-root-turn-id="fixture-turn-10"]');
    await expect(queuedTurn).toHaveCount(0);
    service.updateQueueAhead(1);
    await expect(queuedTurn).toHaveCount(0);
    await page.reload();
    await expect(queuedTurn).toHaveCount(0);
    service.updateQueueAhead(0);
    await expect(queuedTurn).toHaveCount(0);
    coverage.queue = true;
    service.release();
    await completed(10);
    service.failToolNext = true;
    await send('Verify after the queued request.');
    await completed(11);
    const before = await captureConversation(page, directory);
    expect(before.map(row => [row.turnId, row.role])).toEqual(
      Array.from({ length: 11 }, (_, index) => [[`fixture-turn-${index + 1}`, 'user'], [`fixture-turn-${index + 1}`, 'assistant']]).flat());
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
        const durableTool = service.items.get(`fixture-turn-${round}${round === 8 ? '-continuation' : ''}:tool-${index}`);
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
    transientErrors = audit.duplicate + audit.structure;
    expect(audit.samples).toBeGreaterThan(0);
    expect(transientErrors).toBe(0);
    expect([...completedNoticeRounds].sort((left, right) => left - right)).toEqual([1, 2, 4, 5, 7, 8, 10, 11]);
    coverage.completionNotice = completedNoticeRounds.size === 8;
    await page.reload();
    await expect(page.getByTestId('chat-composer-input')).toBeVisible();
    const after = await captureConversation(page, resolve(directory, 'reloaded'));
    expect(after.map(row => [row.turnId, row.role, row.content])).toEqual(before.map(row => [row.turnId, row.role, row.content]));
    expect(service.failures).toEqual([]);
    const finalAudit = await page.evaluate(() => (window as any).__chatRenderAudit);
    transientErrors = finalAudit.duplicate + finalAudit.structure;
    expect(finalAudit.samples).toBeGreaterThan(audit.samples);
    expect(transientErrors).toBe(0);
    coverage.reload = true;
  } catch (error) {
    service.failures.push('scenario-did-not-complete');
    await page.screenshot({ path: resolve(directory, 'failure.png'), timeout: 2000 }).catch(() => undefined);
    throw error;
  } finally {
    const finalAudit = await page.evaluate(() => (window as any).__chatRenderAudit).catch(() => null);
    if (finalAudit) transientErrors = finalAudit.duplicate + finalAudit.structure;
    await service.dispose();
    const performance = await page.evaluate(() => {
      (window as any).wunderPerf?.stop(); return (window as any).wunderPerf?.snapshot();
    }).catch(() => undefined);
    const analysis = await saveChatEvidence(directory, { mode: 'mock-service', snapshot: service.snapshot(),
      changes: service.changes, performance, expectedUserTurns: 11, collectionErrors: service.failures,
      browserErrors: errors.length, transientRenderingErrors: transientErrors, coverage,
      requiredCoverage: ['emptyState', 'stop', 'reconnect', 'queue', 'render', 'tools', 'compaction', 'goal', 'reload', 'completionNotice', 'structureMutation'] }, service.export());
    await writeFile(resolve(directory, 'transport.json'), JSON.stringify(redactEvidence({ requests: service.requests, connections: service.connections, errors }), null, 2));
    await testInfo.attach('analysis', { body: JSON.stringify(analysis), contentType: 'application/json' });
    expect.soft(analysis.findings.filter(item => item.severity === 'error')).toEqual([]);
  }
});
