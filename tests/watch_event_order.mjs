import assert from 'node:assert/strict';

const events = [
  {
    id: 1,
    type: 'round_start',
    data: { user_round: 1, question: 'user message A' },
    timestamp: '2026-02-05T09:46:06+08:00'
  },
  {
    id: 2,
    type: 'progress',
    data: { user_round: 1, stage: 'llm_call' },
    timestamp: '2026-02-05T09:46:06+08:00'
  },
  {
    id: 3,
    type: 'round_start',
    data: { user_round: 2, question: 'cron trigger B' },
    timestamp: '2026-02-05T09:46:08+08:00'
  },
  {
    id: 4,
    type: 'progress',
    data: { user_round: 2, stage: 'llm_call' },
    timestamp: '2026-02-05T09:46:08+08:00'
  },
  {
    id: 5,
    type: 'final',
    data: { user_round: 1, answer: 'A done' },
    timestamp: '2026-02-05T09:46:10+08:00'
  },
  {
    id: 6,
    type: 'final',
    data: { user_round: 2, answer: 'B done' },
    timestamp: '2026-02-05T09:46:12+08:00'
  }
];

const messages = [];
const roundStates = new Map();
const completedRounds = new Set();

const normalizeRound = (value) => {
  const number = Number(value);
  return Number.isFinite(number) ? number : null;
};

const ensureRoundState = (round, timestamp) => {
  if (!Number.isFinite(round) || round <= 0) return null;
  if (completedRounds.has(round)) return null;
  const existing = roundStates.get(round);
  if (existing) return existing;
  const state = {
    round,
    userInserted: false,
    message: {
      role: 'assistant',
      created_at: timestamp,
      workflowItems: [],
      stream_incomplete: true
    }
  };
  messages.push(state.message);
  roundStates.set(round, state);
  return state;
};

const insertUserMessage = (content, timestamp, anchor) => {
  const message = { role: 'user', content, created_at: timestamp };
  if (anchor) {
    const index = messages.indexOf(anchor);
    if (index >= 0) {
      messages.splice(index, 0, message);
      return;
    }
  }
  messages.push(message);
};

const finalizeRound = (round) => {
  const state = roundStates.get(round);
  if (!state) return;
  state.message.stream_incomplete = false;
  roundStates.delete(round);
  completedRounds.add(round);
};

events.forEach((event) => {
  const round = normalizeRound(event.data?.user_round);
  const state = ensureRoundState(round, event.timestamp);
  const isRoundStart = event.type === 'round_start' || (event.type === 'progress' && event.data?.stage === 'start');
  if (isRoundStart && state && !state.userInserted && event.data?.question) {
    insertUserMessage(event.data.question, event.timestamp, state.message);
    state.userInserted = true;
  }
  if (state) {
    state.message.workflowItems.push(event.type);
  }
  if (event.type === 'final') {
    finalizeRound(round);
  }
});

const roles = messages.map((message) => message.role);
assert.deepEqual(roles, ['user', 'assistant', 'user', 'assistant']);
assert.deepEqual(messages[1].workflowItems, ['round_start', 'progress', 'final']);
assert.deepEqual(messages[3].workflowItems, ['round_start', 'progress', 'final']);

console.log('watch_event_order: ok');
