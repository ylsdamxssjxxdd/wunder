import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const source = readFileSync(resolve(process.cwd(), 'src/components/messenger/AgentSettingsPanel.vue'), 'utf8');

test('agent settings renders cached form immediately and hydrates secondary data in background', () => {
  assert.ok(!source.includes('<HoneycombWaitingOverlay'));
  assert.ok(!source.includes("import HoneycombWaitingOverlay"));
  assert.ok(source.includes('const agentLoading = ref(false);'));
  assert.ok(source.includes('const cachedAgent = agentStore.agentMap[normalizedAgentId.value]'));
  assert.ok(source.includes('await applyAgentToForm(cachedAgent as Record<string, unknown>, requestId);'));
  assert.ok(source.includes('agentLoading.value = false;'));
  assert.ok(source.includes('void beeroomStore.loadGroups().catch(() => null);'));
  assert.ok(source.includes('void loadModelOptions();'));
  assert.ok(source.includes('void toolSummaryPromise;'));
  assert.ok(source.includes('if (cachedAgent && hasUnsavedChanges.value)'));
  assert.ok(source.includes('const hasUserEditedSinceSnapshot = (): boolean => {'));
  assert.ok(source.includes('omitCatalogDerivedSnapshotFields(buildFormSnapshot())'));
  assert.ok(source.includes('!loadedSnapshot.value || hasUserEditedSinceSnapshot()'));
  assert.ok(!source.includes('await Promise.all([\n    loadModelOptions(),\n    loadAgent(requestId),'));
});
