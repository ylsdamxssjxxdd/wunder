// Read credentials and the target session from stdin; never persist them or
// transcript content. This check opens existing threads without sending turns.
import assert from 'node:assert/strict';
import { chromium } from '@playwright/test';

let input = '';
for await (const chunk of process.stdin) input += chunk;
const { username, password, session, origin = 'http://127.0.0.1:18001' } = JSON.parse(input);
input = '';
assert.ok(username && password && session, 'stdin requires username, password and session');
const login = await fetch(`${origin}/wunder/auth/login`, {
  method: 'POST', headers: { 'content-type': 'application/json', 'x-wunder-session-scope': 'view-verification' },
  body: JSON.stringify({ username, password })
});
assert.equal(login.status, 200, 'verification login');
const auth = await login.json();
const token = auth.data?.access_token ?? auth.access_token;
assert.ok(token, 'access token');
const reads = [];
async function read(path, label) {
  const start = performance.now();
  const response = await fetch(`${origin}/wunder${path}`, { headers: { authorization: `Bearer ${token}` } });
  const text = await response.text();
  reads.push({ label, status: response.status, ms: Math.round(performance.now() - start), bytes: Buffer.byteLength(text) });
  assert.equal(response.status, 200, label);
  return JSON.parse(text).data;
}
const prefix = `/chat/sessions/${encodeURIComponent(session)}`;
const sessions = await read('/chat/sessions?limit=20', 'session-list');
const ordinary = (sessions.items ?? []).filter(item => item.id !== session).slice(0, 5);
for (const item of ordinary) {
  const data = await read(`/chat/sessions/${encodeURIComponent(item.id)}/view/bootstrap?kind=chat`, 'ordinary-bootstrap');
  assert.ok(!data.page.degraded && ['ready', 'source'].includes(data.page.projection_state));
}
let cursor;
let agent = '';
for (let n = 0; n < 6; n++) {
  const data = await read(`${prefix}/view/bootstrap?kind=chat`, 'bootstrap');
  assert.ok(!data.page.degraded && ['ready', 'source'].includes(data.page.projection_state), 'nondegraded bootstrap');
  assert.ok(data.page.chat.length > 0 && data.page.chat.length <= 12);
  cursor = data.page.next_before;
  agent = data.session.agent_id || '';
}
for (let wave = 0; wave < 2; wave++) {
  await Promise.all(Array.from({ length: 4 }, async () => {
    const data = await read(`${prefix}/view/bootstrap?kind=chat`, 'concurrent-bootstrap');
    assert.ok(!data.page.degraded && data.page.chat.length > 0, 'concurrent bootstrap');
  }));
}
const rounds = new Set();
for (let n = 0; n < 8 && cursor != null; n++) {
  const data = await read(`${prefix}/view/chat?before=${cursor}&limit=20`, 'history');
  const p = data.page;
  assert.ok(!p.degraded && p.chat.length > 0 && p.chat.length <= 20, 'nondegraded history');
  for (const row of p.chat) { assert.ok(row.user_round < cursor && !rounds.has(row.user_round)); rounds.add(row.user_round); }
  assert.ok(p.next_before < cursor, 'advancing keyset cursor');
  cursor = p.has_more ? p.next_before : null;
}
// A bounded browser window may evict its newer edge while reading old pages.
// The inverse keyset read must recover that edge without re-opening the thread
// or doing an offset scan.
const tail = await read(`${prefix}/view/bootstrap?kind=chat`, 'forward-chat-tail');
const processRoot = tail.page.chat.at(-1)?.root_turn_id;
if (processRoot) {
  const processPath = `${prefix}/thread-log/turns/${encodeURIComponent(processRoot)}`;
  let processPage = (await read(`${processPath}?item_before=${Number.MAX_SAFE_INTEGER}&limit=100`, 'process-tail')).turn;
  assert.ok(processPage.items.length <= 100 && processPage.items.length > 0);
  const processTail = processPage.items.at(-1).item_index;
  assert.equal(processPage.has_newer, false);
  for (let n = 0; n < 5 && processPage.has_older; n++) {
    const before = processPage.next_before;
    processPage = (await read(`${processPath}?item_before=${before}&limit=100`, 'process-older')).turn;
    assert.ok(processPage.items.length > 0 && processPage.items.length <= 100);
    assert.ok(processPage.items.every(item => item.item_index < before));
  }
  for (let n = 0; n < 6 && processPage.has_newer; n++) {
    const after = processPage.next_after;
    processPage = (await read(`${processPath}?item_after=${after}&limit=100`, 'process-newer')).turn;
    assert.ok(processPage.items.length > 0 && processPage.items.length <= 100);
    assert.ok(processPage.items.every(item => item.item_index > after));
  }
  assert.equal(processPage.next_after, processTail);
}
const olderForForward = await read(`${prefix}/view/chat?before=${tail.page.next_before}&limit=20`, 'forward-chat-older');
if (olderForForward.page.chat.length && olderForForward.page.next_after != null) {
  const forward = await read(`${prefix}/view/chat?after=${olderForForward.page.next_after}&limit=20`, 'forward-chat');
  assert.ok(forward.page.chat.length > 0 && forward.page.chat[0].user_round > olderForForward.page.next_after);
}
const trajectoryKinds = new Set();
let trajectoryCursor;
for (let n = 0; n < 3; n++) {
  const data = await read(`${prefix}/view/trajectory?limit=80${trajectoryCursor == null ? '' : `&before=${trajectoryCursor}`}`, 'trajectory');
  assert.ok(!data.page.degraded && data.page.trajectory.length > 0);
  for (const row of data.page.trajectory) trajectoryKinds.add(row.kind);
  if (!data.page.has_more) break;
  assert.ok(trajectoryCursor == null || data.page.next_before < trajectoryCursor);
  trajectoryCursor = data.page.next_before;
}
const trajectoryTail = await read(`${prefix}/view/trajectory?limit=80`, 'forward-trajectory-tail');
const trajectoryOlder = await read(`${prefix}/view/trajectory?before=${trajectoryTail.page.next_before}&limit=80`, 'forward-trajectory-older');
if (trajectoryOlder.page.trajectory.length && trajectoryOlder.page.next_after != null) {
  const forward = await read(`${prefix}/view/trajectory?after=${trajectoryOlder.page.next_after}&limit=80`, 'forward-trajectory');
  assert.ok(forward.page.trajectory.length > 0 && forward.page.trajectory[0].created_seq > trajectoryOlder.page.next_after);
}

const browser = await chromium.launch({ headless: true });
const diagnostics = { pageErrors: 0, consoleErrors: 0, openFailureToasts: 0, httpFailures: 0, degradedPages: 0, browserHistoryPages: 0 };
try {
  const context = await browser.newContext();
  await context.addInitScript(() => {
    window.__viewFailureCount = 0;
    new MutationObserver(records => {
      for (const record of records) for (const node of record.addedNodes) {
        if (!(node instanceof Element)) continue;
        const notices = [...node.querySelectorAll('.el-message, [role="alert"]')];
        if (node.matches('.el-message, [role="alert"]')) notices.push(node);
        for (const notice of notices) if (/打开.*失败|open.*fail|响应无效/i.test(notice.textContent || '')) window.__viewFailureCount++;
      }
    }).observe(document, { childList: true, subtree: true });
  });
  const page = await context.newPage();
  page.on('pageerror', () => diagnostics.pageErrors++);
  page.on('console', message => { if (message.type() === 'error') diagnostics.consoleErrors++; });
  const responses = new Set();
  page.on('response', response => {
    if (!response.url().includes('/view/')) return;
    const task = (async () => {
      if (response.status() >= 400) diagnostics.httpFailures++;
      try {
        const data = (await response.json()).data;
        if (data?.page?.degraded) diagnostics.degradedPages++;
        if (response.url().includes('/view/chat?')) diagnostics.browserHistoryPages++;
      } catch { /* navigation can discard response bodies */ }
    })();
    responses.add(task); void task.finally(() => responses.delete(task));
  });
  // Let the normal first-visit version migration run before installing the
  // test login, just as the login form does. It clears pre-migration tokens.
  await page.goto(`${origin}/login`);
  await page.locator('input[autocomplete="username"]').waitFor();
  await page.evaluate(token => localStorage.setItem('access_token', token), token);
  for (const item of ordinary) {
    const bootstrap = page.waitForResponse(response => response.url().includes(`/sessions/${item.id}/view/bootstrap`));
    await page.goto(`${origin}/app/chat?section=messages&session_id=${encodeURIComponent(item.id)}&agent_id=${encodeURIComponent(item.agent_id || '')}`);
    const response = await bootstrap;
    assert.equal(response.status(), 200);
    await page.getByTestId('chat-composer-input').waitFor();
    await page.waitForTimeout(500);
    diagnostics.openFailureToasts += await page.evaluate(() => window.__viewFailureCount);
  }
  for (let n = 0; n < 3; n++) {
    await page.goto(`${origin}/app/chat?section=messages&session_id=${encodeURIComponent(session)}&agent_id=${encodeURIComponent(agent)}`);
    try { await page.locator('.messenger-turn').first().waitFor({ timeout: 20000 }); }
    catch (error) {
      console.log(JSON.stringify({ reads, diagnostics, pathname: new URL(page.url()).pathname,
        state: await page.evaluate(() => ({ turns: document.querySelectorAll('.messenger-turn').length,
          lists: document.querySelectorAll('[data-testid="messenger-message-list"]').length,
          tokenPresent: Boolean(localStorage.getItem('access_token')), sessionTokenPresent: Boolean(sessionStorage.getItem('access_token')),
          notices: document.querySelectorAll('.el-message, [role="alert"]').length })) }));
      throw error;
    }
    await page.waitForTimeout(700);
    const list = page.getByTestId('messenger-message-list');
    for (let scroll = 0; scroll < 3; scroll++) {
      await list.evaluate(element => { element.scrollTop = 0; element.dispatchEvent(new Event('scroll')); });
      await page.waitForTimeout(1200);
    }
    diagnostics.openFailureToasts += await page.evaluate(() => window.__viewFailureCount);
  }
  // Expand one real large process and cross its 400-item retained window in
  // both directions. Only metadata and counts are emitted by this verifier.
  await page.goto(`${origin}/app/chat?section=messages&session_id=${encodeURIComponent(session)}&agent_id=${encodeURIComponent(agent)}`);
  const processHead = page.locator('.timeline-turn-process .timeline-process-head').last();
  await processHead.waitFor({ timeout: 20000 });
  await processHead.click();
  const processBody = page.locator('.timeline-turn-process .timeline-process-body').last();
  await processBody.locator('[data-process-key]').first().waitFor();
  for (const direction of ['item_before', 'item_after']) {
    for (let n = 0; n < 5; n++) {
      const edge = direction === 'item_before' ? '.timeline-process-page-state-top' : '.timeline-process-page-state:not(.timeline-process-page-state-top)';
      if (!await processBody.locator(edge).count()) break;
      const response = page.waitForResponse(r => r.url().includes('/thread-log/turns/') && new URL(r.url()).searchParams.has(direction));
      await processBody.evaluate((element, direction) => {
        element.scrollTop = direction === 'item_before' ? 0 : element.scrollHeight;
        element.dispatchEvent(new Event('scroll'));
      }, direction);
      assert.equal((await response).status(), 200);
      await page.waitForTimeout(250);
      assert.ok(await processBody.locator('[data-process-key]').count() <= 800);
    }
  }
  await page.goto(`${origin}/app/trajectory?session=${encodeURIComponent(session)}`);
  await page.locator('.tt-root').waitFor({ timeout: 15000 });
  await page.locator('.tt-row').first().waitFor({ timeout: 15000 });
  diagnostics.openFailureToasts += await page.evaluate(() => window.__viewFailureCount);
  await Promise.all(responses);
} finally { await browser.close(); }
console.log(JSON.stringify({ reads, distinctHistoryRounds: rounds.size, trajectoryKinds: [...trajectoryKinds], diagnostics }, null, 2));
assert.equal(diagnostics.pageErrors, 0);
assert.equal(diagnostics.consoleErrors, 0);
assert.equal(diagnostics.openFailureToasts, 0);
assert.equal(diagnostics.httpFailures, 0);
assert.equal(diagnostics.degradedPages, 0);
assert.ok(diagnostics.browserHistoryPages > 0, 'browser must exercise history pagination');
