// Independent contract checks: do not ask the renderer whether its own output is correct.
export type Row = Record<string, any>;
export type ChatEvidence = {
  mode: 'scripted-events' | 'mock-service' | 'real-service';
  snapshot: { cursor: number; turns: Row[]; items: Row[]; blocks: Row[] };
  changes: Row[];
  performance?: Row;
  browserErrors?: number;
  transientRenderingErrors?: number;
  collectionErrors?: string[];
  expectedUserTurns?: number;
  coverage?: Record<string, boolean>;
  requiredCoverage?: string[];
};
export type Finding = { severity: 'error' | 'warning'; code: string; count: number };
export const flattenItem = (row: Row): Row => ({ ...row.payload, ...row,
  model_round: row.model_round ?? row.payload?.model_round,
  content: row.content ?? row.payload?.content,
  meta: row.meta ?? row.payload?.meta });
const terminal = new Set(['completed', 'cancelled', 'failed']);
const active = new Set(['running', 'queued', 'pending', 'loading', 'streaming']);

export function analyzeChatEvidence(evidence: ChatEvidence) {
  const findings: Finding[] = [];
  const add = (code: string, count = 1, severity: Finding['severity'] = 'error') => {
    if (count) findings.push({ severity, code, count });
  };
  const { snapshot } = evidence;
  const turns = snapshot.turns;
  const items = snapshot.items.map(flattenItem);
  const byTurn = new Map(turns.map(turn => [turn.turn_id, turn]));
  const round = (turn: Row) => turn.user_round ?? turn.user_turn_index;
  const userTurns = turns.filter(turn => Number(round(turn)) > 0);
  add('duplicate-turn-id', turns.length - byTurn.size);
  add('duplicate-user-round', userTurns.length - new Set(userTurns.map(round)).size);
  add('duplicate-item-id', items.length - new Set(items.map(item => item.item_id)).size);
  if (evidence.expectedUserTurns !== undefined) {
    add('unexpected-user-turn-count', Number(userTurns.length !== evidence.expectedUserTurns));
  }
  add('unfinished-turn', turns.filter(turn => !terminal.has(turn.status)).length);
  add('orphan-item', items.filter(item => !byTurn.has(item.turn_id)).length);
  add('unfinished-item-in-terminal-turn', items.filter(item =>
    terminal.has(byTurn.get(item.turn_id)?.status) && active.has(item.status)).length);
  const stable = items.filter(item => item.kind === 'assistant_message' &&
    item.item_id === `${item.turn_id}:text-${item.model_round}`);
  for (const turn of userTurns) {
    add('user-item-count', Number(items.filter(item => item.turn_id === turn.turn_id &&
      item.kind === 'user_message').length !== 1));
    if (turn.status === 'completed') {
      const texts = stable.filter(item => item.turn_id === turn.turn_id);
      add('missing-stable-assistant', Number(texts.length === 0));
      const last = texts.sort((a, b) => a.model_round - b.model_round).at(-1);
      if (last) add('missing-terminal-stats', Number(!last.meta?.message_stats && !last.stats), 'warning');
    }
  }
  const itemsById = new Map(items.map(item => [item.item_id, item]));
  add('orphan-text-block', snapshot.blocks.filter(block => !itemsById.has(block.item_id)).length);
  const seqs = new Set<number>();
  let previous = 0;
  const revisions = new Map<string, number>();
  const outcomes = new Map<string, string>();
  for (const change of evidence.changes) {
    const seq = Number(change.change_seq ?? change.cursor ?? change.seq);
    if (!Number.isSafeInteger(seq) || seq <= 0) { add('invalid-change-seq'); continue; }
    add('duplicate-durable-change', Number(seqs.has(seq)));
    add('durable-gap-or-reorder', Number(seq !== previous + 1));
    seqs.add(seq);
    previous = seq;
    const payload = flattenItem(change.payload ?? change.data ?? {});
    const turnId = change.turn_id ?? payload.turn_id;
    if (change.change_type === 'item_upsert') {
      const id = change.item_id ?? payload.item_id;
      const revision = Number(change.revision ?? payload.revision);
      add('item-revision-regression', Number(revisions.has(id) && revision < revisions.get(id)!));
      revisions.set(id, revision);
    }
    if (['turn_status', 'turn_upsert'].includes(change.change_type) && payload.status) {
      const prior = outcomes.get(turnId);
      add('terminal-turn-revived', Number(Boolean(prior && terminal.has(prior) && prior !== payload.status)));
      outcomes.set(turnId, payload.status);
    }
  }
  add('incomplete-change-log', Number(previous !== snapshot.cursor));
  add('browser-error', evidence.browserErrors ?? 0);
  add('transient-rendering-error', evidence.transientRenderingErrors ?? 0);
  add('evidence-collection-failed', evidence.collectionErrors?.length ?? 0);
  for (const name of evidence.requiredCoverage ?? []) {
    add(`unproven-coverage:${name}`, Number(evidence.coverage?.[name] !== true));
  }
  const perf = evidence.performance;
  add('missing-performance-capture', Number(!perf?.capture?.startedAt));
  const frame = perf?.summary?.responsiveness?.frameGaps;
  if (frame) add('frame-p95-exceeds-100ms', Number(frame.p95UpperMs > 100));
  const longTasks = perf?.summary?.responsiveness?.longTasks;
  add('browser-long-task', Number(longTasks?.count ?? 0), 'warning');
  // A single long task needs investigation, not an unsupported "environment noise" dismissal.
  add('main-thread-stall-over-250ms', Number(longTasks?.maxMs > 250));
  return {
    schemaVersion: 1, mode: evidence.mode,
    verdict: findings.some(item => item.severity === 'error') ? 'failed' :
      findings.length ? 'needs-review' : 'passed',
    coverage: evidence.coverage ?? {},
    counts: { turns: turns.length, items: items.length, changes: evidence.changes.length },
    findings,
    limitations: evidence.mode === 'scripted-events'
      ? ['No real send, cancellation, scheduler, compaction execution or network reconnection was exercised.']
      : evidence.mode === 'mock-service'
        ? ['HTTP/WebSocket are simulated; browser actions and rendering are real. Backend execution is not verified.']
        : ['Only observed scenarios are proven; this run cannot guarantee the absence of all defects.']
  };
}
