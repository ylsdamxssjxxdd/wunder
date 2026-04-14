import test from 'node:test';
import assert from 'node:assert/strict';

import {
  clampPlazaPage,
  filterPlazaItemsByKindAndKeyword,
  filterPlazaItemsByKeyword,
  isPublishableBeeroomGroup,
  isPublishableOwnedAgent,
  paginatePlazaItems,
  resolveRetainedSelectedPlazaItemId,
  resolvePlazaPageCount,
  type PlazaBrowseKind
} from '../../src/components/messenger/hivePlazaPanelState';
import type { PlazaItem } from '../../src/stores/plaza';

const buildPlazaItem = (partial: Partial<PlazaItem> & { item_id: string; kind: PlazaBrowseKind; title: string }): PlazaItem => ({
  item_id: partial.item_id,
  kind: partial.kind,
  title: partial.title,
  summary: partial.summary,
  owner_user_id: String(partial.owner_user_id || 'user-1'),
  owner_username: String(partial.owner_username || 'User 1'),
  source_key: String(partial.source_key || partial.item_id),
  artifact_size_bytes: Number(partial.artifact_size_bytes || 0),
  freshness_status: partial.freshness_status,
  tags: partial.tags || [],
  mine: partial.mine
});

test('hive plaza publish sources exclude default agent aliases and default hives', () => {
  assert.equal(isPublishableOwnedAgent({ id: '__default__' }), false);
  assert.equal(isPublishableOwnedAgent({ id: 'default' }), false);
  assert.equal(isPublishableOwnedAgent({ id: 'agent-alpha' }), true);

  assert.equal(isPublishableBeeroomGroup({ group_id: 'default', name: '默认蜂群', is_default: true }), false);
  assert.equal(isPublishableBeeroomGroup({ group_id: 'hive-1', name: 'Hive 1', is_default: false }), true);
});

test('hive plaza filters cards by selected browse page and keyword', () => {
  const items = [
    buildPlazaItem({
      item_id: 'hive-1',
      kind: 'hive_pack',
      title: 'Research Hive',
      summary: 'Shared hive pack'
    }),
    buildPlazaItem({
      item_id: 'worker-1',
      kind: 'worker_card',
      title: 'Planner Bee',
      summary: 'Planning worker'
    }),
    buildPlazaItem({
      item_id: 'skill-1',
      kind: 'skill_pack',
      title: 'Magic Toolkit',
      summary: 'Skill bundle',
      tags: ['magic', 'bundle']
    })
  ];

  assert.deepEqual(
    filterPlazaItemsByKindAndKeyword(items, 'worker_card', '').map((item) => item.item_id),
    ['worker-1']
  );
  assert.deepEqual(
    filterPlazaItemsByKindAndKeyword(items, 'skill_pack', 'magic').map((item) => item.item_id),
    ['skill-1']
  );
  assert.deepEqual(
    filterPlazaItemsByKeyword(items, 'planner').map((item) => item.item_id),
    ['worker-1']
  );
});

test('hive plaza clears detail selection when the selected item leaves the active page', () => {
  const items = [
    buildPlazaItem({
      item_id: 'worker-1',
      kind: 'worker_card',
      title: 'Planner Bee'
    })
  ];

  assert.equal(resolveRetainedSelectedPlazaItemId(items, 'worker-1'), 'worker-1');
  assert.equal(resolveRetainedSelectedPlazaItemId(items, 'missing-item'), '');
  assert.equal(resolveRetainedSelectedPlazaItemId([], 'worker-1'), '');
});

test('hive plaza pagination clamps page bounds and slices stable card windows', () => {
  const items = Array.from({ length: 10 }, (_, index) =>
    buildPlazaItem({
      item_id: `worker-${index + 1}`,
      kind: 'worker_card',
      title: `Worker ${index + 1}`
    })
  );

  assert.equal(resolvePlazaPageCount(items.length, 4), 3);
  assert.equal(clampPlazaPage(0, items.length, 4), 1);
  assert.equal(clampPlazaPage(99, items.length, 4), 3);
  assert.deepEqual(
    paginatePlazaItems(items, 2, 4).map((item) => item.item_id),
    ['worker-5', 'worker-6', 'worker-7', 'worker-8']
  );
});

test('hive plaza item fixtures preserve freshness status for reminder rendering', () => {
  const outdated = buildPlazaItem({
    item_id: 'worker-outdated',
    kind: 'worker_card',
    title: 'Outdated Worker',
    freshness_status: 'outdated'
  });
  const missing = buildPlazaItem({
    item_id: 'skill-missing',
    kind: 'skill_pack',
    title: 'Missing Skill',
    freshness_status: 'source_missing'
  });

  assert.equal(outdated.freshness_status, 'outdated');
  assert.equal(missing.freshness_status, 'source_missing');
});
