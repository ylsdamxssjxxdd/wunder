// AI生成
// 云端版流式节奏探针（诊断脚本，手动运行）。
//
// 用途：当用户反馈「思考/正文卡在开头、结束时一股脑出现」时，用它在真实
// MessengerView 里复现并量化：正文是否随帧连续增长、服务端偏移空间重建后是否
// 还在增长、重放帧是否只算一次。
//
// 前置：`cd frontend && npm run dev:raw -- --port 4174`（或任意 dev server），
// 然后 `BASE=http://127.0.0.1:4174 node scripts/verify-chat-stream-pacing.mjs`。
import { chromium } from '@playwright/test';

const BASE = process.env.BASE || 'http://127.0.0.1:4174';
const SESSION = 'pacing-probe';

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
page.on('pageerror', (error) => console.log('[pageerror]', String(error).slice(0, 300)));

// 探针不打真实后端：登录态与接口都走 dev harness 自己装的 fixture。
await page.routeWebSocket(/\/wunder\//, (socket) => socket.close());
await page.route('**/*', async (route) => {
  const url = new URL(route.request().url());
  if (!url.pathname.startsWith('/wunder/')) return route.continue();
  const body = /thread-log\/turns/.test(url.pathname)
    ? { data: { turn: { items: [], has_older: false, has_newer: false } } }
    : { data: { items: [] } };
  await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
});

await page.goto(`${BASE}/__e2e/messenger-view-performance?session_id=${SESSION}`);
await page.waitForFunction(() => Boolean(window.__messengerViewPerformanceE2E), null, { timeout: 60_000 });
await page.evaluate((session) => window.__messengerViewPerformanceE2E.installSession(session, 4), SESSION);
await page.waitForTimeout(600);

const result = await page.evaluate(async (sessionId) => {
  // 必须拿到**页面已经在用的**那几份模块实例：dev server 会给改动过的文件加
  // `?t=`，裸导入等于另起一份模块图，store 与页面各持一份 state，探针就会读到
  // 空行、DOM 永远 -1。先从资源时序里找回应用真实的模块 URL，再导入。
  const appModuleUrl = (path) => {
    const loaded = performance.getEntriesByType('resource')
      .map((entry) => entry.name)
      .find((name) => name.includes(`${path}?`));
    return loaded ? loaded.replace(location.origin, '') : path;
  };
  const [chatModule, runtime, viewStore, hubModule] = await Promise.all([
    import(appModuleUrl('/src/stores/chat.ts')),
    import(appModuleUrl('/src/realtime/chat/chatThreadRuntime.ts')),
    import(appModuleUrl('/src/realtime/chat/threadViewStore.ts')),
    import(appModuleUrl('/src/stores/sessionHub.ts'))
  ]);
  const chat = chatModule.useChatStore();
  const hub = hubModule.useSessionHubStore();
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const turnId = 'pacing-turn';
  const itemId = `${turnId}:text-1`;

  viewStore.beginThreadView(sessionId, true);
  const generation = viewStore.threadViewGeneration(sessionId);
  viewStore.commitThreadView(sessionId, {
    session_id: sessionId, epoch: 1, view_seq: 3, projection_state: 'ready',
    thread_empty: false, latest_round: 1, has_more: false, has_newer: false,
    next_before: null, trajectory: [], tiles: [], tile_level: 0,
    runtime_status: 'running', status_turn_id: turnId, active_turn_id: turnId,
    chat: [{
      root_turn_id: turnId, execution_turn_id: turnId, user_round: 1, revision: 3,
      status: 'running', user: { item_id: `${turnId}:user`, content: 'probe input' },
      // 持久行只带被截断的窗口：正文头、思考尾，实时文本全靠 tail 帧。
      assistant: { item_id: itemId, content: '', reasoning: '', content_offset: 0, reasoning_offset: 0, created_seq: 3 },
      process_items: [{ item_id: `${turnId}:tool-1`, turn_id: turnId, kind: 'tool_call', status: 'running', payload: { tool: 'exec_command', model_round: 1 } }],
      workflow_count: 1, failed_count: 0
    }]
  }, generation);
  chat.activeSessionId = sessionId;
  hub.setActiveConversation({ kind: 'agent', id: sessionId, agentId: 'perf-agent' });
  await sleep(400);

  const apply = (field, offset, text) => runtime.applyChatThreadServerEvent(chat, sessionId, 'thread_item_tail', {
    data: { item_id: itemId, field, offset, base_seq: 0, text }
  });
  const bodyLength = () => {
    const node = document.querySelector(`[data-virtual-key="runtime:assistant:tturn:${turnId}:assistant"] .message-markdown-body`);
    return node ? (node.textContent || '').length : -1;
  };
  const liveLength = () => {
    const slot = (viewStore.threadViewSlots(sessionId) || [])[0];
    return String(slot?.assistant?.__thread_live || '').length;
  };

  const growth = [];
  let offset = 0;
  for (let index = 0; index < 160; index += 1) {
    const text = `正文片段${index}，用于测量流式节奏。`;
    apply('content', offset, text);
    offset += text.length;
    if (index % 20 === 0 || index === 159) {
      await sleep(16);
      growth.push(bodyLength());
    }
  }
  const stalled = growth.some((length, index) =>
    index > 0 && length <= growth[index - 1] && length !== offset && growth.at(-1) !== offset);

  const beforeRestart = liveLength();
  let restarted = 0;
  for (let index = 0; index < 40; index += 1) {
    const text = `续写片段${index}。`;
    apply('content', restarted, text);
    restarted += text.length;
    await sleep(8);
  }
  await sleep(120);
  const afterRestart = liveLength();

  // 重放帧必须和窗口自身的末尾对齐：流式窗口按自己的长度寻址，偏移重建之后
  // 服务端坐标与窗口坐标不再重合。
  const replayText = '续写片段39。';
  const replayOffset = viewStore.threadViewSlots(sessionId)[0].assistant.__thread_live_offset - replayText.length;
  const beforeReplay = liveLength();
  apply('content', replayOffset, replayText);
  await sleep(60);
  const afterReplay = liveLength();

  return {
    totalContent: offset,
    domGrowth: growth,
    liveBeforeRestart: beforeRestart,
    liveAfterRestart: afterRestart,
    restartedUnits: restarted,
    replayDelta: afterReplay - beforeReplay,
    stalled
  };
}, SESSION);

await browser.close();

const problems = [];
if (result.stalled) problems.push('正文在流式期间出现停滞（DOM 长度不再增长）');
if (result.liveAfterRestart < result.liveBeforeRestart + result.restartedUnits) {
  problems.push(`服务端偏移重建后丢帧：期望 ${result.liveBeforeRestart + result.restartedUnits}，实际 ${result.liveAfterRestart}`);
}
if (result.replayDelta !== 0) problems.push(`重放帧被重复追加了 ${result.replayDelta} 个字符`);

console.log(JSON.stringify(result, null, 1));
if (problems.length) {
  console.error('FAIL:\n- ' + problems.join('\n- '));
  process.exit(1);
}
console.log('chat stream pacing: 正文连续增长、偏移重建后继续流式、重放帧幂等 pass');
