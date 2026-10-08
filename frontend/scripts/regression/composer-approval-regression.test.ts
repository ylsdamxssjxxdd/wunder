import test from 'node:test';
import assert from 'node:assert/strict';
import { nextTick, ref } from 'vue';

import {
  normalizeAgentApprovalMode,
  useComposerApprovalMode,
  type AgentApprovalMode
} from '../../src/views/messenger/composerApprovalMode';

const flushReactive = async () => {
  await Promise.resolve();
  await nextTick();
};

test('normalizeAgentApprovalMode normalizes aliases and falls back to full_auto', () => {
  assert.equal(normalizeAgentApprovalMode('suggest'), 'suggest');
  assert.equal(normalizeAgentApprovalMode('auto-edit'), 'auto_edit');
  assert.equal(normalizeAgentApprovalMode('FULL_AUTO'), 'full_auto');
  assert.equal(normalizeAgentApprovalMode(''), 'full_auto');
  assert.equal(normalizeAgentApprovalMode('unknown-mode'), 'full_auto');
});

test('useComposerApprovalMode applies local selection immediately and persists in order', async () => {
  const isAgentConversationActive = ref(true);
  const activeAgentId = ref('agent-a');
  const activeAgentApprovalMode = ref<AgentApprovalMode>('suggest');
  const persistCalls: Array<{ agentId: string; mode: AgentApprovalMode }> = [];
  const pendingResolves: Array<() => void> = [];
  const persistApprovalMode = (agentId: string, mode: AgentApprovalMode) =>
    new Promise<void>((resolve) => {
      persistCalls.push({ agentId, mode });
      pendingResolves.push(resolve);
    });

  const { composerApprovalMode, composerApprovalModeSyncing, updateComposerApprovalMode } =
    useComposerApprovalMode({
      isAgentConversationActive,
      activeAgentId,
      activeAgentApprovalMode,
      resolvePersistAgentId: () => activeAgentId.value,
      persistApprovalMode
    });

  assert.equal(composerApprovalMode.value, 'suggest');
  updateComposerApprovalMode('auto_edit');
  assert.equal(composerApprovalMode.value, 'auto_edit');

  await flushReactive();
  assert.equal(composerApprovalModeSyncing.value, true);
  assert.deepEqual(persistCalls, [{ agentId: 'agent-a', mode: 'auto_edit' }]);

  updateComposerApprovalMode('full_auto');
  assert.equal(composerApprovalMode.value, 'full_auto');
  await flushReactive();
  assert.deepEqual(persistCalls, [{ agentId: 'agent-a', mode: 'auto_edit' }]);

  pendingResolves.shift()?.();
  await flushReactive();
  assert.deepEqual(persistCalls, [
    { agentId: 'agent-a', mode: 'auto_edit' },
    { agentId: 'agent-a', mode: 'full_auto' }
  ]);

  pendingResolves.shift()?.();
  await flushReactive();
  assert.equal(composerApprovalModeSyncing.value, false);
  assert.equal(composerApprovalMode.value, 'full_auto');
});

test('useComposerApprovalMode syncs external updates and skips persistence when conversation inactive', async () => {
  const isAgentConversationActive = ref(true);
  const activeAgentId = ref('agent-b');
  const activeAgentApprovalMode = ref<AgentApprovalMode>('suggest');
  let persistCount = 0;

  const { composerApprovalMode, updateComposerApprovalMode } = useComposerApprovalMode({
    isAgentConversationActive,
    activeAgentId,
    activeAgentApprovalMode,
    resolvePersistAgentId: () => activeAgentId.value,
    persistApprovalMode: async () => {
      persistCount += 1;
    }
  });

  activeAgentApprovalMode.value = 'auto_edit';
  await flushReactive();
  assert.equal(composerApprovalMode.value, 'auto_edit');

  isAgentConversationActive.value = false;
  updateComposerApprovalMode('suggest');
  await flushReactive();
  assert.equal(persistCount, 0);
  assert.equal(composerApprovalMode.value, 'suggest');
});
