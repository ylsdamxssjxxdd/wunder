import test from 'node:test';
import assert from 'node:assert/strict';

import type { InterlinkNode } from '../../src/api/interlink';
import { applyWorkspaceOrder, moveWorkspaceRow } from '../../src/views/messenger/interlink/interlinkWorkspaceOrder';
import type { InterlinkWorkspaceRow } from '../../src/views/messenger/interlink/interlinkNodeModel';

// The workspace row model reaches the shared API module, whose locale helper
// reads browser preferences at module initialization. Keep this Node-only
// regression harness explicit about that minimal browser surface.
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: { getItem: () => null, setItem: () => undefined }
});
Object.defineProperty(globalThis, 'navigator', {
  configurable: true,
  value: { languages: ['en-US'], language: 'en-US' }
});

const loadWorkspaceRows = async () =>
  (await import('../../src/views/messenger/interlink/interlinkNodeModel')).interlinkWorkspaceRows;

const asNode = (nodeId: string): InterlinkNode =>
  ({
    node_id: nodeId,
    node_type: 'desktop',
    user_id: 'u1',
    label: nodeId,
    status: 'online',
    last_seen_at: 0,
    capabilities: [],
    shadow_revision: 1,
    connected: true,
    meta: {}
  }) as unknown as InterlinkNode;

const row = (key: string, workspaceId = ''): InterlinkWorkspaceRow => ({
  key,
  deviceId: key.replace(/^device:|@.*$/g, ''),
  workspaceId,
  label: key,
  node: asNode(key.replace(/^device:|@.*$/g, ''))
});

test('applyWorkspaceOrder reorders, appends new rows and drops stale keys', () => {
  const rows = [row('device:a'), row('device:b@ws-1', 'ws-1'), row('device:b@ws-2', 'ws-2')];
  // 保存的顺序在前，没记过的行按自然顺序排在末尾。
  assert.deepEqual(
    applyWorkspaceOrder(rows, ['device:b@ws-2', 'device:a']).map((item) => item.key),
    ['device:b@ws-2', 'device:a', 'device:b@ws-1']
  );
  // 已消失的 key（设备下线/工作区删除）被忽略，不产生幽灵行。
  assert.deepEqual(
    applyWorkspaceOrder(rows, ['device:ghost', 'device:b@ws-1']).map((item) => item.key),
    ['device:b@ws-1', 'device:a', 'device:b@ws-2']
  );
  // 空 saved = 自然顺序原样返回。
  assert.deepEqual(
    applyWorkspaceOrder(rows, []).map((item) => item.key),
    rows.map((item) => item.key)
  );
  assert.deepEqual(applyWorkspaceOrder([], ['device:a']), []);
});


test('moveWorkspaceRow lands the dragged row on the target slot', () => {
  const keys = ['cloud', 'device:a', 'device:b', 'device:c'];
  // 向下拖：落到目标行原槽位，目标行及其后整体后移。
  assert.deepEqual(moveWorkspaceRow(keys, 'cloud', 'device:b'), [
    'device:a',
    'device:b',
    'cloud',
    'device:c'
  ]);
  // 向上拖同语义。
  assert.deepEqual(moveWorkspaceRow(keys, 'device:c', 'device:a'), [
    'cloud',
    'device:c',
    'device:a',
    'device:b'
  ]);
  assert.equal(moveWorkspaceRow(keys, 'device:a', 'device:a'), null, 'same row is a no-op');
  assert.equal(moveWorkspaceRow(keys, 'device:ghost', 'device:a'), null, 'unknown drag key');
  assert.equal(moveWorkspaceRow(keys, 'device:a', 'device:ghost'), null, 'unknown target key');
});

test('local workspace rows retain the default root and never fall back to a device row', async () => {
  const interlinkWorkspaceRows = await loadWorkspaceRows();
  const node = asNode('device-a');
  node.label = 'Local device';
  node.meta = {
    workspaces: [
      { workspace_id: '', name: '' },
      { workspace_id: 'workspace-one', name: 'Project one' },
      { workspace_id: 'workspace-two', name: 'Project two' }
    ]
  };

  const rows = interlinkWorkspaceRows([node]);
  assert.deepEqual(
    rows.map((item) => [item.key, item.workspaceId, item.name, item.label]),
    [
      ['device:device-a@', '', '', 'Local device-local'],
      ['device:device-a@workspace-one', 'workspace-one', 'Project one', 'Local device-Project one'],
      ['device:device-a@workspace-two', 'workspace-two', 'Project two', 'Local device-Project two']
    ]
  );

  const noProjection = asNode('device-b');
  noProjection.label = 'Second device';
  assert.deepEqual(
    interlinkWorkspaceRows([noProjection]).map((item) => [item.key, item.workspaceId, item.name, item.label]),
    [['device:device-b@', '', '', 'Second device-local']]
  );
});
