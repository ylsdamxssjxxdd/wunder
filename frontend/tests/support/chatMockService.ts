import type { Page, WebSocketRoute } from '@playwright/test';
import type { Row } from './chatEvidenceAnalysis';

export const MOCK_SESSION = 'fixture-thread';
export const MOCK_AGENT = 'fixture-agent';
type Subscription = { socket: WebSocketRoute; request: string };

/** In-process protocol peer. Only HTTP/WS are mocked; no application store is accessed. */
export class ChatMockService {
  changes: Row[] = [];
  turns: Row[] = [];
  items = new Map<string, Row>();
  blocks = new Map<string, Row>();
  subscriptions: Subscription[] = [];
  requests: Row[] = [];
  connections = 0;
  goal: Row | null = null;
  failures: string[] = [];
  private round = 0;
  private queue: Row[] = [];
  private active: Row | null = null;
  private tasks = new Set<Promise<void>>();
  private stopped = false;
  private agentRuntimeState: 'idle' | 'running' = 'idle';
  holdNext = false;
  autoCompactNext = false;
  duplicateNext = false;
  failToolNext = false;
  queueNext = false;
  private queueHeld = false;
  updateQueueAhead(ahead: number) {
    const turn = this.queue[0];
    if (!turn) throw new Error('No queued fixture request');
    this.item(turn, 'queue', 'queue', 'queued', { event_type: 'queue_update',
      queue_ahead: ahead, queue_total: ahead + 1, active_ahead: 2, wait_ahead: ahead + 2 });
  }

  snapshot() { return { cursor: this.changes.length, turns: this.turns.map(turn => ({
    turn_id: turn.turn_id, root_turn_id: turn.root_turn_id ?? turn.turn_id,
    trigger_kind: turn.trigger_kind ?? 'user', user_turn_index: turn.user_round, status: turn.status, payload: structuredClone(turn) })),
    items: [...this.items.values()].map(item => ({ item_id: item.item_id, turn_id: item.turn_id,
      kind: item.kind, status: item.status, revision: item.revision, visibility: item.visibility, payload: structuredClone(item) })), blocks: structuredClone([...this.blocks.values()]) }; }
  export() { return [{ record_type: 'thread_meta', export_schema_version: 5, evidence_mode: 'mock-service', session_id: MOCK_SESSION },
    ...this.turns.flatMap(turn => [{ record_type: 'turn', turn }, ...[...this.items.values()]
      .filter(item => item.turn_id === turn.turn_id).map(item => ({ record_type: 'item', item }))]),
    ...[...this.blocks.values()].map(block => ({ record_type: 'block', block })),
    { record_type: 'export_complete' }]; }
  private record(request: Row) {
    if (this.requests.length >= 2000) throw new Error('mock request limit exceeded');
    this.requests.push(request);
  }
  private send(sub: Subscription, event: string, data: Row) {
    try { sub.socket.send(JSON.stringify({ type: 'event', request_id: sub.request,
      payload: { event, data: { session_id: MOCK_SESSION, ...data } } })); } catch { /* closed transport */ }
  }
  private commit(change_type: string, payload: Row) {
    if (this.changes.length >= 5000) throw new Error('mock change limit exceeded');
    const change = { change_seq: this.changes.length + 1, cursor: this.changes.length + 1,
      change_type, turn_id: payload.turn_id, item_id: payload.item_id,
      revision: payload.revision, payload: structuredClone(payload) };
    this.changes.push(change);
    for (const sub of this.subscriptions) {
      this.send(sub, 'thread_change', change);
      if (this.duplicateNext) this.send(sub, 'thread_change', change);
    }
  }
  private item(turn: Row, suffix: string, kind: string, status: string, extra: Row = {}) {
    const id = `${turn.turn_id}:${suffix}`;
    const previous = this.items.get(id);
    const item: Row = { ...previous, item_id: id, turn_id: turn.turn_id, user_round: turn.user_round,
      model_round: 1, kind, status, visibility: 'user', revision: (previous?.revision ?? 0) + 1, ...extra };
    this.items.set(id, item);
    this.commit('item_upsert', item);
    if (kind === 'assistant_message' && extra.content) {
      const block = { item_id: id, turn_id: turn.turn_id, field: 'content', block_index: 0, content_offset: 0, content: item.content };
      this.blocks.set(id, block);
      this.commit('text_block', block);
    }
  }
  private state(turn: Row, status: string) { turn.status = status; this.commit('turn_upsert', turn); }
  private accept(content: string, client_message_id = '') {
    const turn = { turn_id: `fixture-turn-${++this.round}`, user_round: this.round,
      content, client_message_id, status: 'queued', held: this.holdNext,
      autoCompact: this.autoCompactNext, failTool: this.failToolNext };
    this.holdNext = false; this.autoCompactNext = false; this.failToolNext = false;
    this.turns.push(turn);
    this.commit('turn_upsert', turn);
    this.item(turn, 'user', 'user_message', 'completed', { role: 'user', content });
    this.queue.push(turn);
    this.queueHeld = this.queueNext;
    this.queueNext = false;
    if (this.queueHeld) this.updateQueueAhead(3);
    this.schedule();
    return { ...turn, resume_from_seq: 0 };
  }
  private schedule() {
    if (this.active || this.stopped || this.queueHeld) return;
    const turn = this.queue.shift();
    if (!turn) return;
    this.active = turn;
    this.agentRuntimeState = 'running';
    const task = this.run(turn).catch(error => { this.failures.push(String(error)); }).finally(() => {
      this.tasks.delete(task);
      if (this.active === turn || this.active?.root_turn_id === turn.turn_id) this.active = null;
      if (!this.active && !this.queue.length && !this.queueHeld) this.agentRuntimeState = 'idle';
      this.schedule();
    });
    this.tasks.add(task);
  }
  private async pause() { await new Promise(resolve => setTimeout(resolve, 80)); }
  private async run(turn: Row) {
    await this.pause();
    if (this.stopped || turn.status === 'cancelled') return;
    if (turn.content.startsWith('/goal')) {
      // Production accepts the command, then starts a separate internal
      // execution turn under the SAME user round. Do not flatten this in mocks.
      const root = turn;
      this.state(root, 'completed');
      turn = { ...root, turn_id: `${root.turn_id}-continuation`, root_turn_id: root.turn_id,
        trigger_kind: 'continuation', content: '', client_message_id: '', status: 'queued', goalRun: true };
      this.turns.push(turn);
      this.active = turn;
      this.commit('turn_upsert', turn);
      this.item(turn, 'user', 'user_message', 'completed', { role: 'user',
        content: 'Internal fixture continuation.', visibility: 'model_internal', root_turn_id: root.turn_id });
    }
    this.state(turn, 'running');
    if (this.items.has(`${turn.turn_id}:queue`)) this.item(turn, 'queue', 'queue', 'completed', {
      event_type: 'queue_start', queue_ahead: 0, wait_ahead: 0 });
    this.item(turn, 'text-1', 'assistant_message', 'running', { role: 'assistant', content: '' });
    let offset = 0;
    const base = this.changes.length;
    for (const text of ['Partial ', 'reply ', `${turn.user_round}.`]) {
      for (const sub of this.subscriptions) this.send(sub, 'thread_item_tail', {
        item_id: `${turn.turn_id}:text-1`, field: 'content', offset, base_seq: base, text });
      offset += text.length;
      await this.pause();
      if (this.stopped || turn.status === 'cancelled') return;
    }
    this.item(turn, 'text-1', 'assistant_message', 'running', { content: `Partial reply ${turn.user_round}.` });
    const compact = turn.content === '/compact';
    for (let index = 1; index <= (compact ? 0 : 2); index++) {
      const data = { tool: 'read_file', tool_call_id: `call-${turn.user_round}-${index}`,
        args: { path: `fixture-${index}.txt` }, event_type: 'tool_call' };
      this.item(turn, `tool-${index}`, 'tool_call', 'running', data);
      await this.pause();
      if (this.stopped || turn.status === 'cancelled') return;
      const failed = turn.failTool && index === 2;
      this.item(turn, `tool-${index}`, 'tool_call', failed ? 'failed' : 'completed', { ...data, event_type: 'tool_result',
        result: { ok: !failed, content: failed ? 'Fixture tool unavailable.' : `Fixture result ${turn.user_round}-${index}` },
        duration_ms: 80 });
    }
    while (turn.held && turn.status !== 'cancelled' && !this.stopped) await this.pause();
    if (this.stopped || turn.status === 'cancelled') return;
    if (compact || turn.autoCompact) {
      this.item(turn, 'compaction', 'compaction', 'running', { trigger_mode: compact ? 'manual' : 'auto' });
      await this.pause();
      this.item(turn, 'compaction', 'compaction', 'completed', { trigger_mode: compact ? 'manual' : 'auto',
        summary_text: 'Retained fixture summary.', event_type: 'compaction' });
    }
    if (turn.goalRun) {
      this.goal = { status: 'complete', objective: 'Fixture objective' };
      this.item(turn, 'goal', 'plan', 'completed', { event_type: 'goal_completed', goal: this.goal });
    }
    this.item(turn, 'text-1', 'assistant_message', 'completed', { role: 'assistant',
      content: compact ? 'Retained fixture summary.' : `Completed reply ${turn.user_round}.`,
      meta: { message_stats: { interaction_duration_s: 1.2, visible_decode_speed_tps: 24,
        context_occupancy_tokens: 2048, toolCalls: compact ? 0 : 2 } } });
    this.state(turn, 'completed');
    for (const sub of this.subscriptions) this.send(sub, 'final', { status: 'completed', turn_id: turn.turn_id });
  }
  cancel() {
    const turn = this.active;
    if (!turn || !['running', 'queued'].includes(turn.status)) return;
    for (const item of this.items.values()) {
      if (item.turn_id === turn.turn_id && ['running', 'queued'].includes(item.status)) {
        this.item(turn, item.item_id.slice(turn.turn_id.length + 1), item.kind, 'cancelled', {
          ...(item.kind === 'assistant_message' ? { meta: { message_stats: {
            interaction_duration_s: 0.4, visible_decode_speed_tps: 20, toolCalls: 2 } } } : {}) });
      }
    }
    this.state(turn, 'cancelled');
    // Cancellation is not a successful task completion. Keep the runtime hot
    // until the worker exits so the next real completion still has a running →
    // idle edge, while no cancellation toast is generated.
  }
  /** Background scheduler uses durable events; it has no foreground start request. */
  scheduledTurn(status: 'queued' | 'rejected' | 'completed') {
    const turn = { turn_id: `fixture-turn-${++this.round}`, user_round: this.round,
      content: 'Scheduled fixture task', status };
    this.turns.push(turn);
    this.commit('turn_upsert', turn);
    this.item(turn, 'user', 'user_message', 'completed', { role: 'user', content: turn.content });
    if (status === 'rejected') this.item(turn, 'terminal', 'terminal', status,
      { error: { code: 'USER_BUSY', message: 'Fixture admission rejected' } });
    if (status === 'queued') this.item(turn, 'queue', 'queue', status,
      { event_type: 'queue_enter', queue_ahead: 2, wait_ahead: 2, queue_total: 3 });
    if (status === 'completed') this.item(turn, 'text-0', 'assistant_message', status,
      { role: 'assistant', model_round: 0, content: 'Scheduled fixture result',
        meta: { message_stats: { interaction_duration_s: 0.8 } } });
    return turn;
  }
  settleScheduled(turn: Row, status: 'cancelled' | 'completed') {
    for (const item of [...this.items.values()]) {
      if (item.turn_id === turn.turn_id && item.status === 'queued') {
        this.item(turn, item.item_id.slice(turn.turn_id.length + 1), item.kind, status);
      }
    }
    this.state(turn, status);
  }
  release() { if (this.active) this.active.held = false; this.queueHeld = false; this.schedule(); }
  disconnect() {
    for (const socket of new Set(this.subscriptions.map(sub => sub.socket))) socket.close({ code: 1012, reason: 'fixture-reconnect' });
    this.subscriptions = [];
  }
  async dispose() { this.stopped = true; await Promise.allSettled([...this.tasks]); }

  async install(page: Page) {
    await page.routeWebSocket(/\/wunder\//, socket => {
      this.connections++;
      socket.send(JSON.stringify({ type: 'ready', payload: {} }));
      socket.onClose(() => { this.subscriptions = this.subscriptions.filter(sub => sub.socket !== socket); });
      socket.onMessage(raw => {
        const message = JSON.parse(String(raw));
        this.record({ transport: 'ws', type: message.type, payload: message.payload });
        const sub = { socket, request: message.request_id };
        if (message.type === 'cancel') { this.cancel(); return; }
        if (!['start', 'watch', 'resume'].includes(message.type)) {
          if (!['ping', 'pong', 'unwatch'].includes(message.type)) this.failures.push('unexpected-ws-request');
          return;
        }
        this.subscriptions = this.subscriptions.filter(prior => prior.socket !== socket);
        this.subscriptions.push(sub);
        if (message.type === 'start') {
          const turn = this.accept(message.payload.content, message.payload.client_message_id);
          this.send(sub, 'stream_started', turn);
        }
        const after = Number(message.payload?.after_change_seq ?? 0);
        for (const change of this.changes.filter(change => change.change_seq > after)) this.send(sub, 'thread_change', change);
      });
    });
    await page.route('**/*', async route => {
      const request = route.request();
      const pathname = new URL(request.url()).pathname;
      if (!pathname.startsWith('/wunder/')) return route.continue();
      const path = pathname.slice('/wunder'.length);
      const payload = request.postDataJSON();
      this.record({ transport: 'http', path, method: request.method() });
      const session = { id: MOCK_SESSION, agent_id: MOCK_AGENT, title: 'Fixture conversation',
        updated_at: '2026-01-01T00:00:00Z', status: 'active', running: Boolean(this.active) };
      const agent = { id: MOCK_AGENT, name: 'Fixture Agent', description: '', tool_names: ['read_file'] };
      let data: any = { items: [], total: 0 };
      if (path === '/auth/login') data = { access_token: 'fixture-auth', user: { id: 'fixture-user', username: 'fixture-user', roles: ['user'] } };
      else if (path === '/auth/me') data = { id: 'fixture-user', username: 'fixture-user', roles: ['user'] };
      else if (path === '/auth/me/preferences' && request.method() === 'PATCH') data = { preferences: payload };
      else if (path === '/agents') data = { items: [agent] };
      else if (path === '/agents/running') data = {
        items: [{ agent_id: MOCK_AGENT, state: this.agentRuntimeState,
          session_id: this.agentRuntimeState === 'running' ? MOCK_SESSION : '' }], total: 1
      };
      else if (path === `/agents/${MOCK_AGENT}`) data = agent;
      else if (path === '/chat/sessions') data = request.method() === 'POST' ? session : { items: [session], total: 1 };
      else if (path.endsWith('/thread-log/snapshot')) data = this.snapshot();
      else if (path === `/chat/sessions/${MOCK_SESSION}`) data = { ...session, messages: [], transcript: [], thread_change_cursor: this.changes.length };
      else if (path.endsWith('/events')) data = { events: [], items: [], running: Boolean(this.active),
        runtime: { status: this.active ? 'running' : 'idle' } };
      else if (path.endsWith('/cancel')) { this.cancel(); data = { cancelled: true }; }
      else if (path.endsWith('/compaction')) data = this.accept('/compact', payload?.client_message_id);
      else if (path.endsWith('/goal')) {
        if (request.method() === 'PUT') {
          this.goal = { status: 'active', objective: payload?.objective };
          data = { ...this.accept(`/goal ${payload?.objective}`, payload?.client_message_id), goal: this.goal };
        } else data = { goal: this.goal };
      } else if (!['GET', 'HEAD'].includes(request.method())) {
        this.failures.push('unexpected-http-mutation');
        return route.fulfill({ status: 501, body: 'Unconfigured mock mutation' });
      }
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ data }) });
    });
  }
}
