import test from 'node:test';
import assert from 'node:assert/strict';

import {
  COMPOSER_MODEL_MAX_ITEMS,
  buildComposerModelSnapshot,
  normalizeComposerModelItems
} from '../../src/components/chat/composerModelCatalogModel';
import {
  COMPOSER_REFERENCE_MAX,
  buildComposerReferenceBlock,
  buildComposerSendContent,
  mergeComposerReferences,
  normalizeComposerReference,
  removeComposerReference
} from '../../src/components/chat/composerReferences';

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

test('workspace references normalize to removable chips', () => {
  assert.deepEqual(normalizeComposerReference({ path: 'docs\\notes.md', isDir: false }), {
    id: 'ws:docs/notes.md',
    path: 'docs/notes.md',
    name: 'notes.md',
    isDir: false,
    source: 'workspace'
  });
  assert.equal(normalizeComposerReference({ path: '   ' }), null);
});

test('merge keeps order, drops duplicates and stays bounded', () => {
  const first = mergeComposerReferences([], [{ path: 'a.md' }, { path: 'b.md' }, { path: 'a.md' }]);
  assert.equal(first.added, 2);
  assert.deepEqual(
    first.items.map((item) => item.path),
    ['a.md', 'b.md']
  );

  const grown = mergeComposerReferences(
    first.items,
    Array.from({ length: COMPOSER_REFERENCE_MAX + 4 }, (_, index) => ({ path: `f-${index}.md` }))
  );
  assert.equal(grown.items.length, COMPOSER_REFERENCE_MAX);
  assert.equal(grown.items[grown.items.length - 1].path, `f-${COMPOSER_REFERENCE_MAX + 3}.md`);

  assert.deepEqual(
    removeComposerReference(first.items, 'ws:a.md').map((item) => item.path),
    ['b.md']
  );
});

test('send content appends an explicit reference block', () => {
  const references = [
    normalizeComposerReference({ path: 'src/a.ts' }),
    normalizeComposerReference({ path: 'docs/b.md' })
  ].filter(Boolean);
  const block = buildComposerReferenceBlock(references, '引用文件');
  assert.equal(block, '引用文件\n@src/a.ts\n@docs/b.md');
  assert.equal(buildComposerSendContent('帮我看下', references, '引用文件'), '帮我看下\n\n引用文件\n@src/a.ts\n@docs/b.md');
  assert.equal(buildComposerSendContent('只有正文', [], '引用文件'), '只有正文');
  assert.equal(buildComposerSendContent('', references, '引用文件'), block);
});
