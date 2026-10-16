import test from 'node:test';
import assert from 'node:assert/strict';
import { createPinia, setActivePinia } from 'pinia';

const storage = new Map<string, string>();
(globalThis as Record<string, unknown>).localStorage = {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => storage.set(key, String(value)),
  removeItem: (key: string) => storage.delete(key),
  clear: () => storage.clear()
};

const goal = {
  goal_id: 'goal-fixture',
  objective: 'Complete the example task',
  phase: 'active',
  activation: 'armed',
  revision: 1,
  max_goal_rounds: 8,
  rounds_started: 0,
  blocked_code: null,
  blocked_message: null,
  created_at: 100,
  updated_at: 100
};

test('API goal projection populates the goal bar and session cache without legacy status fields', async () => {
  const { applyGoalStreamEvent, isGoalCompletedForNotice } = await import('../../src/stores/chatPersist');
  const { useChatStore } = await import('../../src/stores/chat');
  setActivePinia(createPinia());
  const store = useChatStore();
  store.activeSessionId = 'session-goal-fixture';
  store.sessions = [{ id: store.activeSessionId, agent_id: 'agent-fixture' }];
  store.syncSessionGoal(store.activeSessionId, goal);

  assert.equal(store.sessionGoal(store.activeSessionId)?.objective, goal.objective);
  assert.equal(store.sessionGoal(store.activeSessionId)?.phase, 'active');
  assert.equal(store.isSessionGoalLocked(store.activeSessionId), true);
  assert.equal(store.sessions[0].goal?.goal_id, goal.goal_id);

  assert.equal(applyGoalStreamEvent(store, store.activeSessionId, 'goal_updated', {
    operation: 'paused', goal: { ...goal, phase: 'paused', activation: 'disarmed', revision: 2 }
  }), true);
  assert.equal(store.sessionGoal(store.activeSessionId)?.phase, 'paused');
  assert.equal(store.isSessionGoalLocked(store.activeSessionId), false);

  applyGoalStreamEvent(store, store.activeSessionId, 'goal_updated', {
    operation: 'blocked', goal: { ...goal, phase: 'blocked', activation: 'disarmed',
      blocked_code: 'example-blocker', blocked_message: 'Example resource unavailable.' }
  });
  assert.equal(store.sessionGoal(store.activeSessionId)?.blocked_message, 'Example resource unavailable.');
  applyGoalStreamEvent(store, store.activeSessionId, 'goal_updated', {
    operation: 'completed', goal: { ...goal, phase: 'complete', activation: 'disarmed' }
  });
  assert.equal(isGoalCompletedForNotice(store.sessionGoal(store.activeSessionId)), true);
  applyGoalStreamEvent(store, store.activeSessionId, 'goal_updated', { operation: 'cleared', goal: null });
  assert.equal(store.sessionGoal(store.activeSessionId), null);
  assert.equal(store.sessions[0].goal, undefined);
});

test('persisted disarmed goal is visible and does not lock input before human resume', async () => {
  const { normalizeSessionGoal, isGoalActiveForLock } = await import('../../src/stores/chatPersist');
  const normalized = normalizeSessionGoal({ ...goal, activation: 'disarmed', rounds_started: 3 });
  assert.equal(normalized?.phase, 'active');
  assert.equal(normalized?.rounds_started, 3);
  assert.equal(normalized?.max_goal_rounds, 8);
  assert.equal(isGoalActiveForLock(normalized), false);
  assert.equal(normalizeSessionGoal({ ...goal, phase: 'unknown' }), null);
});
