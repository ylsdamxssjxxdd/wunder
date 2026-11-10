import test from 'node:test';
import assert from 'node:assert/strict';
import { readComposerDraftState, writeComposerDraftState, clearComposerDraftState } from '../../src/components/chat/composerDraftCache';

import {
  COMPOSER_MODEL_MAX_ITEMS,
  buildComposerModelSnapshot,
  hasConfiguredModelField,
  normalizeComposerModelItems,
  normalizeConfiguredModelName
} from '../../src/components/chat/composerModelCatalogModel';
import {
  WORKSPACE_REFERENCE_MAX_ITEMS,
  queueWorkspaceChatReference,
  takeWorkspaceChatReferences
} from '../../src/views/messenger/workspace/workspaceChatReference';

// ---------------------------------------------------------------- model catalog

test('legacy string items degrade to name-only options', () => {
  const items = normalizeComposerModelItems(['fast-model', 'deep-model'], 'deep-model');
  assert.deepEqual(items, [
    { id: 'fast-model', name: 'fast-model', context: null, source: '', isDefault: false },
    { id: 'deep-model', name: 'deep-model', context: null, source: '', isDefault: true }
  ]);
});

test('contract object items keep context/source/is_default', () => {
  const items = normalizeComposerModelItems(
    [
      { id: 'sys-a', name: 'Sys A', context: 128000, source: 'system', is_default: true },
      { id: 'usr-b', name: 'Usr B', context: null, source: 'user', is_default: false }
    ],
    'sys-a'
  );
  assert.deepEqual(items, [
    { id: 'sys-a', name: 'Sys A', context: 128000, source: 'system', isDefault: true },
    { id: 'usr-b', name: 'Usr B', context: null, source: 'user', isDefault: false }
  ]);
});

test('model items are de-duplicated by id and bounded', () => {
  const many = Array.from({ length: COMPOSER_MODEL_MAX_ITEMS + 25 }, (_, index) => `m-${index}`);
  assert.equal(normalizeComposerModelItems(many, '').length, COMPOSER_MODEL_MAX_ITEMS);
  assert.equal(normalizeComposerModelItems(['a', 'a', 'A'], '').length, 1);
});

test('a null configured_model_name means "follow system default"', () => {
  const payload = {
    configured_model_name: null,
    model_name: 'old-system-default'
  };
  assert.equal(hasConfiguredModelField(payload), true);
  assert.equal(normalizeConfiguredModelName(payload.configured_model_name), '');

  assert.equal(hasConfiguredModelField({ model_name: 'legacy-default' }), false);
  assert.equal(
    hasConfiguredModelField({ configuredModelName: null }),
    true,
    'camelCase payloads count as delivered as well'
  );
});

test('configured model aliases normalize to a concrete key or the default', () => {
  assert.equal(normalizeConfiguredModelName('model-a'), 'model-a');
  assert.equal(normalizeConfiguredModelName(' default '), '');
  assert.equal(normalizeConfiguredModelName('__default__'), '');
  assert.equal(normalizeConfiguredModelName('SYSTEM'), '');
  assert.equal(normalizeConfiguredModelName(null), '');
  assert.equal(normalizeConfiguredModelName({ id: 'nested' }), '');
});

test('missing contract fields degrade without inventing values', () => {
  const legacy = buildComposerModelSnapshot({
    items: ['only-model'],
    default_model_name: 'only-model'
  });
  assert.equal(legacy.supportsUserDefault, false);
  assert.equal(legacy.userDefaultModelName, '');
  assert.equal(legacy.items.length, 1);
  assert.equal(legacy.items[0].context, null);
  assert.equal(legacy.items[0].source, '');

  const delivered = buildComposerModelSnapshot({
    items: [{ id: 'a', name: 'A', context: 32000, source: 'system', is_default: false }],
    default_model_name: 'a',
    user_default_model_name: 'a'
  });
  assert.equal(delivered.supportsUserDefault, true);
  assert.equal(delivered.userDefaultModelName, 'a');
  assert.equal(delivered.items[0].context, 32000);

  const emptyPayload = buildComposerModelSnapshot({});
  assert.deepEqual(emptyPayload.items, []);
  assert.equal(emptyPayload.loaded, true);
  assert.equal(emptyPayload.failed, false);
});

// ------------------------------------------------------------------ references

test('workspace references hand off normalized relative paths as text', () => {
  takeWorkspaceChatReferences();
  assert.equal(queueWorkspaceChatReference({ path: 'docs\\notes.md' }), true);
  assert.equal(queueWorkspaceChatReference({ path: '   ' }), false);
  assert.deepEqual(takeWorkspaceChatReferences(), [{ path: 'docs/notes.md' }]);
  assert.deepEqual(takeWorkspaceChatReferences(), []);
});

test('pending path handoffs remain bounded and can repeat after being consumed', () => {
  queueWorkspaceChatReference({ path: 'notes.md' });
  queueWorkspaceChatReference({ path: 'notes.md' });
  assert.deepEqual(takeWorkspaceChatReferences(), [{ path: 'notes.md' }]);
  queueWorkspaceChatReference({ path: 'notes.md' });
  assert.deepEqual(takeWorkspaceChatReferences(), [{ path: 'notes.md' }]);
  for (let index = 0; index < WORKSPACE_REFERENCE_MAX_ITEMS + 4; index++) {
    queueWorkspaceChatReference({ path: `file-${index}.md` });
  }
  const paths = takeWorkspaceChatReferences();
  assert.equal(paths.length, WORKSPACE_REFERENCE_MAX_ITEMS);
  assert.equal(paths.at(-1)?.path, `file-${WORKSPACE_REFERENCE_MAX_ITEMS + 3}.md`);
});

test('file reference drafts preserve the path independently from editable text', () => {
  const key = 'fixture-file-draft';
  writeComposerDraftState(key, { content: 'fixture draft', attachments: [{
    id: 'fixture-reference', type: 'workspace', name: 'notes.txt', content: '', workspace_path: 'fixtures/notes.txt'
  }] });
  const restored = readComposerDraftState(key)!;
  assert.equal(restored.content, 'fixture draft');
  assert.equal(restored.attachments[0].workspace_path, 'fixtures/notes.txt');
  restored.attachments.splice(0);
  assert.equal(readComposerDraftState(key)!.attachments.length, 1);
  clearComposerDraftState(key);
});
